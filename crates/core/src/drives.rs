//! Lecteurs détectés (pour `hide_drives`, l'interface et les règles conditionnelles).

use crate::config::ProfileEnv;
use serde::Serialize;
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
use windows::Win32::System::Diagnostics::Debug::{SetErrorMode, SEM_FAILCRITICALERRORS};

#[derive(Clone, Debug, Serialize)]
pub struct DriveInfo {
    pub letter: String,
    pub kind: String,
    pub label: String,
}

pub fn drives_mask() -> u32 {
    // SAFETY: aucun argument, aucun invariant mémoire.
    unsafe { GetLogicalDrives() }
}

pub fn list_drives() -> Vec<DriveInfo> {
    let mask = drives_mask();
    // Pas de boîte de dialogue « Insérez un disque » pour les lecteurs sans média.
    // SAFETY: change seulement le mode d'erreur du processus ; restauré en fin de fonction.
    let old = unsafe { SetErrorMode(SEM_FAILCRITICALERRORS) };
    let out = (0..26u32)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| {
            let letter = (b'A' + i as u8) as char;
            let root: Vec<u16> = format!("{letter}:\\").encode_utf16().chain([0]).collect();
            // SAFETY: `root` est une chaîne UTF-16 terminée par NUL qui vit pendant l'appel.
            let kind = match unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) } {
                2 => "amovible",
                3 => "fixe",
                4 => "réseau",
                5 => "cd/dvd",
                6 => "ram",
                _ => "inconnu",
            };
            let mut label = String::new();
            // Les lecteurs réseau peuvent bloquer des secondes s'ils sont hors ligne : pas d'étiquette pour eux.
            if kind != "réseau" {
                let mut buf = [0u16; 261];
                // SAFETY: `root` est terminée par NUL ; `buf` est un tampon local dont la taille est transmise via la slice.
                if unsafe { GetVolumeInformationW(PCWSTR(root.as_ptr()), Some(&mut buf), None, None, None, None) }
                    .is_ok()
                {
                    let n = buf.iter().position(|c| *c == 0).unwrap_or(0);
                    label = String::from_utf16_lossy(&buf[..n]);
                }
            }
            DriveInfo { letter: letter.to_string(), kind: kind.into(), label }
        })
        .collect();
    // SAFETY: restaure la valeur renvoyée par le `SetErrorMode` précédent.
    unsafe { SetErrorMode(old) };
    out
}

/// Environnement réel des règles conditionnelles.
pub struct SysEnv;

impl ProfileEnv for SysEnv {
    fn drive_present(&self, letter: &str) -> bool {
        match crate::tweaks::thispc::drive_bit(letter) {
            Some(bit) => drives_mask() & bit != 0,
            None => false,
        }
    }
}
