//! F4 — menu contextuel : menu classique, extensions Shell bloquées, verbes statiques désactivés.
//!
//! Tout est non destructif et en HKCU : on ajoute des valeurs/clés, jamais on ne supprime ni ne
//! modifie ce que les applications ont installé.

use crate::config::Config;
use crate::error::Result;
use crate::registry::{Hive, RegKey, RegValue, RegistryBackend};
use crate::tweak::*;
use std::collections::BTreeMap;

pub static META: TweakMeta = TweakMeta {
    id: "context-menu",
    name: "Menu contextuel",
    description: "Menu contextuel classique, blocage d'extensions Shell, désactivation de verbes statiques.",
    touches: r"HKCU\Software\Classes\CLSID\{86ca1aa0-…}\InprocServer32 ; HKCU\…\Shell Extensions\Blocked ; HKCU\Software\Classes\<clé>\shell\<verbe> : LegacyDisable",
    needs_elevation: false,
};

pub const CLASSIC_MENU_CLSID: &str = "{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}";
const BLOCKED: &str = r"Software\Microsoft\Windows\CurrentVersion\Shell Extensions\Blocked";
const APPROVED: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Shell Extensions\Approved";
/// Racines de classes où vivent des gestionnaires de menu contextuel / verbes.
const CLASS_ROOTS: &[&str] = &["*", "AllFilesystemObjects", "Directory", "Directory\\Background", "Folder", "Drive"];

pub struct ContextMenu;

#[derive(Clone, Debug)]
pub struct ShellExt {
    pub clsid: String,
    pub name: String,
    pub blocked: bool,
}

fn is_guid(s: &str) -> bool {
    let t = s.trim();
    t.len() == 38
        && t.starts_with('{')
        && t.ends_with('}')
        && t[1..37].chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

fn sz(v: Option<RegValue>) -> Option<String> {
    match v {
        Some(RegValue::Sz(s) | RegValue::ExpandSz(s)) if !s.is_empty() => Some(s),
        _ => None,
    }
}

/// Extensions de menu contextuel connues : gestionnaires déclarés dans `ContextMenuHandlers`
/// (HKLM + HKCU) et liste `Approved`. Nom lisible = nom de la clé du gestionnaire.
pub fn discover_extensions(reg: &dyn RegistryBackend) -> Result<Vec<ShellExt>> {
    let mut by_clsid: BTreeMap<String, String> = BTreeMap::new();
    for (hive, base) in [(Hive::Hklm, r"SOFTWARE\Classes"), (Hive::Hkcu, r"Software\Classes")] {
        for root in CLASS_ROOTS {
            let k =
                RegKey { hive, path: format!(r"{base}\{root}\shellex\ContextMenuHandlers"), view: Default::default() };
            for h in reg.list_subkeys(&k)? {
                let clsid = sz(reg.get_value(&k.child(&h), "")?)
                    .filter(|s| is_guid(s))
                    .or_else(|| is_guid(&h).then(|| h.clone()));
                if let Some(c) = clsid {
                    let entry = by_clsid.entry(c.to_lowercase()).or_default();
                    if entry.is_empty() && !is_guid(&h) {
                        *entry = h.clone();
                    }
                }
            }
        }
    }
    for (n, v) in reg.list_values(&RegKey::hklm(APPROVED))? {
        if is_guid(&n) {
            let e = by_clsid.entry(n.to_lowercase()).or_default();
            if e.is_empty() {
                *e = sz(Some(v)).unwrap_or_default();
            }
        }
    }
    let blocked: Vec<String> =
        [RegKey::hkcu(BLOCKED), RegKey::hklm(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Shell Extensions\Blocked")]
            .iter()
            .flat_map(|k| reg.list_values(k).unwrap_or_default())
            .map(|(n, _)| n.to_lowercase())
            .collect();
    let mut v: Vec<ShellExt> = by_clsid
        .into_iter()
        .map(|(clsid, name)| ShellExt {
            blocked: blocked.contains(&clsid),
            name: if name.is_empty() { clsid.clone() } else { name },
            clsid,
        })
        .collect();
    v.sort_by_key(|e| e.name.to_lowercase());
    Ok(v)
}

/// Verbes statiques (`<racine>\shell\<verbe>`) que l'on peut désactiver.
pub fn discover_verbs(reg: &dyn RegistryBackend) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for (hive, base) in [(Hive::Hklm, r"SOFTWARE\Classes"), (Hive::Hkcu, r"Software\Classes")] {
        for root in CLASS_ROOTS {
            let k = RegKey { hive, path: format!(r"{base}\{root}\shell"), view: Default::default() };
            for verb in reg.list_subkeys(&k)? {
                let p = format!(r"{root}\shell\{verb}");
                if !out.iter().any(|x| x.eq_ignore_ascii_case(&p)) {
                    out.push(p);
                }
            }
        }
    }
    out.sort_by_key(|s| s.to_lowercase());
    Ok(out)
}

/// Un verbe ne peut viser que `<…>\shell\<verbe>` d'une classe existante : on refuse tout autre
/// chemin pour qu'une config ne puisse pas écrire n'importe où dans HKCU\Software\Classes.
fn verb_ok(reg: &dyn RegistryBackend, path: &str) -> bool {
    let p = path.trim_matches('\\');
    if p.contains("..") || !p.to_lowercase().contains(r"\shell\") {
        return false;
    }
    reg.key_exists(&RegKey::hklm(format!(r"SOFTWARE\Classes\{p}"))).unwrap_or(false)
        || reg.key_exists(&RegKey::hkcu(format!(r"Software\Classes\{p}"))).unwrap_or(false)
}

fn desired(ctx: &Ctx) -> (Vec<RegSetting>, Vec<String>) {
    let c = &ctx.cfg.context_menu;
    let mut out = Vec::new();
    let mut notes = Vec::new();
    if c.classic_menu {
        out.push(RegSetting {
            key: RegKey::hkcu(format!(r"Software\Classes\CLSID\{CLASSIC_MENU_CLSID}\InprocServer32")),
            name: String::new(),
            value: RegValue::Sz(String::new()),
            label: "Menu contextuel classique".into(),
        });
    }
    if !c.blocked_extensions.is_empty() {
        let known = discover_extensions(ctx.reg).unwrap_or_default();
        for n in &c.blocked_extensions {
            let clsid = if is_guid(n) {
                Some(n.trim().to_lowercase())
            } else {
                known.iter().find(|e| e.name.eq_ignore_ascii_case(n.trim())).map(|e| e.clsid.clone())
            };
            match clsid {
                Some(id) => out.push(RegSetting {
                    key: RegKey::hkcu(BLOCKED),
                    name: id.to_uppercase(),
                    value: RegValue::Sz(String::new()),
                    label: format!("Extension bloquée : {n}"),
                }),
                None => notes.push(format!("extension « {n} » introuvable (voir `filecustomizer shell-extensions`)")),
            }
        }
    }
    for v in &c.disabled_verbs {
        if verb_ok(ctx.reg, v) {
            out.push(RegSetting {
                key: RegKey::hkcu(format!(r"Software\Classes\{}", v.trim_matches('\\'))),
                name: "LegacyDisable".into(),
                value: RegValue::Sz(String::new()),
                label: format!("Verbe désactivé : {v}"),
            });
        } else {
            notes.push(format!("verbe « {v} » refusé ou introuvable (forme attendue : Directory\\shell\\cmd)"));
        }
    }
    (out, notes)
}

const MAX_VERB_WATCHES: usize = 8;

impl Tweak for ContextMenu {
    fn meta(&self) -> &'static TweakMeta {
        &META
    }
    fn requested(&self, cfg: &Config) -> bool {
        cfg.context_menu != Default::default()
    }
    fn detect(&self, ctx: &mut Ctx) -> Result<Vec<DetectedItem>> {
        let (d, _) = desired(ctx);
        detect_registry(ctx, &d)
    }
    fn apply(&self, ctx: &mut Ctx) -> Result<()> {
        let (d, notes) = desired(ctx);
        for n in notes {
            ctx.report.push(META.id, ChangeKind::Skipped, "Menu contextuel", n);
        }
        reconcile_registry(ctx, META.id, &d)
    }
    fn revert(&self, ctx: &mut Ctx) -> Result<()> {
        revert_registry(ctx, META.id)
    }
    fn watch(&self, cfg: &Config) -> Vec<WatchTarget> {
        let c = &cfg.context_menu;
        let mut w = Vec::new();
        if c.classic_menu {
            w.push(WatchTarget::RegKey { key: RegKey::hkcu(r"Software\Classes\CLSID"), subtree: true });
        }
        if !c.blocked_extensions.is_empty() {
            w.push(WatchTarget::RegKey { key: RegKey::hkcu(BLOCKED), subtree: false });
        }
        // Une surveillance par verbe, sauf au-delà de quelques-uns : le démon attend au plus 63
        // handles à la fois, une seule surveillance de `Software\Classes` couvre alors tout.
        if c.disabled_verbs.len() > MAX_VERB_WATCHES {
            w.push(WatchTarget::RegKey { key: RegKey::hkcu(r"Software\Classes"), subtree: true });
        } else {
            for v in &c.disabled_verbs {
                w.push(WatchTarget::RegKey {
                    key: RegKey::hkcu(format!(r"Software\Classes\{}", v.trim_matches('\\'))),
                    subtree: false,
                });
            }
        }
        w
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::BackupStore;
    use crate::conflict::ConflictGuard;
    use crate::registry::MockRegistry;
    use crate::shell::MockShell;

    fn run(toml: &str, reg: &MockRegistry, b: &mut BackupStore) -> Report {
        let cfg = Config::from_toml(toml).unwrap();
        let shell = MockShell::default();
        let mut g = ConflictGuard::new(5, 30);
        let mut ctx = Ctx {
            reg,
            shell: &shell,
            backup: b,
            cfg: &cfg,
            guard: &mut g,
            dry_run: false,
            now_ms: 0,
            build: 26200,
            defer_shell: false,
            elevated: false,
            node_cache: None,
            report: Report::default(),
        };
        ContextMenu.apply(&mut ctx).unwrap();
        ctx.report
    }

    fn classic() -> RegKey {
        RegKey::hkcu(format!(r"Software\Classes\CLSID\{CLASSIC_MENU_CLSID}\InprocServer32"))
    }

    #[test]
    fn many_disabled_verbs_share_one_watch() {
        let few = Config::from_toml(
            r"
            [context_menu]
            disabled_verbs = ['Directory\shell\a', 'Directory\shell\b']",
        )
        .unwrap();
        assert_eq!(ContextMenu.watch(&few).len(), 2);
        let verbs: Vec<String> = (0..80).map(|i| format!(r"'Directory\shell\v{i}'")).collect();
        let many = Config::from_toml(&format!("[context_menu]\ndisabled_verbs = [{}]", verbs.join(","))).unwrap();
        assert_eq!(
            ContextMenu.watch(&many),
            vec![WatchTarget::RegKey { key: RegKey::hkcu(r"Software\Classes"), subtree: true }]
        );
    }

    #[test]
    fn classic_menu_sets_empty_default_and_is_fully_removed_on_restore() {
        let reg = MockRegistry::new();
        let mut b = BackupStore::in_memory();
        run("[context_menu]\nclassic_menu = true", &reg, &mut b);
        assert_eq!(reg.get_value(&classic(), "").unwrap(), Some(RegValue::Sz(String::new())));
        run("", &reg, &mut b);
        assert!(!reg.key_exists(&classic()).unwrap());
        assert!(!reg.key_exists(&RegKey::hkcu(format!(r"Software\Classes\CLSID\{CLASSIC_MENU_CLSID}"))).unwrap());
    }

    #[test]
    fn block_extension_by_name_and_guid() {
        let reg = MockRegistry::new().with_value(
            RegKey::hklm(r"SOFTWARE\Classes\Directory\shellex\ContextMenuHandlers\Mon Extension"),
            "",
            RegValue::Sz("{AAAAAAAA-0000-0000-0000-000000000001}".into()),
        );
        let mut b = BackupStore::in_memory();
        run("[context_menu]\nblocked_extensions = [\"mon extension\", \"{BBBBBBBB-0000-0000-0000-000000000002}\", \"inconnu\"]", &reg, &mut b);
        let k = RegKey::hkcu(BLOCKED);
        assert!(reg.get_value(&k, "{AAAAAAAA-0000-0000-0000-000000000001}").unwrap().is_some());
        assert!(reg.get_value(&k, "{BBBBBBBB-0000-0000-0000-000000000002}").unwrap().is_some());
        let ext = discover_extensions(&reg).unwrap();
        assert!(ext.iter().any(|e| e.name == "Mon Extension" && e.blocked));
    }

    #[test]
    fn verb_disable_is_restricted_to_existing_shell_verbs() {
        let reg = MockRegistry::new().with_key(RegKey::hklm(r"SOFTWARE\Classes\Directory\shell\cmd"));
        let mut b = BackupStore::in_memory();
        let r = run(
            "[context_menu]\ndisabled_verbs = ['Directory\\shell\\cmd', 'Software\\Run', 'Directory\\shell\\absent']",
            &reg,
            &mut b,
        );
        assert!(reg
            .get_value(&RegKey::hkcu(r"Software\Classes\Directory\shell\cmd"), "LegacyDisable")
            .unwrap()
            .is_some());
        assert_eq!(r.changes.iter().filter(|c| c.kind == ChangeKind::Skipped).count(), 2);
        assert!(!reg.key_exists(&RegKey::hkcu(r"Software\Classes\Software\Run")).unwrap());
        run("", &reg, &mut b);
        assert!(!reg.key_exists(&RegKey::hkcu(r"Software\Classes\Directory\shell\cmd")).unwrap());
    }
}
