//! F2 — « Ce PC » : lecteurs masqués (HKCU, démon) et dossiers masqués (HKLM, helper élevé).
//!
//! Les deux tweaks exigent l'élévation et refusent d'agir ailleurs que dans le helper élevé :
//! - `thispc-folders` écrit en HKLM ;
//! - `thispc-drives` écrit `HKCU\...\Policies\Explorer`, clé en lecture seule pour l'utilisateur
//!   (ACL constatée sur Windows 11 25H2 : seuls SYSTEM et Administrateurs ont le contrôle total).
//!
//! Le démon résident ne les écrit donc jamais.

use crate::config::{Config, ThisPc};
use crate::error::Result;
use crate::registry::{RegKey, RegValue, View};
use crate::tweak::*;

// ---------------------------------------------------------------------------
// Lecteurs : bitmask NoDrives
// ---------------------------------------------------------------------------

pub static DRIVES_META: TweakMeta = TweakMeta {
    id: "thispc-drives",
    name: "Ce PC : lecteurs masqués",
    description:
        "Masque des lecteurs dans l'Explorateur. N'empêche PAS l'accès par chemin (ex. D:\\). Nécessite l'élévation.",
    touches: r"HKCU\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer : NoDrives (bitmask ; clé protégée en écriture pour l'utilisateur)",
    needs_elevation: true,
};

fn policies() -> RegKey {
    RegKey::hkcu(r"Software\Microsoft\Windows\CurrentVersion\Policies\Explorer")
}

/// `"D"`, `"d:"`, `"D:\"` -> bit 3.
pub fn drive_bit(s: &str) -> Option<u32> {
    let c = s.trim().chars().next()?.to_ascii_uppercase();
    c.is_ascii_uppercase().then(|| 1u32 << (c as u32 - 'A' as u32))
}

pub struct ThisPcDrives;

/// `reconcile_registry` seulement dans un processus élevé ; sinon on signale ce qui reste à faire.
fn gated_apply(ctx: &mut Ctx, id: &str, what: &str, d: &[RegSetting]) -> Result<()> {
    if !ctx.elevated && !ctx.dry_run {
        let stale = ctx.backup.entries_for(id).any(|e| !d.iter().any(|s| s.key == e.key && s.name == e.name));
        if stale || detect_registry(ctx, d)?.iter().any(|i| i.status == ItemStatus::Pending) {
            ctx.report.push(
                id,
                ChangeKind::Skipped,
                what,
                "clé protégée en écriture : nécessite l'élévation (`explorerbender apply --elevate`, invite UAC)",
            );
            return Ok(());
        }
    }
    reconcile_registry(ctx, id, d)
}

fn gated_revert(ctx: &mut Ctx, id: &str, what: &str) -> Result<()> {
    if !ctx.elevated && !ctx.dry_run && ctx.backup.entries_for(id).next().is_some() {
        ctx.report.push(id, ChangeKind::Skipped, what, "restauration : nécessite l'élévation");
        return Ok(());
    }
    revert_registry(ctx, id)
}

/// Masque voulu = bits d'origine (conservés : une stratégie existante ne doit pas sauter) OR nos lecteurs.
fn desired_drives(ctx: &Ctx) -> Vec<RegSetting> {
    let ours = ctx.cfg.this_pc.hide_drives.iter().filter_map(|d| drive_bit(d)).fold(0, |a, b| a | b);
    if ours == 0 {
        return vec![];
    }
    let base = match ctx.backup.find(&policies(), "NoDrives") {
        Some(e) => match &e.original {
            Some(RegValue::Dword(v)) => *v,
            _ => 0,
        },
        None => match ctx.reg.get_value(&policies(), "NoDrives") {
            Ok(Some(RegValue::Dword(v))) => v,
            _ => 0,
        },
    };
    let letters: String = (0..26u32).filter(|i| ours & (1 << i) != 0).map(|i| (b'A' + i as u8) as char).collect();
    vec![RegSetting {
        key: policies(),
        name: "NoDrives".into(),
        value: RegValue::Dword(base | ours),
        label: format!("Ce PC : lecteurs masqués ({letters})"),
    }]
}

impl Tweak for ThisPcDrives {
    fn meta(&self) -> &'static TweakMeta {
        &DRIVES_META
    }
    fn requested(&self, cfg: &Config) -> bool {
        !cfg.this_pc.hide_drives.is_empty()
    }
    fn detect(&self, ctx: &mut Ctx) -> Result<Vec<DetectedItem>> {
        detect_registry(ctx, &desired_drives(ctx))
    }
    fn apply(&self, ctx: &mut Ctx) -> Result<()> {
        let d = desired_drives(ctx);
        gated_apply(ctx, DRIVES_META.id, "Ce PC : lecteurs", &d)
    }
    fn revert(&self, ctx: &mut Ctx) -> Result<()> {
        gated_revert(ctx, DRIVES_META.id, "Ce PC : lecteurs")
    }
    fn watch(&self, _cfg: &Config) -> Vec<WatchTarget> {
        // Le démon ne peut pas corriger cette clé (protégée) : rien à surveiller.
        vec![]
    }
}

// ---------------------------------------------------------------------------
// Dossiers : FolderDescriptions\{GUID}\PropertyBag\ThisPCPolicy (HKLM)
// ---------------------------------------------------------------------------

pub static FOLDERS_META: TweakMeta = TweakMeta {
    id: "thispc-folders",
    name: "Ce PC : dossiers masqués",
    description: "Masque Bureau, Documents, Images, Musique, Vidéos, Téléchargements, Objets 3D de « Ce PC ». Nécessite l'élévation (HKLM).",
    touches: r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\FolderDescriptions\{GUID}\PropertyBag : ThisPCPolicy (+ WOW6432Node)",
    needs_elevation: true,
};

/// (identifiant, noms acceptés en minuscules, GUID de la variante « Local* » affichée dans Ce PC, libellé)
pub const FOLDERS: &[(&str, &[&str], &str, &str)] = &[
    ("desktop", &["desktop", "bureau"], "{B4BFCC3A-DB2C-424C-B029-7FE99A87C641}", "Bureau"),
    ("documents", &["documents"], "{f42ee2d3-909f-4907-8871-4c22fc0bf756}", "Documents"),
    ("pictures", &["pictures", "images"], "{0ddd015d-b06c-45d5-8c4c-f59713854639}", "Images"),
    ("music", &["music", "musique"], "{a0c69a99-21c8-4671-8703-7934162fcf1d}", "Musique"),
    ("videos", &["videos", "vidéos"], "{35286a68-3c57-41a1-bbb1-0eae73d76c95}", "Vidéos"),
    (
        "downloads",
        &["downloads", "téléchargements", "telechargements"],
        "{7d83ee9b-2244-4e70-b1f5-5393042af1e4}",
        "Téléchargements",
    ),
    ("3d-objects", &["3d-objects", "3d objects", "objets 3d"], "{31C0DD25-9439-4F12-BF41-7FF4EDA38722}", "Objets 3D"),
];

const FD_ROOT: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\FolderDescriptions";

fn lookup(name: &str) -> Option<&'static (&'static str, &'static [&'static str], &'static str, &'static str)> {
    let n = name.trim().to_lowercase();
    FOLDERS.iter().find(|(id, names, _, _)| *id == n || names.contains(&n.as_str()))
}

fn bag(guid: &str, view: View) -> RegKey {
    RegKey::hklm(format!(r"{FD_ROOT}\{guid}\PropertyBag")).with_view(view)
}

pub struct ThisPcFolders;

/// Réglages voulus + notes sur ce qu'on ne peut pas faire (nom inconnu, dossier absent de cette build).
fn desired_folders(ctx: &Ctx, cfg: &ThisPc) -> (Vec<RegSetting>, Vec<String>) {
    let mut out = Vec::new();
    let mut notes = Vec::new();
    for (list, policy) in [(&cfg.hide_folders, "Hide"), (&cfg.show_folders, "Show")] {
        for name in list {
            let Some((_, _, guid, label)) = lookup(name) else {
                notes.push(format!("dossier « {name} » inconnu"));
                continue;
            };
            for (view, suffix) in [(View::Native, ""), (View::Wow32, " (32 bits)")] {
                // N'écrire que si la description du dossier existe sur cette build (Objets 3D : absent sur 25H2).
                let desc = RegKey::hklm(format!(r"{FD_ROOT}\{guid}")).with_view(view);
                if !ctx.reg.key_exists(&desc).unwrap_or(false) {
                    if view == View::Native {
                        notes.push(format!("dossier « {label} » absent de cette build de Windows : rien à faire"));
                    }
                    break;
                }
                out.push(RegSetting {
                    key: bag(guid, view),
                    name: "ThisPCPolicy".into(),
                    value: RegValue::Sz(policy.into()),
                    label: format!("Ce PC : {label} {}{suffix}", if policy == "Hide" { "masqué" } else { "affiché" }),
                });
            }
        }
    }
    (out, notes)
}

// ---------------------------------------------------------------------------
// Liste blanche du helper élevé
// ---------------------------------------------------------------------------

/// Une entrée de `backup.json` peut-elle être restaurée par le helper élevé ?
///
/// `backup.json` vit dans `%APPDATA%`, modifiable par n'importe quel processus de l'utilisateur.
/// Sans ce filtre, une entrée forgée (`HKLM\...\Run`, etc.) serait écrite avec les droits
/// administrateur à la prochaine invite UAC acceptée. On n'accepte donc que les valeurs exactes
/// que nos tweaks élevés savent écrire, avec le bon type et les seules clés qu'ils peuvent créer.
pub fn is_trusted_elevated_entry(e: &crate::backup::BackupEntry) -> bool {
    if e.tweak == DRIVES_META.id {
        // Si `Policies\Explorer` (voire `Policies`) n'existait pas, nous les avons créées.
        let explorer = policies();
        let allowed_created = |k: &RegKey| *k == explorer || Some(k) == explorer.parent().as_ref();
        return e.key == explorer
            && e.name == "NoDrives"
            && matches!(e.original, None | Some(RegValue::Dword(_)))
            && e.created_keys.iter().all(allowed_created);
    }
    if e.tweak == FOLDERS_META.id {
        let Some(view_key) = FOLDERS
            .iter()
            .flat_map(|(_, _, guid, _)| [bag(guid, View::Native), bag(guid, View::Wow32)])
            .find(|k| *k == e.key)
        else {
            return false;
        };
        // La description du dossier existe toujours (vérifié avant d'écrire) : seul PropertyBag peut être créé.
        return e.name == "ThisPCPolicy"
            && matches!(e.original, None | Some(RegValue::Sz(_)))
            && e.created_keys.iter().all(|k| *k == view_key);
    }
    false
}

impl Tweak for ThisPcFolders {
    fn meta(&self) -> &'static TweakMeta {
        &FOLDERS_META
    }
    fn requested(&self, cfg: &Config) -> bool {
        !cfg.this_pc.hide_folders.is_empty() || !cfg.this_pc.show_folders.is_empty()
    }
    fn detect(&self, ctx: &mut Ctx) -> Result<Vec<DetectedItem>> {
        let (d, _) = desired_folders(ctx, &ctx.cfg.this_pc.clone());
        detect_registry(ctx, &d)
    }
    fn apply(&self, ctx: &mut Ctx) -> Result<()> {
        let (d, notes) = desired_folders(ctx, &ctx.cfg.this_pc.clone());
        for n in notes {
            ctx.report.push(FOLDERS_META.id, ChangeKind::Skipped, "Ce PC : dossiers", n);
        }
        gated_apply(ctx, FOLDERS_META.id, "Ce PC : dossiers", &d)
    }
    fn revert(&self, ctx: &mut Ctx) -> Result<()> {
        gated_revert(ctx, FOLDERS_META.id, "Ce PC : dossiers")
    }
    fn watch(&self, _cfg: &Config) -> Vec<WatchTarget> {
        // Le démon ne corrige jamais HKLM : rien à surveiller.
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::BackupStore;
    use crate::conflict::ConflictGuard;
    use crate::registry::{MockRegistry, RegistryBackend};
    use crate::shell::MockShell;

    fn reg_with_folders() -> MockRegistry {
        let r = MockRegistry::new();
        let r = r.with_value(
            bag("{35286a68-3c57-41a1-bbb1-0eae73d76c95}", View::Native),
            "ThisPCPolicy",
            RegValue::Sz("Show".into()),
        );
        r.with_value(
            bag("{35286a68-3c57-41a1-bbb1-0eae73d76c95}", View::Wow32),
            "ThisPCPolicy",
            RegValue::Sz("Show".into()),
        )
    }

    fn run(
        tweak: &dyn Tweak,
        toml: &str,
        reg: &MockRegistry,
        b: &mut BackupStore,
        elevated: bool,
        dry: bool,
    ) -> Report {
        let cfg = Config::from_toml(toml).unwrap();
        let shell = MockShell::default();
        let mut g = ConflictGuard::new(5, 30);
        let mut ctx = Ctx {
            reg,
            shell: &shell,
            backup: b,
            cfg: &cfg,
            guard: &mut g,
            dry_run: dry,
            now_ms: 0,
            build: 26200,
            defer_shell: false,
            elevated,
            report: Report::default(),
        };
        tweak.apply(&mut ctx).unwrap();
        ctx.report
    }

    #[test]
    fn drive_letters() {
        assert_eq!(drive_bit("D"), Some(8));
        assert_eq!(drive_bit("d:\\"), Some(8));
        assert_eq!(drive_bit("A:"), Some(1));
        assert_eq!(drive_bit("1"), None);
    }

    #[test]
    fn nodrives_keeps_existing_policy_bits_and_restores() {
        let reg = MockRegistry::new().with_value(policies(), "NoDrives", RegValue::Dword(4)); // C masqué par une stratégie
        let mut b = BackupStore::in_memory();
        run(&ThisPcDrives, "[this_pc]\nhide_drives = [\"D\", \"E:\"]", &reg, &mut b, true, false);
        assert_eq!(reg.get_value(&policies(), "NoDrives").unwrap(), Some(RegValue::Dword(4 | 8 | 16)));
        // idempotent : la base vient du backup, pas de la valeur déjà modifiée
        run(&ThisPcDrives, "[this_pc]\nhide_drives = [\"D\", \"E:\"]", &reg, &mut b, true, false);
        assert_eq!(reg.get_value(&policies(), "NoDrives").unwrap(), Some(RegValue::Dword(4 | 8 | 16)));
        run(&ThisPcDrives, "", &reg, &mut b, true, false);
        assert_eq!(reg.get_value(&policies(), "NoDrives").unwrap(), Some(RegValue::Dword(4)));
    }

    #[test]
    fn drives_refuse_without_elevation() {
        let reg = MockRegistry::new();
        reg.deny_writes_to(crate::registry::Hive::Hkcu);
        let mut b = BackupStore::in_memory();
        let r = run(&ThisPcDrives, "[this_pc]\nhide_drives = [\"D\"]", &reg, &mut b, false, false);
        assert!(r.changes.iter().any(|c| c.kind == ChangeKind::Skipped && c.detail.contains("élévation")));
        assert_eq!(*reg.write_count.borrow(), 0);
    }

    #[test]
    fn folders_refuse_without_elevation_and_never_write_hklm() {
        let reg = reg_with_folders();
        let mut b = BackupStore::in_memory();
        reg.deny_writes_to(crate::registry::Hive::Hklm);
        let r = run(&ThisPcFolders, "[this_pc]\nhide_folders = [\"Vidéos\"]", &reg, &mut b, false, false);
        assert_eq!(*reg.write_count.borrow(), 0);
        assert!(r.changes.iter().any(|c| c.kind == ChangeKind::Skipped && c.detail.contains("élévation")));
        assert!(b.is_empty());
    }

    #[test]
    fn folders_elevated_write_both_views_and_restore() {
        let reg = reg_with_folders();
        let mut b = BackupStore::in_memory();
        run(&ThisPcFolders, "[this_pc]\nhide_folders = [\"videos\"]", &reg, &mut b, true, false);
        for v in [View::Native, View::Wow32] {
            assert_eq!(
                reg.get_value(&bag("{35286a68-3c57-41a1-bbb1-0eae73d76c95}", v), "ThisPCPolicy").unwrap(),
                Some(RegValue::Sz("Hide".into()))
            );
        }
        run(&ThisPcFolders, "", &reg, &mut b, true, false);
        assert_eq!(
            reg.get_value(&bag("{35286a68-3c57-41a1-bbb1-0eae73d76c95}", View::Native), "ThisPCPolicy").unwrap(),
            Some(RegValue::Sz("Show".into()))
        );
        assert!(b.is_empty());
    }

    #[test]
    fn entries_written_by_elevated_tweaks_are_trusted() {
        // Pas de PropertyBag ni de Policies\Explorer : les clés créées doivent aussi passer le filtre.
        let reg = MockRegistry::new()
            .with_key(RegKey::hklm(format!(r"{FD_ROOT}\{{35286a68-3c57-41a1-bbb1-0eae73d76c95}}")))
            .with_key(
                RegKey::hklm(format!(r"{FD_ROOT}\{{35286a68-3c57-41a1-bbb1-0eae73d76c95}}")).with_view(View::Wow32),
            )
            .with_key(RegKey::hkcu(r"Software\Microsoft\Windows\CurrentVersion"));
        let mut b = BackupStore::in_memory();
        run(&ThisPcFolders, "[this_pc]\nhide_folders = [\"videos\"]", &reg, &mut b, true, false);
        run(&ThisPcDrives, "[this_pc]\nhide_drives = [\"D\"]", &reg, &mut b, true, false);
        assert_eq!(b.data.entries.len(), 3);
        assert!(b.data.entries.iter().any(|e| !e.created_keys.is_empty()));
        assert!(b.data.entries.iter().all(is_trusted_elevated_entry), "{:#?}", b.data.entries);
    }

    #[test]
    fn forged_entries_are_rejected() {
        use crate::backup::BackupEntry;
        let ok = BackupEntry {
            tweak: FOLDERS_META.id.into(),
            key: bag("{35286a68-3c57-41a1-bbb1-0eae73d76c95}", View::Native),
            name: "ThisPCPolicy".into(),
            existed: true,
            original: Some(RegValue::Sz("Show".into())),
            created_keys: vec![],
        };
        assert!(is_trusted_elevated_entry(&ok));
        let forged = [
            // Clé arbitraire (persistance au démarrage avec les droits admin)
            BackupEntry {
                key: RegKey::hklm(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run"),
                name: "x".into(),
                original: Some(RegValue::Sz("C:\\evil.exe".into())),
                ..ok.clone()
            },
            // Bon emplacement, mauvais nom de valeur
            BackupEntry { name: "Other".into(), ..ok.clone() },
            // Bon emplacement, type inattendu
            BackupEntry { original: Some(RegValue::ExpandSz("%x%".into())), ..ok.clone() },
            // Suppression de clé arbitraire via created_keys
            BackupEntry { created_keys: vec![RegKey::hklm(r"SOFTWARE\Policies")], ..ok.clone() },
            // Tweak non élevé portant une clé HKLM
            BackupEntry { tweak: "navpane".into(), ..ok.clone() },
            // NoDrives ailleurs que dans Policies\Explorer
            BackupEntry {
                tweak: DRIVES_META.id.into(),
                key: RegKey::hklm(r"Software\Microsoft\Windows\CurrentVersion\Policies\Explorer"),
                name: "NoDrives".into(),
                original: Some(RegValue::Dword(0)),
                ..ok.clone()
            },
        ];
        for f in &forged {
            assert!(!is_trusted_elevated_entry(f), "accepté à tort : {f:?}");
        }
    }

    #[test]
    fn absent_folder_and_unknown_name_are_skipped_not_created() {
        let reg = reg_with_folders();
        let mut b = BackupStore::in_memory();
        let r = run(&ThisPcFolders, "[this_pc]\nhide_folders = [\"objets 3d\", \"truc\"]", &reg, &mut b, true, false);
        assert_eq!(r.changes.iter().filter(|c| c.kind == ChangeKind::Skipped).count(), 2);
        assert_eq!(*reg.write_count.borrow(), 0);
    }
}
