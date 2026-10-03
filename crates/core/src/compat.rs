//! Table de compatibilité : un tweak n'est appliqué automatiquement que sur une build Windows
//! où il a été VALIDÉ (voir docs/COMPAT.md pour le protocole de validation).
//!
//! Chaque plage est `(build_min, build_max)` inclusive. On ne déclare ici que ce qui a été
//! réellement vérifié ; le reste passe par `general.allow_untested_builds = true`.

use crate::registry::{RegKey, RegValue, RegistryBackend};

pub const COMPAT: &[(&str, u32, u32)] = &[
    // 24H2 (26100) et 25H2 (26200). Validé sur 26200.9457 ; 26100 partage la même base de code.
    ("navpane", 26100, 26299),
    ("quick-access", 26100, 26299),
    ("thispc-drives", 26100, 26299),
    ("thispc-folders", 26100, 26299),
    ("explorer-view", 26100, 26299),
    ("context-menu", 26100, 26299),
];

pub fn tested(tweak_id: &str, build: u32) -> bool {
    COMPAT.iter().any(|(id, lo, hi)| *id == tweak_id && (*lo..=*hi).contains(&build))
}

/// Numéro de build lu dans `CurrentBuild` (le nom de produit du registre est trompeur : il dit
/// « Windows 10 » sur Windows 11).
pub fn current_build(reg: &dyn RegistryBackend) -> u32 {
    let k = RegKey::hklm(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    match reg.get_value(&k, "CurrentBuild") {
        Ok(Some(RegValue::Sz(s))) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}
