//! Helper élevé, lancé À LA DEMANDE par le CLI/l'UI avec une invite UAC (jamais résident).
//!
//! Il ne fait que ce qui exige l'élévation (les tweaks `needs_elevation`, aujourd'hui l'écriture
//! HKLM de « Ce PC : dossiers »). Il n'accepte que deux verbes et aucun chemin arbitraire : les
//! clés écrites viennent d'une table fixe (`tweaks::thispc::FOLDERS`), pas de la config brute.
//! Pas de console : le rapport est écrit dans `elevated-last.json`, relu par l'appelant.

#![windows_subsystem = "windows"]

use eb_core::config::Config;
use eb_core::drives::SysEnv;
use eb_core::session::Session;
use eb_core::tweak::Report;
use eb_core::{log, log_error, log_info, paths, profile};

fn finish(report: &Report, ok: bool) -> ! {
    let _ = std::fs::create_dir_all(paths::data_dir());
    let json = serde_json::to_vec_pretty(&serde_json::json!({ "ok": ok, "changes": report.changes })).unwrap_or_default();
    let _ = std::fs::write(paths::elevated_result(), json);
    std::process::exit(if ok { 0 } else { 1 });
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

    let base = Config::load(&paths::config_path());
    log::init(paths::log_dir(), "elevated", base.as_ref().map(|c| c.general.log_level).unwrap_or_default());
    let mut session = match Session::open() {
        Ok(s) => s,
        Err(e) => {
            log_error!("session : {e}");
            finish(&Report::default(), false);
        }
    };
    session.elevated = true;

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
