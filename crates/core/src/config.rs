//! Modèle de configuration (`%APPDATA%\FileCustomizer\config.toml`).
//!
//! Règle d'or : **toutes les valeurs par défaut signifient « ne rien toucher »**.
//! Un fichier vide, ou absent, ne modifie donc jamais le système.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CONFIG_VERSION: u32 = 1;

/// Ce que les règles conditionnelles peuvent observer (injectable pour les tests).
pub trait ProfileEnv {
    fn drive_present(&self, letter: &str) -> bool;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    /// Ne pas toucher : si FileCustomizer avait posé une valeur, elle est restaurée.
    #[default]
    Default,
    Show,
    Hide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Off,
    Error,
    Warn,
    #[default]
    Info,
    Debug,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct General {
    pub log_level: LogLevel,
    /// Icône de zone de notification. Désactivée par défaut, non implémentée pour l'instant.
    pub tray_icon: bool,
    /// Appliquer aussi sur une build Windows non validée dans la table de compatibilité.
    pub allow_untested_builds: bool,
    /// Détection de conflit : plus de `conflict_max_rewrites` réécritures de la même
    /// valeur en `conflict_window_secs` secondes => on arrête de la réécrire.
    pub conflict_max_rewrites: u32,
    pub conflict_window_secs: u32,
    /// Délai de regroupement des événements avant réapplication.
    pub debounce_ms: u32,
}

impl Default for General {
    fn default() -> Self {
        Self {
            log_level: LogLevel::Info,
            tray_icon: false,
            allow_untested_builds: false,
            conflict_max_rewrites: 5,
            conflict_window_secs: 30,
            debounce_ms: 1500,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NavigationPane {
    pub home: Visibility,
    pub gallery: Visibility,
    /// Tout autre nœud du volet : clé = nom lisible (voir `filecustomizer nodes`) ou CLSID.
    pub nodes: BTreeMap<String, Visibility>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuickAccessMode {
    #[default]
    Default,
    /// Plus aucun dossier épinglé, ShowFrequent=0, ShowRecent=0, surveillance des ré-épinglages.
    Disabled,
    /// Garde uniquement les dossiers de `whitelist` épinglés.
    Whitelist,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct QuickAccess {
    pub mode: QuickAccessMode,
    /// Chemins (ou noms d'éléments) à garder épinglés en mode `whitelist`.
    pub whitelist: Vec<String>,
    /// Pour le mode `whitelist` uniquement (`disabled` force les deux à `false`).
    /// `None` = ne pas toucher.
    pub show_frequent: Option<bool>,
    pub show_recent: Option<bool>,
}

/// F2 — « Ce PC ».
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThisPc {
    /// Dossiers à masquer : desktop, documents, pictures, music, videos, downloads, 3d-objects
    /// (noms français acceptés). Écrit en HKLM : nécessite l'élévation (helper à la demande).
    pub hide_folders: Vec<String>,
    /// Dossiers à (r)afficher explicitement.
    pub show_folders: Vec<String>,
    /// Lecteurs à masquer, ex. `["D", "E:"]`. Masque dans l'Explorateur mais n'empêche PAS
    /// l'accès par chemin (`NoDrives`).
    pub hide_drives: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchTo {
    ThisPc,
    Home,
    Downloads,
    Onedrive,
}

/// F3 — options d'affichage (`HKCU\...\Explorer\Advanced`). `None` = ne pas toucher.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExplorerView {
    pub show_file_extensions: Option<bool>,
    pub show_hidden_files: Option<bool>,
    pub show_system_files: Option<bool>,
    pub launch_to: Option<LaunchTo>,
    pub use_checkboxes: Option<bool>,
    pub nav_show_all_folders: Option<bool>,
    pub nav_expand_to_current_folder: Option<bool>,
    pub sync_provider_notifications: Option<bool>,
    pub compact_mode: Option<bool>,
    pub show_status_bar: Option<bool>,
    pub hide_drives_with_no_media: Option<bool>,
}

/// F4 — menu contextuel.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContextMenu {
    /// Restaure le menu contextuel classique de Windows 10.
    pub classic_menu: bool,
    /// Extensions Shell à bloquer (nom ou CLSID ; liste : `filecustomizer shell-extensions`).
    /// Méthode non destructive : `Shell Extensions\Blocked`.
    pub blocked_extensions: Vec<String>,
    /// Verbes statiques à désactiver (`LegacyDisable`), ex. `Directory\shell\cmd`.
    pub disabled_verbs: Vec<String>,
}

/// F5 — un profil remplace, section par section, la configuration de base.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProfileConfig {
    pub navigation_pane: Option<NavigationPane>,
    pub quick_access: Option<QuickAccess>,
    pub this_pc: Option<ThisPc>,
    pub explorer_view: Option<ExplorerView>,
    pub context_menu: Option<ContextMenu>,
}

/// Condition d'une règle ; si plusieurs champs sont renseignés ils s'additionnent (ET).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Condition {
    /// Lettre de lecteur présente (ex. `"D"`).
    pub drive_present: Option<String>,
    pub drive_absent: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    #[serde(default)]
    pub when: Condition,
    pub profile: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub general: General,
    pub navigation_pane: NavigationPane,
    pub quick_access: QuickAccess,
    pub this_pc: ThisPc,
    pub explorer_view: ExplorerView,
    pub context_menu: ContextMenu,
    /// Profils nommés (`[profiles.Travail.navigation_pane]`...).
    pub profiles: BTreeMap<String, ProfileConfig>,
    /// Règles automatiques, évaluées dans l'ordre : la première qui correspond choisit le profil.
    pub rules: Vec<Rule>,
}

impl Config {
    pub fn from_toml(s: &str) -> Result<Self> {
        let c: Config = toml::from_str(s).map_err(|e| Error::Config(e.to_string()))?;
        if c.version > CONFIG_VERSION {
            return Err(Error::Config(format!(
                "version {} plus récente que celle supportée ({CONFIG_VERSION})",
                c.version
            )));
        }
        Ok(c)
    }

    pub fn to_toml(&self) -> Result<String> {
        toml::to_string_pretty(self).map_err(|e| Error::Config(e.to_string()))
    }

    /// Comme `to_toml`, mais réécrit `existing` en place : les commentaires et la mise en forme
    /// des clés inchangées sont conservés, seules les valeurs modifiées sont remplacées.
    /// Si `existing` est illisible ou si la fusion ne redonne pas exactement `self`, renvoie
    /// le texte de `to_toml` (jamais une configuration différente de celle demandée).
    pub fn to_toml_preserving(&self, existing: &str) -> Result<String> {
        let fresh = self.to_toml()?;
        let merged = (|| {
            let mut doc: toml_edit::DocumentMut = existing.parse().ok()?;
            let new: toml_edit::DocumentMut = fresh.parse().ok()?;
            toml_merge::merge_table(doc.as_table_mut(), new.as_table());
            let text = doc.to_string();
            (Self::from_toml(&text).ok()? == *self).then_some(text)
        })();
        Ok(merged.unwrap_or(fresh))
    }

    /// Fichier absent = configuration par défaut (rien à faire).
    pub fn load(path: &std::path::Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(s) => Self::from_toml(&s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Vrai si la config (hors profils) demande au moins une modification.
    pub fn is_noop(&self) -> bool {
        self.navigation_pane == NavigationPane::default()
            && self.quick_access == QuickAccess::default()
            && self.this_pc == ThisPc::default()
            && self.explorer_view == ExplorerView::default()
            && self.context_menu == ContextMenu::default()
    }

    /// Choisit le profil actif : choix manuel (`apply <profil>`) s'il existe, sinon première
    /// règle qui correspond. Renvoie aussi un avertissement (profil manuel inconnu...).
    pub fn select_profile(&self, manual: Option<&str>, env: &dyn ProfileEnv) -> (Option<String>, Option<String>) {
        let mut warning = None;
        if let Some(m) = manual {
            if self.profiles.contains_key(m) {
                return (Some(m.to_string()), None);
            }
            warning = Some(format!("profil manuel « {m} » inconnu, ignoré"));
        }
        for r in &self.rules {
            if !self.profiles.contains_key(&r.profile) {
                warning = Some(format!("règle vers le profil inconnu « {} »", r.profile));
                continue;
            }
            let c = &r.when;
            // Une condition vide ne correspond jamais : une règle sans `when` est une erreur de frappe.
            if c.drive_present.is_none() && c.drive_absent.is_none() {
                continue;
            }
            let ok = c.drive_present.as_deref().is_none_or(|d| env.drive_present(d))
                && c.drive_absent.as_deref().is_none_or(|d| !env.drive_present(d));
            if ok {
                return (Some(r.profile.clone()), warning);
            }
        }
        (None, warning)
    }

    /// Configuration effective : la base, dont les sections sont remplacées par celles du profil.
    pub fn effective(&self, profile: Option<&str>) -> Config {
        let mut c = self.clone();
        if let Some(p) = profile.and_then(|n| self.profiles.get(n)) {
            if let Some(v) = &p.navigation_pane {
                c.navigation_pane = v.clone();
            }
            if let Some(v) = &p.quick_access {
                c.quick_access = v.clone();
            }
            if let Some(v) = &p.this_pc {
                c.this_pc = v.clone();
            }
            if let Some(v) = &p.explorer_view {
                c.explorer_view = v.clone();
            }
            if let Some(v) = &p.context_menu {
                c.context_menu = v.clone();
            }
        }
        c
    }
}

/// Contenu écrit par `filecustomizer init` : documente le schéma, ne modifie rien.
pub const DEFAULT_CONFIG_TOML: &str = r##"# FileCustomizer — configuration
# Toutes les valeurs par défaut = « ne rien toucher ». Décommentez ce que vous voulez.
# Le démon relit ce fichier dès qu'il change. Erreur de syntaxe => ancienne config conservée.
version = 1

[general]
# log_level = "info"            # off | error | warn | info | debug
# allow_untested_builds = false # appliquer aussi sur une build Windows non validée
# conflict_max_rewrites = 5     # N réécritures ...
# conflict_window_secs = 30     # ... en M secondes => conflit (Windhawk ?), on s'arrête sur la clé
# debounce_ms = 1500

[navigation_pane]
# Valeurs : "default" (ne pas toucher) | "show" | "hide"
# home = "hide"                 # Accueil
# gallery = "hide"              # Galerie

# Autres nœuds du volet (liste : `filecustomizer nodes`), par nom lisible ou par CLSID :
# [navigation_pane.nodes]
# "Proton Drive" = "hide"
# "Bibliothèques" = "hide"

[quick_access]
# mode = "default"    # default | disabled | whitelist
#   disabled  : aucun dossier épinglé + ShowFrequent=0 + ShowRecent=0 + surveillance
#   whitelist : garde seulement les dossiers de `whitelist` épinglés
# whitelist = ['C:\Documents']
# show_frequent = false   # disabled : forcé à false ; whitelist : false par défaut
# show_recent = false

[this_pc]
# hide_folders = ["videos", "3d-objects"]  # desktop documents pictures music videos downloads 3d-objects
#                                          # écrit en HKLM : invite UAC via `apply --elevate`
# show_folders = []
# hide_drives = ["D"]                      # masque dans l'Explorateur, n'empêche pas l'accès par chemin

[explorer_view]            # true/false ; absent = ne pas toucher
# show_file_extensions = true
# show_hidden_files = true
# show_system_files = false
# launch_to = "this_pc"    # this_pc | home | downloads | onedrive (= fournisseur cloud principal)
# use_checkboxes = false
# nav_show_all_folders = false
# nav_expand_to_current_folder = true
# sync_provider_notifications = false      # pubs du fournisseur de synchronisation
# compact_mode = true
# show_status_bar = true
# hide_drives_with_no_media = true

[context_menu]
# classic_menu = true                      # menu contextuel classique (Windows 10)
# blocked_extensions = ["{CLSID}", "Nom"]  # liste : `filecustomizer shell-extensions`
# disabled_verbs = ['Directory\shell\cmd']

# Profils : chaque section présente REMPLACE la section de base. `filecustomizer apply Minimal`.
# [profiles.Minimal.navigation_pane]
# home = "hide"
# gallery = "hide"
# [profiles.Minimal.quick_access]
# mode = "disabled"

# Règles automatiques (la première qui correspond ; un `apply <profil>` manuel les supplante,
# `apply --auto` redonne la main aux règles) :
# [[rules]]
# profile = "Minimal"
# [rules.when]
# drive_absent = "D"
"##;

/// Fusion d'un document TOML produit par sérialisation dans le fichier de l'utilisateur.
mod toml_merge {
    use toml_edit::{Item, Table, Value};

    /// Rend `old` égal à `new` en touchant le moins possible : clés absentes de `new` retirées,
    /// valeurs différentes remplacées (décor conservé), nouvelles clés ajoutées en fin de table.
    pub fn merge_table(old: &mut Table, new: &Table) {
        let gone: Vec<String> = old.iter().map(|(k, _)| k.to_string()).filter(|k| !new.contains_key(k)).collect();
        for k in gone {
            old.remove(&k);
        }
        for (k, n) in new.iter() {
            match old.get_mut(k) {
                Some(o) => merge_item(o, n),
                None => {
                    old.insert(k, fresh(n));
                }
            }
        }
    }

    fn merge_item(old: &mut Item, new: &Item) {
        match (old, new) {
            (Item::Table(o), Item::Table(n)) => merge_table(o, n),
            (Item::Value(o), Item::Value(n)) => {
                if !value_eq(o, n) {
                    let decor = o.decor().clone();
                    *o = n.clone();
                    *o.decor_mut() = decor;
                }
            }
            (Item::ArrayOfTables(o), Item::ArrayOfTables(n)) => {
                while o.len() > n.len() {
                    o.remove(o.len() - 1);
                }
                for (i, nt) in n.iter().enumerate() {
                    match o.get_mut(i) {
                        Some(ot) => merge_table(ot, nt),
                        None => o.push(fresh_table(nt)),
                    }
                }
            }
            (o, n) => *o = fresh(n),
        }
    }

    /// Copie d'un élément du document neuf, sans ses positions : il s'insère après ses voisins.
    fn fresh(item: &Item) -> Item {
        match item {
            Item::Table(t) => Item::Table(fresh_table(t)),
            Item::ArrayOfTables(a) => {
                let mut a = a.clone();
                for t in a.iter_mut() {
                    *t = fresh_table(t);
                }
                Item::ArrayOfTables(a)
            }
            other => other.clone(),
        }
    }

    fn fresh_table(t: &Table) -> Table {
        let mut t = t.clone();
        t.set_position(None);
        for (_, item) in t.iter_mut() {
            *item = fresh(item);
        }
        // Pas d'en-tête `[x.y]` vide ajouté au fichier de l'utilisateur.
        t.set_implicit(t.is_empty());
        t
    }

    /// Égalité de valeur, indépendante de la mise en forme (guillemets, espaces, commentaires).
    fn value_eq(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::String(x), Value::String(y)) => x.value() == y.value(),
            (Value::Integer(x), Value::Integer(y)) => x.value() == y.value(),
            (Value::Float(x), Value::Float(y)) => x.value() == y.value(),
            (Value::Boolean(x), Value::Boolean(y)) => x.value() == y.value(),
            (Value::Datetime(x), Value::Datetime(y)) => x.value() == y.value(),
            (Value::Array(x), Value::Array(y)) => {
                x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| value_eq(p, q))
            }
            (Value::InlineTable(x), Value::InlineTable(y)) => {
                x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| value_eq(v, w)))
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Drives(&'static str);
    impl ProfileEnv for Drives {
        fn drive_present(&self, l: &str) -> bool {
            self.0.contains(&l.trim_end_matches(':').to_uppercase())
        }
    }

    const PROFILES: &str = r#"
        [navigation_pane]
        home = "hide"
        [profiles.Travail.navigation_pane]
        gallery = "hide"
        [profiles.Minimal.quick_access]
        mode = "disabled"
        [[rules]]
        profile = "Minimal"
        [rules.when]
        drive_absent = "D"
    "#;

    #[test]
    fn empty_config_is_noop() {
        assert!(Config::from_toml("").unwrap().is_noop());
    }

    #[test]
    fn shipped_default_is_noop() {
        assert!(Config::from_toml(DEFAULT_CONFIG_TOML).unwrap().is_noop());
    }

    #[test]
    fn parses_example() {
        let c = Config::from_toml(
            r#"
            [navigation_pane]
            home = "hide"
            [navigation_pane.nodes]
            "Proton Drive" = "hide"
            [quick_access]
            mode = "disabled"
            [explorer_view]
            launch_to = "this_pc"
            show_file_extensions = true
            "#,
        )
        .unwrap();
        assert_eq!(c.navigation_pane.home, Visibility::Hide);
        assert_eq!(c.quick_access.mode, QuickAccessMode::Disabled);
        assert_eq!(c.explorer_view.launch_to, Some(LaunchTo::ThisPc));
        assert!(!c.is_noop());
    }

    #[test]
    fn toml_roundtrip() {
        let c = Config::from_toml(PROFILES).unwrap();
        assert_eq!(Config::from_toml(&c.to_toml().unwrap()).unwrap(), c);
    }

    #[test]
    fn json_roundtrip_as_sent_by_the_ui() {
        // L'interface renvoie le JSON qu'elle a reçu (options à null, listes vides...) : il doit être accepté tel quel.
        let c = Config::from_toml(PROFILES).unwrap();
        let v = serde_json::to_value(&c).unwrap();
        assert!(v["explorer_view"]["launch_to"].is_null());
        assert_eq!(serde_json::from_value::<Config>(v).unwrap(), c);
        let d = serde_json::to_value(Config::default()).unwrap();
        assert_eq!(serde_json::from_value::<Config>(d).unwrap(), Config::default());
    }

    #[test]
    fn profile_replaces_whole_section_only() {
        let c = Config::from_toml(PROFILES).unwrap();
        let e = c.effective(Some("Travail"));
        assert_eq!(e.navigation_pane.gallery, Visibility::Hide);
        assert_eq!(e.navigation_pane.home, Visibility::Default, "la section est REMPLACÉE, pas fusionnée");
        assert_eq!(c.effective(Some("Minimal")).quick_access.mode, QuickAccessMode::Disabled);
        assert_eq!(c.effective(None), c);
    }

    #[test]
    fn rules_and_manual_override() {
        let c = Config::from_toml(PROFILES).unwrap();
        assert_eq!(c.select_profile(None, &Drives("CE")).0.as_deref(), Some("Minimal"));
        assert_eq!(c.select_profile(None, &Drives("CDE")).0, None);
        assert_eq!(c.select_profile(Some("Travail"), &Drives("CE")).0.as_deref(), Some("Travail"));
        let (p, w) = c.select_profile(Some("Nope"), &Drives("CDE"));
        assert!(p.is_none() && w.is_some());
    }

    #[test]
    fn empty_rule_condition_never_matches() {
        let c = Config::from_toml("[profiles.A.quick_access]\nmode=\"disabled\"\n[[rules]]\nprofile=\"A\"\n").unwrap();
        assert_eq!(c.select_profile(None, &Drives("C")).0, None);
    }

    #[test]
    fn ui_save_keeps_comments_and_untouched_lines() {
        let old = r#"# ma config
version = 1

[navigation_pane]
# Accueil : je n'en veux pas
home = 'hide' # fin de ligne
gallery = "hide"

[explorer_view]
# extensions
show_file_extensions = true
"#;
        let mut c = Config::from_toml(old).unwrap();
        c.navigation_pane.gallery = Visibility::Default; // retirée
        c.explorer_view.show_file_extensions = Some(false); // modifiée
        c.quick_access.mode = QuickAccessMode::Disabled; // nouvelle section
        c.profiles.insert("Minimal".into(), Config::from_toml(PROFILES).unwrap().profiles["Minimal"].clone());
        c.rules = Config::from_toml(PROFILES).unwrap().rules;
        let text = c.to_toml_preserving(old).unwrap();
        assert_eq!(Config::from_toml(&text).unwrap(), c);
        for kept in ["# ma config", "# Accueil : je n'en veux pas", "home = 'hide' # fin de ligne", "# extensions"] {
            assert!(text.contains(kept), "« {kept} » perdu :\n{text}");
        }
        assert!(text.contains("show_file_extensions = false"), "{text}");
        // Toutes les clés sont écrites (valeur par défaut comprise) : la valeur retirée redevient « default ».
        assert!(text.contains("gallery = \"default\""), "{text}");
        assert!(!text.contains("[navigation_pane.nodes]"), "en-tête vide ajouté :\n{text}");
    }

    #[test]
    fn ui_save_falls_back_when_existing_is_unreadable() {
        let c = Config::from_toml(PROFILES).unwrap();
        assert_eq!(c.to_toml_preserving("[[[ cassé").unwrap(), c.to_toml().unwrap());
        // Rien d'ancien : identique à une écriture neuve.
        assert_eq!(Config::from_toml(&c.to_toml_preserving("").unwrap()).unwrap(), c);
    }

    #[test]
    fn typo_is_rejected() {
        assert!(Config::from_toml("[navigation_pane]\nhom = \"hide\"").is_err());
    }
}
