//! Sources d'événements du démon. Tout est attente noyau sur des HANDLE : aucun polling.

use eb_core::registry::{Hive, RegKey, View};
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::{CreateEventW, ResetEvent};
use windows::Win32::System::IO::{CancelIo, GetOverlappedResult, OVERLAPPED};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Notification de changement du registre (une clé, éventuellement son sous-arbre).
pub struct RegWatch {
    hkey: HKEY,
    event: HANDLE,
    subtree: bool,
    pub label: String,
}

impl RegWatch {
    /// Si `key` n'existe pas (cas fréquent : nous la créons nous-mêmes, ou un autre outil la supprime),
    /// on surveille son plus proche ancêtre existant avec tout son sous-arbre.
    pub fn open(key: &RegKey, subtree: bool) -> Option<Self> {
        let mut target = key.clone();
        let mut subtree = subtree;
        loop {
            if let Some(h) = open_for_notify(&target) {
                let event = unsafe { CreateEventW(None, true, false, PCWSTR::null()).ok()? };
                let mut w = RegWatch { hkey: h, event, subtree, label: target.display() };
                if w.arm() {
                    return Some(w);
                }
                return None;
            }
            subtree = true;
            target = target.parent()?;
        }
    }

    /// (Ré)arme la notification. À appeler après chaque signal : elle est à usage unique.
    pub fn arm(&mut self) -> bool {
        unsafe {
            let _ = ResetEvent(self.event);
            RegNotifyChangeKeyValue(
                self.hkey,
                self.subtree,
                REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET,
                Some(self.event),
                true,
            ) == ERROR_SUCCESS
        }
    }

    pub fn handle(&self) -> HANDLE {
        self.event
    }
}

impl Drop for RegWatch {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.hkey);
            let _ = CloseHandle(self.event);
        }
    }
}

fn open_for_notify(key: &RegKey) -> Option<HKEY> {
    let root = match key.hive {
        Hive::Hkcu => HKEY_CURRENT_USER,
        Hive::Hklm => HKEY_LOCAL_MACHINE,
    };
    let sam = if key.view == View::Wow32 { KEY_NOTIFY | KEY_WOW64_32KEY } else { KEY_NOTIFY };
    let p = wide(&key.path);
    let mut h = HKEY::default();
    let r = unsafe { RegOpenKeyExW(root, PCWSTR(p.as_ptr()), None, sam, &mut h) };
    (r == ERROR_SUCCESS).then_some(h)
}

/// `ReadDirectoryChangesW` asynchrone sur un dossier, filtré sur une liste de noms de fichiers.
pub struct DirWatch {
    dir: HANDLE,
    // Boxé : le noyau garde un pointeur sur ces deux structures entre deux appels.
    overlapped: Box<OVERLAPPED>,
    buffer: Box<[u32; 2048]>,
    event: HANDLE,
    files: Vec<String>,
    pub label: String,
}

impl DirWatch {
    pub fn open(dir: &Path, files: &[&str]) -> Option<Box<Self>> {
        let d = wide(&dir.to_string_lossy());
        let handle = unsafe {
            CreateFileW(
                PCWSTR(d.as_ptr()),
                FILE_LIST_DIRECTORY.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                None,
            )
            .ok()?
        };
        let event = unsafe { CreateEventW(None, true, false, PCWSTR::null()).ok()? };
        let mut w = Box::new(DirWatch {
            dir: handle,
            overlapped: Box::new(OVERLAPPED::default()),
            buffer: Box::new([0u32; 2048]),
            event,
            files: files.iter().map(|f| f.to_lowercase()).collect(),
            label: dir.display().to_string(),
        });
        w.overlapped.hEvent = event;
        w.issue().then_some(w)
    }

    fn issue(&mut self) -> bool {
        unsafe {
            let _ = ResetEvent(self.event);
            ReadDirectoryChangesW(
                self.dir,
                self.buffer.as_mut_ptr() as *mut _,
                (self.buffer.len() * 4) as u32,
                false,
                FILE_NOTIFY_CHANGE_LAST_WRITE
                    | FILE_NOTIFY_CHANGE_SIZE
                    | FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_CREATION,
                None,
                Some(&mut *self.overlapped),
                None,
            )
            .is_ok()
        }
    }

    /// À appeler quand l'événement est signalé. Renvoie `true` si un fichier surveillé a changé,
    /// et relance l'écoute dans tous les cas.
    pub fn collect(&mut self) -> bool {
        let mut bytes = 0u32;
        let ok = unsafe { GetOverlappedResult(self.dir, &*self.overlapped, &mut bytes, false).is_ok() };
        let mut matched = false;
        if !ok || bytes == 0 {
            // Tampon débordé ou erreur : on ne sait pas ce qui a changé, mieux vaut revérifier.
            matched = true;
        } else {
            // FILE_NOTIFY_INFORMATION : NextEntryOffset(u32) Action(u32) FileNameLength(u32) FileName[..]
            let base = self.buffer.as_ptr() as *const u8;
            let mut off = 0usize;
            loop {
                unsafe {
                    let p = base.add(off);
                    let next = *(p as *const u32) as usize;
                    let name_len = *(p.add(8) as *const u32) as usize / 2;
                    let name = std::slice::from_raw_parts(p.add(12) as *const u16, name_len);
                    let name = String::from_utf16_lossy(name).to_lowercase();
                    if self.files.contains(&name) {
                        matched = true;
                    }
                    if next == 0 {
                        break;
                    }
                    off += next;
                }
            }
        }
        self.issue();
        matched
    }

    pub fn handle(&self) -> HANDLE {
        self.event
    }
}

impl Drop for DirWatch {
    fn drop(&mut self) {
        unsafe {
            // Annule l'E/S en cours avant de libérer les tampons que le noyau pourrait encore écrire.
            let _ = CancelIo(self.dir);
            // Attendre l'acquittement de l'annulation : le noyau écrit encore dans `overlapped`/`buffer`.
            let mut b = 0u32;
            let _ = GetOverlappedResult(self.dir, &*self.overlapped, &mut b, true);
            let _ = CloseHandle(self.dir);
            let _ = CloseHandle(self.event);
        }
    }
}
