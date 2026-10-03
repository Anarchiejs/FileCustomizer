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

pub struct BackupStore {
    path: Option<PathBuf>,
    pub data: BackupFile,
    /// Entrées écartées par `quarantine` : jamais restaurées, mais réécrites telles quelles
    /// dans le fichier pour ne rien perdre.
    quarantined: Vec<BackupEntry>,
}

impl BackupStore {
    pub fn in_memory() -> Self {
        Self { path: None, data: BackupFile::default(), quarantined: vec![] }
    }

    pub fn load(path: &Path) -> Result<Self> {
        // Dans le helper élevé, lu avec les droits de l'utilisateur (voir `fsutil::as_user`).
        let data = match crate::fsutil::as_user(|| std::fs::read_to_string(path)) {
            Ok(s) => {
                let f: BackupFile = serde_json::from_str(&s)?;
                if f.version > BACKUP_VERSION {
                    return Err(Error::Io(format!("backup.json version {} inconnue", f.version)));
                }
                f
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BackupFile::default(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self { path: Some(path.to_path_buf()), data, quarantined: vec![] })
    }

    /// Écriture atomique (fichier temporaire + renommage) pour ne jamais laisser
    /// un backup tronqué si la session s'arrête en plein milieu.
    pub fn save(&self) -> Result<()> {
        let Some(p) = &self.path else { return Ok(()) };
        crate::fsutil::as_user(|| self.save_to(p))
    }

    fn save_to(&self, p: &Path) -> Result<()> {
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
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
        self.data.entries.iter().find(|e| &e.key == key && e.name == name)
    }

    pub fn entries_for<'a>(&'a self, tweak: &'a str) -> impl Iterator<Item = &'a BackupEntry> {
        self.data.entries.iter().filter(move |e| e.tweak == tweak)
    }

    /// Mémorise l'état actuel de `key\name` s'il n'est pas déjà géré. Sauvegarde sur disque.
    pub fn record(&mut self, reg: &dyn RegistryBackend, tweak: &str, key: &RegKey, name: &str) -> Result<()> {
        if self.find(key, name).is_some() {
            return Ok(());
        }
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
        self.data.entries.push(BackupEntry {
            tweak: tweak.to_string(),
            key: key.clone(),
            name: name.to_string(),
            existed: original.is_some(),
            original,
            created_keys: missing,
        });
        self.save()
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
        self.data.entries.retain(|e| !(&e.key == key && e.name == name));
        self.save()
    }

    pub fn note_unpinned(&mut self, parsing_name: &str) -> Result<()> {
        if !self.data.unpinned_quick_access.iter().any(|n| n.eq_ignore_ascii_case(parsing_name)) {
            self.data.unpinned_quick_access.push(parsing_name.to_string());
            self.save()?;
        }
        Ok(())
    }

    /// Retire de la liste de travail les entrées que `keep` refuse (elles ne seront plus ni
    /// lues ni restaurées par ce processus) et renvoie leur nombre.
    pub fn quarantine(&mut self, keep: impl Fn(&BackupEntry) -> bool) -> usize {
        let (ok, bad): (Vec<_>, Vec<_>) = std::mem::take(&mut self.data.entries).into_iter().partition(|e| keep(e));
        self.data.entries = ok;
        self.quarantined.extend(bad);
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
}
