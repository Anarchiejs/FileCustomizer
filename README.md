# ExplorerBender (nom provisoire)

Personnalise la **structure** de l'Explorateur de fichiers Windows 11 (pas l'esthétique) : volet de navigation, Accès rapide, « Ce PC », options d'affichage, menu contextuel. L'utilisateur **déclare** ce qu'il veut dans un fichier de config (ou dans l'interface) ; un petit démon l'**applique et le maintient** quand Windows ou une application le défait.

> **État : les 3 phases sont livrées** (F1–F5, démon, CLI, helper élevé, interface, installateur). Ce qui n'a pas pu être vérifié est listé honnêtement dans [Limites connues](#limites-connues).

## Principes

- **Aucune injection, aucun hook, aucun patch** de `explorer.exe`/`shell32`/`ExplorerFrame` : seulement registre, API Shell officielles (COM), fichiers utilisateur et messages de fenêtre. Compatible avec Windhawk.
- **Défaut = ne rien toucher.** Un `config.toml` vide ne modifie rien.
- **Réversible.** Chaque valeur d'origine est sauvegardée (`backup.json`) *avant* d'être modifiée ; `explorerbender restore` remet exactement l'état d'origine (y compris les clés créées par nous et les dossiers désépinglés).
- **Pas de guerre d'écriture.** Plus de N réécritures de la même valeur en M secondes par un autre outil → le démon s'arrête sur cette valeur et le signale.
- **Jamais de redémarrage d'`explorer.exe` automatique** : uniquement sur demande explicite (`--restart-explorer`).
- **Privilèges minimaux.** Le démon n'écrit qu'en HKCU, sans admin. Ce qui est protégé en écriture passe par un **helper élevé lancé à la demande** (invite UAC), jamais par le démon. Le helper ne fait pas confiance aux fichiers de `%APPDATA%` : il ne restaure que les valeurs d'une liste blanche et refuse un dossier de données redirigé (lien, jonction).
- Pas de réseau, pas de télémétrie.

## Installation

### Installateur (recommandé)
```powershell
cargo build --release                          # démon, CLI, helper
cargo build --release --manifest-path ui\src-tauri\Cargo.toml   # interface
& "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe" installer\ExplorerBender.iss
.\dist\ExplorerBender-Setup-0.1.0.exe          # installation par utilisateur, sans élévation
```
L'installateur copie les binaires dans `%LOCALAPPDATA%\Programs\ExplorerBender`, crée `config.toml` (inactif), crée la **tâche planifiée** `ExplorerBender` (ouverture de session, délai 0, priorité 3 = au-dessus de la normale mais pas temps réel, privilèges normaux, une instance, relance auto en cas de plantage), démarre le démon et crée un raccourci vers l'interface. À la désinstallation il **propose la restauration** de l'état d'origine puis la suppression des données.

### Scripts
`scripts\install.ps1` / `scripts\uninstall.ps1` font la même chose sans installateur (`-AddToPath`, `-KeepSettings`, `-RemoveData`, `-Yes`).

Pas de clé `Run` : elle démarre trop tard. Le démon est prêt ~30 ms après sa création ; le logon trigger le lance dès l'ouverture de session mais Windows ne garantit pas qu'il précède le premier `explorer.exe`. Ce n'est pas bloquant : le registre est appliqué immédiatement, l'Explorateur relit ces valeurs à chaque nouvelle fenêtre, et `TaskbarCreated` (redémarrage d'Explorer) déclenche une réapplication.

## Interface de configuration

`explorerbender-ui.exe` (Tauri 2, WebView2) est un processus **séparé, lancé à la demande, jamais résident** : il lit/écrit `config.toml`, le démon détecte le changement. Pages : vue d'ensemble (état, conflits, aperçu/appliquer/restaurer), volet de navigation (nœuds détectés), Accès rapide, Ce PC (dossiers + lecteurs détectés avec étiquette), affichage, menu contextuel (extensions et verbes détectés), profils et règles, TOML avancé. Un sélecteur « Édition de » permet d'éditer la configuration de base ou un profil (section par section : héritée ou remplacée).
Les boutons délèguent au CLI : un seul chemin de code modifie le système. La page web n'a accès qu'à 5 commandes dédiées (aucun accès fichier/shell générique) ; tout texte lu dans le registre est affiché via `textContent`.
Enregistrer depuis l'interface réécrit `config.toml` **sans commentaires** (l'ancienne version est gardée dans `config.toml.bak`) ; l'onglet « TOML avancé » conserve les commentaires.

## Configuration

`%APPDATA%\ExplorerBender\config.toml` — relu dès qu'il change. Erreur de syntaxe → refusée, **l'ancienne configuration reste active** (signalé). `explorerbender init` écrit un modèle commenté complet. Les clés inconnues sont rejetées (détecte les fautes de frappe).

```toml
version = 1

[general]
log_level = "info"              # off | error | warn | info | debug
allow_untested_builds = false
conflict_max_rewrites = 5       # N réécritures ...
conflict_window_secs = 30       # ... en M secondes => conflit
debounce_ms = 1500

[navigation_pane]               # default | show | hide
home = "hide"
gallery = "hide"
[navigation_pane.nodes]         # nom (cf. `explorerbender nodes`) ou CLSID
"Proton Drive" = "hide"

[quick_access]
mode = "disabled"               # default | disabled | whitelist
# whitelist = ['C:\Documents']
# show_frequent = false / show_recent = false

[this_pc]                       # ÉLÉVATION requise (apply --elevate)
hide_folders = ["videos", "3d-objects"]   # desktop documents pictures music videos downloads 3d-objects
show_folders = []
hide_drives = ["D"]             # masque dans l'Explorateur, n'empêche PAS l'accès par chemin

[explorer_view]                 # true/false ; absent = ne pas toucher
show_file_extensions = true
show_hidden_files = true
show_system_files = false
launch_to = "this_pc"           # this_pc | home | downloads | onedrive
use_checkboxes = false
nav_show_all_folders = false
nav_expand_to_current_folder = true
sync_provider_notifications = false
compact_mode = true
show_status_bar = true
hide_drives_with_no_media = true

[context_menu]
classic_menu = true
blocked_extensions = ["{CLSID}", "Nom"]    # `explorerbender shell-extensions`
disabled_verbs = ['Directory\shell\cmd']   # `explorerbender verbs`

# Profils : chaque section présente REMPLACE la section de base.
[profiles.Minimal.navigation_pane]
home = "hide"
[profiles.Minimal.quick_access]
mode = "disabled"

# Règles : la première qui correspond choisit le profil (apply <profil> manuel les supplante).
[[rules]]
profile = "Minimal"
[rules.when]
drive_absent = "D"              # et/ou drive_present = "E"
```

### Accès rapide
| Mode | Registre | Dossiers épinglés | Surveillance |
|---|---|---|---|
| `default` | rien (sauf `show_*`) | rien — ré-épingle ce que nous avions retiré | — |
| `disabled` | `ShowFrequent=0`, `ShowRecent=0` | tous désépinglés | oui |
| `whitelist` | `ShowFrequent=0`, `ShowRecent=0` sauf `show_*=true` | tous sauf `whitelist` | oui |

Les dossiers **fréquents** (chez vous ReviPlan, NamelessXIII, sketch : ce ne sont *pas* des épingles) ne sont pas purgés de l'historique : `ShowFrequent=0` les masque. Les dossiers désépinglés sont mémorisés et ré-épinglés par `restore`.

### Profils et règles (F5)
`explorerbender apply Minimal` choisit un profil à la main (écrit `%APPDATA%\ExplorerBender\profile`) ; `apply --auto` redonne la main aux règles. Les règles se réévaluent quand `config.toml`/`profile` changent **et** quand un lecteur apparaît ou disparaît (`WM_DEVICECHANGE`, sans polling). Une règle sans condition ne correspond jamais.

## CLI

```
explorerbender init
explorerbender apply [profil] [--auto] [--dry-run] [--elevate] [--restart-explorer] [--config f]
explorerbender profiles                   profils, règles, profil actif
explorerbender status                     build, démon, profil, backup, conflits, état de chaque tweak
explorerbender restore [--dry-run] [--restart-explorer]    (élévation UAC automatique si nécessaire)
explorerbender stop                       arrête le démon sans rien restaurer
explorerbender nodes | drives | shell-extensions | verbs     inventaires pour la config
explorerbender validate [fichier]
explorerbender debug-pin <dossier>        diagnostic
```
`restore` suspend le démon (marqueur `disabled`) ; `apply` le réactive.

## Architecture

```
crates/core            config, RegistryBackend/ShellBackend (trait + Mock), backup, conflits, profils,
                       tweaks, moteur, compat, lecteurs — testable sans effet de bord
crates/daemon          démon résident : fenêtre cachée (TaskbarCreated, WM_DEVICECHANGE),
                       RegNotifyChangeKeyValue, ReadDirectoryChangesW, minuterie ; zéro polling
crates/cli             explorerbender.exe
crates/elevated-helper explorerbender-elevated.exe : lancé à la demande (UAC), jamais résident,
                       ne fait que les tweaks `needs_elevation`, clés issues d'une table fixe
ui/src-tauri + ui/dist interface Tauri 2 (espace de travail Cargo séparé)
installer/             ExplorerBender.iss (Inno Setup)
scripts/               install.ps1, uninstall.ps1, register-task.ps1
docs/                  PHASE0.md, TESTING.md, COMPAT.md
```

Chaque fonctionnalité est un `Tweak` : métadonnées, `detect()` (lecture seule), `apply()` (réconciliation idempotente, restaure ce que la config ne demande plus), `revert()`, `watch()`. La table `compat.rs` liste les builds validées ; hors table, le tweak est désactivé avec avertissement.

## Tweaks et clés touchées

| Tweak | Clés / API | Élévation |
|---|---|---|
| `navpane` | `HKCU\Software\Classes\CLSID\{clsid}` : `System.IsPinnedToNameSpaceTree` | non |
| `quick-access` | `HKCU\…\Explorer` : `ShowFrequent`, `ShowRecent` ; épingles via `windows.storage.dll` (verbes `pintohome`/`unpinfromhome`) ; surveille `…\Recent\AutomaticDestinations\f01b4d95cf55d32a.automaticDestinations-ms` | non |
| `thispc-drives` | `HKCU\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer` : `NoDrives` (bits existants conservés) | **oui** : clé en lecture seule pour l'utilisateur sur Windows 11 25H2 |
| `thispc-folders` | `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\FolderDescriptions\{GUID}\PropertyBag` : `ThisPCPolicy` (+ WOW6432Node) | **oui** |
| `explorer-view` | `HKCU\…\Explorer\Advanced` : `HideFileExt`, `Hidden`, `ShowSuperHidden`, `LaunchTo`, `AutoCheckSelect`, `NavPaneShowAllFolders`, `NavPaneExpandToCurrentFolder`, `ShowSyncProviderNotifications`, `UseCompactMode`, `ShowStatusBar`, `HideDrivesWithNoMedia` | non |
| `context-menu` | `HKCU\Software\Classes\CLSID\{86ca1aa0-…}\InprocServer32` (défaut vide) ; `HKCU\…\Shell Extensions\Blocked` ; `HKCU\Software\Classes\<…>\shell\<verbe>` : `LegacyDisable` | non |

Guid « Ce PC » : variantes `Local*` (Documents `{f42ee2d3-…}`, Téléchargements `{7d83ee9b-…}`, Images `{0ddd015d-…}`, Musique `{a0c69a99-…}`, Vidéos `{35286a68-…}`), Bureau `{B4BFCC3A-…}`, Objets 3D `{31C0DD25-…}` (absent de cette build : ignoré proprement).

## Dépannage

| Symptôme | Piste |
|---|---|
| Rien ne change | `explorerbender status` : tweak « NON VALIDÉ », démon arrêté, config rejetée ? Ouvrez une **nouvelle** fenêtre ; sinon `apply --restart-explorer` (explicite). |
| `CONFLIT` | Un autre outil (Windhawk, une app cloud) remet la valeur. Le démon a cessé de se battre ; modifiez la config ou redémarrez Explorer pour retenter. |
| « nécessite l'élévation » | Normal pour `this_pc` : `explorerbender apply --elevate` (ou « Appliquer avec élévation » dans l'interface). |
| Invite UAC refusée | Rien n'est écrit dans les zones protégées ; relancez quand vous voulez. Si votre compte est « standard », l'UAC demande un administrateur : `HKCU` est alors **le sien**, pas le vôtre — utilisez un compte administrateur. |
| Journal | `%APPDATA%\ExplorerBender\logs\` (tournant 256 Ko × 2). |
| Revenir à l'origine | `explorerbender restore`. Ce qui n'a pas pu l'être reste dans `backup.json`. |

## Limites connues

- **Testé en conditions réelles** : installé avec l'installateur et utilisé sur la machine de développement (build 26200), globalement fonctionnel. Les points fins de la checklist (`docs/TESTING.md`) n'ont pas tous été confirmés un à un, notamment les valeurs de `LaunchTo` (1 Ce PC, 2 Accueil, 3 Téléchargements, 4 OneDrive), issues de la documentation communautaire, et le rafraîchissement d'une fenêtre Explorateur déjà ouverte.
- **Interface** : son WebView2 est requis (présent sur Windows 11).
- **Réseau, Linux (WSL), OneDrive** : Réseau/Linux ne portent pas `System.IsPinnedToNameSpaceTree` ici, OneDrive est absent. Non implémenté (règle : pas de clé non vérifiée).
- **Ordre des nœuds du volet** : non fait (`SortOrderIndex` existe mais son effet n'est pas validé).
- **Ne peut pas être fait sans hook** (donc pas fait) : boutons dans la barre de commandes, réordonnancement fin du volet, modification du rendu des entrées.
- Extensions de menu contextuel « modernes » (paquets MSIX, ex. PDFelement) : `Shell Extensions\Blocked` vise les CLSID ; son effet sur les extensions empaquetées n'est pas validé.
- `NoDrives` masque dans l'Explorateur, n'empêche pas l'accès par chemin.
- Les réglages élevés ne sont pas maintenus par le démon (il n'écrit jamais en zone protégée) : ils persistent d'eux-mêmes, mais si un autre outil les défait il faut relancer `apply --elevate`.
- Un `WM_DEVICECHANGE` réel (branchement d'un disque) n'a pas été testé ; le message a été simulé.
- Un conflit détecté reste actif jusqu'à un changement de config ou un redémarrage d'Explorer.
- Builds : **26200.9457 (25H2)** testée ; 26100 déclarée par analogie.
