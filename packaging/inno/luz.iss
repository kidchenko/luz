; Inno Setup script for Luz.
;   iscc /DAppVersion=x.y.z packaging\inno\luz.iss
; Expects target\release\luz.exe to already be built (build.ps1 does both).

#define AppName      "Luz"
#ifndef AppVersion
  #define AppVersion "0.0.1"
#endif
#define AppPublisher "kidchenko"
#define AppURL       "https://github.com/kidchenko/luz"
#define AppExe       "luz.exe"

[Setup]
; Never reuse this GUID for another product: it is how Windows matches
; upgrades and uninstalls, and winget's ProductCode is derived from it.
AppId={{C8A73E14-04E8-48B2-A4C7-C700709B7055}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppURL}
AppSupportURL={#AppURL}/issues
AppUpdatesURL={#AppURL}/releases
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=auto
LicenseFile=..\..\LICENSE
OutputDir=..\..\dist
OutputBaseFilename=Luz-Setup-{#AppVersion}
SetupIconFile=..\..\assets\icon.ico
UninstallDisplayIcon={app}\{#AppExe}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; The app itself never needs elevation; Program Files and the gamma limit
; under HKLM do.
PrivilegesRequired=admin
; Close a running instance on upgrade/uninstall instead of hitting a locked exe.
CloseApplications=yes
CloseApplicationsFilter=*.exe
; [Tasks] and [Code] touch HKCU on purpose; see the notes there.
UsedUserAreasWarning=no
MinVersion=10.0

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
; Ticked by default: a screen tint that isn't running after a reboot does
; nothing at sunset. Written to HKCU under the same value name the app's own
; "Run at login" toggle uses, so the two agree.
Name: "autostart"; Description: "Start {#AppName} when I sign in"; \
  GroupDescription: "Startup:"
; Windows refuses gamma ramps far from a straight line, which stops most
; screens around 3000 K, short of the bedtime colour. f.lux lifts the same
; limit. Machine-wide, and only read when Windows starts.
Name: "warmer"; Description: "Allow colours warmer than Windows' usual limit (after a restart)"; \
  GroupDescription: "Colours:"

[Files]
Source: "..\..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
; The AppUserModelID is what lets notifications show as "Luz" with its icon:
; Windows drops toasts for an id with no shortcut behind it. Must match
; APP_ID in src/notify.rs.
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"; AppUserModelID: "kidchenko.Luz"
Name: "{group}\Uninstall {#AppName}"; Filename: "{uninstallexe}"

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; \
  ValueType: string; ValueName: "{#AppName}"; ValueData: """{app}\{#AppExe}"""; \
  Tasks: autostart; Flags: uninsdeletevalue
; Left in place on uninstall: f.lux and other tools rely on it too, and it
; only allows ramps, it sets none. Must match ICM_KEY in src/gamma.rs.
Root: HKLM; Subkey: "SOFTWARE\Microsoft\Windows NT\CurrentVersion\ICM"; \
  ValueType: dword; ValueName: "GdiIcmGammaRange"; ValueData: "256"; Tasks: warmer

[Run]
; As the signed-in user, not the elevated installer, so it writes that
; user's settings.
Filename: "{app}\{#AppExe}"; Description: "Launch {#AppName}"; \
  Flags: nowait postinstall skipifsilent runasoriginaluser

[UninstallRun]
; A hard kill skips Luz's own clean-up, which puts the screens back, so that
; is done by hand after it. Ramps belong to the session, which the elevated
; uninstaller shares.
Filename: "{sys}\taskkill.exe"; Parameters: "/f /im {#AppExe}"; \
  Flags: runhidden; RunOnceId: "KillLuz"
Filename: "{app}\{#AppExe}"; Parameters: "reset"; \
  Flags: runhidden; RunOnceId: "ResetScreens"

[UninstallDelete]
Type: files; Name: "{userappdata}\{#AppName}\luz.log"
Type: files; Name: "{userappdata}\{#AppName}\luz.log.1"

[Code]
// The app's own "Run at login" toggle writes the same HKCU value; clear it
// even if the installer task was never ticked. Under UAC an admin elevating
// their own account keeps the same HKCU, which is the normal case.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
  begin
    RegDeleteValue(HKEY_CURRENT_USER,
      'Software\Microsoft\Windows\CurrentVersion\Run', '{#AppName}');
    // Only succeeds once config.json is gone: settings survive a reinstall.
    RemoveDir(ExpandConstant('{userappdata}\{#AppName}'));
    RemoveDir(ExpandConstant('{app}'));
  end;
end;
