; Drone Deck per-user installer (Inno Setup 6).
;
; Built by `drone-deck\Build.ps1 -Installer` after it has packaged dist\DroneDeck-<version>\, or by hand:
;   ISCC.exe /DAppVersion=0.8.0 /DSourceDir=<repo>\dist\DroneDeck-0.8.0 /DOutputDir=<repo>\dist drone-deck\installer\drone-deck.iss
;
; Installs for the current user only (no administrator prompt) into %LOCALAPPDATA%\Programs\DroneDeck with a
; Start menu shortcut. User data lives elsewhere (%LOCALAPPDATA%\DroneDeck\workspace, \logs, \models, \tools and
; the WSL distribution) and is never removed by the uninstaller.
;
; The setup executable is not code-signed yet; see drone-deck\docs\release.md.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir AddBackslash(SourcePath) + "..\..\dist\DroneDeck-" + AppVersion
#endif
#ifndef OutputDir
  #define OutputDir AddBackslash(SourcePath) + "..\..\dist"
#endif
#define AppName "Drone Deck"
#define AppExe "Drone Deck.exe"

[Setup]
; Keep AppId stable across versions: it identifies the installation for upgrades and uninstall.
AppId={{754CAFF3-A576-4C99-A03D-CB3706FAEB2B}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Drone Deck
VersionInfoVersion={#AppVersion}
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Programs\DroneDeck
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
UsePreviousAppDir=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename=DroneDeck-{#AppVersion}-setup
SetupIconFile={#AddBackslash(SourcePath)}..\assets\drone-deck.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
; Restart Manager asks a running Drone Deck to close before files are replaced or removed.
CloseApplications=yes
RestartApplications=no

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[InstallDelete]
; Replace the shipped folders whole, so files a newer version no longer ships do not linger.
Type: filesandordirs; Name: "{app}\runtime"
Type: filesandordirs; Name: "{app}\assets"
Type: filesandordirs; Name: "{app}\docs"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; WorkingDir: "{app}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Helper scripts run from runtime\ inside WSL and may leave Python caches there. Only the program folder's own
; subfolders are listed: a portable workspace\ beside the exe (portable.txt) is user data and stays.
Type: filesandordirs; Name: "{app}\runtime"
Type: dirifempty; Name: "{app}"
