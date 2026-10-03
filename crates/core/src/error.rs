use std::fmt;

#[derive(Debug, Clone)]
pub enum Error {
    /// Écriture refusée (typiquement HKLM sans élévation).
    AccessDenied(String),
    Io(String),
    Config(String),
    Registry(String),
    Shell(String),
    /// Un autre outil réécrit la même valeur : on s'arrête sur cette clé.
    Conflict(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::AccessDenied(m) => write!(f, "accès refusé : {m}"),
            Error::Io(m) => write!(f, "E/S : {m}"),
            Error::Config(m) => write!(f, "configuration : {m}"),
            Error::Registry(m) => write!(f, "registre : {m}"),
            Error::Shell(m) => write!(f, "shell : {m}"),
            Error::Conflict(m) => write!(f, "conflit : {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.to_string())
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Io(format!("JSON : {e}"))
    }
}
impl From<windows::core::Error> for Error {
    fn from(e: windows::core::Error) -> Self {
        Error::Shell(e.to_string())
    }
}
