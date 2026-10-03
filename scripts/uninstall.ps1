<#
.SYNOPSIS
  Désinstalle FileCustomizer, en proposant de restaurer l'état d'origine de l'Explorateur.
.PARAMETER KeepSettings
  Ne PAS restaurer l'état d'origine : les modifications déjà faites restent en place
  (le démon, lui, n'existe plus pour les maintenir).
.PARAMETER RemoveData
  Supprime aussi %APPDATA%\FileCustomizer (config, backup, journaux). Sans -KeepSettings, la
  restauration a lieu AVANT la suppression, donc le backup n'est jamais perdu avant d'avoir servi.
.PARAMETER Yes
  Ne pose aucune question (répond « restaurer »).
#>
[CmdletBinding()]
param([switch]$KeepSettings, [switch]$RemoveData, [switch]$Yes)
#Requires -RunAsAdministrator
$ErrorActionPreference = 'Stop'
$TaskName = 'FileCustomizer'
$InstallDir = Join-Path $env:ProgramFiles 'FileCustomizer'
$DataDir = Join-Path $env:APPDATA 'FileCustomizer'
$cli = Join-Path $InstallDir 'filecustomizer.exe'

$restore = -not $KeepSettings
if ($restore -and -not $Yes -and (Test-Path $cli)) {
  $a = Read-Host "Restaurer l'état d'origine de l'Explorateur (dossiers épinglés, nœuds du volet, valeurs de registre) ? [O/n]"
  if ($a -match '^[nN]') { $restore = $false }
}

if ($restore -and (Test-Path $cli)) {
  # `restore` demande aussi au démon de s'arrêter proprement.
  & $cli restore
  if ($LASTEXITCODE -ne 0) { throw "La restauration a échoué (voir ci-dessus). Rien n'a été désinstallé ; relancez après correction." }
}

if (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue) {
  Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
  Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false
  Write-Host "Tâche planifiée supprimée."
}
Get-Process filecustomizer-daemon -ErrorAction SilentlyContinue | Stop-Process -Force

$p = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($p -and (($p -split ';') -contains $InstallDir)) {
  [Environment]::SetEnvironmentVariable('Path', (($p -split ';' | Where-Object { $_ -ne $InstallDir }) -join ';'), 'User')
}

# Données supprimées par le CLI (droits NON élevés de l'utilisateur, aucun lien ni point de jonction
# suivi) AVANT les binaires : ce script tourne en administrateur sur un dossier que tout programme de
# l'utilisateur peut modifier.
if ($RemoveData -and (Test-Path $DataDir)) {
  if (-not (Test-Path $cli)) { throw "CLI introuvable ($cli) : supprimez $DataDir à la main, sans élévation." }
  & $cli purge-data
  if ($LASTEXITCODE -ne 0) { throw "Suppression des données incomplète : $DataDir" }
  Write-Host "Données supprimées."
}
elseif (Test-Path $DataDir) { Write-Host "Données conservées dans $DataDir (-RemoveData pour les supprimer)." }
if (Test-Path $InstallDir) { [IO.Directory]::Delete($InstallDir, $true); Write-Host "Binaires supprimés." }
Write-Host "FileCustomizer est désinstallé."
