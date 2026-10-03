; ExplorerBender — installateur unique (Inno Setup 6).
; Compilation : iscc installer\ExplorerBender.iss   (après `cargo build --release` à la racine
; ET dans ui\src-tauri). Sortie : dist\ExplorerBender-Setup-<version>.exe
;
; Installation par utilisateur, SANS élévation : le démon n'écrit que dans HKCU. L'élévation
; n'est demandée qu'à la demande (helper), jamais par l'installateur.

#define AppName "ExplorerBender"
#define AppVersion "0.1.0"

[Setup]
AppId={{6B2F3E58-9C1A-4B7D-8E21-5A0D7C4F1E93}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=ExplorerBender
DefaultDirName={localappdata}\Programs\ExplorerBender
DefaultGroupName={#AppName}
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=ExplorerBender-Setup-{#AppVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayName={#AppName}
DisableProgramGroupPage=yes
CloseApplications=no

[Languages]
Name: "french"; MessagesFile: "compiler:Languages\French.isl"

[Files]
Source: "..\target\release\explorerbender.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\explorerbender-daemon.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\explorerbender-elevated.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\ui\src-tauri\target\release\explorerbender-ui.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\scripts\register-task.ps1"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion isreadme

[Dirs]
Name: "{userappdata}\ExplorerBender"

[Icons]
Name: "{group}\ExplorerBender (configuration)"; Filename: "{app}\explorerbender-ui.exe"

[Run]
; Crée config.toml documenté (sans rien activer s'il n'existe pas déjà).
Filename: "{app}\explorerbender.exe"; Parameters: "init"; Flags: runhidden
; Tâche planifiée : ouverture de session, délai 0, priorité 3, privilèges normaux.
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\register-task.ps1"" -InstallDir ""{app}"""; Flags: runhidden; StatusMsg: "Création de la tâche planifiée..."
; Applique la config (la réactive si un ancien `restore` l'avait suspendue) puis démarre le démon.
Filename: "{app}\explorerbender.exe"; Parameters: "apply"; Flags: runhidden
Filename: "schtasks.exe"; Parameters: "/Run /TN ExplorerBender"; Flags: runhidden
Filename: "{app}\explorerbender-ui.exe"; Description: "Ouvrir la configuration"; Flags: postinstall nowait skipifsilent

[UninstallRun]
Filename: "powershell.exe"; Parameters: "-NoProfile -Command ""Stop-ScheduledTask -TaskName ExplorerBender -ErrorAction SilentlyContinue; Unregister-ScheduledTask -TaskName ExplorerBender -Confirm:$false -ErrorAction SilentlyContinue"""; Flags: runhidden; RunOnceId: "RemoveTask"

[Code]
var
  RestoreChosen: Boolean;

procedure RunCli(Params: String);
var
  R: Integer;
begin
  if FileExists(ExpandConstant('{app}\explorerbender.exe')) then
    Exec(ExpandConstant('{app}\explorerbender.exe'), Params, '', SW_HIDE, ewWaitUntilTerminated, R);
end;

procedure CloseUi();
var
  R: Integer;
begin
  // L'interface ouverte verrouille son .exe (DeleteFile code 5) : on la ferme avant de remplacer/supprimer.
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/im explorerbender-ui.exe /f', '', SW_HIDE, ewWaitUntilTerminated, R);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  CloseUi();
  // Mise à jour : arrêter proprement le démon pour pouvoir remplacer son .exe.
  RunCli('stop');
  Sleep(600);
  Result := '';
end;

function InitializeUninstall(): Boolean;
begin
  Result := True;
  RestoreChosen := (not UninstallSilent) and
    (MsgBox('Restaurer l''état d''origine de l''Explorateur (dossiers épinglés, nœuds du volet, valeurs de registre) avant de désinstaller ?' + #13#10 + #13#10 +
            'Oui : toutes les modifications faites par ExplorerBender sont annulées (une invite UAC peut apparaître).' + #13#10 +
            'Non : les modifications déjà faites restent en place, mais plus rien ne les maintiendra.',
            mbConfirmation, MB_YESNO) = IDYES);
  if UninstallSilent then
    RestoreChosen := True;  // sans interaction : on défait par défaut, c'est le choix le plus sûr
  CloseUi();
  RunCli('stop');
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  R: Integer;
begin
  if CurUninstallStep = usUninstall then
  begin
    if RestoreChosen then
    begin
      // La restauration utilise le backup.json et demande l'élévation seulement si nécessaire.
      if FileExists(ExpandConstant('{app}\explorerbender.exe')) then
        Exec(ExpandConstant('{app}\explorerbender.exe'), 'restore', '', SW_HIDE, ewWaitUntilTerminated, R);
      if R <> 0 then
        MsgBox('Certaines valeurs n''ont pas pu être restaurées (elles restent listées dans backup.json). ' +
               'Relancez « explorerbender restore » avant de supprimer le dossier de données.', mbError, MB_OK);
    end;
  end;
  if CurUninstallStep = usPostUninstall then
  begin
    if (not UninstallSilent) and
       (MsgBox('Supprimer aussi la configuration, le backup et les journaux (' + ExpandConstant('{userappdata}\ExplorerBender') + ') ?',
               mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES) then
      DelTree(ExpandConstant('{userappdata}\ExplorerBender'), True, True, True);
  end;
end;
