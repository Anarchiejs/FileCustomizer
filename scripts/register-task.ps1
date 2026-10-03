<#
.SYNOPSIS
  Crée (ou remplace) la tâche planifiée « FileCustomizer » : ouverture de session, délai 0,
  priorité 3 (au-dessus de la normale, pas temps réel), privilèges NORMAUX, une seule instance,
  relance automatique en cas de plantage. Partagé par install.ps1 et l'installateur Inno Setup.
#>
param([Parameter(Mandatory)][string]$InstallDir)
$ErrorActionPreference = 'Stop'
$TaskName = 'FileCustomizer'
$daemon = Join-Path $InstallDir 'filecustomizer-daemon.exe'
# Échappés : un `&` ou un `<` dans le nom d'utilisateur ou le chemin casserait le XML de la tâche.
$daemon = [Security.SecurityElement]::Escape($daemon)
$me = [Security.SecurityElement]::Escape([System.Security.Principal.WindowsIdentity]::GetCurrent().Name)
$xml = @"
<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>FileCustomizer : applique et maintient la structure de l'Explorateur de fichiers.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>$me</UserId>
      <Delay>PT0S</Delay>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>$me</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>3</Priority>
    <RestartOnFailure>
      <Interval>PT1M</Interval>
      <Count>3</Count>
    </RestartOnFailure>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>$daemon</Command>
    </Exec>
  </Actions>
</Task>
"@
Register-ScheduledTask -TaskName $TaskName -Xml $xml -Force | Out-Null
