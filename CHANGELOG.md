# Changelog

Format inspiré de [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/).

## [Non publié]

Corrections issues d'un second audit.

### Corrigé
- **`backup.json` pouvait perdre des entrées** : le démon, le CLI (lancé aussi par l'interface) et le helper élevé le lisaient une seule fois puis le réécrivaient entièrement depuis leur copie. Le démon effaçait ainsi les entrées HKLM ajoutées par `apply --elevate` (« Ce PC : dossiers » plus restaurables), et la « valeur d'origine » pouvait devenir la nôtre. Chaque modification est désormais une lecture-modification-écriture sous verrou (mutex nommé), à partir de l'état du disque ; chaque passe relit le fichier ; `restore` suspend le démon avant de lire le backup.
- Un caractère non-ASCII dans une donnée binaire de `backup.json` faisait planter (panique) le démon, le CLI ou le helper : erreur propre à la place.
- `backup.json` illisible bloquait tout : la version précédente (`backup.json.bak`) prend le relais, le fichier abîmé est conservé (`backup.json.corrupt`).
- L'interface écrasait sans prévenir un `config.toml` modifié ailleurs depuis son ouverture : l'enregistrement est refusé, il faut recharger.
- Lecture de registre : nouvel essai si la valeur grandit entre deux appels ; suppression des clés créées dans la bonne vue (32/64 bits) ; comparaison des clés insensible à la casse.

### Sécurité
- Désinstallation (installateur et `uninstall.ps1`) : le dossier de données n'est plus supprimé en administrateur (`DelTree`), mais par `filecustomizer purge-data`, avec les droits de l'utilisateur et sans suivre de lien ni de point de jonction.
- Liste blanche du helper élevé : `ThisPCPolicy` d'origine limitée à `Show`/`Hide`, `NoDrives` aux 26 bits de lecteurs.
- `--restart-explorer` ignoré dans un processus élevé (il visait les Explorateurs de toutes les sessions et relançait l'Explorateur élevé).
- CI/release : actions épinglées par SHA, Inno Setup en version figée avec empreinte vérifiée, `cargo audit`, contrôle de cohérence des versions (tag, Cargo, interface, installateur).

### Performances
- Un événement registre ne déclenche plus qu'une passe « registre seul » (sans énumération COM de l'Accès rapide ni vidage de la mémoire de travail) ; la résolution des noms de nœuds du volet est mise en cache jusqu'au prochain changement de config ; `status.json` n'est plus synchronisé sur disque à chaque passe.

### Divers
- `[general]` : `debounce_ms` (0–60000), `conflict_max_rewrites` (1–1000) et `conflict_window_secs` (1–3600) sont validés.

## [0.2.0] — 2026-10-03

Version corrective issue d'un audit complet (sécurité, robustesse, build).

### Sécurité
- **Installation dans Program Files** (installateur avec invite UAC) au lieu de `%LOCALAPPDATA%\Programs` : en 0.1.0, n'importe quel programme de l'utilisateur pouvait remplacer `filecustomizer-elevated.exe` et obtenir les droits administrateur à la prochaine invite UAC acceptée. Une installation 0.1.0 est retirée automatiquement (données conservées).
- **Le helper élevé fait toutes ses lectures/écritures de fichiers avec le jeton non élevé de l'utilisateur** (pris sur l'Explorateur de la session). Un lien ou une jonction dans le dossier de données ne peut plus rediriger une écriture administrateur, même par une course entre vérification et écriture. Le contrôle des jonctions couvre aussi les dossiers parents. Si l'invite UAC est validée avec un autre compte que celui de la session, le helper refuse (code 4) au lieu d'écrire dans le mauvais profil.
- Le CLI lancé élevé (désinstallateur) applique la même règle et lance le helper sans seconde invite.

### Corrigé
- Le démon s'arrêtait au-delà d'environ 60 verbes désactivés (limite de 63 handles de `MsgWaitForMultipleObjects`) : une seule surveillance de `Software\Classes` au-delà de 8 verbes, et plafond global de surveillances.
- `backup.json`, `status.json` et `config.toml` (interface) sont forcés sur disque avant le renommage : une coupure de courant ne laisse plus de fichier vide.
- Désinstallateur : code de retour non initialisé quand `filecustomizer.exe` manquait (faux message d'échec).
- Tâche planifiée : nom d'utilisateur et chemin échappés dans le XML (un `&` cassait l'inscription).
- La détection « élévation requise » ne dépend plus du texte des messages (champ `needs_elevation`).
- `--home` passé au helper : un `\` final n'échappe plus le guillemet fermant.

### Divers
- Builds de dev bien plus légers (`target/` : ~3 Go → ~450 Mo ; interface : ~4 Go → ~850 Mo).
- `rust-version = "1.88"` déclarée ; restes de l'ancien nom retirés.

## [0.1.0] — 2026-10-03

Première version complète (phases 1 à 3), sous le nom **File Customizer** (binaires `filecustomizer*`, données dans `%APPDATA%\FileCustomizer`, variable `FILECUSTOMIZER_HOME`).

### Ajouté
- Volet de navigation, Accès rapide, « Ce PC », options d'affichage, menu contextuel, profils et règles.
- Démon (HKCU uniquement, détection de conflits), CLI, helper élevé à la demande (liste blanche), interface Tauri.
- Sauvegarde avant modification et `restore` exact.
- Installateur Inno Setup, scripts d'installation, CI (fmt, clippy, tests, scénarios UI), workflow de release sur tag `vX.Y.Z`.

### Vérifié sur Windows 11 25H2 (26200.9457)
- Installation par l'installateur, valeurs de `LaunchTo`, changement de profil sur `WM_DEVICECHANGE`.
- Non confirmé : rafraîchissement d'une fenêtre Explorateur déjà ouverte, cycle de désinstallation avec restauration.
