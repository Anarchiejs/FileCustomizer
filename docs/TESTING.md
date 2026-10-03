# Tests

## Automatiques

```
cargo test -p eb-core        # 42 tests, backend de registre et Shell simulés, aucun effet sur le système
cargo clippy --workspace
```

Couvert : défauts = rien à faire ; config invalide rejetée ; idempotence (2e passe = 0 écriture) ; `--dry-run` n'écrit rien ; restauration exacte (valeur, absence de valeur, clés créées) ; « première sauvegarde gagne » ; guerre d'écriture arrêtée ; épingles : désépinglage, liste blanche, ré-épinglage, jamais de bascule sur un élément non épinglé, dry-run, `revert`.

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
1. Lancer `explorerbender-ui.exe` : pages, enregistrement (« Enregistrer » → config.toml mis à jour, ancienne version dans `config.toml.bak`), « Aperçu », « Appliquer », « Tout restaurer ».
2. Banc de test sans Tauri : `.claude/launch.json` (« ui-mock ») sert `ui/` ; ouvrir `/test/mock.html`.

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
