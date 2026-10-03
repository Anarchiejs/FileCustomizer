//! Abstraction du registre. Tout le reste du cœur parle à `RegistryBackend`,
//! ce qui permet de tester sans toucher au vrai registre (`MockRegistry`).

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Hive {
    Hkcu,
    Hklm,
}

/// Vue du registre. `Wow32` = `KEY_WOW64_32KEY` (partie WOW6432Node sous HKLM).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub enum View {
    #[default]
    Native,
    Wow32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RegKey {
    pub hive: Hive,
    /// Chemin sans antislash initial, ex. `Software\Classes\CLSID`.
    pub path: String,
    #[serde(default)]
    pub view: View,
}

impl RegKey {
    pub fn hkcu(path: impl Into<String>) -> Self {
        Self { hive: Hive::Hkcu, path: path.into(), view: View::Native }
    }
    pub fn hklm(path: impl Into<String>) -> Self {
        Self { hive: Hive::Hklm, path: path.into(), view: View::Native }
    }
    pub fn with_view(mut self, view: View) -> Self {
        self.view = view;
        self
    }
    pub fn child(&self, name: &str) -> Self {
        Self { hive: self.hive, path: format!("{}\\{}", self.path, name), view: self.view }
    }
    /// Parent immédiat, `None` à la racine d'une ruche.
    pub fn parent(&self) -> Option<Self> {
        self.path.rsplit_once('\\').map(|(p, _)| Self {
            hive: self.hive,
            path: p.to_string(),
            view: self.view,
        })
    }
    pub fn display(&self) -> String {
        let h = match self.hive {
            Hive::Hkcu => "HKCU",
            Hive::Hklm => "HKLM",
        };
        let v = if self.view == View::Wow32 { " [32 bits]" } else { "" };
        format!("{h}\\{}{v}", self.path)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum RegValue {
    #[serde(rename = "REG_DWORD")]
    Dword(u32),
    #[serde(rename = "REG_QWORD")]
    Qword(u64),
    #[serde(rename = "REG_SZ")]
    Sz(String),
    #[serde(rename = "REG_EXPAND_SZ")]
    ExpandSz(String),
    #[serde(rename = "REG_MULTI_SZ")]
    MultiSz(Vec<String>),
    /// Encodé en hexadécimal dans backup.json.
    #[serde(rename = "REG_BINARY", with = "hex_bytes")]
    Binary(Vec<u8>),
}

impl RegValue {
    pub fn display(&self) -> String {
        match self {
            RegValue::Dword(v) => format!("dword:{v}"),
            RegValue::Qword(v) => format!("qword:{v}"),
            RegValue::Sz(s) => format!("\"{s}\""),
            RegValue::ExpandSz(s) => format!("expand:\"{s}\""),
            RegValue::MultiSz(v) => format!("multi:{v:?}"),
            RegValue::Binary(b) => format!("binary({} octets)", b.len()),
        }
    }
}

mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(b: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        let mut out = String::with_capacity(b.len() * 2);
        for x in b {
            out.push_str(&format!("{x:02x}"));
        }
        s.serialize_str(&out)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        if s.len() % 2 != 0 {
            return Err(serde::de::Error::custom("hex de longueur impaire"));
        }
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(serde::de::Error::custom))
            .collect()
    }
}

pub trait RegistryBackend {
    fn key_exists(&self, key: &RegKey) -> Result<bool>;
    /// `name == ""` désigne la valeur par défaut de la clé.
    fn get_value(&self, key: &RegKey, name: &str) -> Result<Option<RegValue>>;
    /// Crée la clé si nécessaire.
    fn set_value(&self, key: &RegKey, name: &str, value: &RegValue) -> Result<()>;
    /// Sans erreur si la valeur ou la clé n'existe pas.
    fn delete_value(&self, key: &RegKey, name: &str) -> Result<()>;
    /// Supprime la clé seulement si elle n'a ni sous-clé ni valeur. Renvoie `true` si supprimée.
    fn delete_key_if_empty(&self, key: &RegKey) -> Result<bool>;
    fn list_subkeys(&self, key: &RegKey) -> Result<Vec<String>>;
    /// Valeurs de la clé (nom, donnée). Vide si la clé n'existe pas.
    fn list_values(&self, key: &RegKey) -> Result<Vec<(String, RegValue)>>;
}

// ---------------------------------------------------------------------------
// Backend simulé (tests)
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct MockRegistry {
    keys: RefCell<BTreeMap<RegKey, BTreeMap<String, RegValue>>>,
    /// Chemins (`display()`) en écriture refusée, pour simuler HKLM sans admin.
    deny_write: RefCell<Vec<Hive>>,
    pub write_count: RefCell<usize>,
}

impl MockRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_key(self, key: RegKey) -> Self {
        self.ensure(&key);
        self
    }
    pub fn with_value(self, key: RegKey, name: &str, v: RegValue) -> Self {
        self.ensure(&key);
        self.keys.borrow_mut().get_mut(&key).unwrap().insert(name.to_string(), v);
        self
    }
    pub fn deny_writes_to(&self, hive: Hive) {
        self.deny_write.borrow_mut().push(hive);
    }
    fn ensure(&self, key: &RegKey) {
        // Crée aussi les ancêtres, comme le vrai registre.
        let mut cur = Some(key.clone());
        while let Some(k) = cur {
            self.keys.borrow_mut().entry(k.clone()).or_default();
            cur = k.parent();
        }
    }
}

impl RegistryBackend for MockRegistry {
    fn key_exists(&self, key: &RegKey) -> Result<bool> {
        Ok(self.keys.borrow().contains_key(key))
    }
    fn get_value(&self, key: &RegKey, name: &str) -> Result<Option<RegValue>> {
        Ok(self.keys.borrow().get(key).and_then(|m| m.get(name).cloned()))
    }
    fn set_value(&self, key: &RegKey, name: &str, value: &RegValue) -> Result<()> {
        if self.deny_write.borrow().contains(&key.hive) {
            return Err(Error::AccessDenied(key.display()));
        }
        self.ensure(key);
        self.keys.borrow_mut().get_mut(key).unwrap().insert(name.to_string(), value.clone());
        *self.write_count.borrow_mut() += 1;
        Ok(())
    }
    fn delete_value(&self, key: &RegKey, name: &str) -> Result<()> {
        if self.deny_write.borrow().contains(&key.hive) {
            return Err(Error::AccessDenied(key.display()));
        }
        if let Some(m) = self.keys.borrow_mut().get_mut(key) {
            if m.remove(name).is_some() {
                *self.write_count.borrow_mut() += 1;
            }
        }
        Ok(())
    }
    fn delete_key_if_empty(&self, key: &RegKey) -> Result<bool> {
        let has_sub = !self.list_subkeys(key)?.is_empty();
        let mut keys = self.keys.borrow_mut();
        match keys.get(key) {
            Some(m) if m.is_empty() && !has_sub => {
                keys.remove(key);
                Ok(true)
            }
            _ => Ok(false),
        }
    }
    fn list_values(&self, key: &RegKey) -> Result<Vec<(String, RegValue)>> {
        Ok(self
            .keys
            .borrow()
            .get(key)
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default())
    }
    fn list_subkeys(&self, key: &RegKey) -> Result<Vec<String>> {
        let prefix = format!("{}\\", key.path);
        Ok(self
            .keys
            .borrow()
            .keys()
            .filter(|k| k.hive == key.hive && k.view == key.view && k.path.starts_with(&prefix))
            .filter_map(|k| {
                let rest = &k.path[prefix.len()..];
                (!rest.contains('\\')).then(|| rest.to_string())
            })
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Backend réel (API Win32)
// ---------------------------------------------------------------------------

#[cfg(windows)]
pub use win::WinRegistry;

#[cfg(windows)]
mod win {
    use super::*;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::*;
    use windows::Win32::System::Registry::*;

    #[derive(Default)]
    pub struct WinRegistry;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn root(h: Hive) -> HKEY {
        match h {
            Hive::Hkcu => HKEY_CURRENT_USER,
            Hive::Hklm => HKEY_LOCAL_MACHINE,
        }
    }

    fn sam(view: View, base: REG_SAM_FLAGS) -> REG_SAM_FLAGS {
        match view {
            View::Native => base,
            View::Wow32 => base | KEY_WOW64_32KEY,
        }
    }

    fn map_err(code: WIN32_ERROR, key: &RegKey) -> Error {
        if code == ERROR_ACCESS_DENIED {
            Error::AccessDenied(key.display())
        } else {
            Error::Registry(format!("{} : code {}", key.display(), code.0))
        }
    }

    /// Fermeture automatique du handle.
    struct Handle(HKEY);
    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: `self.0` est une clé ouverte par `open`/`create`, refermée une seule fois ici.
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    fn open(key: &RegKey, access: REG_SAM_FLAGS) -> Result<Option<Handle>> {
        let p = wide(&key.path);
        let mut h = HKEY::default();
        // SAFETY: `p` est un chemin UTF-16 terminé par NUL ; `h` est une sortie locale valide.
        let r = unsafe {
            RegOpenKeyExW(root(key.hive), PCWSTR(p.as_ptr()), None, sam(key.view, access), &mut h)
        };
        if r == ERROR_FILE_NOT_FOUND || r == ERROR_PATH_NOT_FOUND {
            return Ok(None);
        }
        if r != ERROR_SUCCESS {
            return Err(map_err(r, key));
        }
        Ok(Some(Handle(h)))
    }

    fn create(key: &RegKey) -> Result<Handle> {
        let p = wide(&key.path);
        let mut h = HKEY::default();
        // SAFETY: `p` est un chemin UTF-16 terminé par NUL ; `h` est une sortie locale valide.
        let r = unsafe {
            RegCreateKeyExW(
                root(key.hive),
                PCWSTR(p.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                sam(key.view, KEY_SET_VALUE | KEY_QUERY_VALUE),
                None,
                &mut h,
                None,
            )
        };
        if r != ERROR_SUCCESS {
            return Err(map_err(r, key));
        }
        Ok(Handle(h))
    }

    impl RegistryBackend for WinRegistry {
        fn key_exists(&self, key: &RegKey) -> Result<bool> {
            Ok(open(key, KEY_QUERY_VALUE)?.is_some())
        }

        fn get_value(&self, key: &RegKey, name: &str) -> Result<Option<RegValue>> {
            let Some(h) = open(key, KEY_QUERY_VALUE)? else { return Ok(None) };
            let n = wide(name);
            let mut ty = REG_VALUE_TYPE(0);
            let mut len: u32 = 0;
            // SAFETY: `h` est une clé ouverte ; `n` est terminé par NUL ; on ne demande que le type et la taille.
            let r = unsafe {
                RegQueryValueExW(h.0, PCWSTR(n.as_ptr()), None, Some(&mut ty), None, Some(&mut len))
            };
            if r == ERROR_FILE_NOT_FOUND {
                return Ok(None);
            }
            if r != ERROR_SUCCESS {
                return Err(map_err(r, key));
            }
            let mut buf = vec![0u8; len as usize];
            // SAFETY: `buf` fait exactement `len` octets, taille transmise à l'API qui n'écrit pas au-delà.
            let r = unsafe {
                RegQueryValueExW(
                    h.0,
                    PCWSTR(n.as_ptr()),
                    None,
                    Some(&mut ty),
                    Some(buf.as_mut_ptr()),
                    Some(&mut len),
                )
            };
            if r != ERROR_SUCCESS {
                return Err(map_err(r, key));
            }
            buf.truncate(len as usize);
            let utf16 = |b: &[u8]| -> Vec<u16> {
                b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect()
            };
            let trim0 = |mut v: Vec<u16>| {
                while v.last() == Some(&0) {
                    v.pop();
                }
                String::from_utf16_lossy(&v)
            };
            Ok(Some(match ty {
                REG_DWORD if buf.len() >= 4 => {
                    RegValue::Dword(u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]))
                }
                REG_QWORD if buf.len() >= 8 => {
                    RegValue::Qword(u64::from_le_bytes(buf[..8].try_into().unwrap()))
                }
                REG_SZ => RegValue::Sz(trim0(utf16(&buf))),
                REG_EXPAND_SZ => RegValue::ExpandSz(trim0(utf16(&buf))),
                REG_MULTI_SZ => {
                    let s = utf16(&buf);
                    RegValue::MultiSz(
                        s.split(|c| *c == 0)
                            .filter(|p| !p.is_empty())
                            .map(String::from_utf16_lossy)
                            .collect(),
                    )
                }
                _ => RegValue::Binary(buf),
            }))
        }

        fn set_value(&self, key: &RegKey, name: &str, value: &RegValue) -> Result<()> {
            let h = create(key)?;
            let n = wide(name);
            let (ty, data): (REG_VALUE_TYPE, Vec<u8>) = match value {
                RegValue::Dword(v) => (REG_DWORD, v.to_le_bytes().to_vec()),
                RegValue::Qword(v) => (REG_QWORD, v.to_le_bytes().to_vec()),
                RegValue::Sz(s) => (REG_SZ, wide(s).iter().flat_map(|c| c.to_le_bytes()).collect()),
                RegValue::ExpandSz(s) => {
                    (REG_EXPAND_SZ, wide(s).iter().flat_map(|c| c.to_le_bytes()).collect())
                }
                RegValue::MultiSz(v) => {
                    let mut u: Vec<u16> = Vec::new();
                    for s in v {
                        u.extend(s.encode_utf16());
                        u.push(0);
                    }
                    u.push(0);
                    (REG_MULTI_SZ, u.iter().flat_map(|c| c.to_le_bytes()).collect())
                }
                RegValue::Binary(b) => (REG_BINARY, b.clone()),
            };
            // SAFETY: `h` est une clé ouverte ; `n` est terminé par NUL ; `data` est une slice dont la taille est connue.
            let r = unsafe { RegSetValueExW(h.0, PCWSTR(n.as_ptr()), None, ty, Some(&data)) };
            if r != ERROR_SUCCESS {
                return Err(map_err(r, key));
            }
            Ok(())
        }

        fn delete_value(&self, key: &RegKey, name: &str) -> Result<()> {
            let Some(h) = open(key, KEY_SET_VALUE)? else { return Ok(()) };
            let n = wide(name);
            // SAFETY: `h` est une clé ouverte ; `n` est terminé par NUL.
            let r = unsafe { RegDeleteValueW(h.0, PCWSTR(n.as_ptr())) };
            if r != ERROR_SUCCESS && r != ERROR_FILE_NOT_FOUND {
                return Err(map_err(r, key));
            }
            Ok(())
        }

        fn delete_key_if_empty(&self, key: &RegKey) -> Result<bool> {
            let Some(h) = open(key, KEY_QUERY_VALUE)? else { return Ok(false) };
            let (mut subkeys, mut values) = (0u32, 0u32);
            // SAFETY: `h` est une clé ouverte ; les pointeurs de sortie sont des variables locales.
            let r = unsafe {
                RegQueryInfoKeyW(
                    h.0,
                    None,
                    None,
                    None,
                    Some(&mut subkeys),
                    None,
                    None,
                    Some(&mut values),
                    None,
                    None,
                    None,
                    None,
                )
            };
            if r != ERROR_SUCCESS {
                return Err(map_err(r, key));
            }
            drop(h);
            if subkeys != 0 || values != 0 {
                return Ok(false);
            }
            let Some(parent) = key.parent() else { return Ok(false) };
            let leaf = key.path.rsplit('\\').next().unwrap_or_default();
            let Some(ph) = open(&parent, KEY_WRITE_ACCESS)? else { return Ok(false) };
            let l = wide(leaf);
            // SAFETY: `ph` est la clé parente ouverte ; `l` est terminé par NUL.
            let r = unsafe { RegDeleteKeyW(ph.0, PCWSTR(l.as_ptr())) };
            if r != ERROR_SUCCESS {
                return Err(map_err(r, key));
            }
            Ok(true)
        }

        fn list_values(&self, key: &RegKey) -> Result<Vec<(String, RegValue)>> {
            let Some(h) = open(key, KEY_QUERY_VALUE)? else { return Ok(vec![]) };
            let mut names = Vec::new();
            let mut i = 0u32;
            loop {
                let mut buf = vec![0u16; 16384];
                let mut len = buf.len() as u32;
                // SAFETY: `buf` est un tampon local de `len` caractères, longueur transmise à l'API.
                let r = unsafe {
                    RegEnumValueW(h.0, i, Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None)
                };
                if r == ERROR_NO_MORE_ITEMS {
                    break;
                }
                if r != ERROR_SUCCESS {
                    return Err(map_err(r, key));
                }
                names.push(String::from_utf16_lossy(&buf[..len as usize]));
                i += 1;
            }
            drop(h);
            let mut out = Vec::new();
            for n in names {
                if let Some(v) = self.get_value(key, &n)? {
                    out.push((n, v));
                }
            }
            Ok(out)
        }

        fn list_subkeys(&self, key: &RegKey) -> Result<Vec<String>> {
            let Some(h) = open(key, KEY_ENUMERATE_SUB_KEYS)? else { return Ok(vec![]) };
            let mut out = Vec::new();
            let mut i = 0u32;
            loop {
                let mut buf = [0u16; 256];
                let mut len = buf.len() as u32;
                // SAFETY: `buf` est un tampon local de `len` caractères, longueur transmise à l'API.
                let r = unsafe {
                    RegEnumKeyExW(
                        h.0,
                        i,
                        Some(windows::core::PWSTR(buf.as_mut_ptr())),
                        &mut len,
                        None,
                        None,
                        None,
                        None,
                    )
                };
                if r == ERROR_NO_MORE_ITEMS {
                    break;
                }
                if r != ERROR_SUCCESS {
                    return Err(map_err(r, key));
                }
                out.push(String::from_utf16_lossy(&buf[..len as usize]));
                i += 1;
            }
            Ok(out)
        }
    }

    // KEY_WRITE sans KEY_CREATE_LINK, suffisant pour supprimer une sous-clé.
    const KEY_WRITE_ACCESS: REG_SAM_FLAGS = KEY_SET_VALUE;
}
