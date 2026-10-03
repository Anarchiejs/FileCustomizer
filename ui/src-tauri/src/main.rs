//! Interface de configuration d'ExplorerBender.
//!
//! Processus SÉPARÉ lancé à la demande, jamais résident : il lit/écrit `config.toml`, le démon
//! détecte le changement et applique. Les actions « Appliquer » / « Tout restaurer » délèguent au
//! CLI (`explorerbender.exe`, installé à côté) pour qu'il n'y ait qu'un seul chemin de code qui
//! modifie le système. Aucun accès fichier/shell générique n'est exposé à la page web.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eb_core::compat;
use eb_core::config::Config;
use eb_core::drives::{self, SysEnv};
use eb_core::paths;
use eb_core::profile;
use eb_core::registry::WinRegistry;
use eb_core::shell::WinShell;
use eb_core::status::Status;
use eb_core::tweaks::{context_menu, navpane, thispc};
use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn daemon_running() -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};
    let name: Vec<u16> = paths::mutex_name().encode_utf16().chain([0]).collect();
    // SAFETY: `name` est un nom UTF-16 terminé par NUL, vivant jusqu'à la fin de l'appel ; le handle obtenu est refermé aussitôt.
    unsafe {
        match OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, PCWSTR(name.as_ptr())) {
            Ok(h) => {
                let _ = CloseHandle(h);
                true
            }
            Err(_) => false,
        }
    }
}

/// Tout l'état nécessaire à l'affichage, en un seul appel.
#[tauri::command(async)]
fn get_state() -> Result<Value, String> {
    let raw = std::fs::read_to_string(paths::config_path()).unwrap_or_default();
    let (cfg, config_error) = match Config::from_toml(&raw) {
        Ok(c) => (c, None),
        Err(e) => (Config::default(), Some(e.to_string())),
    };
    let reg = WinRegistry;
    // COM (STA) pour la résolution des noms localisés ; libéré à la fin de la commande.
    let shell = WinShell::new().map_err(|e| e.to_string())?;
    let nodes: Vec<Value> = navpane::discover_nodes(&reg, &shell)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|n| json!({ "clsid": n.clsid, "name": n.name, "effective": n.effective, "override": n.hkcu_override }))
        .collect();
    let extensions: Vec<Value> = context_menu::discover_extensions(&reg)
        .unwrap_or_default()
        .into_iter()
        .map(|e| json!({ "clsid": e.clsid, "name": e.name, "blocked": e.blocked }))
        .collect();
    let verbs = context_menu::discover_verbs(&reg).unwrap_or_default();
    let folders: Vec<Value> =
        thispc::FOLDERS.iter().map(|(id, _, _, label)| json!({ "id": id, "label": label })).collect();
    let resolved = profile::resolve(cfg.clone(), &SysEnv);
    Ok(json!({
        "config": cfg,
        "config_raw": raw,
        "config_path": paths::config_path().to_string_lossy(),
        "config_error": config_error,
        "windows_build": compat::current_build(&reg),
        "daemon_running": daemon_running(),
        "suspended": paths::disabled_marker().exists(),
        "profile": resolved.profile,
        "profile_manual": resolved.manual,
        "profile_warning": resolved.warning,
        "nodes": nodes,
        "drives": drives::list_drives(),
        "extensions": extensions,
        "verbs": verbs,
        "folders": folders,
        "status": Status::load(&paths::status_path()),
        "compat": compat::COMPAT,
    }))
}

fn write_config_text(text: &str) -> Result<(), String> {
    let path = paths::config_path();
    std::fs::create_dir_all(paths::data_dir()).map_err(|e| e.to_string())?;
    // Une réécriture depuis l'interface perd les commentaires : on garde l'ancien fichier.
    if let Ok(old) = std::fs::read_to_string(&path) {
        if old != text {
            let _ = std::fs::write(path.with_extension("toml.bak"), old);
        }
    }
    // Écriture atomique : le démon ne doit jamais lire un fichier à moitié écrit.
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// Enregistre la configuration éditée dans l'interface (validée avant d'être écrite).
#[tauri::command(async)]
fn save_config(config: Value) -> Result<(), String> {
    let cfg: Config = serde_json::from_value(config).map_err(|e| format!("configuration invalide : {e}"))?;
    let text = cfg.to_toml().map_err(|e| e.to_string())?;
    // Re-parse du texte produit : garantit que ce que le démon lira est bien valide.
    Config::from_toml(&text).map_err(|e| e.to_string())?;
    write_config_text(&text)
}

/// Enregistre le TOML brut (onglet avancé, conserve les commentaires).
#[tauri::command(async)]
fn save_raw(text: String) -> Result<(), String> {
    Config::from_toml(&text).map_err(|e| e.to_string())?;
    write_config_text(&text)
}

fn run_cli(args: &[String]) -> Result<Value, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?.with_file_name("explorerbender.exe");
    if !exe.exists() {
        return Err(format!("explorerbender.exe introuvable à côté de l'interface ({})", exe.display()));
    }
    let out = Command::new(exe).args(args).creation_flags(CREATE_NO_WINDOW).output().map_err(|e| e.to_string())?;
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok(json!({ "code": out.status.code(), "output": text }))
}

/// `apply` via le CLI. `profile` : nom d'un profil de la config (choix manuel) ; `auto` : rend la main aux règles.
#[tauri::command(async)]
fn apply_now(profile: Option<String>, auto: bool, elevate: bool, dry_run: bool) -> Result<Value, String> {
    let mut args = vec!["apply".to_string()];
    if let Some(p) = profile.filter(|p| !p.is_empty()) {
        if p.starts_with('-') {
            return Err("nom de profil invalide".into());
        }
        args.push(p);
    }
    if auto {
        args.push("--auto".into());
    }
    if elevate {
        args.push("--elevate".into());
    }
    if dry_run {
        args.push("--dry-run".into());
    }
    run_cli(&args)
}

/// « Tout restaurer » : état d'origine exact (UAC si des réglages protégés doivent être restaurés).
#[tauri::command(async)]
fn restore_now() -> Result<Value, String> {
    run_cli(&["restore".to_string()])
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_state, save_config, save_raw, apply_now, restore_now])
        .run(tauri::generate_context!())
        .expect("erreur au démarrage de l'interface");
}
