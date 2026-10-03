# Tests

## Automatiques

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace       # 59 tests, aucun effet sur le registre (voir ci-dessous)
node ui/test/run.mjs         # 9 scénarios de l'interface (Edge ou Chrome headless, sans npm)
```

La CI GitHub Actions (`.github/workflows/ci.yml`, runner Windows) lance les mêmes commandes, plus fmt et clippy sur `ui/src-tauri`, à chaque push sur `main` et sur chaque pull request. Le lint `undocumented_unsafe_blocks` est actif : tout bloc `unsafe` doit être précédé d'un commentaire `// SAFETY:`.

Couvert : défauts = rien à faire ; config invalide rejetée ; idempotence (2e passe = 0 écriture) ; `--dry-run` n'écrit rien ; restauration exacte (valeur, absence de valeur, clés créées) ; « première sauvegarde gagne » ; guerre d'écriture arrêtée ; épingles : désépinglage, liste blanche, ré-épinglage, jamais de bascule sur un élément non épinglé, dry-run, `revert`.

Tests d'intégration sur les vrais binaires, chacun dans un `EXPLORERBENDER_HOME` temporaire :

- **CLI** (`crates/cli/tests`) : `init` ne réécrit jamais une config existante et produit une config sans effet ; `validate` rejette un TOML cassé ; profil inconnu refusé ; `apply`/`restore --dry-run` ne créent ni backup ni marqueur.
- **Démon** (`crates/daemon/tests`) : sortie immédiate s'il est suspendu ; `status.json` écrit au démarrage ; deuxième instance bloquée par le mutex ; rechargement de `config.toml` sur événement ; config invalide signalée sans arrêt ; arrêt propre par l'événement nommé. Le mutex et l'événement dépendent du dossier de données : un vrai démon sur la session n'est ni gêné ni arrêté.
- **Interface** (`ui/test/scenarios.html`, lancé par `ui/test/run.mjs`) : le vrai `app.js` piloté par clics et saisies contre un backend simulé (`ui/test/mock-state.js`) — noms venus du système insérés comme texte (une injection HTML/script échoue le test), masquer un nœud puis enregistrer, liste blanche nettoyée, annulation, action refusée tant que des modifications sont en cours, section de profil héritée/remplacée, suppression d'un profil et de ses règles.
- **Enregistrement depuis l'interface** (`config.rs`) : commentaires et lignes inchangées de `config.toml` conservés, repli sur une écriture neuve si le fichier existant est illisible.
- **Helper élevé** (`crates/elevated-helper/tests`, sans élévation, en `--dry-run`) : une entrée forgée dans `backup.json` (ex. `HKLM\...\Run`) est refusée et signalée, l'entrée légitime est restaurée, rien n'est perdu du fichier ; un dossier de données qui est une jonction est refusé avant toute écriture (code 3).

## Test en conditions réelles

| | |
|---|---|
| Rapporté le | 2026-10-03 |
| Testeur | l'auteur du projet, sur sa machine personnelle (pas une VM) |
| Système | Windows 11 25H2, build 26200.9457 |
| Version | 0.1.0, installée avec `ExplorerBender-Setup-0.1.0.exe` |
| Résultat | **globalement fonctionnel** |

Ce test valide la chaîne complète en usage réel : installation, démarrage du démon par la tâche planifiée, interface et application de la configuration dans l'Explorateur.

Ce qu'il ne couvre pas : les points de la checklist ci-dessous n'ont pas tous été cochés un à un. Restent à confirmer : le rafraîchissement d'une fenêtre Explorateur déjà ouverte et le cycle de désinstallation avec restauration. Un problème constaté plus tard doit être ajouté ici avec la build Windows concernée.

### Vérifications ciblées (2026-10-03, même machine)

| Point | Méthode | Résultat |
|---|---|---|
| Valeurs de `LaunchTo` | démon arrêté ; pour chaque valeur, `explorer.exe` sans argument puis lecture de la nouvelle fenêtre via `Shell.Application` | 1 → Ce PC, 2 → Accueil, 3 → Téléchargements, 4 → dossier du fournisseur cloud principal (Proton Drive ici, OneDrive absent). Valeur d'origine restaurée, démon relancé. |
| Vrai `WM_DEVICECHANGE` | démon isolé (`EXPLORERBENDER_HOME`), règle `drive_present = "Q"`, `subst Q: …` puis `subst Q: /d` | `profil actif : Test` ~3 s après l'apparition, `aucun (base)` après le retrait. |

Piège : un shell lancé depuis une application empaquetée (MSIX, par ex. l'application de bureau Claude) voit un **HKCU virtualisé** — ses lectures et écritures ne sont pas celles du démon. Pour ces vérifications, lancer les commandes hors du conteneur (par ex. `Invoke-CimMethod Win32_Process Create`) ; un `explorerbender stop` doit toujours garder le même `EXPLORERBENDER_HOME` que le démon visé.

## Intégration manuelle (checklist)

À faire avec `EXPLORERBENDER_HOME=<dossier de test>` pour isoler config/backup/journaux. Le registre, lui, est bien réel : terminer par `explorerbender restore`.

### navpane (Accueil, Galerie, nœuds)
1. `config.toml` : `home="hide"`, `gallery="hide"`. `explorerbender apply`. **Ouvrir une NOUVELLE fenêtre Explorateur** : Accueil et Galerie ont disparu du volet. Une fenêtre déjà ouverte se rafraîchit-elle ? (noter le résultat ; sinon `--restart-explorer`).
2. `explorerbender nodes` : « EFFECTIF » = masqué, « OVERRIDE HKCU » = 0.
3. Dérive : `Set-ItemProperty HKCU:\Software\Classes\CLSID\{f874310e-…} System.IsPinnedToNameSpaceTree 1 -Type DWord` avec le démon lancé → remis à 0 en ~1,5 s.
4. Conflit : réécrire la valeur toutes les 2 s, 10 fois → `status` affiche `CONFLIT`, le démon cesse de se battre.
5. `restore` : la clé HKCU de Accueil/Galerie n'existe plus (`reg query`), Proton Drive retrouve sa valeur 1 d'origine.

### quick-access
1. Créer un dossier de test, `explorerbender debug-pin <dossier>` (épinglé).
2. `mode="whitelist"` avec la liste de **vos** dossiers épinglés actuels → `apply --dry-run` ne propose que le dossier de test ; `apply` le désépingle, les autres restent.
3. Démon lancé : `debug-pin` du dossier de test → désépinglé automatiquement en ~1,5 s, **une seule passe** dans `daemon.log` (pas de boucle).
4. `restore` : le dossier de test est ré-épinglé (puis `debug-pin` pour le retirer).
5. `mode="disabled"` : `ShowFrequent`/`ShowRecent` à 0, plus aucune épingle ; `restore` rend l'état d'origine (clés absentes supprimées).

### démon
1. Instance unique : lancer deux fois, un seul processus.
2. `explorerbender stop` / `restore` : arrêt propre, marqueur `disabled` respecté au démarrage suivant.
3. `TaskbarCreated` : poster le message à la fenêtre cachée (classe `ExplorerBenderHidden`) → réapplication ~1,5 s après.
4. Au repos : CPU 0 ms sur 30 s, aucun thread actif.

### this_pc (élévation)
1. Cibles inoffensives : `hide_drives = ["Y"]`, `hide_folders = ["objets 3d"]`. `apply` sans élévation : « nécessite l'élévation », rien d'écrit (aucune entrée dans `backup.json`).
2. `apply --elevate` : invite UAC ; `reg query` montre `NoDrives` (0x1000000 pour Y) dans `HKCU\...\Policies\Explorer` et `ThisPCPolicy=Hide` (natif + WOW6432Node). 2e `apply` : « inchangé », aucune invite.
3. `restore` : invite UAC ; la clé `Policies\Explorer` et les `PropertyBag` créés disparaissent ; Vidéos reste à `Show`.
4. Avec vos vraies cibles : ouvrir « Ce PC » dans une nouvelle fenêtre et constater que les entrées ont disparu.

### explorer-view / context-menu
1. `show_file_extensions = true`, `launch_to = "this_pc"` : `reg query` (HideFileExt=0, LaunchTo=1) ; nouvelle fenêtre : extensions visibles, ouverture sur Ce PC.
2. `disabled_verbs = ['Directory\shell\find']` : `reg query HKCR\Directory\shell\find` montre `LegacyDisable` **et** les valeurs d'origine (fusion HKCU/HKLM) ; clic droit sur un dossier : l'entrée a disparu.
3. `classic_menu = true` : nouvelle fenêtre, clic droit → menu classique (parfois redémarrage d'Explorer).
4. `blocked_extensions = ["{GUID}"]` avec une extension réelle (liste : `shell-extensions`) puis `restore`.

### profils
1. Deux profils + une règle `drive_absent = "Z"` : `profiles` indique le profil actif ; `apply` l'applique.
2. `apply Autre` : bascule, restaure ce que l'ancien profil avait posé ; `apply --auto` revient aux règles.
3. Démon lancé : écrire `Autre` dans `%APPDATA%\ExplorerBender\profile` → bascule en ~2 s ; supprimer le fichier → retour à la règle.
4. Brancher/débrancher un lecteur utilisé par une règle → le profil change sans action.

### interface
1. Lancer `explorerbender-ui.exe` : pages, enregistrement (« Enregistrer » → config.toml mis à jour en place, commentaires conservés, ancienne version dans `config.toml.bak`), « Aperçu », « Appliquer », « Tout restaurer ».
2. Banc de test sans Tauri : `.claude/launch.json` (« ui-mock ») sert `ui/` ; ouvrir `/test/mock.html` (manipulation libre) ou `/test/scenarios.html` (scénarios automatiques).

## Mesures (build release, Windows 26200.9457)

| Mesure | A : volet seul (sans COM) | B : volet + Accès rapide liste blanche (COM) |
|---|---|---|
| Prêt (registre appliqué, surveillances armées), depuis la création du processus | **29 ms** (9 ms internes) | **22 ms** (7 ms internes) |
| Passe Shell/COM différée (en arrière-plan) | — | ~170–230 ms (froid) |
| RAM, working set au repos | **0,46 Mo** | **2,69 Mo** |
| RAM, mémoire privée au repos | 1,88 Mo | 4,31 Mo |
| CPU sur 30 s au repos | **0,0 ms** | **0,0 ms** |
| Threads / handles | 2 / 155 | 10 / 339 |
| Taille `explorerbender-daemon.exe` | 446 Ko | |

La passe différée mesurée à 740–878 ms contenait la notification Shell (`SHChangeNotify` + diffusion `WM_SETTINGCHANGE`), qui n'a lieu que lorsqu'une valeur vient réellement d'être écrite — jamais aux logons suivants une fois l'état conforme.

### Phase 2/3 (daemon release, profils + règles)
Prêt en 32 ms après la création du processus (9 ms internes) ; 0,45 Mo de working set, 1,95 Mo de mémoire privée, 3 threads, 0,0 ms de CPU sur 30 s au repos. Daemon : 553 Ko.
