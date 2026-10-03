//! `filecustomizer` — CLI : init, apply, status, restore, stop, nodes, drives, shell-extensions,
//! verbs, profiles, validate.

use fc_core::config::{Config, DEFAULT_CONFIG_TOML};
use fc_core::drives::{self, SysEnv};
use fc_core::engine::TweakDetection;
use fc_core::profile;
use fc_core::session::Session;
use fc_core::status::Status;
use fc_core::tweak::{Change, ChangeKind, ItemStatus, Report};
use fc_core::tweaks::{context_menu, navpane};
use fc_core::{fsutil, log, paths};
use std::path::PathBuf;
use std::process::{Command, ExitCode};
use windows::core::PCWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenEventW, OpenMutexW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE, INFINITE,
    SYNCHRONIZATION_SYNCHRONIZE,
};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

const HELP: &str = "\
FileCustomizer — personnalisation de la structure de l'Explorateur Windows

USAGE
  filecustomizer <commande> [options]

COMMANDES
  init                      Crée config.toml documenté (sans rien activer) s'il n'existe pas
  apply [profil]            Applique la configuration (réactive le démon si `restore` l'avait arrêté).
                            Avec un profil : le choisit à la main (supplante les règles automatiques).
  apply --auto              Rend la main aux règles automatiques
  profiles                  Liste les profils, les règles et le profil actif
  status                    État du démon, de la build Windows, du profil et de chaque tweak
  stop                      Arrête le démon (jusqu'à la prochaine ouverture de session), sans rien restaurer
  restore                   Remet EXACTEMENT l'état d'origine et suspend le démon (`apply` le réactive)
  nodes                     Nœuds du volet de navigation et leur état
  drives                    Lecteurs détectés (pour [this_pc] hide_drives)
  shell-extensions          Extensions de menu contextuel connues (pour [context_menu] blocked_extensions)
  verbs                     Verbes statiques désactivables (pour [context_menu] disabled_verbs)
  validate [fichier]        Vérifie la syntaxe d'une configuration
  debug-pin <dossier>       (diagnostic) épingle un dossier à l'Accès rapide, ou le désépingle s'il l'est déjà

OPTIONS
  --dry-run                 (apply, restore) liste ce qui serait modifié, n'écrit rien
  --elevate                 (apply) lance le helper élevé (invite UAC) pour les réglages protégés
                            (« Ce PC : dossiers/lecteurs »). `restore` le fait seul quand c'est nécessaire.
  --restart-explorer        (apply, restore) redémarre explorer.exe APRÈS les modifications.
                            Jamais fait automatiquement : action explicite uniquement.
  --config <fichier>        Utilise un autre fichier de configuration
  -h, --help                Cette aide
";

struct Opts {
    cmd: String,
    positional: Vec<String>,
    dry_run: bool,
    restart_explorer: bool,
    elevate: bool,
    auto: bool,
    config: Option<PathBuf>,
}

fn parse() -> Result<Opts, String> {
    let mut args = std::env::args().skip(1);
    let mut o = Opts {
        cmd: String::new(),
        positional: vec![],
        dry_run: false,
        restart_explorer: false,
        elevate: false,
        auto: false,
        config: None,
    };
    while let Some(a) = args.next() {
        match a.as_str() {
            "--dry-run" => o.dry_run = true,
            "--restart-explorer" => o.restart_explorer = true,
            "--elevate" => o.elevate = true,
            "--auto" => o.auto = true,
            "--config" => o.config = Some(args.next().ok_or("--config attend un fichier")?.into()),
            "-h" | "--help" | "help" => {
                o.cmd = "help".into();
                return Ok(o);
            }
            s if s.starts_with("--") => return Err(format!("option inconnue : {s}")),
            _ if o.cmd.is_empty() => o.cmd = a,
            _ => o.positional.push(a),
        }
    }
    Ok(o)
}

fn main() -> ExitCode {
    let o = match parse() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("erreur : {e}\n\n{HELP}");
            return ExitCode::from(2);
        }
    };
    // Lancé élevé (ex. par le désinstallateur) : comme le helper, toutes les lectures/écritures de
    // fichiers se font avec les droits NON élevés de l'utilisateur (voir `fsutil::as_user`).
    if let Err(e) = fsutil::drop_file_rights_to_user() {
        eprintln!("erreur : processus élevé sans jeton utilisateur exploitable ({e})");
        return ExitCode::from(4);
    }
    ExitCode::from(fsutil::as_user(|| run(&o)))
}

fn run(o: &Opts) -> u8 {
    log::init(paths::log_dir(), "cli", fc_core::config::LogLevel::Info);
    match o.cmd.as_str() {
        "" | "help" => {
            print!("{HELP}");
            0
        }
        "init" => cmd_init(),
        "apply" => cmd_apply(o),
        "profiles" => cmd_profiles(o),
        "status" => cmd_status(o),
        "restore" => cmd_restore(o),
        "stop" => cmd_stop(),
        "nodes" => cmd_nodes(),
        "drives" => cmd_drives(),
        "shell-extensions" => cmd_shell_extensions(),
        "verbs" => cmd_verbs(),
        "validate" => cmd_validate(o),
        "debug-pin" => cmd_debug_pin(o),
        other => {
            eprintln!("commande inconnue : {other}\n\n{HELP}");
            2
        }
    }
}

fn config_path(o: &Opts) -> PathBuf {
    o.config.clone().unwrap_or_else(paths::config_path)
}

fn load_config(o: &Opts) -> Result<Config, u8> {
    Config::load(&config_path(o)).map_err(|e| {
        eprintln!("{} : {e}", config_path(o).display());
        1
    })
}

fn open_session() -> Result<Session, u8> {
    Session::open().map_err(|e| {
        eprintln!("{e}");
        1
    })
}

fn wide(name: &str) -> Vec<u16> {
    name.encode_utf16().chain([0]).collect()
}

fn daemon_running() -> bool {
    let name = wide(&paths::mutex_name());
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

fn signal_daemon_stop() -> bool {
    let name = wide(&paths::stop_event_name());
    // SAFETY: `name` est un nom UTF-16 terminé par NUL, vivant jusqu'à la fin de l'appel ; le handle obtenu est refermé aussitôt.
    unsafe {
        match OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(name.as_ptr())) {
            Ok(h) => {
                let ok = SetEvent(h).is_ok();
                let _ = CloseHandle(h);
                ok
            }
            Err(_) => false,
        }
    }
}

fn print_changes(changes: &[Change]) {
    for c in changes {
        let tag = match c.kind {
            ChangeKind::Applied => "[appliqué]    ",
            ChangeKind::Reverted => "[restauré]    ",
            ChangeKind::Unchanged => "[inchangé]    ",
            ChangeKind::WouldApply => "[à appliquer] ",
            ChangeKind::WouldRevert => "[à restaurer] ",
            ChangeKind::Skipped => "[ignoré]      ",
            ChangeKind::Conflict => "[CONFLIT]     ",
            ChangeKind::Error => "[ERREUR]      ",
        };
        println!("{tag}{} : {}", c.what, c.detail);
    }
}

fn exit_for(r: &Report) -> u8 {
    u8::from(r.has_errors())
}

fn restart_explorer() {
    println!("Redémarrage d'explorer.exe (demandé explicitement)...");
    let _ = Command::new("taskkill").args(["/f", "/im", "explorer.exe"]).output();
    // Windows relance normalement le shell tout seul ; on ne le force que s'il tarde.
    std::thread::sleep(std::time::Duration::from_secs(3));
    let running = Command::new("tasklist")
        .args(["/fi", "imagename eq explorer.exe", "/nh"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains("explorer.exe"))
        .unwrap_or(false);
    if !running {
        let _ = Command::new("explorer.exe").spawn();
    }
}

/// Lance `filecustomizer-elevated.exe <verbe>` avec l'invite UAC, attend sa fin et affiche son rapport.
/// Retourne `None` si l'utilisateur refuse l'invite ou si le helper est introuvable.
fn run_elevated(verb: &str, dry_run: bool) -> Option<bool> {
    let exe = std::env::current_exe().ok()?.with_file_name("filecustomizer-elevated.exe");
    if !exe.exists() {
        eprintln!("helper introuvable : {}", exe.display());
        return None;
    }
    let _ = std::fs::remove_file(paths::elevated_result());
    let code = if fsutil::process_elevated() {
        // Déjà élevé (désinstallateur) : pas d'invite, lancement direct. Le processus créé hérite du
        // jeton principal élevé, même si ce thread emprunte l'identité de l'utilisateur.
        let mut cmd = Command::new(&exe);
        cmd.arg(verb).arg("--home").arg(paths::data_dir());
        if dry_run {
            cmd.arg("--dry-run");
        }
        match cmd.status() {
            Ok(s) => s.code().unwrap_or(1) as u32,
            Err(e) => {
                eprintln!("helper impossible à lancer : {e}");
                return None;
            }
        }
    } else {
        launch_with_uac(&exe, verb, dry_run)?
    };
    match code {
        3 => eprintln!(
            "le helper élevé a refusé {} : lien ou point de jonction dans le dossier de données.",
            paths::data_dir().display()
        ),
        4 => eprintln!(
            "le helper élevé n'a pas pu reprendre les droits de l'utilisateur de la session : validez \
             l'invite UAC avec ce même compte (administrateur), l'Explorateur étant lancé."
        ),
        _ => {}
    }
    if let Ok(s) = std::fs::read_to_string(paths::elevated_result()) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            if let Ok(ch) = serde_json::from_value::<Vec<Change>>(v["changes"].clone()) {
                print_changes(&ch);
            }
        }
    }
    Some(code == 0)
}

/// `ShellExecuteExW` + verbe `runas` : invite UAC. `None` si refusée.
fn launch_with_uac(exe: &std::path::Path, verb: &str, dry_run: bool) -> Option<u32> {
    let file = wide(&exe.to_string_lossy());
    // Un `\` final échapperait le guillemet fermant (règles de CommandLineToArgvW).
    let home = paths::data_dir().display().to_string();
    let mut params = format!("{verb} --home \"{}\"", home.trim_end_matches('\\'));
    if dry_run {
        params.push_str(" --dry-run");
    }
    let params = wide(&params);
    let verb_w = wide("runas");
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: PCWSTR(verb_w.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    println!("Élévation demandée (invite UAC) pour les réglages protégés (HKLM, Policies)...");
    // SAFETY: `info` et les chaînes qu'il pointe (`file`, `params`, `verb_w`) vivent pendant tout le bloc ; `hProcess` n'est utilisé qu'après un `ShellExecuteExW` réussi (SEE_MASK_NOCLOSEPROCESS) puis refermé.
    unsafe {
        if ShellExecuteExW(&mut info).is_err() {
            eprintln!("élévation refusée ou impossible : rien n'a été écrit en HKLM.");
            return None;
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code = 1u32;
        let _ = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = CloseHandle(info.hProcess);
        Some(code)
    }
}

fn cmd_init() -> u8 {
    let p = paths::config_path();
    if p.exists() {
        println!("{} existe déjà : inchangé.", p.display());
        return 0;
    }
    if let Err(e) = std::fs::create_dir_all(paths::data_dir()).and_then(|_| std::fs::write(&p, DEFAULT_CONFIG_TOML)) {
        eprintln!("écriture impossible : {e}");
        return 1;
    }
    println!(
        "Configuration créée : {}\nElle ne modifie rien tant que vous n'avez pas décommenté des options.",
        p.display()
    );
    0
}

fn cmd_apply(o: &Opts) -> u8 {
    let base = match load_config(o) {
        Ok(c) => c,
        Err(c) => return c,
    };
    // Choix de profil : `apply <profil>` (manuel), `apply --auto` (règles), sinon l'existant.
    let requested = o.positional.first().cloned();
    if let Some(p) = &requested {
        if !base.profiles.contains_key(p) {
            eprintln!(
                "Profil « {p} » inconnu. Profils définis : {}",
                base.profiles.keys().cloned().collect::<Vec<_>>().join(", ")
            );
            return 2;
        }
        if !o.dry_run {
            if let Err(e) = profile::write_override(Some(p)) {
                eprintln!("{e}");
                return 1;
            }
        }
    } else if o.auto && !o.dry_run {
        let _ = profile::write_override(None);
    }
    let mut res = profile::resolve(base, &SysEnv);
    if o.dry_run {
        // En dry-run on n'écrit pas le choix de profil : on simule son effet.
        if let Some(p) = requested {
            res.effective = res.base.effective(Some(&p));
            res.profile = Some(p);
            res.manual = true;
        }
    }
    if let Some(w) = &res.warning {
        eprintln!("avertissement : {w}");
    }
    match &res.profile {
        Some(p) => {
            println!("Profil actif : {p}{}", if res.manual { " (choix manuel)" } else { " (règle automatique)" })
        }
        None => println!("Profil actif : aucun (configuration de base)"),
    }
    let mut s = match open_session() {
        Ok(s) => s,
        Err(c) => return c,
    };
    if !o.dry_run {
        // `apply` annule un `restore` précédent : le démon reprend la main au prochain démarrage.
        let _ = std::fs::remove_file(paths::disabled_marker());
    }
    let r = s.apply(&res.effective, o.dry_run);
    print_changes(&r.changes);
    let mut code = exit_for(&r);
    let needs_elevation = r.needs_elevation();
    if needs_elevation && !o.dry_run {
        if o.elevate {
            if run_elevated("apply", false) != Some(true) {
                code = 1;
            }
        } else {
            println!("\nDes réglages protégés sont en attente : relancez avec `apply --elevate` (invite UAC).");
        }
    }
    if o.dry_run {
        println!("\n(--dry-run : rien n'a été écrit)");
    } else {
        log::log(
            fc_core::config::LogLevel::Info,
            &format!(
                "apply CLI : {} changement(s)",
                r.changes.iter().filter(|c| c.kind == ChangeKind::Applied).count()
            ),
        );
        if !daemon_running() {
            println!("\nNote : le démon n'est pas lancé ; les modifications ne seront pas maintenues. Voir `scripts\\install.ps1`.");
        }
        if o.restart_explorer {
            restart_explorer();
        }
    }
    code
}

fn cmd_profiles(o: &Opts) -> u8 {
    let base = match load_config(o) {
        Ok(c) => c,
        Err(c) => return c,
    };
    let r = profile::resolve(base, &SysEnv);
    if r.base.profiles.is_empty() {
        println!("Aucun profil défini. Voir la section [profiles.<nom>.*] de config.toml (`filecustomizer init`).");
        return 0;
    }
    println!("Profils :");
    for (name, p) in &r.base.profiles {
        let mut sections = Vec::new();
        if p.navigation_pane.is_some() {
            sections.push("navigation_pane");
        }
        if p.quick_access.is_some() {
            sections.push("quick_access");
        }
        if p.this_pc.is_some() {
            sections.push("this_pc");
        }
        if p.explorer_view.is_some() {
            sections.push("explorer_view");
        }
        if p.context_menu.is_some() {
            sections.push("context_menu");
        }
        let mark = if r.profile.as_deref() == Some(name) { "*" } else { " " };
        println!(" {mark} {name:<16} remplace : {}", sections.join(", "));
    }
    println!("\nRègles automatiques :");
    if r.base.rules.is_empty() {
        println!("  (aucune)");
    }
    for (i, rule) in r.base.rules.iter().enumerate() {
        let mut conds = Vec::new();
        if let Some(d) = &rule.when.drive_present {
            conds.push(format!("lecteur {d} présent"));
        }
        if let Some(d) = &rule.when.drive_absent {
            conds.push(format!("lecteur {d} absent"));
        }
        let cond = if conds.is_empty() { "(condition vide : ignorée)".to_string() } else { conds.join(" et ") };
        println!("  {}. si {cond} -> {}", i + 1, rule.profile);
    }
    match (&r.profile, r.manual) {
        (Some(p), true) => println!("\nProfil actif : {p} (choix manuel ; `apply --auto` rend la main aux règles)"),
        (Some(p), false) => println!("\nProfil actif : {p} (règle automatique)"),
        (None, _) => println!("\nProfil actif : aucun (configuration de base)"),
    }
    if let Some(w) = r.warning {
        println!("avertissement : {w}");
    }
    0
}

fn cmd_stop() -> u8 {
    if signal_daemon_stop() {
        println!("Arrêt du démon demandé.");
    } else {
        println!("Le démon n'est pas en cours d'exécution.");
    }
    0
}

fn cmd_restore(o: &Opts) -> u8 {
    let cfg = Config::default();
    let mut s = match open_session() {
        Ok(s) => s,
        Err(c) => return c,
    };
    if !o.dry_run {
        // Suspendre d'abord : sinon le démon réappliquerait la config en nous voyant restaurer.
        let _ = std::fs::create_dir_all(paths::data_dir());
        let _ = std::fs::write(
            paths::disabled_marker(),
            "FileCustomizer suspendu par `restore`. `filecustomizer apply` le réactive.\n",
        );
        if signal_daemon_stop() {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
    let r = s.revert(&cfg, o.dry_run);
    print_changes(&r.changes);
    let mut code = exit_for(&r);
    // Les réglages HKLM ne se restaurent que par le helper élevé. L'utilisateur vient de demander
    // explicitement la restauration complète : l'invite UAC est la suite logique.
    let hklm_left = s.backup.data.entries.iter().any(|e| fc_core::engine::needs_elevation_for(&e.tweak));
    if hklm_left && !o.dry_run && run_elevated("restore", false) != Some(true) {
        eprintln!("Les réglages HKLM n'ont pas été restaurés ; ils restent dans backup.json (relancez `restore`).");
        code = 1;
    }
    if o.dry_run {
        println!("\n(--dry-run : rien n'a été écrit)");
    } else {
        if code != 0 {
            eprintln!("\nCertaines valeurs n'ont pas pu être restaurées ; elles restent dans backup.json pour un nouvel essai.");
        } else {
            println!("\nÉtat d'origine restauré. Le démon est suspendu (`filecustomizer apply` pour le réactiver).");
        }
        if o.restart_explorer {
            restart_explorer();
        }
    }
    code
}

fn print_detection(d: &[TweakDetection]) {
    for t in d {
        let state = if !t.requested {
            "non demandé"
        } else if !t.tested_on_build {
            "NON VALIDÉ sur cette build (désactivé)"
        } else if t.items.iter().all(|i| i.status == ItemStatus::Ok) {
            "conforme"
        } else if t.meta.needs_elevation {
            "à appliquer (élévation requise)"
        } else {
            "à appliquer"
        };
        println!("\n* {} [{}] — {state}", t.meta.name, t.meta.id);
        if let Some(e) = &t.error {
            println!("    erreur de détection : {e}");
        }
        for i in &t.items {
            let mark = if i.status == ItemStatus::Ok { "ok " } else { "!! " };
            println!("    {mark}{} (actuel {} / voulu {})", i.label, i.current, i.desired);
        }
    }
}

fn cmd_status(o: &Opts) -> u8 {
    let (base, cfg_err) = match Config::load(&config_path(o)) {
        Ok(c) => (c, None),
        Err(e) => (Config::default(), Some(e.to_string())),
    };
    let res = profile::resolve(base, &SysEnv);
    let mut s = match open_session() {
        Ok(s) => s,
        Err(c) => return c,
    };
    println!("Windows build  : {}", s.build);
    let quiet = res.base.is_noop() && res.base.profiles.is_empty();
    println!(
        "Configuration  : {}{}",
        config_path(o).display(),
        if quiet { " (aucune modification demandée)" } else { "" }
    );
    if let Some(e) = cfg_err {
        println!("  ERREUR de configuration : {e}");
    }
    match &res.profile {
        Some(p) => println!("Profil actif   : {p}{}", if res.manual { " (manuel)" } else { " (règle)" }),
        None => println!("Profil actif   : aucun"),
    }
    if let Some(w) = &res.warning {
        println!("  avertissement : {w}");
    }
    println!("Démon          : {}", if daemon_running() { "en cours d'exécution" } else { "arrêté" });
    if paths::disabled_marker().exists() {
        println!("  suspendu par `restore` (marqueur {})", paths::disabled_marker().display());
    }
    println!(
        "Backup         : {} valeur(s), {} épingle(s) retirée(s)",
        s.backup.data.entries.len(),
        s.backup.data.unpinned_quick_access.len()
    );
    if let Some(st) = Status::load(&paths::status_path()) {
        println!("Dernière application par le démon : {}", st.last_apply_at);
        if let Some(e) = st.config_error {
            println!("  config rejetée par le démon : {e}");
        }
        for c in st.conflicts {
            println!("  CONFLIT : {c}");
        }
    }
    let d = s.detect(&res.effective);
    print_detection(&d);
    0
}

fn cmd_nodes() -> u8 {
    let s = match open_session() {
        Ok(s) => s,
        Err(c) => return c,
    };
    let nodes = match navpane::discover_nodes(s.registry(), s.shell()) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    println!("{:<28} {:<40} {:<9} OVERRIDE HKCU", "NOM", "CLSID", "EFFECTIF");
    for n in nodes {
        let eff = n.effective.map(|v| if v == 0 { "masqué" } else { "affiché" }).unwrap_or("?");
        let ov = n.hkcu_override.map(|v| v.to_string()).unwrap_or_else(|| "-".into());
        println!("{:<28} {:<40} {:<9} {ov}", n.name, n.clsid, eff);
    }
    println!("\nUtilisation : [navigation_pane.nodes] \"<nom ou CLSID>\" = \"hide\" | \"show\" dans config.toml");
    0
}

fn cmd_drives() -> u8 {
    println!("{:<7} {:<10} ÉTIQUETTE", "LECTEUR", "TYPE");
    for d in drives::list_drives() {
        println!("{:<7} {:<10} {}", format!("{}:", d.letter), d.kind, d.label);
    }
    println!("\nUtilisation : [this_pc] hide_drives = [\"D\"]  (masque dans l'Explorateur, n'empêche pas l'accès par chemin)");
    0
}

fn cmd_shell_extensions() -> u8 {
    let s = match open_session() {
        Ok(s) => s,
        Err(c) => return c,
    };
    match context_menu::discover_extensions(s.registry()) {
        Ok(v) => {
            println!("{:<44} {:<9} NOM", "CLSID", "BLOQUÉE");
            for e in v {
                println!("{:<44} {:<9} {}", e.clsid, if e.blocked { "oui" } else { "-" }, e.name);
            }
            println!("\nUtilisation : [context_menu] blocked_extensions = [\"<nom ou CLSID>\"]");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn cmd_verbs() -> u8 {
    let s = match open_session() {
        Ok(s) => s,
        Err(c) => return c,
    };
    match context_menu::discover_verbs(s.registry()) {
        Ok(v) => {
            for p in v {
                println!("{p}");
            }
            println!("\nUtilisation : [context_menu] disabled_verbs = ['Directory\\shell\\cmd']");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn cmd_validate(o: &Opts) -> u8 {
    let p = o.positional.first().map(PathBuf::from).unwrap_or_else(|| config_path(o));
    match std::fs::read_to_string(&p)
        .map_err(|e| e.to_string())
        .and_then(|s| Config::from_toml(&s).map_err(|e| e.to_string()))
    {
        Ok(c) => {
            println!(
                "{} : valide{}",
                p.display(),
                if c.is_noop() && c.profiles.is_empty() { " (ne modifie rien)" } else { "" }
            );
            for r in &c.rules {
                if !c.profiles.contains_key(&r.profile) {
                    println!("  avertissement : règle vers le profil inconnu « {} »", r.profile);
                }
            }
            0
        }
        Err(e) => {
            eprintln!("{} : {e}", p.display());
            1
        }
    }
}

/// Diagnostic / tests d'intégration : bascule l'épinglage d'un dossier via la commande Windows.
fn cmd_debug_pin(o: &Opts) -> u8 {
    use fc_core::shell::ShellBackend;
    let Some(p) = o.positional.first() else {
        eprintln!("usage : filecustomizer debug-pin <dossier>");
        return 2;
    };
    let s = match open_session() {
        Ok(s) => s,
        Err(c) => return c,
    };
    let norm = |x: &str| x.trim_end_matches('\\').to_lowercase();
    let pinned = s
        .shell()
        .quick_access_items()
        .map(|v| v.iter().any(|i| i.pinned && norm(&i.parsing_name) == norm(p)))
        .unwrap_or(false);
    match if pinned { s.shell().unpin(p) } else { s.shell().pin(p) } {
        Ok(()) => {
            println!("{p} : {}", if pinned { "désépinglé" } else { "épinglé" });
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}
