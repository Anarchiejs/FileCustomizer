//! Tests d'intégration du démon, sur un dossier de données isolé et une config qui ne demande
//! rien : le démon tourne pour de vrai (mutex, surveillances, arrêt propre) sans toucher au
//! registre. Le mutex et l'événement d'arrêt dépendent du dossier de données : un vrai démon
//! sur la session ne gêne pas ces tests, et ces tests ne peuvent pas l'arrêter.

use eb_core::paths;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use windows::core::PCWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{OpenEventW, OpenMutexW, SetEvent, EVENT_MODIFY_STATE, SYNCHRONIZATION_SYNCHRONIZE};

struct TempHome(PathBuf);

impl TempHome {
    fn new(name: &str) -> Self {
        let p = std::env::temp_dir().join(format!("eb-daemon-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// Les noms d'objets noyau de l'instance dépendent de `EXPLORERBENDER_HOME` : on le pose dans
/// le processus de test pour viser le démon isolé (les autres tests le passent explicitement).
fn target_instance(home: &Path) {
    std::env::set_var("EXPLORERBENDER_HOME", home);
}

fn instance_running() -> bool {
    let name = wide(&paths::mutex_name());
    match unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, PCWSTR(name.as_ptr())) } {
        Ok(h) => {
            let _ = unsafe { CloseHandle(h) };
            true
        }
        Err(_) => false,
    }
}

fn signal_stop() -> bool {
    let name = wide(&paths::stop_event_name());
    unsafe {
        let Ok(h) = OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(name.as_ptr())) else { return false };
        let ok = SetEvent(h).is_ok();
        let _ = CloseHandle(h);
        ok
    }
}

fn spawn(home: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_explorerbender-daemon")).env("EXPLORERBENDER_HOME", home).spawn().unwrap()
}

fn wait_until(timeout: Duration, mut f: impl FnMut() -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < timeout {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn wait_exit(child: &mut Child, timeout: Duration) -> Option<std::process::ExitStatus> {
    let mut st = None;
    wait_until(timeout, || {
        st = child.try_wait().unwrap();
        st.is_some()
    });
    if st.is_none() {
        let _ = child.kill();
    }
    st
}

#[test]
fn suspended_daemon_exits_immediately() {
    let h = TempHome::new("disabled");
    std::fs::write(h.path().join("disabled"), "").unwrap();
    let mut d = spawn(h.path());
    let st = wait_exit(&mut d, Duration::from_secs(5)).expect("le démon suspendu ne s'est pas arrêté");
    assert!(st.success());
    assert!(!h.path().join("status.json").exists());
}

#[test]
fn lifecycle_status_reload_and_clean_stop() {
    let h = TempHome::new("lifecycle");
    target_instance(h.path());
    assert!(!instance_running());
    let cfg = h.path().join("config.toml");
    std::fs::write(&cfg, "# aucune modification demandée\n").unwrap();
    let status = h.path().join("status.json");

    let mut d = spawn(h.path());
    let started = wait_until(Duration::from_secs(10), || status.exists());
    if !started {
        let _ = d.kill();
        panic!("status.json jamais écrit");
    }
    let st: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&status).unwrap()).unwrap();
    assert_eq!(st["pid"].as_u64(), Some(d.id() as u64));
    assert!(instance_running(), "mutex d'instance absent");
    assert!(st["config_error"].is_null());

    // Deuxième instance : le mutex doit la faire sortir tout de suite, sans toucher à rien.
    let mut second = spawn(h.path());
    assert!(wait_exit(&mut second, Duration::from_secs(5)).is_some(), "deuxième instance toujours vivante");

    // Modification de config.toml : le démon réagit (événement, pas de polling) et réécrit son état.
    let before = std::fs::metadata(&status).unwrap().modified().unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    std::fs::write(&cfg, "[general]\nlog_level = \"debug\"\n").unwrap();
    let reloaded = wait_until(Duration::from_secs(10), || std::fs::metadata(&status).unwrap().modified().unwrap() > before);

    // Config invalide : signalée dans status.json, le démon continue de tourner.
    std::fs::write(&cfg, "[general\n").unwrap();
    let reported = wait_until(Duration::from_secs(10), || {
        std::fs::read_to_string(&status).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()).is_some_and(|v| !v["config_error"].is_null())
    });
    let alive = d.try_wait().unwrap().is_none();

    assert!(signal_stop(), "événement d'arrêt introuvable");
    let st = wait_exit(&mut d, Duration::from_secs(5));
    assert!(reloaded, "changement de config.toml non détecté");
    assert!(reported, "config invalide non signalée dans status.json");
    assert!(alive, "le démon s'est arrêté sur une config invalide");
    assert!(st.is_some_and(|s| s.success()), "arrêt propre impossible : {st:?}");
}
