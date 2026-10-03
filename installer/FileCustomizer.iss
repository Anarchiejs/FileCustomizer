; FileCustomizer — installateur unique (Inno Setup 6).
; Compilation : iscc installer\FileCustomizer.iss   (après `cargo build --release` à la racine
; ET dans ui\src-tauri). Sortie : dist\FileCustomizer-Setup-<version>.exe
;
; Installation dans Program Files, AVEC élévation : le helper élevé doit vivre dans un dossier que
; l'utilisateur ne peut pas modifier. Sinon, n'importe quel programme lancé par l'utilisateur
; pourrait remplacer filecustomizer-elevated.exe et obtenir les droits administrateur à la
; prochaine invite UAC acceptée. Tout ce qui est propre à l'utilisateur (config, tâche planifiée,
; apply, démon) est lancé en tant qu'utilisateur d'origine : le démon tourne toujours sans élévation.

#define AppName "File Customizer"
#define AppVersion "0.2.0"

[Setup]
AppId={{6B2F3E58-9C1A-4B7D-8E21-5A0D7C4F1E93}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=FileCustomizer
DefaultDirName={autopf}\FileCustomizer
DefaultGroupName={#AppName}
PrivilegesRequired=admin
; {userappdata}/{localappdata} visent le compte qui valide l'invite UAC : le même que celui de la
; session quand l'utilisateur est administrateur (cas normal). Le CLI et le helper refusent de
; travailler si l'invite a été validée avec un autre compte.
UsedUserAreasWarning=no
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

[Icons]
Name: "{group}\FileCustomizer (configuration)"; Filename: "{app}\filecustomizer-ui.exe"

[Run]
; Tout ici tourne SANS élévation, en tant qu'utilisateur de la session (`runasoriginaluser`).
; Crée config.toml documenté (sans rien activer s'il n'existe pas déjà).
Filename: "{app}\filecustomizer.exe"; Parameters: "init"; Flags: runhidden runasoriginaluser
; Tâche planifiée : ouverture de session, délai 0, priorité 3, privilèges normaux.
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\register-task.ps1"" -InstallDir ""{app}"""; Flags: runhidden runasoriginaluser; StatusMsg: "Création de la tâche planifiée..."
; Applique la config (la réactive si un ancien `restore` l'avait suspendue) puis démarre le démon.
Filename: "{app}\filecustomizer.exe"; Parameters: "apply"; Flags: runhidden runasoriginaluser
Filename: "schtasks.exe"; Parameters: "/Run /TN FileCustomizer"; Flags: runhidden runasoriginaluser
Filename: "{app}\filecustomizer-ui.exe"; Description: "Ouvrir la configuration"; Flags: postinstall nowait skipifsilent runasoriginaluser

[UninstallRun]
Filename: "powershell.exe"; Parameters: "-NoProfile -Command ""Stop-ScheduledTask -TaskName FileCustomizer -ErrorAction SilentlyContinue; Unregister-ScheduledTask -TaskName FileCustomizer -Confirm:$false -ErrorAction SilentlyContinue"""; Flags: runhidden; RunOnceId: "RemoveTask"

[Code]
const
  OldUninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{6B2F3E58-9C1A-4B7D-8E21-5A0D7C4F1E93}_is1';

var
  RestoreChosen: Boolean;

procedure CloseUi();
var
  R: Integer;
begin
  // L'interface ouverte verrouille son .exe (DeleteFile code 5) : on la ferme avant de remplacer/supprimer.
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/im filecustomizer-ui.exe /f', '', SW_HIDE, ewWaitUntilTerminated, R);
end;

// Demande l'arrêt propre du démon via le CLI indiqué (en tant qu'utilisateur de la session).
procedure StopDaemon(Cli: String);
var
  R: Integer;
begin
  if FileExists(Cli) then
  begin
    ExecAsOriginalUser(Cli, 'stop', '', SW_HIDE, ewWaitUntilTerminated, R);
    Sleep(600);
  end;
end;

// Versions <= 0.1.0 : installation par utilisateur dans %LOCALAPPDATA%\Programs\FileCustomizer.
// On arrête son démon et on retire ses fichiers et son entrée de désinstallation ; les données
// (%APPDATA%\FileCustomizer : config, backup) sont conservées et reprises telles quelles.
procedure RemoveOldPerUserInstall();
var
  OldDir: String;
begin
  OldDir := ExpandConstant('{localappdata}\Programs\FileCustomizer');
  StopDaemon(OldDir + '\filecustomizer.exe');
  if DirExists(OldDir) then
    DelTree(OldDir, True, True, True);
  RegDeleteKeyIncludingSubkeys(HKCU, OldUninstallKey);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  CloseUi();
  // Mise à jour : arrêter proprement le démon pour pouvoir remplacer son .exe.
  StopDaemon(ExpandConstant('{app}\filecustomizer.exe'));
  RemoveOldPerUserInstall();
  Result := '';
end;

function InitializeUninstall(): Boolean;
var
  R: Integer;
begin
  Result := True;
  RestoreChosen := (not UninstallSilent) and
    (MsgBox('Restaurer l''état d''origine de l''Explorateur (dossiers épinglés, nœuds du volet, valeurs de registre) avant de désinstaller ?' + #13#10 + #13#10 +
            'Oui : toutes les modifications faites par FileCustomizer sont annulées.' + #13#10 +
            'Non : les modifications déjà faites restent en place, mais plus rien ne les maintiendra.',
            mbConfirmation, MB_YESNO) = IDYES);
  if UninstallSilent then
    RestoreChosen := True;  // sans interaction : on défait par défaut, c'est le choix le plus sûr
  CloseUi();
  if FileExists(ExpandConstant('{app}\filecustomizer.exe')) then
    Exec(ExpandConstant('{app}\filecustomizer.exe'), 'stop', '', SW_HIDE, ewWaitUntilTerminated, R);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  R: Integer;
  Cli: String;
begin
  // Tout se fait à l'étape usUninstall, AVANT la suppression des fichiers : le CLI est encore là.
  if CurUninstallStep = usUninstall then
  begin
    Cli := ExpandConstant('{app}\filecustomizer.exe');
    if RestoreChosen then
    begin
      // Le désinstallateur est élevé : le CLI le détecte, fait ses accès fichiers avec les droits de
      // l'utilisateur et lance le helper sans nouvelle invite.
      R := -1;  // exe absent = restauration impossible, à signaler
      if FileExists(Cli) then
        Exec(Cli, 'restore', '', SW_HIDE, ewWaitUntilTerminated, R);
      if R <> 0 then
        MsgBox('Certaines valeurs n''ont pas pu être restaurées (elles restent listées dans backup.json). ' +
               'Réinstallez puis lancez « filecustomizer restore » avant de supprimer le dossier de données.', mbError, MB_OK);
    end;
    // Suppression des données par le CLI et non par DelTree : le CLI travaille avec les droits NON
    // élevés de l'utilisateur et ne suit aucun lien ni point de jonction. DelTree, lancé ici en
    // administrateur sur un dossier que n'importe quel programme de l'utilisateur peut modifier,
    // pourrait être redirigé vers des fichiers système.
    if (not UninstallSilent) and FileExists(Cli) and
       (MsgBox('Supprimer aussi la configuration, le backup et les journaux (' + ExpandConstant('{userappdata}\FileCustomizer') + ') ?' + #13#10#13#10 +
               'Sans restauration préalable, le backup est perdu : les modifications ne pourront plus être annulées.',
               mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES) then
    begin
      Exec(Cli, 'purge-data', '', SW_HIDE, ewWaitUntilTerminated, R);
      if R <> 0 then
        MsgBox('Le dossier de données n''a pas pu être supprimé entièrement : ' + ExpandConstant('{userappdata}\FileCustomizer'), mbError, MB_OK);
    end;
  end;
end;
