; FileCustomizer — installateur unique (Inno Setup 6).
; Compilation : iscc installer\FileCustomizer.iss   (après `cargo build --release` à la racine
; ET dans ui\src-tauri). Sortie : dist\FileCustomizer-Setup-<version>.exe
;
; Installation par utilisateur, SANS élévation : le démon n'écrit que dans HKCU. L'élévation
; n'est demandée qu'à la demande (helper), jamais par l'installateur.

#define AppName "File Customizer"
#define AppVersion "0.1.0"

[Setup]
AppId={{6B2F3E58-9C1A-4B7D-8E21-5A0D7C4F1E93}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=FileCustomizer
DefaultDirName={localappdata}\Programs\FileCustomizer
DefaultGroupName={#AppName}
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=FileCustomizer-Setup-{#AppVersion}
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
Source: "..\target\release\filecustomizer.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\filecustomizer-daemon.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\filecustomizer-elevated.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\ui\src-tauri\target\release\filecustomizer-ui.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\scripts\register-task.ps1"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion isreadme

[Dirs]
Name: "{userappdata}\FileCustomizer"

[Icons]
Name: "{group}\FileCustomizer (configuration)"; Filename: "{app}\filecustomizer-ui.exe"

[Run]
; Crée config.toml documenté (sans rien activer s'il n'existe pas déjà).
Filename: "{app}\filecustomizer.exe"; Parameters: "init"; Flags: runhidden
; Tâche planifiée : ouverture de session, délai 0, priorité 3, privilèges normaux.
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\register-task.ps1"" -InstallDir ""{app}"""; Flags: runhidden; StatusMsg: "Création de la tâche planifiée..."
; Applique la config (la réactive si un ancien `restore` l'avait suspendue) puis démarre le démon.
Filename: "{app}\filecustomizer.exe"; Parameters: "apply"; Flags: runhidden
Filename: "schtasks.exe"; Parameters: "/Run /TN FileCustomizer"; Flags: runhidden
Filename: "{app}\filecustomizer-ui.exe"; Description: "Ouvrir la configuration"; Flags: postinstall nowait skipifsilent

[UninstallRun]
Filename: "powershell.exe"; Parameters: "-NoProfile -Command ""Stop-ScheduledTask -TaskName FileCustomizer -ErrorAction SilentlyContinue; Unregister-ScheduledTask -TaskName FileCustomizer -Confirm:$false -ErrorAction SilentlyContinue"""; Flags: runhidden; RunOnceId: "RemoveTask"

[Code]
var
  RestoreChosen: Boolean;

procedure RunCli(Params: String);
var
  R: Integer;
begin
  if FileExists(ExpandConstant('{app}\filecustomizer.exe')) then
    Exec(ExpandConstant('{app}\filecustomizer.exe'), Params, '', SW_HIDE, ewWaitUntilTerminated, R);
end;

procedure CloseUi();
var
  R: Integer;
begin
  // L'interface ouverte verrouille son .exe (DeleteFile code 5) : on la ferme avant de remplacer/supprimer.
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/im filecustomizer-ui.exe /f', '', SW_HIDE, ewWaitUntilTerminated, R);
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
            'Oui : toutes les modifications faites par FileCustomizer sont annulées (une invite UAC peut apparaître).' + #13#10 +
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
      if FileExists(ExpandConstant('{app}\filecustomizer.exe')) then
        Exec(ExpandConstant('{app}\filecustomizer.exe'), 'restore', '', SW_HIDE, ewWaitUntilTerminated, R);
      if R <> 0 then
        MsgBox('Certaines valeurs n''ont pas pu être restaurées (elles restent listées dans backup.json). ' +
               'Relancez « filecustomizer restore » avant de supprimer le dossier de données.', mbError, MB_OK);
    end;
  end;
  if CurUninstallStep = usPostUninstall then
  begin
    if (not UninstallSilent) and
       (MsgBox('Supprimer aussi la configuration, le backup et les journaux (' + ExpandConstant('{userappdata}\FileCustomizer') + ') ?',
               mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES) then
      DelTree(ExpandConstant('{userappdata}\FileCustomizer'), True, True, True);
  end;
end;
