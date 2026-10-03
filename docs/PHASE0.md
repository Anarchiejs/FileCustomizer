# Phase 0 — état de la machine (2026-10-03)

Relevé en lecture seule sur la machine cible. Rien n'a été modifié dans le registre.

## Système
- Windows 11 25H2, build **26200.9457** (le registre affiche `ProductName = "Windows 10 Home"` : bug connu, se fier au numéro de build, jamais au nom).
- Lecteurs : A:, C:, D:, E:. Dossiers utilisateur relocalisés (`C:\Documents`, `C:\Downloads`, `C:\Desktop`, `C:\Pictures`, `C:\Music`, `C:\Videos`).
- Windhawk actif (2 processus). Mods actifs touchant l'Explorateur : `windows-11-file-explorer-styler`, `explorer-details-better-file-sizes`, `modernize-folder-picker-dialog`. Aucun ne semble gérer le volet de navigation → coexistence OK, détection de conflit conservée par sécurité.

## Chaîne d'outils : MANQUANTE
| Outil | État |
|---|---|
| `cargo` / `rustc` / `rustup` | absents |
| MSVC (`link.exe`, VS Build Tools), Windows SDK | absents |
| Inno Setup / WiX | absents |
| `winget`, `dotnet`, `node` | présents |

## Hypothèses du cahier des charges vs réalité

| Sujet | Hypothèse | Constat | Statut |
|---|---|---|---|
| CLSID Accueil `{f874310e-…}` | existe, porte `System.IsPinnedToNameSpaceTree` | présent en HKLM (+WOW6432), valeur 1, **aucun override HKCU** | ✅ CLSID confirmé ; effet réel de l'override HKCU à tester |
| CLSID Galerie `{e88865ea-…}` | idem | idem, valeur 1 | ✅ idem |
| Accès rapide `{679f85cb-…}` | nœud du volet | présent, valeur 1 | ✅ |
| OneDrive | présent | **absent** de cette machine | ⚠️ |
| Proton Drive | présent | `HKCU\…\CLSID\{105FEB6F-4195-4140-B5BB-1FBDCB984AA1}`, valeur 1 | ✅ |
| Bibliothèques | à masquer | `{031E4825-…}` "UsersLibraries", **déjà à 0** (HKLM) | ⚠️ déjà masqué |
| Réseau, Linux/WSL | portent la propriété | **ne la portent pas** → autre mécanisme à vérifier (clé `Desktop\NameSpace`/`NonEnum`, `HideDesktopIcons`…) | ❌ à investiguer |
| Accès rapide : « dossiers épinglés » ReviPlan, NamelessXIII, sketch | épinglés | **ce sont des dossiers FRÉQUENTS**, pas épinglés (verbe `Supprimer de l'accès rapide` au lieu de `Désépingler`). Verbe d'épinglage = `unpinfromhome` pour les vrais épinglés, `removefromhome` pour les fréquents | ⚠️ le démon doit traiter les deux catégories |
| Contenu de `shell:::{679f85cb-…}` | dossiers épinglés | contient aussi la **Corbeille** (épinglée) et ~25 **fichiers récents** (verbe `Supprimer de la liste`) | ⚠️ |
| `ShowFrequent`, `ShowRecent` | valeurs présentes | **absentes** (= défaut Windows : activé) | ✅ à créer avec backup « n'existait pas » |
| `f01b4d95cf55d32a.automaticDestinations-ms` | fichier surveillé | existe, 152 Ko, modifié plusieurs fois par heure (chaque accès à un dossier) → **debounce indispensable**, comparer l'ensemble épinglé avant d'agir | ✅ |
| `NoDrives` | à poser | absent (aucun lecteur masqué) | ✅ |
| Menu classique `{86ca1aa0-…}` | à poser | clé absente (menu Win11 actif) | ✅ |
| `Shell Extensions\Blocked` | liste | vide (HKLM et HKCU) | ✅ |
| `ThisPCPolicy` | à mettre à `Hide` | `Personal`(Documents) = **Hide déjà**, `Downloads` = **Hide déjà**, Vidéos/Musique/Images = Show, Bureau = valeur absente (clé PropertyBag présente), Objets 3D = clé PropertyBag **absente** | ⚠️ état initial non uniforme → backup « existait ou non » indispensable |
| `Explorer\Advanced` | — | `Hidden=2`, `HideFileExt=1`, `ShowSuperHidden=0`, `AutoCheckSelect=0`, `NavPaneExpandToCurrentFolder=1`, `LaunchTo` absente | ✅ |

## Effet de bord découvert (important pour la conception)
Énumérer les **verbes** d'un élément Shell (`item.Verbs()`) charge toutes les extensions de menu contextuel dans le processus appelant. Effet constaté : l'extension *Wondershare PDFelement* a désinstallé/réinstallé son paquet MSIX épars (`PEShellExtension.msix`) à ce moment. → Le démon **ne doit jamais énumérer les verbes**. Il invoquera directement `unpinfromhome` / `removefromhome` par nom canonique (`InvokeVerb`), sans passer par `Verbs()`, et ne le fera que quand l'état à corriger est détecté.

## Points à valider empiriquement (avant de les activer dans la table de compatibilité)
1. L'override HKCU de `System.IsPinnedToNameSpaceTree` pour Accueil/Galerie est-il pris en compte sur 26200 sans redémarrer l'Explorateur (nouvelle fenêtre) ?
2. Mécanisme réel de masquage de Réseau, Linux, Proton Drive sur cette build.
3. Le verbe `unpinfromhome` sans énumération fonctionne-t-il sur les éléments de `shell:::{679f85cb-…}` ?

## Points à valider — résultats (2026-10-03)
1. **Override HKCU Accueil/Galerie** : le Shell relit la valeur surchargée immédiatement (`ExtendedProperty('System.IsPinnedToNameSpaceTree')` : 1 → 0, puis 0 → 1 après restauration). Le rafraîchissement *visuel* d'une fenêtre déjà ouverte n'a pas pu être constaté (UI Automation ne voit pas le contenu du volet ; capture d'écran abandonnée) — à vérifier à l'œil, voir `TESTING.md`.
2. **Réseau / Linux / OneDrive** : non traités (Réseau/Linux sans la propriété, OneDrive absent). À investiguer avant implémentation.
3. **Désépinglage sans énumérer les verbes** : validé, mais le mécanisme est différent de l'hypothèse : le CLSID `{b455f46e-…}` est un handler `DelegateExecute` (`IExecuteCommand`), pas un `IExplorerCommand`. C'est le **nom du verbe** passé à `IInitializeCommand` (`pintohome` / `unpinfromhome`) qui décide de l'action — ce n'est donc pas une bascule.
4. **Détection de l'état épinglé** : propriété `System.Home.IsPinned` (booléen) — sans charger d'extension tierce.

## Phases 2 et 3 — relevé complémentaire (2026-10-03)
- `FolderDescriptions` : variantes `Local*` (Documents, Téléchargements, Images, Musique, Vidéos) à `ThisPCPolicy=Show` en natif **et** WOW6432Node ; Bureau : `PropertyBag` présent sans valeur ; **Objets 3D : `PropertyBag` absent**. Les GUID non-`Local` (`Personal`, `374DE290…`) sont les « Hide » par défaut de Windows et ne sont pas ceux de « Ce PC ».
- **`HKCU\…\CurrentVersion\Policies` : lecture seule pour l'utilisateur** (ReadKey ; contrôle total SYSTEM/Administrateurs) et `Policies\Explorer` n'existe pas → `NoDrives` ne peut pas être écrit par le démon : élévation nécessaire (différent de l'hypothèse « HKCU sans admin »).
- Vue HKCR fusionnée : un `LegacyDisable` posé en HKCU sur `Directory\shellind` s'ajoute aux valeurs HKLM (`SuppressionPolicy`, sous-clé `command` conservés).
- `Shell Extensions\Blocked` : vide en HKLM et HKCU ; `Approved` : 27 entrées. Les gestionnaires de menu contextuel détectés incluent des extensions tierces (MEGA, WinRAR, PDFelement…).
- Lecteurs A:, C:, D:, E: tous de type fixe.
