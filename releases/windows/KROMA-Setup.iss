#define AppName "KROMA"
#define AppVersion "0.1.43"
#define AppExe "kroma-server.exe"
#ifndef ServerBuildExe
  #error Pass /DServerBuildExe pointing to the freshly built native Windows executable
#endif
#ifndef WebBuildDir
  #error Pass /DWebBuildDir pointing to the freshly built clients/web/dist/client
#endif
#ifndef ModulePayloadDir
  #error Pass /DModulePayloadDir pointing to the verified cloud115 bundle contents
#endif
#ifnexist WebBuildDir + "\_shell.html"
  #error Missing KROMA web shell
#endif
#ifnexist ModulePayloadDir + "\origin.json"
  #error Missing cloud115 installation record
#endif

[Setup]
AppId={{B7026452-7F99-4C17-B958-4D62D4E17A34}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=KROMA
DefaultDirName={localappdata}\Programs\KROMA
DefaultGroupName=KROMA
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesInstallIn64BitMode=x64
OutputDir=.
OutputBaseFilename=KROMA-Setup-{#AppVersion}
SetupIconFile=kroma.ico
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName=KROMA

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "chinesesimp"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Dirs]
; User database, logs, images and settings live here. Keep them when uninstalling.
Name: "{app}\data"; Flags: uninsneveruninstall

[Files]
Source: "{#ServerBuildExe}"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion
Source: "{#WebBuildDir}\*"; DestDir: "{app}\web"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "bin\*"; DestDir: "{app}\bin"; Flags: ignoreversion recursesubdirs createallsubdirs
; Existing module installations (or an operator's decision to uninstall) belong to the module manager.
Source: "{#ModulePayloadDir}\*"; DestDir: "{app}\data\installed\tv.kroma.cloud115"; Flags: ignoreversion recursesubdirs createallsubdirs uninsneveruninstall; Check: ShouldSeedModule
Source: "README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\KROMA"; Filename: "{app}\{#AppExe}"; WorkingDir: "{app}"
Name: "{autodesktop}\KROMA"; Filename: "{app}\{#AppExe}"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "Launch KROMA"; Flags: postinstall nowait skipifsilent

[Code]
var
  SeedModule: Boolean;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  SeedModule := not FileExists(ExpandConstant('{app}\{#AppExe}')) and not DirExists(ExpandConstant('{app}\data\installed\tv.kroma.cloud115')) and not FileExists(ExpandConstant('{app}\data\kroma.db'));
  Result := '';
end;

function ShouldSeedModule: Boolean;
begin
  Result := SeedModule;
end;
