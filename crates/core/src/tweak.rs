//! Contrat commun des « tweaks » et mécanique de réconciliation du registre.
//!
//! Modèle : à partir de la config, chaque tweak calcule un ÉTAT DÉSIRÉ ; `apply()` n'écrit que
//! ce qui diffère (idempotent) et restaure ce que nous avions géré mais que la config ne demande
//! plus. C'est ce qui permet au démon de rappeler `apply()` à chaque événement sans risque.

use crate::backup::BackupStore;
use crate::config::Config;
use crate::conflict::ConflictGuard;
use crate::error::{Error, Result};
use crate::registry::{RegKey, RegValue, RegistryBackend};
use crate::shell::ShellBackend;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub struct TweakMeta {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// Clés/valeurs touchées, pour le README et `status`.
    pub touches: &'static str,
    pub needs_elevation: bool,
}

/// Une valeur de registre que le tweak veut voir à une certaine valeur.
#[derive(Clone, Debug)]
pub struct RegSetting {
    pub key: RegKey,
    pub name: String,
    pub value: RegValue,
    /// Libellé lisible pour les rapports (« Accueil masqué »).
    pub label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeKind {
    Applied,
    Reverted,
    Unchanged,
    WouldApply,
    WouldRevert,
    Skipped,
    Conflict,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Change {
    pub tweak: String,
    pub kind: ChangeKind,
    pub what: String,
    pub detail: String,
}

#[derive(Default, Debug)]
pub struct Report {
    pub changes: Vec<Change>,
}

impl Report {
    pub fn push(&mut self, tweak: &str, kind: ChangeKind, what: impl Into<String>, detail: impl Into<String>) {
        self.changes.push(Change { tweak: tweak.into(), kind, what: what.into(), detail: detail.into() });
    }
    /// Vrai si le système a réellement été modifié (hors dry-run).
    pub fn modified_system(&self) -> bool {
        self.changes.iter().any(|c| matches!(c.kind, ChangeKind::Applied | ChangeKind::Reverted))
    }
    pub fn has_errors(&self) -> bool {
        self.changes.iter().any(|c| matches!(c.kind, ChangeKind::Error | ChangeKind::Conflict))
    }
}

pub struct Ctx<'a> {
    pub reg: &'a dyn RegistryBackend,
    pub shell: &'a dyn ShellBackend,
    pub backup: &'a mut BackupStore,
    pub cfg: &'a Config,
    pub guard: &'a mut ConflictGuard,
    pub dry_run: bool,
    /// Horloge monotone injectée (ms) : testable sans `sleep`.
    pub now_ms: u64,
    pub build: u32,
    /// Démarrage rapide du démon : ne pas faire le travail qui passe par COM/Shell (coûteux,
    /// ~100 ms à froid). Le démon le rattrape juste après avoir armé ses surveillances.
    pub defer_shell: bool,
    /// Processus élevé (helper à la demande). Le démon et le CLI normal ne le sont JAMAIS : les
    /// tweaks qui écrivent en HKLM refusent d'agir sans ce drapeau.
    pub elevated: bool,
    pub report: Report,
}

/// Ce que le démon doit surveiller pour ce tweak (événementiel, jamais de polling).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WatchTarget {
    /// `RegNotifyChangeKeyValue`. Si la clé n'existe pas encore, le démon surveille son plus proche ancêtre.
    RegKey { key: RegKey, subtree: bool },
    /// `ReadDirectoryChangesW` sur `dir`, filtré sur le nom `file`.
    DirFile { dir: PathBuf, file: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemStatus {
    /// Conforme à la config.
    Ok,
    /// Différent de ce que demande la config.
    Pending,
}

#[derive(Clone, Debug)]
pub struct DetectedItem {
    pub label: String,
    pub current: String,
    pub desired: String,
    pub status: ItemStatus,
}

pub trait Tweak {
    fn meta(&self) -> &'static TweakMeta;
    /// La config demande-t-elle quelque chose pour ce tweak ?
    fn requested(&self, cfg: &Config) -> bool;
    /// Lecture seule : compare l'état courant à l'état désiré.
    fn detect(&self, ctx: &mut Ctx) -> Result<Vec<DetectedItem>>;
    /// Réconcilie : applique le désiré, restaure ce que la config ne demande plus. Idempotent.
    fn apply(&self, ctx: &mut Ctx) -> Result<()>;
    /// Remet l'état d'origine de tout ce que ce tweak a géré (indépendamment de la config).
    fn revert(&self, ctx: &mut Ctx) -> Result<()>;
    fn watch(&self, cfg: &Config) -> Vec<WatchTarget>;
}

// ---------------------------------------------------------------------------
// Aides partagées par les tweaks « registre »
// ---------------------------------------------------------------------------

fn setting_id(s: &RegSetting) -> String {
    format!("{}\\{}", s.key.display(), s.name)
}

pub fn detect_registry(ctx: &Ctx, desired: &[RegSetting]) -> Result<Vec<DetectedItem>> {
    desired
        .iter()
        .map(|s| {
            let cur = ctx.reg.get_value(&s.key, &s.name)?;
            Ok(DetectedItem {
                label: s.label.clone(),
                current: cur.as_ref().map(|v| v.display()).unwrap_or_else(|| "(absent)".into()),
                desired: s.value.display(),
                status: if cur.as_ref() == Some(&s.value) { ItemStatus::Ok } else { ItemStatus::Pending },
            })
        })
        .collect()
}

pub fn reconcile_registry(ctx: &mut Ctx, tweak: &str, desired: &[RegSetting]) -> Result<()> {
    for s in desired {
        let cur = ctx.reg.get_value(&s.key, &s.name)?;
        if cur.as_ref() == Some(&s.value) {
            ctx.report.push(tweak, ChangeKind::Unchanged, &s.label, "déjà conforme");
            continue;
        }
        let from = cur.as_ref().map(|v| v.display()).unwrap_or_else(|| "(absent)".into());
        let detail = format!("{} \\ {} : {from} -> {}", s.key.display(), s.name, s.value.display());
        if ctx.dry_run {
            ctx.report.push(tweak, ChangeKind::WouldApply, &s.label, detail);
            continue;
        }
        // Une valeur déjà gérée par nous qui a dérivé = quelqu'un d'autre l'a modifiée.
        let drifted = ctx.backup.find(&s.key, &s.name).is_some();
        if drifted && !ctx.guard.note_rewrite(&setting_id(s), ctx.now_ms) {
            ctx.report.push(
                tweak,
                ChangeKind::Conflict,
                &s.label,
                format!("{} est réécrite en boucle par un autre outil : on s'arrête sur cette valeur", setting_id(s)),
            );
            continue;
        }
        // Sauvegarde persistée AVANT toute écriture.
        if let Err(e) = ctx.backup.record(ctx.reg, tweak, &s.key, &s.name) {
            ctx.report.push(tweak, ChangeKind::Error, &s.label, format!("sauvegarde impossible, rien écrit : {e}"));
            continue;
        }
        match ctx.reg.set_value(&s.key, &s.name, &s.value) {
            Ok(()) => ctx.report.push(tweak, ChangeKind::Applied, &s.label, detail),
            Err(e) => {
                // Écriture échouée : rien n'a changé, on retire l'entrée qu'on vient de créer pour ne pas
                // « restaurer » plus tard une valeur que nous n'avons jamais modifiée.
                if !drifted {
                    let _ = ctx.backup.remove(&s.key, &s.name);
                }
                let msg = if matches!(e, Error::AccessDenied(_)) {
                    format!("{e} (élévation requise)")
                } else {
                    e.to_string()
                };
                ctx.report.push(tweak, ChangeKind::Error, &s.label, msg);
            }
        }
    }

    // Valeurs que nous gérions mais que la config ne demande plus : retour à l'origine.
    let stale: Vec<_> = ctx
        .backup
        .entries_for(tweak)
        .filter(|e| !desired.iter().any(|s| s.key == e.key && s.name == e.name))
        .cloned()
        .collect();
    for e in stale {
        restore_one(ctx, tweak, &e)?;
    }
    Ok(())
}

pub fn revert_registry(ctx: &mut Ctx, tweak: &str) -> Result<()> {
    let all: Vec<_> = ctx.backup.entries_for(tweak).cloned().collect();
    for e in all {
        restore_one(ctx, tweak, &e)?;
    }
    Ok(())
}

fn restore_one(ctx: &mut Ctx, tweak: &str, e: &crate::backup::BackupEntry) -> Result<()> {
    let what = format!("{} \\ {}", e.key.display(), e.name);
    let to = e.original.as_ref().map(|v| v.display()).unwrap_or_else(|| "(supprimée)".into());
    if ctx.dry_run {
        ctx.report.push(tweak, ChangeKind::WouldRevert, &what, format!("-> {to}"));
        return Ok(());
    }
    match BackupStore::restore_entry(ctx.reg, e) {
        Ok(()) => {
            ctx.backup.remove(&e.key, &e.name)?;
            ctx.report.push(tweak, ChangeKind::Reverted, &what, format!("-> {to}"));
        }
        // On garde l'entrée de backup : on pourra réessayer (ex. après élévation).
        Err(err) => ctx.report.push(tweak, ChangeKind::Error, &what, err.to_string()),
    }
    Ok(())
}
