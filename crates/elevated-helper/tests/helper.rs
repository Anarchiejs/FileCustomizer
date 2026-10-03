//! Tests d'intégration du helper élevé. Ils tournent SANS élévation et en `--dry-run` : on
//! vérifie ce que le helper accepterait de faire, sans rien écrire dans le registre.

use std::path::{Path, PathBuf};
use std::process::Command;

struct TempHome(PathBuf);

impl TempHome {
    fn new(name: &str) -> Self {
        let p = std::env::temp_dir().join(format!("fc-helper-{name}-{}", std::process::id()));
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

fn helper(home: &Path, verb: &str) -> (Option<i32>, serde_json::Value) {
    let _ = std::fs::remove_file(home.join("elevated-last.json"));
    let st = Command::new(env!("CARGO_BIN_EXE_filecustomizer-elevated"))
        .args([verb, "--dry-run", "--home"])
        .arg(home)
        .status()
        .unwrap();
    let report = std::fs::read_to_string(home.join("elevated-last.json"))
        .map(|s| serde_json::from_str(&s).unwrap())
        .unwrap_or(serde_json::Value::Null);
    (st.code(), report)
}

fn changes(report: &serde_json::Value) -> Vec<(String, String, String)> {
    report["changes"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| {
                    (
                        c["kind"].as_str().unwrap_or("").to_string(),
                        c["what"].as_str().unwrap_or("").to_string(),
                        c["detail"].as_str().unwrap_or("").to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

const LEGIT: &str = r#"{
  "tweak": "thispc-folders",
  "key": { "hive": "Hklm", "path": "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FolderDescriptions\\{35286a68-3c57-41a1-bbb1-0eae73d76c95}\\PropertyBag", "view": "Native" },
  "name": "ThisPCPolicy",
  "existed": true,
  "original": { "type": "REG_SZ", "data": "Show" },
  "created_keys": []
}"#;

const FORGED: &str = r#"{
  "tweak": "thispc-folders",
  "key": { "hive": "Hklm", "path": "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run", "view": "Native" },
  "name": "EvilStartup",
  "existed": true,
  "original": { "type": "REG_SZ", "data": "C:\\evil.exe" },
  "created_keys": []
}"#;

fn write_backup(home: &Path, entries: &[&str]) {
    let json = format!(r#"{{ "version": 1, "entries": [{}], "unpinned_quick_access": [] }}"#, entries.join(","));
    std::fs::write(home.join("backup.json"), json).unwrap();
}

#[test]
fn forged_backup_entry_is_never_restored() {
    let h = TempHome::new("forged");
    write_backup(h.path(), &[LEGIT, FORGED]);
    let (code, report) = helper(h.path(), "restore");
    assert_eq!(code, Some(1), "une entrée rejetée doit faire échouer le helper");
    let ch = changes(&report);

    let run_key = ch.iter().filter(|(_, what, _)| what.contains(r"CurrentVersion\Run")).collect::<Vec<_>>();
    assert!(!run_key.is_empty(), "l'entrée forgée doit être signalée : {ch:#?}");
    assert!(run_key.iter().all(|(kind, _, _)| kind == "Error"), "entrée forgée traitée : {run_key:#?}");

    // L'entrée légitime, elle, est bien restaurée (en dry-run : « would revert »).
    assert!(ch.iter().any(|(kind, what, _)| kind == "WouldRevert" && what.contains("PropertyBag")), "{ch:#?}");

    // Rien n'est perdu : backup.json garde les deux entrées.
    let on_disk = std::fs::read_to_string(h.path().join("backup.json")).unwrap();
    assert!(on_disk.contains("EvilStartup") && on_disk.contains("ThisPCPolicy"));
}

#[test]
fn legit_backup_is_accepted() {
    let h = TempHome::new("legit");
    write_backup(h.path(), &[LEGIT]);
    let (code, report) = helper(h.path(), "restore");
    assert_eq!(code, Some(0), "{report:#}");
}

#[test]
fn junction_data_dir_is_refused_before_any_write() {
    let h = TempHome::new("junction");
    let target = h.path().join("cible");
    let link = h.path().join("lien");
    std::fs::create_dir_all(&target).unwrap();
    let ok =
        Command::new("cmd").args(["/c", "mklink", "/J"]).arg(&link).arg(&target).output().unwrap().status.success();
    assert!(ok, "mklink /J impossible");

    let (code, _) = helper(&link, "restore");
    assert_eq!(code, Some(3));
    let written: Vec<_> = std::fs::read_dir(&target).unwrap().collect();
    assert!(written.is_empty(), "le helper a écrit à travers la jonction : {written:?}");
}
