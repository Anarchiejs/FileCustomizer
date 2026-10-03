//! Accès aux API Shell officielles (COM), derrière un trait pour pouvoir tester sans Explorateur.
//!
//! Pourquoi pas les verbes de menu contextuel (`unpinfromhome` via `IContextMenu`) ?
//! Construire un `IContextMenu` charge TOUTES les extensions de menu contextuel installées dans
//! notre processus (constaté sur cette machine : une extension tierce se réinstalle à chaque
//! énumération). On lit donc l'état épinglé par la propriété Shell `System.Home.IsPinned`, et on
//! exécute directement la commande « Épingler/Désépingler de l'Accès rapide » de Windows
//! (`IExplorerCommand`, CLSID {b455f46e-…} dans windows.storage.dll — c'est le même objet que celui
//! derrière le verbe `pintohome`/`unpinfromhome`).

use crate::error::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QaItem {
    pub name: String,
    /// `SIGDN_DESKTOPABSOLUTEPARSING` : chemin, ou `::{CLSID}` pour les éléments virtuels.
    pub parsing_name: String,
    pub pinned: bool,
}

pub trait ShellBackend {
    /// Dossiers (épinglés ou non) listés dans l'Accès rapide. Les fichiers récents sont ignorés.
    fn quick_access_items(&self) -> Result<Vec<QaItem>>;
    /// À n'appeler QUE sur un élément `pinned` : la commande Windows est une bascule.
    fn unpin(&self, parsing_name: &str) -> Result<()>;
    /// À n'appeler QUE sur un élément non épinglé (restauration).
    fn pin(&self, parsing_name: &str) -> Result<()>;
    /// Demande à l'Explorateur de se rafraîchir sans redémarrer.
    fn notify_settings_changed(&self);
    /// Résout `@dll,-id` en texte localisé.
    fn resolve_indirect_string(&self, s: &str) -> Option<String>;
}

pub const QUICK_ACCESS_PARSING: &str = "shell:::{679f85cb-0220-4080-b29b-5540cc05aab6}";

// ---------------------------------------------------------------------------
// Backend simulé
// ---------------------------------------------------------------------------

use std::cell::RefCell;

#[derive(Default)]
pub struct MockShell {
    pub items: RefCell<Vec<QaItem>>,
    pub notified: RefCell<u32>,
    pub fail_unpin: RefCell<bool>,
}

impl MockShell {
    pub fn with_items(items: Vec<QaItem>) -> Self {
        Self { items: RefCell::new(items), ..Default::default() }
    }
}

impl ShellBackend for MockShell {
    fn quick_access_items(&self) -> Result<Vec<QaItem>> {
        Ok(self.items.borrow().clone())
    }
    fn unpin(&self, p: &str) -> Result<()> {
        if *self.fail_unpin.borrow() {
            return Err(Error::Shell("échec simulé".into()));
        }
        for i in self.items.borrow_mut().iter_mut() {
            if i.parsing_name.eq_ignore_ascii_case(p) {
                i.pinned = false;
            }
        }
        Ok(())
    }
    fn pin(&self, p: &str) -> Result<()> {
        for i in self.items.borrow_mut().iter_mut() {
            if i.parsing_name.eq_ignore_ascii_case(p) {
                i.pinned = true;
            }
        }
        Ok(())
    }
    fn notify_settings_changed(&self) {
        *self.notified.borrow_mut() += 1;
    }
    fn resolve_indirect_string(&self, _s: &str) -> Option<String> {
        None
    }
}

// ---------------------------------------------------------------------------
// Backend réel
// ---------------------------------------------------------------------------

#[cfg(windows)]
pub use win::WinShell;

#[cfg(windows)]
mod win {
    use super::*;
    use windows::core::{w, Interface, GUID, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::System::Com::StructuredStorage::IPropertyBag;
    use windows::Win32::System::Com::*;
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::System::SystemServices::SFGAO_FOLDER;
    use windows::Win32::UI::Shell::PropertiesSystem::PSGetPropertyKeyFromName;
    use windows::Win32::UI::Shell::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    /// « Pin To Frequent Command » (windows.storage.dll).
    const CLSID_PIN_TO_FREQUENT: GUID = GUID::from_u128(0xb455f46e_e4af_4035_b0a4_cf18d2f6f28e);

    /// Initialise COM (STA) pour la durée de vie de l'objet.
    pub struct WinShell {
        com_initialized: bool,
    }

    impl WinShell {
        pub fn new() -> Result<Self> {
            // STA : le serveur de la commande est déclaré `ThreadingModel=Apartment`.
            // SAFETY: appelé une fois par `WinShell`, équilibré par `CoUninitialize` dans `Drop`.
            let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            // S_FALSE (déjà initialisé) est acceptable ; un autre mode est un vrai problème.
            if hr.is_err() {
                return Err(Error::Shell(format!("CoInitializeEx : {hr:?}")));
            }
            Ok(Self { com_initialized: true })
        }
    }

    impl Drop for WinShell {
        fn drop(&mut self) {
            if self.com_initialized {
                // SAFETY: équilibre le `CoInitializeEx` réussi de `new` sur le même thread.
                unsafe { CoUninitialize() };
            }
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn item_from_parsing(name: &str) -> Result<IShellItem> {
        let n = wide(name);
        let item: IShellItem =
            // SAFETY: `n` est terminé par NUL et vit pendant l'appel.
            unsafe { SHCreateItemFromParsingName(PCWSTR(n.as_ptr()), None::<&IBindCtx>)? };
        Ok(item)
    }

    fn display_name(item: &IShellItem, kind: SIGDN) -> Result<String> {
        // SAFETY: `p` est alloué par le Shell, copié dans `s` puis libéré une seule fois avec `CoTaskMemFree`.
        unsafe {
            let p: PWSTR = item.GetDisplayName(kind)?;
            let s = p.to_string().unwrap_or_default();
            CoTaskMemFree(Some(p.0 as *const _));
            Ok(s)
        }
    }

    fn is_pinned_key() -> Result<PROPERTYKEY> {
        let mut k = PROPERTYKEY::default();
        // SAFETY: `w!` produit une chaîne statique terminée par NUL ; `k` est une sortie locale.
        unsafe { PSGetPropertyKeyFromName(w!("System.Home.IsPinned"), &mut k)? };
        Ok(k)
    }

    /// `verb` : `pintohome` ou `unpinfromhome`. Le CLSID est un handler `DelegateExecute`
    /// (`IExecuteCommand`) : c'est le NOM DU VERBE passé à `IInitializeCommand` qui décide de
    /// l'action, exactement comme quand l'Explorateur exécute l'entrée de menu correspondante.
    fn run_pin_command(verb: &str, parsing_name: &str) -> Result<()> {
        let item = item_from_parsing(parsing_name)?;
        // SAFETY: `item` est un `IShellItem` valide obtenu juste avant.
        let array = unsafe { SHCreateShellItemArrayFromShellItem::<_, IShellItemArray>(&item)? };
        let cmd: IExecuteCommand =
            // SAFETY: COM est initialisé (STA) par `WinShell::new` avant tout appel.
            unsafe { CoCreateInstance(&CLSID_PIN_TO_FREQUENT, None, CLSCTX_INPROC_SERVER)? };
        let v = wide(verb);
        // SAFETY: `v` est terminé par NUL et vit pendant le bloc ; `cmd` et `array` sont des interfaces COM valides.
        unsafe {
            if let Ok(init) = cmd.cast::<IInitializeCommand>() {
                init.Initialize(PCWSTR(v.as_ptr()), None::<&IPropertyBag>)?;
            }
            cmd.cast::<IObjectWithSelection>()?.SetSelection(&array)?;
            cmd.Execute()?;
        }
        Ok(())
    }

    impl ShellBackend for WinShell {
        fn quick_access_items(&self) -> Result<Vec<QaItem>> {
            let key = is_pinned_key()?;
            let root = item_from_parsing(QUICK_ACCESS_PARSING)?;
            // SAFETY: `root` est un `IShellItem` valide ; BHID_EnumItems est un GUID statique.
            let en: IEnumShellItems = unsafe { root.BindToHandler(None::<&IBindCtx>, &BHID_EnumItems)? };
            let mut out = Vec::new();
            loop {
                let mut slot = [None::<IShellItem>];
                let mut fetched = 0u32;
                // SAFETY: `slot` contient un élément et `fetched` est une sortie locale.
                unsafe { en.Next(&mut slot, Some(&mut fetched))? };
                if fetched == 0 {
                    break;
                }
                let Some(item) = slot[0].take() else { break };
                // Les fichiers récents ne nous intéressent pas : seulement les dossiers.
                // SAFETY: `item` est un `IShellItem` valide renvoyé par l'énumérateur.
                let is_folder = unsafe { item.GetAttributes(SFGAO_FOLDER) }
                    .map(|a| a.0 & SFGAO_FOLDER.0 != 0)
                    .unwrap_or(false);
                if !is_folder {
                    continue;
                }
                let pinned = item
                    .cast::<IShellItem2>()
                    .ok()
                    // SAFETY: `i2` est une interface valide ; `key` est une PROPERTYKEY initialisée.
                    .and_then(|i2| unsafe { i2.GetBool(&key) }.ok())
                    .map(|b| b.as_bool())
                    .unwrap_or(false);
                out.push(QaItem {
                    name: display_name(&item, SIGDN_NORMALDISPLAY).unwrap_or_default(),
                    parsing_name: display_name(&item, SIGDN_DESKTOPABSOLUTEPARSING)?,
                    pinned,
                });
            }
            Ok(out)
        }

        fn unpin(&self, parsing_name: &str) -> Result<()> {
            run_pin_command("unpinfromhome", parsing_name)
        }

        fn pin(&self, parsing_name: &str) -> Result<()> {
            run_pin_command("pintohome", parsing_name)
        }

        fn notify_settings_changed(&self) {
            // SAFETY: diffusions sans pointeur hormis `ShellState`, chaîne statique produite par `w!`.
            unsafe {
                SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
                // Délai court : on ne veut pas bloquer sur une fenêtre qui ne répond plus.
                let _ = SendMessageTimeoutW(
                    HWND_BROADCAST,
                    WM_SETTINGCHANGE,
                    WPARAM(0),
                    LPARAM(w!("ShellState").as_ptr() as isize),
                    SMTO_ABORTIFHUNG,
                    200,
                    None,
                );
            }
        }

        fn resolve_indirect_string(&self, s: &str) -> Option<String> {
            let src = wide(s);
            let mut buf = [0u16; 256];
            // SAFETY: `src` est terminé par NUL ; `buf` est un tampon local dont la taille est transmise.
            unsafe { SHLoadIndirectString(PCWSTR(src.as_ptr()), &mut buf, None).ok()? };
            let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
            Some(String::from_utf16_lossy(&buf[..len]))
        }
    }
}
