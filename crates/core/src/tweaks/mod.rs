pub mod context_menu;
pub mod explorer_view;
pub mod navpane;
pub mod quick_access;
pub mod thispc;

use crate::tweak::Tweak;

/// Tous les tweaks connus, dans l'ordre d'application.
pub fn all() -> Vec<Box<dyn Tweak>> {
    vec![
        Box::new(navpane::NavPane),
        Box::new(quick_access::QuickAccess),
        Box::new(thispc::ThisPcDrives),
        Box::new(thispc::ThisPcFolders),
        Box::new(explorer_view::ExplorerView),
        Box::new(context_menu::ContextMenu),
    ]
}
