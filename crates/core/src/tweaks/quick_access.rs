//! F1 — Accès rapide : mode « désactivé complet » ou « liste blanche ».
//!
//! Trois volets :
//! 1. registre : `ShowFrequent` / `ShowRecent` (absents par défaut sur Windows = activés) ;
//! 2. dossiers épinglés : désépinglés via la commande Shell officielle (voir `shell.rs`) ;
//! 3. surveillance du fichier de l'Accès rapide pour désépingler tout ré-épinglage.
//!
//! Les dossiers « fréquents » (ni épinglés ni récents) ne sont pas supprimés un par un : avec
//! `ShowFrequent = 0` Windows cesse de les afficher, et rien de destructif n'est fait sur
//! l'historique de l'utilisateur.

use crate::config::{Config, QuickAccessMode};
use crate::error::Result;
use crate::paths;
use crate::registry::{RegKey, RegValue};
use crate::shell::QaItem;
use crate::tweak::*;

pub static META: TweakMeta = TweakMeta {
    id: "quick-access",
    name: "Accès rapide",
    description: "Désactive l'Accès rapide (plus d'épingles, plus de dossiers fréquents/récents) ou n'en garde qu'une liste blanche.",
    touches: r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer : ShowFrequent, ShowRecent ; épingles via l'API Shell",
    needs_elevation: false,
};

pub struct QuickAccess;

fn explorer_key() -> RegKey {
    RegKey::hkcu(r"Software\Microsoft\Windows\CurrentVersion\Explorer")
}

/// Normalise un chemin pour la comparaison. Tolère les formes collées depuis un TOML
/// (`"C:\Docs"`, `'C:\Docs',`, `["C:\Docs"`) : guillemets, virgules et crochets sont ignorés.
fn norm(s: &str) -> String {
    s.trim_matches(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | '[' | ']'))
        .trim_end_matches('\\')
        .to_lowercase()
}

fn is_whitelisted(cfg: &Config, item: &QaItem) -> bool {
    cfg.quick_access.whitelist.iter().any(|w| {
        let w = norm(w);
        w == norm(&item.parsing_name) || w == norm(&item.name)
    })
}

/// Un élément est-il autorisé à être épinglé avec la config courante ?
fn allowed_pinned(cfg: &Config, item: &QaItem) -> bool {
    match cfg.quick_access.mode {
        QuickAccessMode::Default => true,
        QuickAccessMode::Disabled => false,
        QuickAccessMode::Whitelist => is_whitelisted(cfg, item),
    }
}

fn desired(cfg: &Config) -> Vec<RegSetting> {
    let q = &cfg.quick_access;
    // `disabled` force tout à false. `whitelist` est une variante de la désactivation : fréquents et
    // récents sont masqués par défaut (sinon ils « réapparaissent » à côté des épingles gardées),
    // sauf `true` explicite. `default` ne touche qu'aux valeurs explicitement demandées.
    let (freq, recent) = match q.mode {
        QuickAccessMode::Disabled => (Some(false), Some(false)),
        QuickAccessMode::Whitelist => (Some(q.show_frequent.unwrap_or(false)), Some(q.show_recent.unwrap_or(false))),
        QuickAccessMode::Default => (q.show_frequent, q.show_recent),
    };
    let mut v = Vec::new();
    for (name, val, label) in [("ShowFrequent", freq, "dossiers fréquents"), ("ShowRecent", recent, "fichiers récents")]
    {
        if let Some(b) = val {
            v.push(RegSetting {
                key: explorer_key(),
                name: name.into(),
                value: RegValue::Dword(b as u32),
                label: format!("Accès rapide : {label} {}", if b { "affichés" } else { "masqués" }),
            });
        }
    }
    v
}

impl Tweak for QuickAccess {
    fn meta(&self) -> &'static TweakMeta {
        &META
    }

    fn requested(&self, cfg: &Config) -> bool {
        cfg.quick_access != Default::default()
    }

    fn detect(&self, ctx: &mut Ctx) -> Result<Vec<DetectedItem>> {
        let mut items = detect_registry(ctx, &desired(ctx.cfg))?;
        if ctx.cfg.quick_access.mode != QuickAccessMode::Default {
            for it in ctx.shell.quick_access_items()?.into_iter().filter(|i| i.pinned) {
                let ok = allowed_pinned(ctx.cfg, &it);
                items.push(DetectedItem {
                    label: format!("Épingle « {} »", it.name),
                    current: "épinglé".into(),
                    desired: if ok { "épinglé".into() } else { "désépinglé".into() },
                    status: if ok { ItemStatus::Ok } else { ItemStatus::Pending },
                });
            }
        }
        Ok(items)
    }

    fn apply(&self, ctx: &mut Ctx) -> Result<()> {
        reconcile_registry(ctx, META.id, &desired(ctx.cfg))?;
        if ctx.defer_shell {
            return Ok(());
        }
        let cfg = ctx.cfg;

        // Rien à demander à l'Explorateur si la config est neutre et que nous n'avons rien défait.
        if cfg.quick_access.mode == QuickAccessMode::Default && ctx.backup.data.unpinned_quick_access.is_empty() {
            return Ok(());
        }
        let items = ctx.shell.quick_access_items()?;

        // (a) ré-épingler ce que nous avions retiré et que la config autorise à nouveau.
        let previously: Vec<String> = ctx.backup.data.unpinned_quick_access.clone();
        for p in previously {
            let probe = QaItem { name: p.clone(), parsing_name: p.clone(), pinned: false };
            let currently_pinned = items.iter().any(|i| i.pinned && norm(&i.parsing_name) == norm(&p));
            if allowed_pinned(cfg, &probe)
                || items.iter().any(|i| norm(&i.parsing_name) == norm(&p) && allowed_pinned(cfg, i))
            {
                if ctx.dry_run {
                    ctx.report.push(META.id, ChangeKind::WouldRevert, format!("Épingle {p}"), "ré-épinglage");
                    continue;
                }
                if !currently_pinned {
                    match ctx.shell.pin(&p) {
                        Ok(()) => ctx.report.push(META.id, ChangeKind::Reverted, format!("Épingle {p}"), "ré-épinglé"),
                        Err(e) => {
                            ctx.report.push(META.id, ChangeKind::Error, format!("Épingle {p}"), e.to_string());
                            continue;
                        }
                    }
                }
                ctx.backup.data.unpinned_quick_access.retain(|x| norm(x) != norm(&p));
                ctx.backup.save()?;
            }
        }

        // (b) désépingler ce que la config n'autorise pas.
        let targets: Vec<&QaItem> = items.iter().filter(|i| i.pinned && !allowed_pinned(cfg, i)).collect();
        if targets.is_empty() {
            ctx.report.push(META.id, ChangeKind::Unchanged, "Épingles de l'Accès rapide", "conformes");
            return Ok(());
        }
        if ctx.dry_run {
            for t in &targets {
                ctx.report.push(
                    META.id,
                    ChangeKind::WouldApply,
                    format!("Épingle « {} »", t.name),
                    format!("désépingler {}", t.parsing_name),
                );
            }
            return Ok(());
        }
        // Anti-boucle : si quelque chose ré-épingle sans cesse, on s'arrête (et on le dit).
        if !ctx.guard.note_rewrite("quick-access:unpin", ctx.now_ms) {
            ctx.report.push(
                META.id,
                ChangeKind::Conflict,
                "Épingles de l'Accès rapide",
                "des dossiers sont ré-épinglés en boucle par un autre outil : on s'arrête",
            );
            return Ok(());
        }
        for t in targets {
            // Mémoriser AVANT d'agir : si on plante entre les deux, `restore` pourra ré-épingler.
            ctx.backup.note_unpinned(&t.parsing_name)?;
            match ctx.shell.unpin(&t.parsing_name) {
                Ok(()) => ctx.report.push(
                    META.id,
                    ChangeKind::Applied,
                    format!("Épingle « {} »", t.name),
                    format!("désépinglé ({})", t.parsing_name),
                ),
                Err(e) => {
                    // Rien n'a changé : on retire la note pour ne pas « restaurer » une épingle qui n'a jamais disparu.
                    ctx.backup.data.unpinned_quick_access.retain(|x| norm(x) != norm(&t.parsing_name));
                    ctx.backup.save()?;
                    ctx.report.push(META.id, ChangeKind::Error, format!("Épingle « {} »", t.name), e.to_string());
                }
            }
        }
        Ok(())
    }

    fn revert(&self, ctx: &mut Ctx) -> Result<()> {
        revert_registry(ctx, META.id)?;
        let list = ctx.backup.data.unpinned_quick_access.clone();
        if list.is_empty() {
            return Ok(());
        }
        let items = ctx.shell.quick_access_items().unwrap_or_default();
        for p in list {
            if ctx.dry_run {
                ctx.report.push(META.id, ChangeKind::WouldRevert, format!("Épingle {p}"), "ré-épinglage");
                continue;
            }
            // La commande Windows est une bascule : ne jamais l'appeler sur un élément déjà épinglé.
            let already = items.iter().any(|i| i.pinned && norm(&i.parsing_name) == norm(&p));
            let res = if already { Ok(()) } else { ctx.shell.pin(&p) };
            match res {
                Ok(()) => {
                    ctx.backup.data.unpinned_quick_access.retain(|x| norm(x) != norm(&p));
                    ctx.backup.save()?;
                    ctx.report.push(META.id, ChangeKind::Reverted, format!("Épingle {p}"), "ré-épinglé");
                }
                Err(e) => ctx.report.push(META.id, ChangeKind::Error, format!("Épingle {p}"), e.to_string()),
            }
        }
        Ok(())
    }

    fn watch(&self, cfg: &Config) -> Vec<WatchTarget> {
        let mut w = Vec::new();
        if !desired(cfg).is_empty() {
            w.push(WatchTarget::RegKey { key: explorer_key(), subtree: false });
        }
        if cfg.quick_access.mode != QuickAccessMode::Default {
            w.push(WatchTarget::DirFile {
                dir: paths::automatic_destinations_dir(),
                file: paths::QUICK_ACCESS_FILE.to_string(),
            });
        }
        w
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::BackupStore;
    use crate::conflict::ConflictGuard;
    use crate::registry::{MockRegistry, RegistryBackend};
    use crate::shell::{MockShell, ShellBackend};

    fn item(name: &str, path: &str, pinned: bool) -> QaItem {
        QaItem { name: name.into(), parsing_name: path.into(), pinned }
    }

    struct Rig {
        reg: MockRegistry,
        shell: MockShell,
        backup: BackupStore,
        guard: ConflictGuard,
    }

    impl Rig {
        fn new(items: Vec<QaItem>) -> Self {
            Rig {
                reg: MockRegistry::new(),
                shell: MockShell::with_items(items),
                backup: BackupStore::in_memory(),
                guard: ConflictGuard::new(5, 30),
            }
        }
        fn apply(&mut self, toml: &str, dry: bool) -> Report {
            let cfg = Config::from_toml(toml).unwrap();
            let mut ctx = Ctx {
                reg: &self.reg,
                shell: &self.shell,
                backup: &mut self.backup,
                cfg: &cfg,
                guard: &mut self.guard,
                dry_run: dry,
                now_ms: 0,
                build: 26200,
                report: Report::default(),
                defer_shell: false,
                elevated: false,
            };
            QuickAccess.apply(&mut ctx).unwrap();
            ctx.report
        }
        fn pinned(&self) -> Vec<String> {
            self.shell.items.borrow().iter().filter(|i| i.pinned).map(|i| i.name.clone()).collect()
        }
    }

    fn sample() -> Vec<QaItem> {
        vec![
            item("Documents", r"C:\Documents", true),
            item("Bureau", r"C:\Desktop", true),
            item("ReviPlan", r"C:\Documents\GitHub\ReviPlan", false),
        ]
    }

    #[test]
    fn disabled_unpins_all_and_sets_registry() {
        let mut r = Rig::new(sample());
        r.apply("[quick_access]\nmode = \"disabled\"", false);
        assert!(r.pinned().is_empty());
        assert_eq!(r.reg.get_value(&explorer_key(), "ShowFrequent").unwrap(), Some(RegValue::Dword(0)));
        assert_eq!(r.reg.get_value(&explorer_key(), "ShowRecent").unwrap(), Some(RegValue::Dword(0)));
        assert_eq!(r.backup.data.unpinned_quick_access.len(), 2);
    }

    #[test]
    fn unpinned_item_is_never_toggled_back_on() {
        // La commande Windows est une bascule : ReviPlan (non épinglé) ne doit pas être touché.
        let mut r = Rig::new(sample());
        r.apply("[quick_access]\nmode = \"disabled\"", false);
        assert!(!r.shell.items.borrow().iter().find(|i| i.name == "ReviPlan").unwrap().pinned);
    }

    #[test]
    fn whitelist_keeps_listed_folder() {
        let mut r = Rig::new(sample());
        r.apply("[quick_access]\nmode = \"whitelist\"\nwhitelist = ['c:\\documents\\']", false);
        assert_eq!(r.pinned(), vec!["Documents".to_string()]);
    }

    #[test]
    fn whitelist_hides_frequent_and_recent_by_default() {
        let mut r = Rig::new(sample());
        r.apply("[quick_access]\nmode = \"whitelist\"\nwhitelist = ['C:\\Documents']", false);
        assert_eq!(r.reg.get_value(&explorer_key(), "ShowFrequent").unwrap(), Some(RegValue::Dword(0)));
        assert_eq!(r.reg.get_value(&explorer_key(), "ShowRecent").unwrap(), Some(RegValue::Dword(0)));
        r.apply("[quick_access]\nmode = \"whitelist\"\nwhitelist = ['C:\\Documents']\nshow_frequent = true", false);
        assert_eq!(r.reg.get_value(&explorer_key(), "ShowFrequent").unwrap(), Some(RegValue::Dword(1)));
    }

    #[test]
    fn whitelist_tolerates_pasted_quotes_and_commas() {
        let mut r = Rig::new(sample());
        // Ce que l'utilisateur a réellement collé dans l'interface : guillemets littéraux dans les valeurs.
        r.apply("[quick_access]\nmode = \"whitelist\"\nwhitelist = ['\"C:\\Documents\",', \"'C:\\\\Desktop'\"]", false);
        let mut p = r.pinned();
        p.sort();
        assert_eq!(p, vec!["Bureau".to_string(), "Documents".to_string()]);
    }

    #[test]
    fn repinned_folder_is_removed_again() {
        let mut r = Rig::new(sample());
        r.apply("[quick_access]\nmode = \"disabled\"", false);
        r.shell.pin(r"C:\Desktop").unwrap(); // « Windows ou une app ré-épingle »
        let rep = r.apply("[quick_access]\nmode = \"disabled\"", false);
        assert!(rep.modified_system());
        assert!(r.pinned().is_empty());
    }

    #[test]
    fn pin_war_is_stopped() {
        let mut r = Rig::new(sample());
        r.guard = ConflictGuard::new(2, 30);
        let mut conflict = false;
        for _ in 0..6 {
            r.shell.pin(r"C:\Desktop").unwrap();
            conflict |= r.apply("[quick_access]\nmode = \"disabled\"", false).has_errors();
        }
        assert!(conflict);
    }

    #[test]
    fn back_to_default_repins_and_restores_registry() {
        let mut r = Rig::new(sample());
        r.apply("[quick_access]\nmode = \"disabled\"", false);
        r.apply("", false);
        assert_eq!(r.pinned().len(), 2);
        assert!(r.backup.is_empty());
        assert_eq!(r.reg.get_value(&explorer_key(), "ShowFrequent").unwrap(), None);
    }

    #[test]
    fn dry_run_changes_nothing() {
        let mut r = Rig::new(sample());
        let rep = r.apply("[quick_access]\nmode = \"disabled\"", true);
        assert_eq!(r.pinned().len(), 2);
        assert_eq!(*r.reg.write_count.borrow(), 0);
        assert!(r.backup.is_empty());
        assert!(rep.changes.iter().any(|c| c.kind == ChangeKind::WouldApply));
    }

    #[test]
    fn revert_repins_what_we_unpinned() {
        let mut r = Rig::new(sample());
        r.apply("[quick_access]\nmode = \"disabled\"", false);
        let cfg = Config::default();
        let mut ctx = Ctx {
            reg: &r.reg,
            shell: &r.shell,
            backup: &mut r.backup,
            cfg: &cfg,
            guard: &mut r.guard,
            dry_run: false,
            now_ms: 0,
            build: 26200,
            report: Report::default(),
            defer_shell: false,
            elevated: false,
        };
        QuickAccess.revert(&mut ctx).unwrap();
        assert_eq!(r.pinned().len(), 2);
        assert!(r.backup.is_empty());
    }
}
