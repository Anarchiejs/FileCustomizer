//! Helper élevé, lancé À LA DEMANDE par le CLI/l'UI avec une invite UAC (jamais résident).
//!
//! Il ne fait que ce qui exige l'élévation (les tweaks `needs_elevation`, aujourd'hui l'écriture
//! HKLM de « Ce PC : dossiers »). Il n'accepte que deux verbes et aucun chemin arbitraire : les
//! clés écrites viennent d'une table fixe (`tweaks::thispc::FOLDERS`), pas de la config brute.
//! Il ne fait pas non plus confiance à `backup.json` (modifiable sans élévation) : seules les
//! entrées de la liste blanche (`tweaks::thispc::is_trusted_elevated_entry`) sont restaurées.
//! Pas de console : le rapport est écrit dans `elevated-last.json`, relu par l'appelant.
//! Toutes ses lectures/écritures de fichiers se font avec le jeton NON élevé de l'utilisateur
//! (`fsutil::as_user`) : seules les écritures de registre profitent de l'élévation.

#![windows_subsystem = "windows"]

use fc_core::config::Config;
use fc_core::drives::SysEnv;
use fc_core::fsutil::{as_user, drop_file_rights_to_user};
use fc_core::session::Session;
use fc_core::tweak::Report;
use fc_core::{log, log_error, log_info, paths, profile};

fn finish(report: &Report, ok: bool) -> ! {
    let json =
        serde_json::to_vec_pretty(&serde_json::json!({ "ok": ok, "changes": report.changes })).unwrap_or_default();
    as_user(|| {
        let _ = std::fs::create_dir_all(paths::data_dir());
        let _ = std::fs::write(paths::elevated_result(), json);
    });
    std::process::exit(if ok { 0 } else { 1 });
}

/// Défense en profondeur : la vraie protection est `as_user` (écritures sans élévation). Un lien
/// ou un point de jonction sur le chemin du dossier de données (ancêtres compris) est refusé d'emblée.
fn check_data_dir() -> Result<(), String> {
    let backup = paths::backup_path();
    let tmp = backup.with_extension("json.tmp");
    let mut all: Vec<std::path::PathBuf> = paths::data_dir().ancestors().map(|a| a.to_path_buf()).collect();
    all.extend([paths::log_dir(), backup, tmp, paths::elevated_result()]);
    for p in all {
        match std::fs::symlink_metadata(&p) {
            Ok(m) if is_reparse_point(&m) => {
                return Err(format!("{} est un lien ou un point de jonction : refusé", p.display()))
            }
            _ => {}
        }
    }
    Ok(())
}

fn is_reparse_point(m: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

fn main() {
    let mut verb = None;
    let mut dry_run = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "apply" | "restore" => verb = Some(a),
            "--dry-run" => dry_run = true,
            // Un process élevé n'hérite pas de l'environnement de l'appelant : on le passe en argument.
            "--home" => {
                if let Some(h) = args.next() {
                    std::env::set_var("FILECUSTOMIZER_HOME", h);
                }
            }
            _ => {}
        }
    }
    let Some(verb) = verb else { std::process::exit(2) };
    // Codes de sortie : 2 = verbe manquant, 3 = dossier de données refusé, 4 = jeton non élevé de
    // l'utilisateur indisponible (élévation par un autre compte...). Aucun rapport écrit dans ces cas.
    if drop_file_rights_to_user().is_err() {
        std::process::exit(4);
    }
    // Avant toute écriture, journal compris : un dossier redirigé ne reçoit rien de nous.
    if as_user(check_data_dir).is_err() {
        std::process::exit(3);
    }

    let base = as_user(|| Config::load(&paths::config_path()));
    log::init(paths::log_dir(), "elevated", base.as_ref().map(|c| c.general.log_level).unwrap_or_default());
    let mut session = match Session::open_elevated() {
        Ok(s) => s,
        Err(e) => {
            log_error!("session : {e}");
            finish(&Report::default(), false);
        }
    };

    let report = if verb == "apply" {
        match base {
            Ok(b) => {
                let r = as_user(|| profile::resolve(b, &SysEnv));
                log_info!("helper élevé : apply (profil {:?})", r.profile);
                session.apply(&r.effective, dry_run)
            }
            Err(e) => {
                log_error!("config : {e}");
                finish(&Report::default(), false);
            }
        }
    } else {
        log_info!("helper élevé : restore");
        session.revert(&Config::default(), dry_run)
    };
    let ok = !report.has_errors();
    finish(&report, ok);
}
