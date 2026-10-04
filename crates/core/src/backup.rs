//! Sauvegarde des valeurs d'origine (`backup.json`).
//!
//! Invariants importants :
//! * on sauvegarde AVANT d'écrire, et le fichier est écrit sur disque avant la modification ;
//! * la PREMIÈRE sauvegarde d'une valeur est la seule conservée : réappliquer 10 fois ne doit
//!   pas faire « oublier » l'état d'origine, sinon `restore` rendrait notre propre valeur ;
//! * on retient aussi les clés que nous avons dû créer, pour les supprimer à la restauration
//!   (sinon `restore` ne remet pas EXACTEMENT l'état d'origine).

use crate::error::{Error, Result};
use crate::registry::{RegKey, RegValue, RegistryBackend};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const BACKUP_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BackupEntry {
    /// Identifiant du tweak qui gère cette valeur.
    pub tweak: String,
    pub key: RegKey,
    /// `""` = valeur par défaut de la clé.
    pub name: String,
    /// La valeur existait-elle avant nous ?
    pub existed: bool,
    /// Type + donnée d'origine (absent si `existed == false`).
    pub original: Option<RegValue>,
    /// Clés absentes avant nous, de la plus proche de la racine à la plus profonde.
    #[serde(default)]
    pub created_keys: Vec<RegKey>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BackupFile {
    pub version: u32,
    pub entries: Vec<BackupEntry>,
    /// Éléments de l'Accès rapide que nous avons désépinglés (nom d'analyse Shell),
    /// pour pouvoir les ré-épingler à la restauration (l'ordre d'origine est perdu).
    #[serde(default)]
    pub unpinned_quick_access: Vec<String>,
}

impl Default for BackupFile {
    fn default() -> Self {
        Self { version: BACKUP_VERSION, entries: vec![], unpinned_quick_access: vec![] }
    }
}

/// Plusieurs processus partagent ce fichier : le démon, le CLI (lancé aussi par l'interface) et le
/// helper élevé. Chaque modification est donc une lecture-modification-écriture SOUS VERROU
/// (`fsutil::lock_file`), faite à partir de l'état du disque : la copie en mémoire d'un processus
/// ne peut jamais écraser ce qu'un autre a ajouté entre-temps.
pub struct BackupStore {
    path: Option<PathBuf>,
    pub data: BackupFile,
    /// Entrées écartées par `quarantine` : jamais restaurées, mais réécrites telles quelles
    /// dans le fichier pour ne rien perdre.
    quarantined: Vec<BackupEntry>,
    /// Filtre de `quarantine`, réappliqué à chaque relecture du disque.
    keep: Option<fn(&BackupEntry) -> bool>,
    /// Renseigné si `backup.json` était illisible et que la copie de secours a pris le relais.
    pub recovered: Option<String>,
}

/// Version précédente du fichier, recopiée avant chaque sauvegarde.
fn bak_path(p: &Path) -> PathBuf {
    p.with_extension("json.bak")
}

/// `Ok(None)` : JSON invalide (réparable par la copie de secours). Une version inconnue, elle, est
/// une erreur franche : le fichier vient d'une version plus récente, on n'y touche pas.
fn parse(s: &str) -> Result<Option<BackupFile>> {
    match serde_json::from_str::<BackupFile>(s) {
        Ok(f) if f.version > BACKUP_VERSION => Err(Error::Io(format!("backup.json version {} inconnue", f.version))),
        Ok(f) => Ok(Some(f)),
        Err(_) => Ok(None),
    }
}

/// À appeler sous verrou, avec les droits de l'utilisateur. Fichier illisible : la copie de secours
/// prend le relais et le fichier abîmé est mis de côté (`.json.corrupt`), jamais effacé.
fn read_disk(path: &Path) -> Result<(BackupFile, Option<String>)> {
    let main = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((BackupFile::default(), None)),
        Err(e) => return Err(e.into()),
    };
    if let Some(f) = parse(&main)? {
        return Ok((f, None));
    }
    let why = serde_json::from_str::<BackupFile>(&main).err().map(|e| e.to_string()).unwrap_or_default();
    let bak = bak_path(path);
    match std::fs::read_to_string(&bak).ok().map(|s| parse(&s)) {
        Some(Ok(Some(f))) => {
            let aside = path.with_extension("json.corrupt");
            std::fs::rename(path, &aside)?;
            Ok((
                f,
                Some(format!(
                    "{} illisible ({why}) : copie de secours {} utilisée (elle peut avoir une modification de retard) ; fichier abîmé conservé dans {}",
                    path.display(),
                    bak.display(),
                    aside.display()
                )),
            ))
        }
        Some(Err(e)) => Err(e),
        _ => Err(Error::Io(format!("{} illisible ({why}) et aucune copie de secours valide", path.display()))),
    }
}

fn same_value(e: &BackupEntry, key: &RegKey, name: &str) -> bool {
    e.key.same(key) && e.name.eq_ignore_ascii_case(name)
}

impl BackupStore {
    pub fn in_memory() -> Self {
        Self { path: None, data: BackupFile::default(), quarantined: vec![], keep: None, recovered: None }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let mut s = Self::in_memory();
        s.path = Some(path.to_path_buf());
        s.refresh()?;
        Ok(s)
    }

    /// Relit le fichier : un autre processus a pu le modifier depuis notre dernière lecture.
    pub fn refresh(&mut self) -> Result<()> {
        let Some(p) = self.path.clone() else { return Ok(()) };
        // Dans le helper élevé, lu avec les droits de l'utilisateur (voir `fsutil::as_user`).
        crate::fsutil::as_user(|| {
            let _lock = crate::fsutil::lock_file(&p)?;
            self.reload_locked(&p)
        })
    }

    fn reload_locked(&mut self, p: &Path) -> Result<()> {
        let (file, recovered) = read_disk(p)?;
        self.set(file);
        if recovered.is_some() {
            // Réécrit tout de suite le fichier principal : le prochain lecteur ne doit pas le trouver absent.
            self.save_to(p)?;
            self.recovered = recovered;
        }
        Ok(())
    }

    fn set(&mut self, mut file: BackupFile) {
        self.quarantined.clear();
        if let Some(keep) = self.keep {
            let (ok, bad): (Vec<_>, Vec<_>) = std::mem::take(&mut file.entries).into_iter().partition(keep);
            file.entries = ok;
            self.quarantined = bad;
        }
        self.data = file;
    }

    /// Lecture-modification-écriture sous verrou, à partir de l'état DU DISQUE. `f` renvoie `true`
    /// s'il a modifié quelque chose (sinon rien n'est réécrit).
    fn mutate(&mut self, f: impl FnOnce(&mut BackupFile) -> bool) -> Result<()> {
        let Some(p) = self.path.clone() else {
            f(&mut self.data);
            return Ok(());
        };
        crate::fsutil::as_user(|| {
            let _lock = crate::fsutil::lock_file(&p)?;
            self.reload_locked(&p)?;
            if f(&mut self.data) {
                self.save_to(&p)?;
            }
            Ok(())
        })
    }

    /// Écriture atomique (fichier temporaire + renommage) pour ne jamais laisser un backup tronqué ;
    /// la version précédente est d'abord recopiée dans `.json.bak`.
    fn save_to(&self, p: &Path) -> Result<()> {
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if p.exists() {
            std::fs::copy(p, bak_path(p))?;
        }
        let tmp = p.with_extension("json.tmp");
        let json = if self.quarantined.is_empty() {
            serde_json::to_vec_pretty(&self.data)?
        } else {
            let mut all = self.data.clone();
            all.entries.extend(self.quarantined.iter().cloned());
            serde_json::to_vec_pretty(&all)?
        };
        crate::fsutil::write_durable(&tmp, &json)?;
        std::fs::rename(&tmp, p)?;
        Ok(())
    }

    pub fn find(&self, key: &RegKey, name: &str) -> Option<&BackupEntry> {
        self.data.entries.iter().find(|e| same_value(e, key, name))
    }

    pub fn entries_for<'a>(&'a self, tweak: &'a str) -> impl Iterator<Item = &'a BackupEntry> {
        self.data.entries.iter().filter(move |e| e.tweak == tweak)
    }

    /// Mémorise l'état actuel de `key\name` s'il n'est pas déjà géré. Sauvegarde sur disque.
    pub fn record(&mut self, reg: &dyn RegistryBackend, tweak: &str, key: &RegKey, name: &str) -> Result<()> {
        let original = reg.get_value(key, name)?;
        let mut missing = Vec::new();
        let mut cur = Some(key.clone());
        while let Some(k) = cur {
            if reg.key_exists(&k)? {
                break;
            }
            missing.push(k.clone());
            cur = k.parent();
        }
        missing.reverse();
        let entry = BackupEntry {
            tweak: tweak.to_string(),
            key: key.clone(),
            name: name.to_string(),
            existed: original.is_some(),
            original,
            created_keys: missing,
        };
        // Première sauvegarde gagnante, y compris face aux autres processus : l'existence est
        // vérifiée sur l'état relu sous verrou. Nous enregistrons toujours AVANT d'écrire la valeur,
        // donc une valeur déjà modifiée par un autre processus a forcément déjà son entrée.
        self.mutate(|d| {
            if d.entries.iter().any(|e| same_value(e, key, name)) {
                return false;
            }
            d.entries.push(entry);
            true
        })
    }

    /// Remet l'état d'origine d'une entrée (sans la retirer de la liste).
    pub fn restore_entry(reg: &dyn RegistryBackend, e: &BackupEntry) -> Result<()> {
        match &e.original {
            Some(v) => reg.set_value(&e.key, &e.name, v)?,
            None => reg.delete_value(&e.key, &e.name)?,
        }
        for k in e.created_keys.iter().rev() {
            reg.delete_key_if_empty(k)?;
        }
        Ok(())
    }

    pub fn remove(&mut self, key: &RegKey, name: &str) -> Result<()> {
        self.mutate(|d| {
            let n = d.entries.len();
            d.entries.retain(|e| !same_value(e, key, name));
            d.entries.len() != n
        })
    }

    pub fn note_unpinned(&mut self, parsing_name: &str) -> Result<()> {
        self.mutate(|d| {
            if d.unpinned_quick_access.iter().any(|n| n.eq_ignore_ascii_case(parsing_name)) {
                return false;
            }
            d.unpinned_quick_access.push(parsing_name.to_string());
            true
        })
    }

    /// Oublie les épingles retirées pour lesquelles `matches` est vrai (ré-épinglées, ou jamais retirées).
    pub fn forget_unpinned(&mut self, matches: impl Fn(&str) -> bool) -> Result<()> {
        self.mutate(|d| {
            let n = d.unpinned_quick_access.len();
            d.unpinned_quick_access.retain(|x| !matches(x));
            d.unpinned_quick_access.len() != n
        })
    }

    /// Retire de la liste de travail les entrées que `keep` refuse (elles ne seront plus ni
    /// lues ni restaurées par ce processus, y compris après relecture) et renvoie leur nombre.
    pub fn quarantine(&mut self, keep: fn(&BackupEntry) -> bool) -> usize {
        self.keep = Some(keep);
        let mut all = std::mem::take(&mut self.data);
        all.entries.extend(std::mem::take(&mut self.quarantined));
        self.set(all);
        self.quarantined.len()
    }

    pub fn quarantined(&self) -> &[BackupEntry] {
        &self.quarantined
    }

    pub fn is_empty(&self) -> bool {
        self.data.entries.is_empty() && self.data.unpinned_quick_access.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::MockRegistry;

    fn key() -> RegKey {
        RegKey::hkcu(r"Software\Classes\CLSID\{X}")
    }

    #[test]
    fn restores_missing_value_and_created_keys() {
        let reg = MockRegistry::new().with_key(RegKey::hkcu(r"Software\Classes\CLSID"));
        let mut b = BackupStore::in_memory();
        b.record(&reg, "t", &key(), "v").unwrap();
        reg.set_value(&key(), "v", &RegValue::Dword(0)).unwrap();
        assert!(reg.key_exists(&key()).unwrap());
        let e = b.find(&key(), "v").unwrap().clone();
        BackupStore::restore_entry(&reg, &e).unwrap();
        assert!(!reg.key_exists(&key()).unwrap(), "la clé créée par nous doit disparaître");
        assert!(reg.key_exists(&RegKey::hkcu(r"Software\Classes\CLSID")).unwrap());
    }

    #[test]
    fn restores_original_value() {
        let reg = MockRegistry::new().with_value(key(), "v", RegValue::Dword(1));
        let mut b = BackupStore::in_memory();
        b.record(&reg, "t", &key(), "v").unwrap();
        reg.set_value(&key(), "v", &RegValue::Dword(0)).unwrap();
        let e = b.find(&key(), "v").unwrap().clone();
        BackupStore::restore_entry(&reg, &e).unwrap();
        assert_eq!(reg.get_value(&key(), "v").unwrap(), Some(RegValue::Dword(1)));
    }

    #[test]
    fn first_backup_wins() {
        let reg = MockRegistry::new().with_value(key(), "v", RegValue::Dword(1));
        let mut b = BackupStore::in_memory();
        b.record(&reg, "t", &key(), "v").unwrap();
        reg.set_value(&key(), "v", &RegValue::Dword(0)).unwrap();
        b.record(&reg, "t", &key(), "v").unwrap();
        assert_eq!(b.find(&key(), "v").unwrap().original, Some(RegValue::Dword(1)));
        assert_eq!(b.data.entries.len(), 1);
    }

    #[test]
    fn quarantined_entries_are_hidden_but_kept_on_disk() {
        let dir = std::env::temp_dir().join(format!("fc-backup-q-{}", std::process::id()));
        let path = dir.join("backup.json");
        let reg =
            MockRegistry::new().with_value(key(), "v", RegValue::Dword(1)).with_value(key(), "w", RegValue::Dword(2));
        let mut b = BackupStore::load(&path).unwrap();
        b.record(&reg, "t", &key(), "v").unwrap();
        b.record(&reg, "bad", &key(), "w").unwrap();
        assert_eq!(b.quarantine(|e| e.tweak == "t"), 1);
        assert!(b.find(&key(), "w").is_none());
        b.remove(&key(), "v").unwrap();
        let on_disk = BackupStore::load(&path).unwrap();
        assert_eq!(on_disk.data.entries.len(), 1);
        assert_eq!(on_disk.data.entries[0].tweak, "bad");
        let _ = std::fs::remove_dir_all(dir);
    }

    fn temp_backup(tag: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("fc-backup-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        (dir.join("backup.json"), dir)
    }

    #[test]
    fn concurrent_writers_never_lose_entries() {
        // Deux processus (démon et helper élevé) ouverts sur le même fichier, chacun avec sa copie.
        let (path, dir) = temp_backup("multi");
        let reg =
            MockRegistry::new().with_value(key(), "a", RegValue::Dword(1)).with_value(key(), "b", RegValue::Dword(2));
        let mut daemon = BackupStore::load(&path).unwrap();
        let mut helper = BackupStore::load(&path).unwrap();
        helper.record(&reg, "thispc-folders", &key(), "a").unwrap();
        daemon.record(&reg, "navpane", &key(), "b").unwrap();
        assert_eq!(BackupStore::load(&path).unwrap().data.entries.len(), 2, "entrée de l'autre processus écrasée");
        // Retrait par l'un, puis écriture par l'autre : l'entrée retirée ne doit pas ressusciter.
        helper.remove(&key(), "a").unwrap();
        daemon.note_unpinned(r"C:\X").unwrap();
        let disk = BackupStore::load(&path).unwrap();
        assert_eq!(disk.data.entries.len(), 1);
        assert_eq!(disk.data.unpinned_quick_access, vec![r"C:\X".to_string()]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn simultaneous_writers_are_serialized() {
        // Vraie concurrence : deux fils qui écrivent en même temps, chacun avec son propre store.
        let (path, dir) = temp_backup("threads");
        let workers: Vec<_> = (0..2)
            .map(|t| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let mut reg = MockRegistry::new();
                    for i in 0..25 {
                        reg = reg.with_value(key(), &format!("{t}-{i}"), RegValue::Dword(i));
                    }
                    let mut b = BackupStore::load(&path).unwrap();
                    for i in 0..25 {
                        b.record(&reg, "t", &key(), &format!("{t}-{i}")).unwrap();
                    }
                })
            })
            .collect();
        for w in workers {
            w.join().unwrap();
        }
        assert_eq!(BackupStore::load(&path).unwrap().data.entries.len(), 50);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn first_backup_wins_across_processes() {
        let (path, dir) = temp_backup("first");
        let reg = MockRegistry::new().with_value(key(), "v", RegValue::Dword(1));
        let mut cli = BackupStore::load(&path).unwrap();
        let mut daemon = BackupStore::load(&path).unwrap();
        cli.record(&reg, "t", &key(), "v").unwrap();
        reg.set_value(&key(), "v", &RegValue::Dword(0)).unwrap(); // le CLI écrit sa valeur
        daemon.record(&reg, "t", &key(), "v").unwrap(); // le démon, en retard, voit notre valeur
        let disk = BackupStore::load(&path).unwrap();
        assert_eq!(disk.data.entries.len(), 1);
        assert_eq!(disk.data.entries[0].original, Some(RegValue::Dword(1)), "l'origine doit rester celle du CLI");
        // Casse différente = même valeur pour le registre.
        assert!(disk.find(&RegKey::hkcu(r"software\classes\clsid\{x}"), "V").is_some());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_file_falls_back_to_previous_version() {
        let (path, dir) = temp_backup("corrupt");
        let reg =
            MockRegistry::new().with_value(key(), "a", RegValue::Dword(1)).with_value(key(), "b", RegValue::Dword(2));
        let mut b = BackupStore::load(&path).unwrap();
        b.record(&reg, "t", &key(), "a").unwrap();
        b.record(&reg, "t", &key(), "b").unwrap(); // .bak = état avec la seule entrée « a »
        std::fs::write(&path, "{ tronqué").unwrap();
        let r = BackupStore::load(&path).unwrap();
        assert!(r.recovered.is_some());
        assert_eq!(r.data.entries.len(), 1);
        assert!(path.with_extension("json.corrupt").exists(), "le fichier abîmé est conservé");
        // Le fichier principal est réécrit : un second lecteur ne le trouve pas vide.
        assert_eq!(BackupStore::load(&path).unwrap().data.entries.len(), 1);
        // Sans copie de secours valide : erreur franche, rien n'est inventé.
        std::fs::write(&path, "x").unwrap();
        std::fs::write(path.with_extension("json.bak"), "y").unwrap();
        assert!(BackupStore::load(&path).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn non_ascii_hex_is_an_error_not_a_panic() {
        let s = r#"{"version":1,"entries":[{"tweak":"t","key":{"hive":"Hkcu","path":"X"},"name":"b","existed":true,"original":{"type":"REG_BINARY","data":"aéb"}}]}"#;
        assert!(serde_json::from_str::<BackupFile>(s).is_err());
    }

    #[test]
    fn quarantine_survives_reload() {
        let (path, dir) = temp_backup("qreload");
        let reg =
            MockRegistry::new().with_value(key(), "v", RegValue::Dword(1)).with_value(key(), "w", RegValue::Dword(2));
        let mut other = BackupStore::load(&path).unwrap();
        other.record(&reg, "bad", &key(), "w").unwrap();
        let mut helper = BackupStore::load(&path).unwrap();
        helper.quarantine(|e| e.tweak != "bad");
        helper.record(&reg, "t", &key(), "v").unwrap(); // relit le disque sous verrou
        assert!(helper.find(&key(), "w").is_none(), "le filtre doit être réappliqué après relecture");
        assert_eq!(BackupStore::load(&path).unwrap().data.entries.len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn json_roundtrip_with_binary() {
        let mut f = BackupFile::default();
        f.entries.push(BackupEntry {
            tweak: "t".into(),
            key: key(),
            name: "b".into(),
            existed: true,
            original: Some(RegValue::Binary(vec![0, 15, 255])),
            created_keys: vec![],
        });
        let s = serde_json::to_string(&f).unwrap();
        assert!(s.contains("000fff"));
        assert_eq!(serde_json::from_str::<BackupFile>(&s).unwrap(), f);
    }

    #[test]
    fn exotic_value_types_roundtrip_with_their_type() {
        let mut f = BackupFile::default();
        f.entries.push(BackupEntry {
            tweak: "t".into(),
            key: key(),
            name: "n".into(),
            existed: true,
            original: Some(RegValue::Raw { ty: 0, data: vec![1, 2] }),
            created_keys: vec![],
        });
        let s = serde_json::to_string(&f).unwrap();
        assert_eq!(serde_json::from_str::<BackupFile>(&s).unwrap(), f);
    }
}
