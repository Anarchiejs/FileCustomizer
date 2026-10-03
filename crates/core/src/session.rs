//! Session d'exécution sur le VRAI système (registre + Shell), partagée par le CLI et le démon.

use crate::backup::BackupStore;
use crate::compat;
use crate::config::Config;
use crate::conflict::ConflictGuard;
use crate::engine::{self, TweakDetection};
use crate::error::Result;
use crate::paths;
use crate::registry::WinRegistry;
use crate::shell::WinShell;
use crate::tweak::{ChangeKind, Ctx, NodeCache, Report};
use std::time::Instant;

pub struct Session {
    reg: WinRegistry,
    shell: WinShell,
    pub backup: BackupStore,
    pub guard: ConflictGuard,
    pub build: u32,
    /// Posé uniquement par le helper élevé.
    pub elevated: bool,
    node_cache: NodeCache,
    started: Instant,
}

impl Session {
    /// Initialise COM (STA) sur le thread courant : la session doit rester sur ce thread.
    pub fn open() -> Result<Self> {
        let reg = WinRegistry;
        let build = compat::current_build(&reg);
        Ok(Self {
            shell: WinShell::new()?,
            backup: BackupStore::load(&paths::backup_path())?,
            guard: ConflictGuard::new(5, 30),
            build,
            elevated: false,
            node_cache: NodeCache::default(),
            reg,
            started: Instant::now(),
        })
    }

    /// Session du helper élevé : les entrées de `backup.json` hors liste blanche sont écartées
    /// avant toute opération (le fichier est modifiable sans élévation).
    pub fn open_elevated() -> Result<Self> {
        let mut s = Self::open()?;
        s.elevated = true;
        s.backup.quarantine(engine::elevated_entry_allowed);
        Ok(s)
    }

    fn with_ctx<R>(
        &mut self,
        cfg: &Config,
        dry_run: bool,
        defer_shell: bool,
        f: impl FnOnce(&mut Ctx) -> R,
    ) -> (R, Report) {
        self.guard.configure(cfg.general.conflict_max_rewrites, cfg.general.conflict_window_secs);
        // Un autre processus (démon, CLI, helper élevé) a pu modifier backup.json depuis la dernière passe.
        let refreshed = self.backup.refresh();
        let mut ctx = Ctx {
            reg: &self.reg,
            shell: &self.shell,
            backup: &mut self.backup,
            cfg,
            guard: &mut self.guard,
            dry_run,
            now_ms: self.started.elapsed().as_millis() as u64,
            build: self.build,
            defer_shell,
            elevated: self.elevated,
            node_cache: Some(&self.node_cache),
            report: Report::default(),
        };
        if let Err(e) = refreshed {
            // Aucune écriture ne passera : chaque sauvegarde relit le fichier sous verrou et échouera aussi.
            ctx.report.push("backup", ChangeKind::Error, "backup.json", format!("relecture impossible : {e}"));
        }
        if let Some(m) = ctx.backup.recovered.take() {
            ctx.report.push("backup", ChangeKind::Skipped, "backup.json", m);
        }
        for e in ctx.backup.quarantined().to_vec() {
            ctx.report.push(
                &e.tweak,
                ChangeKind::Error,
                format!("{} \\ {}", e.key.display(), e.name),
                "entrée de backup.json hors liste autorisée : ignorée par le helper élevé",
            );
        }
        let r = f(&mut ctx);
        (r, ctx.report)
    }

    pub fn apply(&mut self, cfg: &Config, dry_run: bool) -> Report {
        self.with_ctx(cfg, dry_run, false, engine::apply_all).1
    }

    /// Passe rapide du démarrage : registre seulement, sans COM/Shell.
    pub fn apply_registry_only(&mut self, cfg: &Config) -> Report {
        self.with_ctx(cfg, false, true, engine::apply_all).1
    }

    pub fn revert(&mut self, cfg: &Config, dry_run: bool) -> Report {
        self.with_ctx(cfg, dry_run, false, engine::revert_all).1
    }

    pub fn detect(&mut self, cfg: &Config) -> Vec<TweakDetection> {
        self.with_ctx(cfg, true, false, engine::detect_all).0
    }

    /// Oublie les résolutions mises en cache (config modifiée, Explorateur redémarré).
    pub fn invalidate_caches(&self) {
        self.node_cache.borrow_mut().clear();
    }

    /// Demande aux fenêtres Explorateur ouvertes de se rafraîchir (sans redémarrer explorer.exe).
    pub fn notify(&self) {
        use crate::shell::ShellBackend;
        self.shell.notify_settings_changed();
    }

    pub fn registry(&self) -> &WinRegistry {
        &self.reg
    }
    pub fn shell(&self) -> &WinShell {
        &self.shell
    }
}
