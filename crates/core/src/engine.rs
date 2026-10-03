//! Orchestration des tweaks : filtre de compatibilité, application, restauration, surveillance.

use crate::backup::BackupStore;
use crate::compat;
use crate::tweak::*;
use crate::tweaks;

/// Applique tous les tweaks. Un tweak en erreur n'empêche pas les suivants.
pub fn apply_all(ctx: &mut Ctx) {
    for t in tweaks::all() {
        let m = t.meta();
        // Le helper élevé ne fait QUE ce qui exige l'élévation : tout le reste reste dans le démon/CLI normal.
        if ctx.elevated && !m.needs_elevation {
            continue;
        }
        // Un tweak non validé sur cette build n'est pas appliqué à l'aveugle. Sa restauration, elle,
        // reste toujours possible (on ne fait que défaire ce qu'on a écrit).
        if !compat::tested(m.id, ctx.build) && !ctx.cfg.general.allow_untested_builds {
            if t.requested(ctx.cfg) {
                ctx.report.push(
                    m.id,
                    ChangeKind::Skipped,
                    m.name,
                    format!(
                        "build Windows {} non validée pour ce tweak : désactivé (general.allow_untested_builds = true pour forcer)",
                        ctx.build
                    ),
                );
            }
            continue;
        }
        if let Err(e) = t.apply(ctx) {
            ctx.report.push(m.id, ChangeKind::Error, m.name, e.to_string());
        }
    }
    // La notification (SHChangeNotify + diffusion WM_SETTINGCHANGE) bloque jusqu'à ~700 ms ;
    // dans la passe rapide du démarrage c'est le démon qui la lance plus tard (`Session::notify`).
    if ctx.report.modified_system() && !ctx.defer_shell {
        // Rafraîchit les fenêtres ouvertes sans redémarrer l'Explorateur.
        ctx.shell.notify_settings_changed();
    }
}

/// Restaure tout : chaque tweak, puis toute entrée de backup orpheline (tweak disparu entre deux versions).
pub fn revert_all(ctx: &mut Ctx) {
    for t in tweaks::all() {
        if ctx.elevated && !t.meta().needs_elevation {
            continue;
        }
        if let Err(e) = t.revert(ctx) {
            ctx.report.push(t.meta().id, ChangeKind::Error, t.meta().name, e.to_string());
        }
    }
    let orphans: Vec<_> = ctx.backup.data.entries.clone();
    for e in orphans {
        let what = format!("{} \\ {}", e.key.display(), e.name);
        // Les valeurs d'un tweak « élévation » ne se restaurent que dans le helper élevé, les autres jamais dedans.
        let hklm = needs_elevation_for(&e.tweak) || e.key.hive == crate::registry::Hive::Hklm;
        if ctx.elevated != hklm {
            if hklm && !ctx.dry_run {
                ctx.report.push(&e.tweak, ChangeKind::Skipped, what, "restauration HKLM : nécessite l'élévation");
            }
            continue;
        }
        if ctx.dry_run {
            ctx.report.push(&e.tweak, ChangeKind::WouldRevert, what, "entrée orpheline");
            continue;
        }
        match BackupStore::restore_entry(ctx.reg, &e).and_then(|_| ctx.backup.remove(&e.key, &e.name)) {
            Ok(()) => ctx.report.push(&e.tweak, ChangeKind::Reverted, what, "entrée orpheline restaurée"),
            Err(err) => ctx.report.push(&e.tweak, ChangeKind::Error, what, err.to_string()),
        }
    }
    if ctx.report.modified_system() {
        ctx.shell.notify_settings_changed();
    }
}

/// Ce tweak exige-t-il le helper élevé ? (Inconnu = non.)
pub fn needs_elevation_for(tweak_id: &str) -> bool {
    tweaks::all().iter().any(|t| t.meta().id == tweak_id && t.meta().needs_elevation)
}

pub struct TweakDetection {
    pub meta: &'static TweakMeta,
    pub requested: bool,
    pub tested_on_build: bool,
    pub items: Vec<DetectedItem>,
    pub error: Option<String>,
}

pub fn detect_all(ctx: &mut Ctx) -> Vec<TweakDetection> {
    tweaks::all()
        .iter()
        .map(|t| {
            let (items, error) = match t.detect(ctx) {
                Ok(i) => (i, None),
                Err(e) => (vec![], Some(e.to_string())),
            };
            TweakDetection {
                meta: t.meta(),
                requested: t.requested(ctx.cfg),
                tested_on_build: compat::tested(t.meta().id, ctx.build),
                items,
                error,
            }
        })
        .collect()
}

pub fn collect_watch(cfg: &crate::config::Config) -> Vec<WatchTarget> {
    let mut out: Vec<WatchTarget> = Vec::new();
    for t in tweaks::all() {
        for w in t.watch(cfg) {
            if !out.contains(&w) {
                out.push(w);
            }
        }
    }
    out
}
