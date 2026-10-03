//! FileCustomizer — cœur : modèle de config, registre/Shell abstraits, tweaks, backup/restore.
//! Aucune injection ni hook : uniquement registre, API Shell officielles et fichiers utilisateur.

pub mod backup;
pub mod compat;
pub mod config;
pub mod conflict;
#[cfg(windows)]
pub mod drives;
pub mod engine;
pub mod error;
pub mod fsutil;
pub mod log;
pub mod paths;
pub mod profile;
pub mod registry;
#[cfg(windows)]
pub mod session;
pub mod shell;
pub mod status;
pub mod tweak;
pub mod tweaks;

pub use error::{Error, Result};
