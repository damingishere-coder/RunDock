; RunDock - Developer Project Console
; Inno Setup Script
; https://github.com/damingishere-coder/RunDock

#define AppName      "RunDock"
#define AppVersion   "1.1.0"
#define AppPublisher "DAMING"
#define AppURL       "https://github.com/damingishere-coder/RunDock"
#define AppExeName   "alter.exe"
#define BinaryDir    "..\target\release"

[Setup]
; AppId uniquely identifies this application — do NOT change after first release
AppId={{B7C4D3E2-F1A0-4B5C-9D8E-2F3A1B4C5D6E}
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
LicenseFile=..\LICENSE
OutputDir=..\dist
OutputBaseFilename=RunDock-{#AppVersion}-windows-x64-setup
SetupIconFile=..\assets\rundock-icon.ico
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=admin
ChangesEnvironment=yes

; Windows 10 1809+ required
MinVersion=10.0.17763

; Uninstall info
UninstallDisplayName={#AppName} {#AppVersion}
UninstallDisplayIcon={app}\rundock-icon.ico

; Architecture — x64 only
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "addtopath"; Description: "Add the compatible alter CLI to PATH (recommended)"; GroupDescription: "System integration:"

[Files]
; Main binary
Source: "{#BinaryDir}\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\rundock-icon.ico"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Parameters: "web"; WorkingDir: "{app}"; IconFilename: "{app}\rundock-icon.ico"
Name: "{group}\Uninstall {#AppName}"; Filename: "{uninstallexe}"

[Registry]
; Add to PATH via registry so it persists across terminals
Root: HKLM; Subkey: "SYSTEM\CurrentControlSet\Control\Session Manager\Environment"; \
    ValueType: expandsz; ValueName: "Path"; \
    ValueData: "{olddata};{app}"; \
    Tasks: addtopath; Check: NeedsAddPath('{app}')

[Code]
// NeedsAddPath — returns True if {app} is not already in the system PATH
function NeedsAddPath(Param: string): boolean;
var
  OrigPath: string;
begin
  if not RegQueryStringValue(HKEY_LOCAL_MACHINE,
    'SYSTEM\CurrentControlSet\Control\Session Manager\Environment',
    'Path', OrigPath)
  then begin
    Result := True;
    exit;
  end;
  // Look for the path with or without trailing backslash
  OrigPath := ';' + Uppercase(OrigPath) + ';';
  Param := Uppercase(RemoveBackslashUnlessRoot(Param));
  Result := (Pos(';' + Param + ';', OrigPath) = 0) and
            (Pos(';' + Param + '\;', OrigPath) = 0);
end;

[UninstallRun]
; Gracefully stop the daemon before uninstalling
Filename: "{app}\alter.exe"; Parameters: "daemon stop"; \
    Flags: runhidden; RunOnceId: "StopDaemon"
