# Compatibilité par build Windows

La table vit dans `crates/core/src/compat.rs`. Un tweak n'est appliqué automatiquement que sur une build où il a été **validé** ; sinon il est ignoré avec un avertissement (`general.allow_untested_builds = true` pour forcer). La restauration reste toujours possible.

| Tweak | Builds validées | Validé le | Preuve |
|---|---|---|---|
| `navpane` | 26100–26299 (24H2/25H2) | 2026-10-03 sur **26200.9457** | Le Shell relit `System.IsPinnedToNameSpaceTree` = 0 après l'override HKCU (Accueil, Galerie, Proton Drive), 1 après `restore`. |
| `quick-access` | 26100–26299 | 2026-10-03 sur **26200.9457** | Propriété `System.Home.IsPinned` lisible ; verbes `pintohome`/`unpinfromhome` exécutés via `windows.storage.dll` ; désépinglage/ré-épinglage observés. |
| `thispc-drives` | 26100–26299 | 2026-10-03 sur **26200.9457** | Écriture/relecture/restauration réelles via le helper élevé (`NoDrives`, clé `Policies\Explorer` créée puis supprimée). Effet visuel non constaté. |
| `thispc-folders` | 26100–26299 | 2026-10-03 sur **26200.9457** | Écriture/restauration HKLM réelles, natif + WOW6432Node (Objets 3D). Effet visuel non constaté. |
| `explorer-view` | 26100–26299 | 2026-10-03 sur **26200.9457** | Écriture/relecture/restauration réelles (`HideFileExt`, `LaunchTo`). Valeurs `LaunchTo` 1–4 constatées (4 = fournisseur cloud principal). |
| `context-menu` | 26100–26299 | 2026-10-03 sur **26200.9457** | Vue HKCR fusionnée vérifiée : `LegacyDisable` s'ajoute aux valeurs d'origine ; `InprocServer32` vide ; `Blocked`. Effet visuel non constaté. |

**Découverte de validation** : `HKCU\Software\Microsoft\Windows\CurrentVersion\Policies` est en *lecture seule* pour l'utilisateur (ACL : SYSTEM et Administrateurs en contrôle total) — d'où l'élévation pour `NoDrives`.

26100 est déclarée par analogie (même base de code que 26200) : à confirmer sur une machine 24H2 avant de s'y fier.

## Protocole pour valider une nouvelle build
1. `filecustomizer status` : noter la build.
2. Dérouler la checklist d'intégration de `docs/TESTING.md` pour chaque tweak.
3. Ajouter la plage dans `COMPAT` **et** une ligne dans le tableau ci-dessus avec la preuve.
