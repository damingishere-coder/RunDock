; RunDock - Developer Project Console
; Inno Setup Script
; https://github.com/damingishere-coder/RunDock

#define AppName      "RunDock"
#define AppVersion   "1.1.0"
#define AppPublisher "DAMING"
#define AppURL       "https://github.com/damingishere-coder/RunDock"
#define AppExeName   "alter.exe"
#define ShellExeName "rundock.exe"
#define BinaryDir    "..\target\release"
#define ShellBinaryDir "..\desktop-shell\target\release"
#define WebViewBootstrapper "..\target\installer-deps\MicrosoftEdgeWebview2Setup.exe"

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
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
; Main binaries and the Microsoft-signed Evergreen WebView2 bootstrapper
Source: "{#BinaryDir}\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BinaryDir}\{#AppExeName}"; Flags: dontcopy
Source: "{#ShellBinaryDir}\{#ShellExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#ShellBinaryDir}\WebView2Loader.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\rundock-icon.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#WebViewBootstrapper}"; DestDir: "{tmp}"; Flags: deleteafterinstall; Check: not IsWebView2Installed

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#ShellExeName}"; WorkingDir: "{app}"; IconFilename: "{app}\rundock-icon.ico"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#ShellExeName}"; WorkingDir: "{app}"; IconFilename: "{app}\rundock-icon.ico"; Tasks: desktopicon
Name: "{group}\Uninstall {#AppName}"; Filename: "{uninstallexe}"

[InstallDelete]
; Remove shortcuts created by the pre-RunDock installer while preserving the
; original AppId and install directory for in-place upgrades.
Type: files; Name: "{commonprograms}\alter\alter.lnk"
Type: files; Name: "{commonprograms}\alter\Uninstall alter.lnk"
Type: dirifempty; Name: "{commonprograms}\alter"
Type: files; Name: "{userprograms}\alter\alter.lnk"
Type: files; Name: "{userprograms}\alter\Uninstall alter.lnk"
Type: dirifempty; Name: "{userprograms}\alter"

[Run]
Filename: "{tmp}\MicrosoftEdgeWebview2Setup.exe"; Parameters: "/silent /install"; StatusMsg: "Installing Microsoft Edge WebView2 Runtime..."; Flags: runhidden waituntilterminated; Check: not IsWebView2Installed
Filename: "{app}\{#ShellExeName}"; Description: "Open {#AppName}"; Flags: nowait postinstall skipifsilent

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

function HasWebView2Version(RootKey: Integer; Subkey: string): Boolean;
var
  Version: string;
begin
  Result := RegQueryStringValue(RootKey, Subkey, 'pv', Version) and
            (Version <> '') and (Version <> '0.0.0.0');
end;

function IsWebView2Installed(): Boolean;
var
  ClientKey: string;
begin
  ClientKey := 'SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}';
  Result := HasWebView2Version(HKEY_LOCAL_MACHINE_32, ClientKey) or
            HasWebView2Version(HKEY_LOCAL_MACHINE_64, ClientKey) or
            HasWebView2Version(HKEY_CURRENT_USER_32, ClientKey) or
            HasWebView2Version(HKEY_CURRENT_USER_64, ClientKey);
end;

function PrepareToInstall(var NeedsRestart: Boolean): string;
var
  ResultCode: Integer;
begin
  Result := '';
  if FileExists(ExpandConstant('{app}\{#ShellExeName}')) then
    Exec(ExpandConstant('{app}\{#ShellExeName}'), '--quit', '', SW_HIDE,
      ewWaitUntilTerminated, ResultCode);
  if FileExists(ExpandConstant('{app}\{#AppExeName}')) then begin
    ExtractTemporaryFile('{#AppExeName}');
    if (not Exec(ExpandConstant('{tmp}\{#AppExeName}'), 'internal-upgrade-handoff', '',
        SW_HIDE, ewWaitUntilTerminated, ResultCode)) or (ResultCode <> 0) then
      Result := '无法确认旧版 RunDock 后台与受管理项目已安全交接，安装已取消。请重新打开 RunDock 检查项目状态后再试。';
  end;
end;

function InitializeUninstall(): Boolean;
var
  ResultCode: Integer;
begin
  Result := True;
  if not FileExists(ExpandConstant('{app}\{#AppExeName}')) then
    exit;

  if (not Exec(ExpandConstant('{app}\{#AppExeName}'),
      'internal-uninstall-preflight', '', SW_HIDE, ewWaitUntilTerminated,
      ResultCode)) or (ResultCode <> 0) then begin
    Result := False;
    if UninstallSilent then
      Log('RunDock uninstall cancelled: managed projects may still be active')
    else
      MsgBox(
        'RunDock 检测到仍在运行的受管理项目，或无法安全确认其状态。' + #13#10 +
        '请先在 RunDock 中明确停止这些项目，再重新卸载。',
        mbError, MB_OK);
  end;
end;

[UninstallRun]
; Exit only the desktop shell first. The preflight above ensures that stopping
; the daemon cannot interrupt an active managed project. User data is retained.
Filename: "{app}\rundock.exe"; Parameters: "--quit"; \
    Flags: runhidden; RunOnceId: "StopDesktopShell"
Filename: "{app}\alter.exe"; Parameters: "daemon stop"; \
    Flags: runhidden; RunOnceId: "StopDaemon"
