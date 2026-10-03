//! F1 — nœuds du volet de navigation (Accueil, Galerie, Accès rapide, OneDrive, Proton Drive...).
//!
//! Mécanisme (vérifié sur la build 26200, voir docs/PHASE0.md) : chaque nœud est un CLSID qui porte
//! la valeur DWORD `System.IsPinnedToNameSpaceTree`. Le CLSID est défini en HKLM (non modifiable
//! sans admin), mais HKCR fusionne HKCU\Software\Classes par-dessus : on pose donc UNIQUEMENT
//! cette valeur dans HKCU, sans toucher à la définition système.

use crate::config::{Config, Visibility};
use crate::error::Result;
use crate::registry::{Hive, RegKey, RegValue, RegistryBackend};
use crate::shell::ShellBackend;
use crate::tweak::*;

pub const PIN_VALUE: &str = "System.IsPinnedToNameSpaceTree";
const CLSID_ROOT: &str = r"Software\Classes\CLSID";

pub const HOME: &str = "{f874310e-b6b7-47dc-bc84-b9e6b38f5903}";
pub const GALLERY: &str = "{e88865ea-0e1c-4e20-9aa6-edcd0212c87c}";
pub const QUICK_ACCESS: &str = "{679f85cb-0220-4080-b29b-5540cc05aab6}";
pub const LIBRARIES: &str = "{031e4825-7b94-4dc3-b131-e946b44c8dd5}";

/// Noms que l'on peut écrire dans la config (en minuscules). Home n'a aucun nom lisible dans le
/// registre de cette machine, d'où cette table.
const KNOWN: &[(&str, &[&str], &str)] = &[
    (HOME, &["home", "accueil"], "Accueil"),
    (GALLERY, &["gallery", "galerie"], "Galerie"),
    (QUICK_ACCESS, &["quick access", "accès rapide", "acces rapide"], "Accès rapide"),
    (LIBRARIES, &["libraries", "bibliothèques", "bibliotheques"], "Bibliothèques"),
];

pub static META: TweakMeta = TweakMeta {
    id: "navpane",
    name: "Volet de navigation",
    description: "Masque ou affiche les nœuds du volet gauche (Accueil, Galerie, Accès rapide, services cloud...).",
    touches: r"HKCU\Software\Classes\CLSID\{clsid} : System.IsPinnedToNameSpaceTree",
    needs_elevation: false,
};

pub struct NavPane;

fn node_key(clsid: &str) -> RegKey {
    RegKey::hkcu(format!(r"{CLSID_ROOT}\{clsid}"))
}

#[derive(Clone, Debug)]
pub struct NavNode {
    pub clsid: String,
    pub name: String,
    /// Valeur effective (HKCU prioritaire sur HKLM), `None` si illisible.
    pub effective: Option<u32>,
    pub hkcu_override: Option<u32>,
}

fn dword(reg: &dyn RegistryBackend, key: &RegKey) -> Option<u32> {
    match reg.get_value(key, PIN_VALUE) {
        Ok(Some(RegValue::Dword(v))) => Some(v),
        _ => None,
    }
}

fn readable_name(reg: &dyn RegistryBackend, shell: &dyn ShellBackend, clsid: &str, key: &RegKey) -> String {
    if let Some((_, names, label)) = KNOWN.iter().find(|(c, _, _)| c.eq_ignore_ascii_case(clsid)) {
        let _ = names;
        return (*label).to_string();
    }
    for value_name in ["", "LocalizedString"] {
        if let Ok(Some(RegValue::Sz(s) | RegValue::ExpandSz(s))) = reg.get_value(key, value_name) {
            if s.starts_with('@') {
                if let Some(r) = shell.resolve_indirect_string(&s) {
                    return r;
                }
            } else if !s.is_empty() {
                return s;
            }
        }
    }
    clsid.to_string()
}

/// Énumère tous les CLSID qui portent `System.IsPinnedToNameSpaceTree` (HKCU et HKLM).
/// Coûteux (parcourt des milliers de CLSID) : réservé à `filecustomizer nodes` et à la
/// résolution d'un nom lisible inconnu.
pub fn discover_nodes(reg: &dyn RegistryBackend, shell: &dyn ShellBackend) -> Result<Vec<NavNode>> {
    let mut found: Vec<NavNode> = Vec::new();
    for hive in [Hive::Hkcu, Hive::Hklm] {
        let root = RegKey { hive, path: CLSID_ROOT.to_string(), view: Default::default() };
        let root = if hive == Hive::Hklm { RegKey::hklm(r"SOFTWARE\Classes\CLSID") } else { root };
        for sub in reg.list_subkeys(&root)? {
            let k = root.child(&sub);
            if dword(reg, &k).is_none() {
                continue;
            }
            if found.iter().any(|n| n.clsid.eq_ignore_ascii_case(&sub)) {
                continue;
            }
            let hkcu = node_key(&sub);
            let name = readable_name(reg, shell, &sub, &k);
            found.push(NavNode {
                effective: dword(reg, &hkcu).or_else(|| dword(reg, &k)),
                hkcu_override: dword(reg, &hkcu),
                clsid: sub,
                name,
            });
        }
    }
    found.sort_by_key(|n| n.name.to_lowercase());
    Ok(found)
}

fn normalize_clsid(s: &str) -> Option<String> {
    let t = s.trim();
    let inner = t.strip_prefix('{')?.strip_suffix('}')?;
    (inner.len() == 36 && inner.chars().all(|c| c.is_ascii_hexdigit() || c == '-'))
        .then(|| format!("{{{}}}", inner.to_lowercase()))
}

fn resolve(ctx: &Ctx, name: &str, discovered: &mut Option<Vec<NavNode>>) -> Option<String> {
    if let Some(c) = normalize_clsid(name) {
        return Some(c);
    }
    let lower = name.trim().to_lowercase();
    if let Some((c, _, _)) = KNOWN.iter().find(|(_, names, _)| names.contains(&lower.as_str())) {
        return Some((*c).to_string());
    }
    if discovered.is_none() {
        *discovered = discover_nodes(ctx.reg, ctx.shell).ok();
    }
    discovered.as_ref()?.iter().find(|n| n.name.trim().to_lowercase() == lower).map(|n| n.clsid.to_lowercase())
}

fn value_for(v: Visibility) -> Option<u32> {
    match v {
        Visibility::Hide => Some(0),
        Visibility::Show => Some(1),
        Visibility::Default => None,
    }
}

/// État désiré + noms de nœuds de la config qu'on n'a pas su résoudre.
fn desired(ctx: &Ctx) -> (Vec<RegSetting>, Vec<String>) {
    let cfg = &ctx.cfg.navigation_pane;
    let mut wanted: Vec<(String, String, Visibility)> =
        vec![(HOME.into(), "Accueil".into(), cfg.home), (GALLERY.into(), "Galerie".into(), cfg.gallery)];
    let mut unresolved = Vec::new();
    let mut discovered = None;
    for (name, vis) in &cfg.nodes {
        if *vis == Visibility::Default {
            continue;
        }
        match resolve(ctx, name, &mut discovered) {
            Some(c) => {
                // `home`/`gallery` ont priorité si le même CLSID est aussi dans `nodes`.
                if !wanted.iter().any(|(w, _, v)| w.eq_ignore_ascii_case(&c) && *v != Visibility::Default) {
                    wanted.retain(|(w, _, _)| !w.eq_ignore_ascii_case(&c));
                    wanted.push((c, name.clone(), *vis));
                }
            }
            None => unresolved.push(name.clone()),
        }
    }
    let settings = wanted
        .into_iter()
        .filter_map(|(clsid, label, vis)| {
            Some(RegSetting {
                key: node_key(&clsid),
                name: PIN_VALUE.into(),
                value: RegValue::Dword(value_for(vis)?),
                label: format!("{label} : {}", if vis == Visibility::Hide { "masqué" } else { "affiché" }),
            })
        })
        .collect();
    (settings, unresolved)
}

impl Tweak for NavPane {
    fn meta(&self) -> &'static TweakMeta {
        &META
    }

    fn requested(&self, cfg: &Config) -> bool {
        cfg.navigation_pane != Default::default()
    }

    fn detect(&self, ctx: &mut Ctx) -> Result<Vec<DetectedItem>> {
        let (settings, unresolved) = desired(ctx);
        let mut items = detect_registry(ctx, &settings)?;
        for u in unresolved {
            items.push(DetectedItem {
                label: format!("Nœud « {u} »"),
                current: "inconnu".into(),
                desired: "-".into(),
                status: ItemStatus::Pending,
            });
        }
        Ok(items)
    }

    fn apply(&self, ctx: &mut Ctx) -> Result<()> {
        let (settings, unresolved) = desired(ctx);
        for u in unresolved {
            ctx.report.push(
                META.id,
                ChangeKind::Skipped,
                format!("Nœud « {u} »"),
                "introuvable (voir `filecustomizer nodes`)",
            );
        }
        reconcile_registry(ctx, META.id, &settings)
    }

    fn revert(&self, ctx: &mut Ctx) -> Result<()> {
        revert_registry(ctx, META.id)
    }

    fn watch(&self, cfg: &Config) -> Vec<WatchTarget> {
        if !self.requested(cfg) {
            return vec![];
        }
        // Un seul observateur sur le parent couvre tous les CLSID, y compris ceux qui
        // n'existent pas encore (clé créée par nous ou réinitialisée par un autre outil).
        vec![WatchTarget::RegKey { key: RegKey::hkcu(CLSID_ROOT), subtree: true }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::BackupStore;
    use crate::config::Config;
    use crate::conflict::ConflictGuard;
    use crate::registry::MockRegistry;
    use crate::shell::MockShell;

    fn run(cfg: &Config, reg: &MockRegistry, backup: &mut BackupStore, guard: &mut ConflictGuard, dry: bool) -> Report {
        let shell = MockShell::default();
        let mut ctx = Ctx {
            reg,
            shell: &shell,
            backup,
            cfg,
            guard,
            dry_run: dry,
            now_ms: 0,
            build: 26200,
            report: Report::default(),
            defer_shell: false,
            elevated: false,
        };
        NavPane.apply(&mut ctx).unwrap();
        ctx.report
    }

    fn cfg(toml: &str) -> Config {
        Config::from_toml(toml).unwrap()
    }

    #[test]
    fn hide_home_writes_hkcu_and_is_idempotent() {
        let reg = MockRegistry::new().with_key(RegKey::hkcu(CLSID_ROOT));
        let (mut b, mut g) = (BackupStore::in_memory(), ConflictGuard::new(5, 30));
        let c = cfg("[navigation_pane]\nhome = \"hide\"");
        let r = run(&c, &reg, &mut b, &mut g, false);
        assert!(r.modified_system());
        assert_eq!(reg.get_value(&node_key(HOME), PIN_VALUE).unwrap(), Some(RegValue::Dword(0)));
        let writes = *reg.write_count.borrow();
        let r2 = run(&c, &reg, &mut b, &mut g, false);
        assert!(!r2.modified_system());
        assert_eq!(*reg.write_count.borrow(), writes, "deuxième passe : zéro écriture");
    }

    #[test]
    fn dry_run_writes_nothing() {
        let reg = MockRegistry::new();
        let (mut b, mut g) = (BackupStore::in_memory(), ConflictGuard::new(5, 30));
        let r = run(&cfg("[navigation_pane]\nhome = \"hide\""), &reg, &mut b, &mut g, true);
        assert_eq!(*reg.write_count.borrow(), 0);
        assert!(b.is_empty());
        assert!(r.changes.iter().any(|c| c.kind == ChangeKind::WouldApply));
    }

    #[test]
    fn back_to_default_restores_original() {
        let reg = MockRegistry::new().with_value(node_key(GALLERY), PIN_VALUE, RegValue::Dword(1));
        let (mut b, mut g) = (BackupStore::in_memory(), ConflictGuard::new(5, 30));
        run(&cfg("[navigation_pane]\ngallery = \"hide\""), &reg, &mut b, &mut g, false);
        assert_eq!(reg.get_value(&node_key(GALLERY), PIN_VALUE).unwrap(), Some(RegValue::Dword(0)));
        run(&cfg(""), &reg, &mut b, &mut g, false);
        assert_eq!(reg.get_value(&node_key(GALLERY), PIN_VALUE).unwrap(), Some(RegValue::Dword(1)));
        assert!(b.is_empty());
    }

    #[test]
    fn named_node_resolved_by_french_name() {
        let reg = MockRegistry::new();
        let (mut b, mut g) = (BackupStore::in_memory(), ConflictGuard::new(5, 30));
        run(&cfg("[navigation_pane.nodes]\n\"Accueil\" = \"hide\""), &reg, &mut b, &mut g, false);
        assert_eq!(reg.get_value(&node_key(HOME), PIN_VALUE).unwrap(), Some(RegValue::Dword(0)));
    }

    #[test]
    fn failed_write_leaves_no_backup_entry() {
        let reg = MockRegistry::new();
        reg.deny_writes_to(Hive::Hkcu);
        let (mut b, mut g) = (BackupStore::in_memory(), ConflictGuard::new(5, 30));
        let r = run(&cfg("[navigation_pane]\nhome = \"hide\""), &reg, &mut b, &mut g, false);
        assert!(r.has_errors());
        assert!(b.is_empty(), "rien n'a changé : rien à restaurer");
    }

    #[test]
    fn rewrite_war_is_stopped() {
        let reg = MockRegistry::new();
        let (mut b, mut g) = (BackupStore::in_memory(), ConflictGuard::new(2, 30));
        let c = cfg("[navigation_pane]\nhome = \"hide\"");
        run(&c, &reg, &mut b, &mut g, false); // pose initiale
        let mut blocked = false;
        for _ in 0..6 {
            // « l'autre outil » remet la valeur
            reg.set_value(&node_key(HOME), PIN_VALUE, &RegValue::Dword(1)).unwrap();
            let r = run(&c, &reg, &mut b, &mut g, false);
            blocked |= r.changes.iter().any(|c| c.kind == ChangeKind::Conflict);
        }
        assert!(blocked, "le conflit doit être détecté");
        assert_eq!(
            reg.get_value(&node_key(HOME), PIN_VALUE).unwrap(),
            Some(RegValue::Dword(1)),
            "on cesse de réécrire"
        );
    }
}
