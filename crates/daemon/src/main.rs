//! Démon résident ExplorerBender.
//!
//! Un seul thread, une seule attente noyau (`MsgWaitForMultipleObjects`) sur :
//!   - notifications de registre (`RegNotifyChangeKeyValue`),
//!   - changements de dossiers (`ReadDirectoryChangesW`) : fichier de l'Accès rapide, config.toml,
//!   - minuterie de regroupement (debounce),
//!   - événement d'arrêt demandé par le CLI,
//!   - messages de la fenêtre cachée (`TaskbarCreated` = explorer.exe (re)démarré, fin de session).
//!
//! Au repos : aucune instruction exécutée, aucun timer périodique.

#![windows_subsystem = "windows"]

mod watch;

use eb_core::config::Config;
use eb_core::drives::SysEnv;
use eb_core::engine;
use eb_core::profile;
use eb_core::session::Session;
use eb_core::status::Status;
use eb_core::tweak::{ChangeKind, Report, WatchTarget};
use eb_core::{log, log_debug, log_error, log_info, log_warn, paths};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;
use watch::{DirWatch, RegWatch};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::ProcessStatus::EmptyWorkingSet;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::*;

static TASKBAR_CREATED_MSG: AtomicU32 = AtomicU32::new(0);
const FLAG_TASKBAR: u32 = 1;
const FLAG_END_SESSION: u32 = 2;
const FLAG_DEVICE: u32 = 4;
static FLAGS: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg != 0 && msg == TASKBAR_CREATED_MSG.load(Ordering::Relaxed) {
        // explorer.exe vient de (re)créer la barre des tâches : il a pu réinitialiser des choses.
        FLAGS.fetch_or(FLAG_TASKBAR, Ordering::Relaxed);
        return LRESULT(0);
    }
    match msg {
        WM_QUERYENDSESSION => LRESULT(1),
        // Un lecteur apparaît/disparaît (DBT_DEVICEARRIVAL 0x8000 / DBT_DEVICEREMOVECOMPLETE 0x8004) :
        // diffusé aux fenêtres de haut niveau sans inscription. Sert aux règles `drive_present/absent`.
        WM_DEVICECHANGE => {
            if wp.0 == 0x8000 || wp.0 == 0x8004 {
                FLAGS.fetch_or(FLAG_DEVICE, Ordering::Relaxed);
            }
            LRESULT(1)
        }
        WM_ENDSESSION => {
            if wp.0 != 0 {
                FLAGS.fetch_or(FLAG_END_SESSION, Ordering::Relaxed);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Fenêtre de haut niveau CACHÉE (jamais affichée) : une fenêtre « message-only » ne reçoit pas
/// les diffusions comme `TaskbarCreated`.
fn create_hidden_window() -> Option<HWND> {
    // SAFETY: classe et titre sont des chaînes statiques (`w!`) ; `wnd_proc` a la signature attendue par Win32.
    unsafe {
        TASKBAR_CREATED_MSG.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Relaxed);
        let hinst = GetModuleHandleW(None).ok()?;
        let class = w!("ExplorerBenderHidden");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinst.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("ExplorerBender"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinst.into()),
            None,
        )
        .ok()
    }
}

enum Source {
    Stop,
    Timer,
    Config,
    Reg(usize),
    Qa(usize),
}

struct Daemon {
    session: Session,
    /// Configuration EFFECTIVE (base + profil actif) : celle que les tweaks appliquent.
    cfg: Config,
    /// config.toml tel qu'écrit (pour réévaluer les règles quand un lecteur change).
    base: Config,
    profile: Option<String>,
    cfg_error: Option<String>,
    started_at: String,
    reg_watches: Vec<RegWatch>,
    qa_watches: Vec<DirWatch>,
    cfg_watch: Option<DirWatch>,
    timer: HANDLE,
    timer_armed: bool,
    /// Un événement lié à notre propre action est ignoré jusqu'à cet instant.
    ignore_until: Option<Instant>,
    /// La passe rapide du démarrage a modifié le registre ; la notification Shell suit dans la passe complète.
    notify_pending: bool,
}

impl Daemon {
    fn arm_timer(&mut self, ms: u32) {
        if self.timer_armed {
            return; // la passe déjà programmée verra l'état le plus récent
        }
        let due = -(ms as i64) * 10_000; // 100 ns, négatif = relatif
                                         // SAFETY: `self.timer` est un handle de timer valide ; `due` vit pendant l'appel.
        unsafe {
            let _ = SetWaitableTimer(self.timer, &due, 0, None, None, false);
        }
        self.timer_armed = true;
    }

    fn rebuild_watches(&mut self) {
        self.reg_watches.clear();
        self.qa_watches.clear();
        for t in engine::collect_watch(&self.cfg) {
            match t {
                WatchTarget::RegKey { key, subtree } => match RegWatch::open(&key, subtree) {
                    Some(w) => {
                        log_debug!("surveille le registre : {}", w.label);
                        self.reg_watches.push(w)
                    }
                    None => log_warn!("impossible de surveiller {}", key.display()),
                },
                WatchTarget::DirFile { dir, file } => match DirWatch::open(&dir, &[&file]) {
                    Some(w) => {
                        log_debug!("surveille le dossier : {}", w.label);
                        self.qa_watches.push(w)
                    }
                    None => log_warn!("impossible de surveiller {}", dir.display()),
                },
            }
        }
    }

    /// Choisit le profil (manuel ou règle) pour `base` et en déduit la configuration effective.
    fn select(&mut self, base: Config) {
        let r = profile::resolve(base, &SysEnv);
        if let Some(w) = &r.warning {
            log_warn!("profils : {w}");
        }
        if r.profile != self.profile {
            log_info!("profil actif : {}", r.profile.as_deref().unwrap_or("aucun (base)"));
        }
        self.cfg = r.effective;
        self.base = r.base;
        self.profile = r.profile;
    }

    fn reload_config(&mut self) {
        match Config::load(&paths::config_path()) {
            Ok(c) => {
                log::set_level(c.general.log_level);
                self.select(c);
                self.cfg_error = None;
                // L'utilisateur vient de changer d'avis : on retente les clés en conflit.
                self.session.guard.reset();
                log_info!("configuration rechargée");
            }
            Err(e) => {
                // Une erreur de frappe ne doit jamais défaire l'état courant : ancienne config conservée.
                log_error!("config.toml rejeté, ancienne configuration conservée : {e}");
                self.cfg_error = Some(e.to_string());
            }
        }
        self.rebuild_watches();
    }

    /// `fast` : registre seulement (démarrage). Le volet COM/Shell suit via la minuterie.
    fn apply(&mut self, reason: &str, fast: bool) {
        let t0 = Instant::now();
        let report =
            if fast { self.session.apply_registry_only(&self.cfg) } else { self.session.apply(&self.cfg, false) };
        let changed =
            report.changes.iter().filter(|c| c.kind == ChangeKind::Applied || c.kind == ChangeKind::Reverted).count();
        if fast && changed > 0 {
            self.notify_pending = true;
        } else if !fast && self.notify_pending {
            // (si cette passe a elle-même modifié quelque chose, le moteur a déjà notifié)
            if changed == 0 {
                self.session.notify();
            }
            self.notify_pending = false;
        }
        log_info!("application ({reason}) : {changed} modification(s) en {} ms", t0.elapsed().as_millis());
        for c in report.changes.iter().filter(|c| !matches!(c.kind, ChangeKind::Unchanged)) {
            let msg = format!("{:?} {} : {}", c.kind, c.what, c.detail);
            if matches!(c.kind, ChangeKind::Error | ChangeKind::Conflict) {
                log_warn!("{msg}");
            } else {
                log_info!("{msg}");
            }
        }
        // Nos propres modifications déclenchent des notifications ; on les laisse passer sans refaire un tour.
        if changed > 0 {
            self.ignore_until =
                Some(Instant::now() + std::time::Duration::from_millis(self.cfg.general.debounce_ms as u64 * 2));
        }
        self.write_status(&report);
        // Le démon doit rester sous quelques Mo : on rend au système les pages utilisées par COM/Shell.
        // SAFETY: le pseudo-handle du processus courant est toujours valide.
        unsafe {
            let _ = EmptyWorkingSet(GetCurrentProcess());
        }
    }

    fn write_status(&self, report: &Report) {
        let st = Status {
            pid: std::process::id(),
            version: env!("CARGO_PKG_VERSION").into(),
            windows_build: self.session.build,
            started_at: self.started_at.clone(),
            last_apply_at: log::format_utc(log::now_secs()),
            config_error: self.cfg_error.clone(),
            profile: self.profile.clone(),
            conflicts: self.session.guard.blocked(),
            last_changes: report.changes.iter().filter(|c| c.kind != ChangeKind::Unchanged).take(50).cloned().collect(),
        };
        if let Err(e) = st.save(&paths::status_path()) {
            log_warn!("status.json : {e}");
        }
    }

    fn ignoring(&self) -> bool {
        self.ignore_until.is_some_and(|t| Instant::now() < t)
    }
}

fn main() {
    let t_start = Instant::now();

    if paths::disabled_marker().exists() {
        return; // suspendu par `explorerbender restore`
    }

    // Instance unique.
    let mutex_name: Vec<u16> = paths::mutex_name().encode_utf16().chain([0]).collect();
    // SAFETY: `mutex_name` est un nom UTF-16 terminé par NUL, vivant jusqu'à la fin de l'appel ; le handle reste ouvert jusqu'à la fin de `main`.
    let _mutex = unsafe {
        let h = match CreateMutexW(None, false, PCWSTR(mutex_name.as_ptr())) {
            Ok(h) => h,
            Err(_) => return,
        };
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        h
    };

    let (base, cfg_error) = match Config::load(&paths::config_path()) {
        Ok(c) => (c, None),
        Err(e) => (Config::default(), Some(e.to_string())),
    };
    let resolved = profile::resolve(base, &SysEnv);
    let cfg = resolved.effective.clone();
    log::init(paths::log_dir(), "daemon", cfg.general.log_level);
    log_info!("démarrage v{} pid {}", env!("CARGO_PKG_VERSION"), std::process::id());
    if let Some(e) = &cfg_error {
        log_error!("config.toml invalide au démarrage : {e}");
    }

    let session = match Session::open() {
        Ok(s) => s,
        Err(e) => {
            log_error!("session impossible : {e}");
            return;
        }
    };

    let Some(_hwnd) = create_hidden_window() else {
        log_error!("fenêtre cachée impossible à créer");
        return;
    };

    let stop_name: Vec<u16> = paths::stop_event_name().encode_utf16().chain([0]).collect();
    // SAFETY: `stop_name` est un nom UTF-16 terminé par NUL, vivant jusqu'à la fin de l'appel.
    let (stop_event, timer) = unsafe {
        (
            CreateEventW(None, true, false, PCWSTR(stop_name.as_ptr())).unwrap_or_default(),
            // Auto-reset : l'état signalé est consommé par l'attente. En manual-reset le timer resterait
            // signalé après échéance et l'attente reviendrait immédiatement, en boucle.
            CreateWaitableTimerW(None, false, PCWSTR::null()).unwrap_or_default(),
        )
    };

    let mut d = Daemon {
        session,
        cfg,
        base: resolved.base,
        profile: resolved.profile,
        cfg_error,
        started_at: log::format_utc(log::now_secs()),
        reg_watches: vec![],
        qa_watches: vec![],
        cfg_watch: DirWatch::open(&paths::data_dir(), &["config.toml", "disabled", "profile"]),
        timer,
        timer_armed: false,
        ignore_until: None,
        notify_pending: false,
    };
    if d.cfg_watch.is_none() {
        log_warn!("config.toml non surveillé : les changements ne seront pris en compte qu'au prochain démarrage");
    }
    d.rebuild_watches();
    d.apply("démarrage, registre", true);
    log_info!("prêt en {} ms (registre appliqué, surveillances armées)", t_start.elapsed().as_millis());
    // Le travail Shell/COM (Accès rapide) passe après, sans retarder l'état « prêt ».
    d.arm_timer(1);

    'main: loop {
        // Table des sources : reconstruite à chaque tour car les watchers peuvent changer.
        let mut handles: Vec<HANDLE> = vec![stop_event, d.timer];
        let mut sources = vec![Source::Stop, Source::Timer];
        if let Some(w) = &d.cfg_watch {
            handles.push(w.handle());
            sources.push(Source::Config);
        }
        for (i, w) in d.reg_watches.iter().enumerate() {
            handles.push(w.handle());
            sources.push(Source::Reg(i));
        }
        for (i, w) in d.qa_watches.iter().enumerate() {
            handles.push(w.handle());
            sources.push(Source::Qa(i));
        }

        // SAFETY: `handles` ne contient que des handles d'événements possédés par le démon et encore ouverts.
        let r = unsafe { MsgWaitForMultipleObjects(Some(&handles), false, INFINITE, QS_ALLINPUT) };
        let idx = (r.0.wrapping_sub(WAIT_OBJECT_0.0)) as usize;

        if idx == handles.len() {
            // Messages de la fenêtre cachée.
            // SAFETY: pompe de messages standard sur le thread qui a créé la fenêtre cachée ; `msg` est local.
            unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        break 'main;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            let flags = FLAGS.swap(0, Ordering::Relaxed);
            if flags & FLAG_END_SESSION != 0 {
                log_info!("fin de session");
                break 'main;
            }
            if flags & FLAG_DEVICE != 0 && !d.base.rules.is_empty() {
                // Règles conditionnelles : un lecteur est apparu ou parti, le profil change peut-être.
                let r = profile::resolve(d.base.clone(), &SysEnv);
                if r.profile != d.profile {
                    d.select(d.base.clone());
                    d.rebuild_watches();
                    d.arm_timer(d.cfg.general.debounce_ms);
                }
            }
            if flags & FLAG_TASKBAR != 0 {
                log_info!("explorer.exe (re)démarré (TaskbarCreated)");
                // Nouvelle session shell : on laisse une nouvelle chance aux valeurs mises en conflit.
                d.session.guard.reset();
                // Délai : à ce stade le shell finit tout juste de s'initialiser.
                d.arm_timer(d.cfg.general.debounce_ms);
            }
            continue;
        }
        if idx >= sources.len() {
            log_error!("attente en échec ({:?}), arrêt", r);
            break;
        }

        match sources[idx] {
            Source::Stop => {
                log_info!("arrêt demandé");
                break 'main;
            }
            Source::Timer => {
                d.timer_armed = false;
                d.apply("événement", false);
            }
            Source::Config => {
                let mut changed = false;
                if let Some(w) = d.cfg_watch.as_mut() {
                    changed = w.collect();
                }
                if changed {
                    if paths::disabled_marker().exists() {
                        log_info!("marqueur « disabled » posé (restore) : arrêt");
                        break 'main;
                    }
                    d.reload_config();
                    d.arm_timer(200);
                }
            }
            Source::Reg(i) => {
                if !d.reg_watches[i].arm() {
                    // Clé supprimée ou plus accessible : on reconstruit toute la surveillance.
                    d.rebuild_watches();
                }
                // Jamais ignoré : une passe qui ne trouve aucune dérive n'écrit rien, donc pas de boucle.
                d.arm_timer(d.cfg.general.debounce_ms);
            }
            Source::Qa(i) => {
                let matched = d.qa_watches[i].collect();
                if matched && !d.ignoring() {
                    d.arm_timer(d.cfg.general.debounce_ms);
                }
            }
        }
    }

    log_info!("arrêt propre");
}
