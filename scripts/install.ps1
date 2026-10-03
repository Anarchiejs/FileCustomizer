<#
.SYNOPSIS
  Installe FileCustomizer pour l'utilisateur courant : binaires, dossier de données, tâche planifiée.
.DESCRIPTION
  - Copie les binaires dans %ProgramFiles%\FileCustomizer (dossier protégé : le helper élevé ne
    doit pas pouvoir être remplacé par un programme de l'utilisateur)
  - Crée %APPDATA%\FileCustomizer\config.toml (documenté, ne modifie RIEN tant que vous ne l'éditez pas)
  - Crée la tâche planifiée « FileCustomizer » : déclencheur « À l'ouverture de session », délai 0,
    priorité 3 (au-dessus de la normale, pas temps réel), privilèges NORMAUX (pas d'admin :
    le démon n'écrit que dans HKCU), un seul exemplaire, relance auto en cas de plantage.
  - Démarre le démon immédiatement (pas besoin de se reconnecter).
  À lancer dans un PowerShell administrateur ouvert avec VOTRE compte (le dossier de données et la
  tâche planifiée sont ceux du compte courant). Aucune clé Run : elle démarrerait trop tard.
.PARAMETER Source
  Dossier contenant les .exe (défaut : ..\target\release).
.PARAMETER AddToPath
  Ajoute le dossier d'installation au PATH utilisateur pour pouvoir taper `filecustomizer`.
#>
[CmdletBinding()]
param(
  [string]$Source = (Join-Path $PSScriptRoot '..\target\release'),
  [switch]$AddToPath
)
#Requires -RunAsAdministrator
$ErrorActionPreference = 'Stop'
$TaskName = 'FileCustomizer'
$InstallDir = Join-Path $env:ProgramFiles 'FileCustomizer'

# Versions <= 0.1.0 : installation par utilisateur, à retirer (les données sont conservées).
$OldDir = Join-Path $env:LOCALAPPDATA 'Programs\FileCustomizer'
if (Test-Path (Join-Path $OldDir 'filecustomizer.exe')) {
  & (Join-Path $OldDir 'filecustomizer.exe') stop | Out-Null
  Start-Sleep -Milliseconds 500
}
if (Test-Path $OldDir) { [IO.Directory]::Delete($OldDir, $true) }

foreach ($f in 'filecustomizer.exe', 'filecustomizer-daemon.exe', 'filecustomizer-elevated.exe') {
  if (-not (Test-Path (Join-Path $Source $f))) { throw "Introuvable : $(Join-Path $Source $f). Compilez d'abord : cargo build --release" }
}

# Arrêter un démon déjà lancé (mise à jour) pour pouvoir remplacer son .exe.
$cliOld = Join-Path $InstallDir 'filecustomizer.exe'
if (Test-Path $cliOld) { & $cliOld stop | Out-Null; Start-Sleep -Milliseconds 500 }
Get-Process filecustomizer-daemon -ErrorAction SilentlyContinue | Stop-Process -Force

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item (Join-Path $Source 'filecustomizer.exe'), (Join-Path $Source 'filecustomizer-daemon.exe'), (Join-Path $Source 'filecustomizer-elevated.exe') -Destination $InstallDir -Force
$ui = Join-Path $PSScriptRoot '..\ui\src-tauri\target\release\filecustomizer-ui.exe'
if (Test-Path $ui) { Copy-Item $ui -Destination $InstallDir -Force }
$cli = Join-Path $InstallDir 'filecustomizer.exe'
$daemon = Join-Path $InstallDir 'filecustomizer-daemon.exe'

& $cli init | Out-Host

& (Join-Path $PSScriptRoot 'register-task.ps1') -InstallDir $InstallDir
Write-Host "Tâche planifiée '$TaskName' créée (ouverture de session, priorité 3, sans élévation)."

if ($AddToPath) {
  $p = [Environment]::GetEnvironmentVariable('Path', 'User')
  if (($p -split ';') -notcontains $InstallDir) {
    [Environment]::SetEnvironmentVariable('Path', "$p;$InstallDir", 'User')
    Write-Host "Ajouté au PATH utilisateur (rouvrez le terminal)."
  }
}

# Un `restore` précédent a pu poser le marqueur de suspension : `apply` le retire et applique.
& $cli apply | Out-Host
Start-ScheduledTask -TaskName $TaskName
Write-Host "`nInstallé dans $InstallDir. Éditez %APPDATA%\FileCustomizer\config.toml ; le démon relit le fichier tout seul."
