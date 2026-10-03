//! F5 — sélection du profil actif et configuration effective.

use crate::config::{Config, ProfileEnv};
use crate::error::Result;
use crate::paths;

pub struct Resolved {
    /// Configuration telle qu'écrite dans config.toml.
    pub base: Config,
    /// Base + profil actif : c'est elle que les tweaks appliquent.
    pub effective: Config,
    pub profile: Option<String>,
    /// `true` si le profil vient d'un `apply <profil>` manuel (sinon : règle ou aucun).
    pub manual: bool,
    pub warning: Option<String>,
}

pub fn read_override() -> Option<String> {
    let s = std::fs::read_to_string(paths::profile_override()).ok()?;
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// `None` rend la main aux règles automatiques.
pub fn write_override(name: Option<&str>) -> Result<()> {
    let p = paths::profile_override();
    match name {
        Some(n) => {
            std::fs::create_dir_all(paths::data_dir())?;
            std::fs::write(p, n)?;
        }
        None => match std::fs::remove_file(p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        },
    }
    Ok(())
}

pub fn resolve(base: Config, env: &dyn ProfileEnv) -> Resolved {
    let manual = read_override();
    let (profile, warning) = base.select_profile(manual.as_deref(), env);
    let is_manual = profile.is_some() && profile == manual;
    Resolved { effective: base.effective(profile.as_deref()), base, profile, manual: is_manual, warning }
}
