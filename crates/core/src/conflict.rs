//! Détection de conflit avec un autre outil (Windhawk, autre utilitaire...).
//!
//! Si une valeur que nous venons de corriger est réécrite par quelqu'un d'autre et que nous
//! la corrigeons à nouveau en boucle, les deux outils se battent : CPU et I/O gaspillés, et
//! risque de scintillement dans l'Explorateur. On compte donc nos réécritures PAR VALEUR ;
//! au-delà de N en M secondes on s'arrête sur cette valeur et on le signale dans le statut.

use std::collections::{BTreeSet, HashMap, VecDeque};

#[derive(Debug)]
pub struct ConflictGuard {
    max_rewrites: u32,
    window_ms: u64,
    history: HashMap<String, VecDeque<u64>>,
    blocked: BTreeSet<String>,
}

impl ConflictGuard {
    pub fn new(max_rewrites: u32, window_secs: u32) -> Self {
        Self {
            max_rewrites,
            window_ms: window_secs as u64 * 1000,
            history: HashMap::new(),
            blocked: BTreeSet::new(),
        }
    }

    pub fn configure(&mut self, max_rewrites: u32, window_secs: u32) {
        self.max_rewrites = max_rewrites;
        self.window_ms = window_secs as u64 * 1000;
    }

    /// À appeler AVANT chaque réécriture d'une valeur qui avait dérivé.
    /// `false` = la réécriture est interdite (conflit détecté, maintenant ou avant).
    pub fn note_rewrite(&mut self, id: &str, now_ms: u64) -> bool {
        if self.blocked.contains(id) {
            return false;
        }
        let h = self.history.entry(id.to_string()).or_default();
        while h.front().is_some_and(|t| now_ms.saturating_sub(*t) > self.window_ms) {
            h.pop_front();
        }
        h.push_back(now_ms);
        if h.len() as u32 > self.max_rewrites {
            self.blocked.insert(id.to_string());
            return false;
        }
        true
    }

    pub fn is_blocked(&self, id: &str) -> bool {
        self.blocked.contains(id)
    }

    pub fn blocked(&self) -> Vec<String> {
        self.blocked.iter().cloned().collect()
    }

    /// Après un changement de configuration par l'utilisateur, on retente.
    pub fn reset(&mut self) {
        self.history.clear();
        self.blocked.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_after_n_rewrites_in_window() {
        let mut g = ConflictGuard::new(3, 10);
        assert!(g.note_rewrite("k", 0));
        assert!(g.note_rewrite("k", 1000));
        assert!(g.note_rewrite("k", 2000));
        assert!(!g.note_rewrite("k", 3000), "4e réécriture en 10 s = conflit");
        assert!(g.is_blocked("k"));
        assert!(!g.note_rewrite("k", 60_000), "reste bloqué jusqu'à reset");
        g.reset();
        assert!(g.note_rewrite("k", 61_000));
    }

    #[test]
    fn slow_rewrites_are_not_a_conflict() {
        let mut g = ConflictGuard::new(2, 10);
        for i in 0..20 {
            assert!(g.note_rewrite("k", i * 11_000));
        }
    }

    #[test]
    fn keys_are_independent() {
        let mut g = ConflictGuard::new(1, 10);
        assert!(g.note_rewrite("a", 0));
        assert!(g.note_rewrite("b", 0));
        assert!(!g.note_rewrite("a", 1));
        assert!(!g.is_blocked("b"));
    }
}
