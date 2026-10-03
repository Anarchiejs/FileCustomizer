use std::path::PathBuf;

/// `%APPDATA%\ExplorerBender` (surchargeable par `EXPLORERBENDER_HOME`, utile aux tests manuels).
pub fn data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("EXPLORERBENDER_HOME") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("ExplorerBender")
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.toml")
}
pub fn backup_path() -> PathBuf {
    data_dir().join("backup.json")
}
pub fn status_path() -> PathBuf {
    data_dir().join("status.json")
}
pub fn log_dir() -> PathBuf {
    data_dir().join("logs")
}
/// Présent => le démon ne gère plus rien (posé par `restore`, retiré par `apply`).
pub fn disabled_marker() -> PathBuf {
    data_dir().join("disabled")
}

/// Dossier des listes de destinations automatiques ; l'Accès rapide y vit dans `QUICK_ACCESS_FILE`.
pub fn automatic_destinations_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default();
    base.join(r"Microsoft\Windows\Recent\AutomaticDestinations")
}
pub const QUICK_ACCESS_FILE: &str = "f01b4d95cf55d32a.automaticDestinations-ms";

/// Mutex d'instance unique du démon.
pub fn mutex_name() -> String {
    format!("Local\\ExplorerBender.Daemon{}", instance_suffix())
}
/// Événement nommé : le CLI le signale pour demander l'arrêt propre du démon.
pub fn stop_event_name() -> String {
    format!("Local\\ExplorerBender.Stop{}", instance_suffix())
}

/// Un dossier de données isolé (`EXPLORERBENDER_HOME`) est une instance à part : son démon ne
/// doit ni être bloqué par le vrai démon de la session, ni pouvoir l'arrêter (tests).
fn instance_suffix() -> String {
    match std::env::var_os("EXPLORERBENDER_HOME") {
        None => String::new(),
        Some(h) => {
            // FNV-1a : stable d'un processus à l'autre (contrairement à `DefaultHasher`).
            let h = h.to_string_lossy().to_lowercase().bytes().fold(0xcbf2_9ce4_8422_2325u64, |a, b| (a ^ b as u64).wrapping_mul(0x100_0000_01b3));
            format!(".{h:016x}")
        }
    }
}

/// Profil choisi à la main (`explorerbender apply <profil>`). Absent = les règles décident.
pub fn profile_override() -> PathBuf {
    data_dir().join("profile")
}
/// Rapport écrit par le helper élevé (il n'a pas de console) et relu par le CLI.
pub fn elevated_result() -> PathBuf {
    data_dir().join("elevated-last.json")
}
