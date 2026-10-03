//! Helper élevé, lancé À LA DEMANDE par le CLI/l'UI avec une invite UAC (jamais résident).
//!
//! Il ne fait que ce qui exige l'élévation (les tweaks `needs_elevation`, aujourd'hui l'écriture
//! HKLM de « Ce PC : dossiers »). Il n'accepte que deux verbes et aucun chemin arbitraire : les
//! clés écrites viennent d'une table fixe (`tweaks::thispc::FOLDERS`), pas de la config brute.
//! Il ne fait pas non plus confiance à `backup.json` (modifiable sans élévation) : seules les
//! entrées de la liste blanche (`tweaks::thispc::is_trusted_elevated_entry`) sont restaurées.
//! Pas de console : le rapport est écrit dans `elevated-last.json`, relu par l'appelant.

#![windows_subsystem = "windows"]

use eb_core::config::Config;
use eb_core::drives::SysEnv;
use eb_core::session::Session;
use eb_core::tweak::Report;
use eb_core::{log, log_error, log_info, paths, profile};

fn finish(report: &Report, ok: bool) -> ! {
    let _ = std::fs::create_dir_all(paths::data_dir());
    let json =
        serde_json::to_vec_pretty(&serde_json::json!({ "ok": ok, "changes": report.changes })).unwrap_or_default();
    let _ = std::fs::write(paths::elevated_result(), json);
    std::process::exit(if ok { 0 } else { 1 });
}

/// Le dossier de données est contrôlé par l'utilisateur, et nous y écrivons en administrateur
/// (backup.json, journaux, rapport). Un point de jonction ou un lien symbolique permettrait de
/// rediriger ces écritures ailleurs (ex. `C:\Windows`) : on refuse d'y travailler.
fn check_data_dir() -> Result<(), String> {
    let backup = paths::backup_path();
    let tmp = backup.with_extension("json.tmp");
    for p in [paths::data_dir(), paths::log_dir(), backup, tmp, paths::elevated_result()] {
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
                    std::env::set_var("EXPLORERBENDER_HOME", h);
                }
            }
            _ => {}
        }
    }
    let Some(verb) = verb else { std::process::exit(2) };
    // Avant toute écriture, journal compris : un dossier redirigé ne reçoit rien de nous.
    // Codes de sortie : 2 = verbe manquant, 3 = dossier de données refusé (aucun rapport écrit).
    if check_data_dir().is_err() {
        std::process::exit(3);
    }

    let base = Config::load(&paths::config_path());
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
                let r = profile::resolve(b, &SysEnv);
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
