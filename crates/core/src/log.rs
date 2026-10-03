//! Journal minimal en fichier tournant (volontairement sans dépendance : le démon doit rester léger).

use crate::config::LogLevel;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

const MAX_BYTES: u64 = 256 * 1024;

struct State {
    path: PathBuf,
    level: LogLevel,
    file: Option<File>,
    size: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn rank(l: LogLevel) -> u8 {
    match l {
        LogLevel::Off => 0,
        LogLevel::Error => 1,
        LogLevel::Warn => 2,
        LogLevel::Info => 3,
        LogLevel::Debug => 4,
    }
}

/// `name` : nom de fichier sans extension (`daemon`, `cli`).
pub fn init(dir: PathBuf, name: &str, level: LogLevel) {
    let path = dir.join(format!("{name}.log"));
    let size = crate::fsutil::as_user(|| {
        let _ = fs::create_dir_all(&dir);
        fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
    });
    *STATE.lock().unwrap() = Some(State { path, level, file: None, size });
}

pub fn set_level(level: LogLevel) {
    if let Some(s) = STATE.lock().unwrap().as_mut() {
        s.level = level;
    }
}

pub fn log(level: LogLevel, msg: &str) {
    let mut g = STATE.lock().unwrap();
    let Some(s) = g.as_mut() else { return };
    if rank(level) == 0 || rank(level) > rank(s.level) {
        return;
    }
    // Le handle ouvert garde les droits de son ouverture : seules ouverture et rotation passent par `as_user`.
    if s.size > MAX_BYTES {
        s.file = None;
        crate::fsutil::as_user(|| fs::rename(&s.path, s.path.with_extension("log.1"))).ok();
        s.size = 0;
    }
    if s.file.is_none() {
        s.file = crate::fsutil::as_user(|| OpenOptions::new().create(true).append(true).open(&s.path)).ok();
    }
    let tag = match level {
        LogLevel::Error => "ERROR",
        LogLevel::Warn => "WARN ",
        LogLevel::Info => "INFO ",
        _ => "DEBUG",
    };
    // Millisecondes : nécessaires pour mesurer les temps de démarrage depuis le journal.
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_millis()).unwrap_or(0);
    let ts = format_utc(now_secs());
    let line = format!("{}.{ms:03}Z {tag} {msg}\n", &ts[..ts.len() - 1]);
    if let Some(f) = s.file.as_mut() {
        if f.write_all(line.as_bytes()).is_ok() {
            s.size += line.len() as u64;
        }
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Secondes Unix -> `2026-10-03T14:15:16Z` (algorithme de Howard Hinnant, sans dépendance).
pub fn format_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[macro_export]
macro_rules! log_info { ($($a:tt)*) => { $crate::log::log($crate::config::LogLevel::Info, &format!($($a)*)) } }
#[macro_export]
macro_rules! log_warn { ($($a:tt)*) => { $crate::log::log($crate::config::LogLevel::Warn, &format!($($a)*)) } }
#[macro_export]
macro_rules! log_error { ($($a:tt)*) => { $crate::log::log($crate::config::LogLevel::Error, &format!($($a)*)) } }
#[macro_export]
macro_rules! log_debug { ($($a:tt)*) => { $crate::log::log($crate::config::LogLevel::Debug, &format!($($a)*)) } }

#[cfg(test)]
mod tests {
    use super::format_utc;
    #[test]
    fn utc_format() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(1_790_000_000), "2026-09-21T14:13:20Z");
        assert_eq!(format_utc(951_782_400), "2000-02-29T00:00:00Z");
    }
}
