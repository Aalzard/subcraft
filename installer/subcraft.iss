; Inno Setup 6 script for the SubCraft Windows installer.
; Build it with scripts\build-installer.ps1, which passes the defines below:
;   AppVersion      version from the root Cargo.toml, e.g. 0.1.0 or 0.2.0-beta.1
;   AppVersionInfo  numeric file version, e.g. 0.1.0.0
;   StageDir        folder with SubCraft.exe, README, licenses (made by build-windows.ps1)
;   RepoRoot        repository root (icon, license)
;   OutputDir       where SubCraft-<version>-setup.exe is written

#ifndef AppVersion
  #error AppVersion is not defined. Run scripts\build-installer.ps1 instead of compiling this file directly.
#endif

#define AppName "SubCraft"
#define AppExe "SubCraft.exe"
#define AppUrl "https://github.com/Aalzard/subcraft"
#define ProgId "SubCraft.Project"

[Setup]
; Fixed AppId: upgrades replace the previous install instead of adding a second entry.
AppId={{2E7545F0-1CE8-4FCD-B26D-AC623D9B17A6}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=SubCraft contributors
AppPublisherURL={#AppUrl}
AppSupportURL={#AppUrl}/issues
AppUpdatesURL={#AppUrl}/releases
VersionInfoVersion={#AppVersionInfo}
VersionInfoProductVersion={#AppVersionInfo}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
; Program Files by default (asks for admin); "Install for me only" is offered as an alternative.
PrivilegesRequired=admin
PrivilegesRequiredOverridesAllowed=dialog commandline
LicenseFile={#RepoRoot}\LICENSE-MIT
SetupIconFile={#RepoRoot}\apps\subcraft\assets\icon.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName} {#AppVersion}
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutputDir}
OutputBaseFilename={#AppName}-{#AppVersion}-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=yes
CloseApplications=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "associate"; Description: "Open .subcraft project files with {#AppName}"; GroupDescription: "File associations:"

[Files]
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; HKA = HKLM for an all-users install, HKCU for "install for me only".
Root: HKA; Subkey: "Software\Classes\.subcraft"; ValueType: string; ValueName: ""; ValueData: "{#ProgId}"; Flags: uninsdeletevalue uninsdeletekeyifempty; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.subcraft\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty; Tasks: associate
Root: HKA; Subkey: "Software\Classes\{#ProgId}"; ValueType: string; ValueName: ""; ValueData: "SubCraft Project"; Flags: uninsdeletekey; Tasks: associate
Root: HKA; Subkey: "Software\Classes\{#ProgId}\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#AppExe},0"; Tasks: associate
Root: HKA; Subkey: "Software\Classes\{#ProgId}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""; Tasks: associate

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
