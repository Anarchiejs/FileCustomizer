//! `status.json` : ce que le démon a fait en dernier, lisible par le CLI et l'UI.

use crate::error::Result;
use crate::tweak::Change;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Status {
    pub pid: u32,
    pub version: String,
    pub windows_build: u32,
    pub started_at: String,
    pub last_apply_at: String,
    /// Erreur de lecture de la config (l'ancienne config reste active).
    pub config_error: Option<String>,
    /// Profil actif (manuel ou règle), `None` = configuration de base.
    pub profile: Option<String>,
    /// Valeurs que nous avons cessé de réécrire car un autre outil les modifie en boucle.
    pub conflicts: Vec<String>,
    pub last_changes: Vec<Change>,
}

impl Status {
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn load(path: &Path) -> Option<Self> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }
}
