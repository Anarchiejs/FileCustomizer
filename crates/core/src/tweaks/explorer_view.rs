//! F3 — options d'affichage de l'Explorateur (`HKCU\...\Explorer\Advanced`).
//!
//! Aucun redémarrage d'Explorer n'est fait : `engine::apply_all` envoie `SHChangeNotify` +
//! `WM_SETTINGCHANGE` après une écriture réelle ; les nouvelles fenêtres relisent ces valeurs.

use crate::config::{Config, LaunchTo};
use crate::error::Result;
use crate::registry::{RegKey, RegValue};
use crate::tweak::*;

pub static META: TweakMeta = TweakMeta {
    id: "explorer-view",
    name: "Options d'affichage",
    description: "Extensions, fichiers cachés/système, ouverture par défaut, cases à cocher, volet de navigation, mode compact...",
    touches: r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced : HideFileExt, Hidden, ShowSuperHidden, LaunchTo, AutoCheckSelect, NavPaneShowAllFolders, NavPaneExpandToCurrentFolder, ShowSyncProviderNotifications, UseCompactMode, ShowStatusBar, HideDrivesWithNoMedia",
    needs_elevation: false,
};

pub struct ExplorerView;

fn advanced() -> RegKey {
    RegKey::hkcu(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced")
}

fn launch_to_value(l: LaunchTo) -> u32 {
    match l {
        LaunchTo::ThisPc => 1,
        LaunchTo::Home => 2,
        LaunchTo::Downloads => 3,
        LaunchTo::Onedrive => 4,
    }
}

fn desired(cfg: &Config) -> Vec<RegSetting> {
    let v = &cfg.explorer_view;
    // (valeur de registre, valeur voulue, libellé). Les booléens « afficher » s'inversent pour
    // les clés « masquer » (HideFileExt, HideDrivesWithNoMedia) ; Hidden vaut 1 (afficher) ou 2 (masquer).
    let b = |on: bool| on as u32;
    let items: Vec<(&str, Option<u32>, &str)> = vec![
        ("HideFileExt", v.show_file_extensions.map(|x| b(!x)), "extensions de fichiers"),
        ("Hidden", v.show_hidden_files.map(|x| if x { 1 } else { 2 }), "fichiers cachés"),
        ("ShowSuperHidden", v.show_system_files.map(b), "fichiers protégés du système"),
        ("LaunchTo", v.launch_to.map(launch_to_value), "ouverture par défaut"),
        ("AutoCheckSelect", v.use_checkboxes.map(b), "cases à cocher"),
        ("NavPaneShowAllFolders", v.nav_show_all_folders.map(b), "volet : afficher tous les dossiers"),
        (
            "NavPaneExpandToCurrentFolder",
            v.nav_expand_to_current_folder.map(b),
            "volet : développer jusqu'au dossier courant",
        ),
        (
            "ShowSyncProviderNotifications",
            v.sync_provider_notifications.map(b),
            "suggestions du fournisseur de synchronisation",
        ),
        ("UseCompactMode", v.compact_mode.map(b), "mode compact"),
        ("ShowStatusBar", v.show_status_bar.map(b), "barre d'état"),
        ("HideDrivesWithNoMedia", v.hide_drives_with_no_media.map(b), "masquer les lecteurs sans média"),
    ];
    items
        .into_iter()
        .filter_map(|(name, val, label)| {
            Some(RegSetting {
                key: advanced(),
                name: name.into(),
                value: RegValue::Dword(val?),
                label: format!("Affichage : {label}"),
            })
        })
        .collect()
}

impl Tweak for ExplorerView {
    fn meta(&self) -> &'static TweakMeta {
        &META
    }
    fn requested(&self, cfg: &Config) -> bool {
        cfg.explorer_view != Default::default()
    }
    fn detect(&self, ctx: &mut Ctx) -> Result<Vec<DetectedItem>> {
        detect_registry(ctx, &desired(ctx.cfg))
    }
    fn apply(&self, ctx: &mut Ctx) -> Result<()> {
        reconcile_registry(ctx, META.id, &desired(ctx.cfg))
    }
    fn revert(&self, ctx: &mut Ctx) -> Result<()> {
        revert_registry(ctx, META.id)
    }
    fn watch(&self, cfg: &Config) -> Vec<WatchTarget> {
        if self.requested(cfg) {
            vec![WatchTarget::RegKey { key: advanced(), subtree: false }]
        } else {
            vec![]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::BackupStore;
    use crate::conflict::ConflictGuard;
    use crate::registry::{MockRegistry, RegistryBackend};
    use crate::shell::MockShell;

    fn run(toml: &str, reg: &MockRegistry, b: &mut BackupStore) {
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
            report: Report::default(),
        };
        ExplorerView.apply(&mut ctx).unwrap();
    }

    #[test]
    fn inverted_and_special_values() {
        let reg = MockRegistry::new();
        let mut b = BackupStore::in_memory();
        run("[explorer_view]\nshow_file_extensions = true\nshow_hidden_files = false\nlaunch_to = \"this_pc\"\ncompact_mode = true", &reg, &mut b);
        let k = advanced();
        assert_eq!(reg.get_value(&k, "HideFileExt").unwrap(), Some(RegValue::Dword(0)));
        assert_eq!(reg.get_value(&k, "Hidden").unwrap(), Some(RegValue::Dword(2)));
        assert_eq!(reg.get_value(&k, "LaunchTo").unwrap(), Some(RegValue::Dword(1)));
        assert_eq!(reg.get_value(&k, "UseCompactMode").unwrap(), Some(RegValue::Dword(1)));
        assert_eq!(reg.get_value(&k, "AutoCheckSelect").unwrap(), None, "non demandé = non touché");
    }

    #[test]
    fn removing_option_restores_original() {
        let reg = MockRegistry::new().with_value(advanced(), "HideFileExt", RegValue::Dword(1));
        let mut b = BackupStore::in_memory();
        run("[explorer_view]\nshow_file_extensions = true", &reg, &mut b);
        assert_eq!(reg.get_value(&advanced(), "HideFileExt").unwrap(), Some(RegValue::Dword(0)));
        run("", &reg, &mut b);
        assert_eq!(reg.get_value(&advanced(), "HideFileExt").unwrap(), Some(RegValue::Dword(1)));
    }
}
