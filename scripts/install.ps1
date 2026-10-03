<#
.SYNOPSIS
  Installe ExplorerBender pour l'utilisateur courant : binaires, dossier de données, tâche planifiée.
.DESCRIPTION
  - Copie explorerbender.exe et explorerbender-daemon.exe dans %LOCALAPPDATA%\Programs\ExplorerBender
  - Crée %APPDATA%\ExplorerBender\config.toml (documenté, ne modifie RIEN tant que vous ne l'éditez pas)
  - Crée la tâche planifiée « ExplorerBender » : déclencheur « À l'ouverture de session », délai 0,
    priorité 3 (au-dessus de la normale, pas temps réel), privilèges NORMAUX (pas d'admin :
    le démon n'écrit que dans HKCU), un seul exemplaire, relance auto en cas de plantage.
  - Démarre le démon immédiatement (pas besoin de se reconnecter).
  Ne nécessite pas d'élévation. Aucune clé Run : elle démarrerait trop tard.
.PARAMETER Source
  Dossier contenant les .exe (défaut : ..\target\release).
.PARAMETER AddToPath
  Ajoute le dossier d'installation au PATH utilisateur pour pouvoir taper `explorerbender`.
#>
[CmdletBinding()]
param(
  [string]$Source = (Join-Path $PSScriptRoot '..\target\release'),
  [switch]$AddToPath
)
$ErrorActionPreference = 'Stop'
$TaskName = 'ExplorerBender'
$InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\ExplorerBender'

foreach ($f in 'explorerbender.exe', 'explorerbender-daemon.exe', 'explorerbender-elevated.exe') {
  if (-not (Test-Path (Join-Path $Source $f))) { throw "Introuvable : $(Join-Path $Source $f). Compilez d'abord : cargo build --release" }
}

# Arrêter un démon déjà lancé (mise à jour) pour pouvoir remplacer son .exe.
$cliOld = Join-Path $InstallDir 'explorerbender.exe'
if (Test-Path $cliOld) { & $cliOld stop | Out-Null; Start-Sleep -Milliseconds 500 }
Get-Process explorerbender-daemon -ErrorAction SilentlyContinue | Stop-Process -Force

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item (Join-Path $Source 'explorerbender.exe'), (Join-Path $Source 'explorerbender-daemon.exe'), (Join-Path $Source 'explorerbender-elevated.exe') -Destination $InstallDir -Force
$ui = Join-Path $PSScriptRoot '..\ui\src-tauri\target\release\explorerbender-ui.exe'
if (Test-Path $ui) { Copy-Item $ui -Destination $InstallDir -Force }
$cli = Join-Path $InstallDir 'explorerbender.exe'
$daemon = Join-Path $InstallDir 'explorerbender-daemon.exe'

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
Write-Host "`nInstallé dans $InstallDir. Éditez %APPDATA%\ExplorerBender\config.toml ; le démon relit le fichier tout seul."
