# Changelog

Format inspiré de [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/).

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
