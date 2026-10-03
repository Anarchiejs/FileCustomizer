//! Écritures de fichiers sûres : durables (`write_durable`) et, dans le helper élevé, faites avec
//! les droits NON élevés de l'utilisateur (`as_user`).
//!
//! Pourquoi `as_user` : le dossier de données vit dans `%APPDATA%`, que l'utilisateur (et tout
//! programme qu'il lance) contrôle. Un point de jonction posé au bon moment redirigerait une
//! écriture faite en administrateur vers `C:\Windows`, etc. Vérifier le chemin avant d'écrire
//! laisse une fenêtre de course ; écrire avec le jeton non élevé la ferme : au pire, l'écriture
//! redirigée échoue avec « accès refusé ». Seules les écritures de registre gardent l'élévation.

use std::fs::File;
use std::io::Write;
use std::path::Path;

/// Écrit `data` puis force l'écriture sur disque : combiné à un renommage, une coupure de courant
/// laisse l'ancien fichier ou le nouveau, jamais un fichier vide.
pub fn write_durable(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut f = File::create(path)?;
    f.write_all(data)?;
    f.sync_all()
}

#[cfg(windows)]
pub use win::{as_user, drop_file_rights_to_user, lock_file, process_elevated, FileLock};

#[cfg(not(windows))]
pub fn as_user<R>(f: impl FnOnce() -> R) -> R {
    f()
}

#[cfg(not(windows))]
pub struct FileLock;
#[cfg(not(windows))]
pub fn lock_file(_path: &Path) -> std::io::Result<FileLock> {
    Ok(FileLock)
}

#[cfg(windows)]
mod win {
    use std::cell::Cell;
    use std::sync::atomic::{AtomicIsize, Ordering};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0};
    use windows::Win32::Security::*;
    use windows::Win32::System::Threading::{
        CreateMutexW, GetCurrentProcess, OpenProcess, OpenProcessToken, ReleaseMutex, WaitForSingleObject,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId};

    /// Jeton d'emprunt d'identité non élevé (0 = aucun : on écrit avec nos propres droits).
    static USER_TOKEN: AtomicIsize = AtomicIsize::new(0);

    thread_local! {
        /// Profondeur d'imbrication de `as_user` (seul l'appel le plus externe revient à soi).
        static DEPTH: Cell<u32> = const { Cell::new(0) };
    }

    /// Exécute `f` sous l'identité non élevée de l'utilisateur si `drop_file_rights_to_user` l'a
    /// préparée, sinon avec les droits du processus. Si l'emprunt d'identité échoue, le processus
    /// s'arrête (code 4) plutôt que d'écrire en administrateur.
    pub fn as_user<R>(f: impl FnOnce() -> R) -> R {
        let t = USER_TOKEN.load(Ordering::Relaxed);
        if t == 0 {
            return f();
        }
        struct Revert;
        impl Drop for Revert {
            fn drop(&mut self) {
                DEPTH.with(|d| {
                    d.set(d.get() - 1);
                    if d.get() == 0 {
                        // SAFETY: met fin à l'emprunt d'identité commencé par l'appel externe de `as_user`.
                        if unsafe { RevertToSelf() }.is_err() {
                            std::process::exit(4);
                        }
                    }
                });
            }
        }
        DEPTH.with(|d| {
            if d.get() == 0 {
                // SAFETY: `t` est un jeton d'emprunt d'identité ouvert par `drop_file_rights_to_user`, jamais refermé.
                if unsafe { ImpersonateLoggedOnUser(HANDLE(t as *mut _)) }.is_err() {
                    std::process::exit(4);
                }
            }
            d.set(d.get() + 1);
        });
        let _revert = Revert;
        f()
    }

    /// Verrou inter-processus sur un fichier (mutex nommé de la session), libéré à la destruction.
    pub struct FileLock(HANDLE);

    impl Drop for FileLock {
        fn drop(&mut self) {
            // SAFETY: mutex possédé par ce thread (acquis dans `lock_file`), libéré puis refermé une seule fois.
            unsafe {
                let _ = ReleaseMutex(self.0);
                let _ = CloseHandle(self.0);
            }
        }
    }

    /// Prend le verrou de `path` (10 s au plus). Le mutex est créé avec les droits NON élevés
    /// (`as_user`) : créé par le helper élevé avec son jeton admin, il serait inaccessible au démon.
    pub fn lock_file(path: &std::path::Path) -> std::io::Result<FileLock> {
        let name: Vec<u16> = crate::paths::file_lock_name(path).encode_utf16().chain([0]).collect();
        // SAFETY: `name` est un nom UTF-16 terminé par NUL, vivant pendant l'appel.
        let h = as_user(|| unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) })
            .map_err(|e| std::io::Error::other(format!("verrou {} : {e}", path.display())))?;
        // SAFETY: `h` est le handle de mutex obtenu juste avant.
        let r = unsafe { WaitForSingleObject(h, 10_000) };
        // Un propriétaire mort en tenant le verrou (WAIT_ABANDONED) n'empêche pas de continuer :
        // les écritures sont atomiques (fichier temporaire + renommage), le fichier reste cohérent.
        if r == WAIT_OBJECT_0 || r == WAIT_ABANDONED {
            return Ok(FileLock(h));
        }
        // SAFETY: handle obtenu ci-dessus, non possédé, refermé une seule fois.
        unsafe {
            let _ = CloseHandle(h);
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("{} verrouillé par un autre processus", path.display()),
        ))
    }

    struct Owned(HANDLE);
    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: handle possédé, refermé une seule fois.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    fn token_info(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> Result<Vec<u8>, String> {
        let mut len = 0u32;
        // SAFETY: premier appel sans tampon pour connaître la taille requise.
        let _ = unsafe { GetTokenInformation(token, class, None, 0, &mut len) };
        let mut buf = vec![0u8; len.max(16) as usize];
        // SAFETY: `buf` fait `len` octets (au moins), taille transmise à l'API.
        unsafe {
            GetTokenInformation(token, class, Some(buf.as_mut_ptr() as *mut _), buf.len() as u32, &mut len)
                .map_err(|e| format!("GetTokenInformation : {e}"))?
        };
        Ok(buf)
    }

    fn is_elevated(token: HANDLE) -> Result<bool, String> {
        // Classe à taille fixe : l'API exige exactement `size_of::<TOKEN_ELEVATION>()` (sinon ERROR_BAD_LENGTH).
        let mut e = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        // SAFETY: `e` est une TOKEN_ELEVATION locale dont la taille exacte est transmise.
        unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut e as *mut _ as *mut _),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut len,
            )
            .map_err(|e| format!("GetTokenInformation(TokenElevation) : {e}"))?
        };
        Ok(e.TokenIsElevated != 0)
    }

    fn same_user(a: HANDLE, b: HANDLE) -> Result<bool, String> {
        let ua = token_info(a, TokenUser)?;
        let ub = token_info(b, TokenUser)?;
        // SAFETY: `TokenUser` remplit une TOKEN_USER en tête du tampon ; ses SID pointent dans ces mêmes tampons, vivants pendant l'appel.
        unsafe {
            let sa = std::ptr::read_unaligned(ua.as_ptr() as *const TOKEN_USER).User.Sid;
            let sb = std::ptr::read_unaligned(ub.as_ptr() as *const TOKEN_USER).User.Sid;
            Ok(EqualSid(sa, sb).is_ok())
        }
    }

    /// Le processus courant est-il élevé ? (En cas de doute : non.)
    pub fn process_elevated() -> bool {
        let mut own = HANDLE::default();
        // SAFETY: pseudo-handle du processus courant ; `own` est une sortie locale, refermée par `Owned`.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut own) }.is_err() {
            return false;
        }
        let own = Owned(own);
        is_elevated(own.0).unwrap_or(false)
    }

    /// À appeler au démarrage du helper. Processus non élevé, ou session entièrement élevée (UAC
    /// désactivé) : rien à faire (`Ok(false)`).
    /// Processus élevé : prépare le jeton non élevé de l'utilisateur, pris sur l'Explorateur de la
    /// session (même utilisateur exigé), pour toutes les opérations de fichiers via `as_user`.
    pub fn drop_file_rights_to_user() -> Result<bool, String> {
        let mut own = HANDLE::default();
        // SAFETY: pseudo-handle du processus courant ; `own` est une sortie locale, refermée par `Owned`.
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut own) }
            .map_err(|e| format!("OpenProcessToken : {e}"))?;
        let own = Owned(own);
        if !is_elevated(own.0)? {
            return Ok(false);
        }
        // SAFETY: appels sans pointeur hormis la sortie locale `pid`.
        let pid = unsafe {
            let shell = GetShellWindow();
            if shell.is_invalid() {
                return Err("Explorateur introuvable (aucune fenêtre du Shell)".into());
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(shell, Some(&mut pid));
            pid
        };
        // SAFETY: `pid` vient de la fenêtre du Shell ; les handles obtenus sont refermés par `Owned`.
        let user = unsafe {
            let proc = Owned(
                OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
                    .map_err(|e| format!("OpenProcess(explorer) : {e}"))?,
            );
            let mut tok = HANDLE::default();
            OpenProcessToken(proc.0, TOKEN_QUERY | TOKEN_DUPLICATE, &mut tok)
                .map_err(|e| format!("OpenProcessToken(explorer) : {e}"))?;
            let tok = Owned(tok);
            let mut dup = HANDLE::default();
            DuplicateTokenEx(
                tok.0,
                TOKEN_QUERY | TOKEN_IMPERSONATE,
                None,
                SecurityImpersonation,
                TokenImpersonation,
                &mut dup,
            )
            .map_err(|e| format!("DuplicateTokenEx : {e}"))?;
            Owned(dup)
        };
        // Élévation par un AUTRE compte administrateur : le dossier de données et HKCU ne seraient
        // pas ceux de l'utilisateur de la session. On refuse plutôt que d'écrire au mauvais endroit.
        if !same_user(own.0, user.0)? {
            return Err("l'invite UAC a été validée avec un autre compte que celui de la session".into());
        }
        if is_elevated(user.0)? {
            // UAC désactivé ou compte Administrateur intégré : toute la session tourne élevée, il
            // n'y a aucune frontière de privilège à protéger (ni de jeton non élevé à reprendre).
            return Ok(false);
        }
        USER_TOKEN.store(user.0 .0 as isize, Ordering::Relaxed);
        std::mem::forget(user); // conservé jusqu'à la fin du processus
        Ok(true)
    }
}
