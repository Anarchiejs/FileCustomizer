# Changelog

Format inspiré de [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/).

## [Non publié]

### Ajouté
- Workflow `release.yml` : un tag `vX.Y.Z` construit l'installateur et le joint à une release GitHub, avec sa somme SHA-256.

## [0.1.0] — 2026-10-03

Première version complète (phases 1 à 3).

### Ajouté
- Volet de navigation, Accès rapide, « Ce PC », options d'affichage, menu contextuel, profils et règles.
- Démon (HKCU uniquement, détection de conflits), CLI, helper élevé à la demande (liste blanche), interface Tauri.
- Sauvegarde avant modification et `restore` exact.
- Installateur Inno Setup, scripts d'installation, CI (fmt, clippy, tests, scénarios UI).

### Vérifié sur Windows 11 25H2 (26200.9457)
- Installation par l'installateur, valeurs de `LaunchTo`, changement de profil sur `WM_DEVICECHANGE`.
- Non confirmé : rafraîchissement d'une fenêtre Explorateur déjà ouverte, cycle de désinstallation avec restauration.
