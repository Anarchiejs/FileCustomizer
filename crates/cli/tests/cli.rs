//! Tests d'intégration du binaire `filecustomizer`, sur un dossier de données isolé
//! (`FILECUSTOMIZER_HOME`). Uniquement des commandes qui n'écrivent pas dans le registre :
//! `init`, `validate`, `--dry-run`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct TempHome(PathBuf);

impl TempHome {
    fn new(name: &str) -> Self {
        let p = std::env::temp_dir().join(format!("fc-cli-{name}-{}", std::process::id()));
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

fn run(home: &TempHome, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_filecustomizer"))
        .args(args)
        .env("FILECUSTOMIZER_HOME", home.path())
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn help_lists_commands() {
    let h = TempHome::new("help");
    let o = run(&h, &["--help"]);
    assert!(o.status.success());
    for cmd in ["init", "apply", "restore", "status", "validate"] {
        assert!(stdout(&o).contains(cmd), "aide sans « {cmd} »");
    }
}

#[test]
fn init_creates_a_noop_config_once() {
    let h = TempHome::new("init");
    assert!(run(&h, &["init"]).status.success());
    let cfg = h.path().join("config.toml");
    let first = std::fs::read_to_string(&cfg).unwrap();

    std::fs::write(&cfg, format!("{first}\n# modifié par l'utilisateur\n")).unwrap();
    let o = run(&h, &["init"]);
    assert!(o.status.success());
    assert!(
        std::fs::read_to_string(&cfg).unwrap().ends_with("# modifié par l'utilisateur\n"),
        "init a écrasé la config"
    );

    // La config générée ne doit rien modifier tant que l'utilisateur ne l'a pas éditée.
    let o = run(&h, &["validate"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(stdout(&o).contains("ne modifie rien"));
}

#[test]
fn validate_rejects_broken_toml() {
    let h = TempHome::new("validate");
    let bad = h.path().join("bad.toml");
    std::fs::write(&bad, "[navpane\nhide = ").unwrap();
    let o = run(&h, &["validate", bad.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
}

#[test]
fn unknown_profile_is_refused() {
    let h = TempHome::new("profile");
    assert!(run(&h, &["init"]).status.success());
    let o = run(&h, &["apply", "inexistant", "--dry-run"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(!h.path().join("profile").exists());
}

#[test]
fn dry_run_writes_nothing() {
    let h = TempHome::new("dryrun");
    // Demande réelle (HKLM) : en dry-run elle doit seulement être listée.
    std::fs::write(h.path().join("config.toml"), "[this_pc]\nhide_folders = [\"videos\"]\n").unwrap();
    let o = run(&h, &["apply", "--dry-run"]);
    assert!(o.status.success(), "{}\n{}", stdout(&o), String::from_utf8_lossy(&o.stderr));
    assert!(!h.path().join("backup.json").exists(), "dry-run a créé un backup");
    assert!(!h.path().join("disabled").exists());

    let o = run(&h, &["restore", "--dry-run"]);
    assert!(o.status.success());
    assert!(!h.path().join("disabled").exists(), "restore --dry-run a suspendu le démon");
}
