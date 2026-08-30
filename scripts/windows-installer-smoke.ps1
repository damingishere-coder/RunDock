[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$InstallerPath,
    [Parameter(Mandatory)][string]$InstallDirectory
)

$ErrorActionPreference = 'Stop'
$installer = (Resolve-Path -LiteralPath $InstallerPath).Path
$installDir = [IO.Path]::GetFullPath($InstallDirectory)
$shell = Join-Path $installDir 'rundock.exe'
$daemon = Join-Path $installDir 'alter.exe'
$managedPid = $null

function Get-InstalledProcess {
    param([Parameter(Mandatory)][string]$ExecutablePath)
    @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq $ExecutablePath })
}

function Wait-RunDockHealth {
    $deadline = [DateTime]::UtcNow.AddSeconds(25)
    do {
        Start-Sleep -Milliseconds 250
        try { $health = Invoke-RestMethod 'http://127.0.0.1:2999/api/v1/system/health' -TimeoutSec 1 }
        catch { $health = $null }
    } until (($health.status -in @('ok', 'degraded')) -or [DateTime]::UtcNow -ge $deadline)
    if ($health.status -notin @('ok', 'degraded')) { throw 'installed daemon did not become healthy' }
}

Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class RunDockWindowSmoke {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);

    [DllImport("user32.dll")]
    public static extern bool IsWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hWnd);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count);

    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr hWnd, uint message, IntPtr wParam, IntPtr lParam);

    public static IntPtr FindMainWindow(uint targetProcessId) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((window, _) => {
            uint processId;
            GetWindowThreadProcessId(window, out processId);
            if (processId != targetProcessId || !IsWindowVisible(window)) return true;
            var title = new StringBuilder(256);
            GetWindowText(window, title, title.Capacity);
            if (title.ToString() == "RunDock") {
                found = window;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@

try {
    $install = Start-Process -FilePath $installer -ArgumentList @(
        '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/DIR=$installDir"
    ) -Wait -PassThru
    if ($install.ExitCode -ne 0) { throw "silent install failed: $($install.ExitCode)" }
    Get-Item -LiteralPath $shell, $daemon, (Join-Path $installDir 'WebView2Loader.dll') -ErrorAction Stop | Out-Null

    $shortcut = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\RunDock\RunDock.lnk'
    Get-Item -LiteralPath $shortcut -ErrorAction Stop | Out-Null
    $dataDir = Join-Path $env:APPDATA 'alter-pm2'
    $trayMarker = Join-Path $dataDir 'desktop-shell-tray-notice.json'
    Remove-Item -LiteralPath $trayMarker -Force -ErrorAction SilentlyContinue

    Start-Process -FilePath $shell -ArgumentList '--background' | Out-Null
    Wait-RunDockHealth
    $firstShell = Get-InstalledProcess -ExecutablePath $shell
    if ($firstShell.Count -ne 1) { throw "expected one desktop shell, found $($firstShell.Count)" }
    Start-Process -FilePath $shell -PassThru | Wait-Process
    Start-Sleep -Milliseconds 500
    $secondShell = Get-InstalledProcess -ExecutablePath $shell
    if ($secondShell.Count -ne 1 -or $secondShell[0].ProcessId -ne $firstShell[0].ProcessId) {
        throw 'second launch did not reuse the existing desktop shell'
    }

    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        $mainWindow = [RunDockWindowSmoke]::FindMainWindow([uint32]$firstShell[0].ProcessId)
        if ($mainWindow -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 100 }
    } until ($mainWindow -ne [IntPtr]::Zero -or [DateTime]::UtcNow -ge $deadline)
    if ($mainWindow -eq [IntPtr]::Zero) { throw 'desktop shell has no visible RunDock main window' }
    if (-not [RunDockWindowSmoke]::PostMessage($mainWindow, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)) {
        throw 'failed to send WM_CLOSE to the desktop shell'
    }
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while ([RunDockWindowSmoke]::IsWindowVisible($mainWindow) -and [DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 100
    }
    if (-not [RunDockWindowSmoke]::IsWindow($mainWindow) -or
        [RunDockWindowSmoke]::IsWindowVisible($mainWindow)) {
        throw 'WM_CLOSE destroyed the window or failed to hide it'
    }
    $afterCloseShell = Get-InstalledProcess -ExecutablePath $shell
    if ($afterCloseShell.Count -ne 1 -or $afterCloseShell[0].ProcessId -ne $firstShell[0].ProcessId) {
        throw 'WM_CLOSE exited or replaced the desktop shell process'
    }

    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    while (-not (Test-Path -LiteralPath $trayMarker) -and [DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 100
    }
    if (-not (Test-Path -LiteralPath $trayMarker)) { throw 'first close did not persist the tray notice marker' }
    $firstMarkerHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $trayMarker).Hash
    $firstMarkerWrite = (Get-Item -LiteralPath $trayMarker).LastWriteTimeUtc

    Start-Process -FilePath $shell -PassThru | Wait-Process
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while (-not [RunDockWindowSmoke]::IsWindowVisible($mainWindow) -and [DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 100
    }
    if (-not [RunDockWindowSmoke]::IsWindowVisible($mainWindow)) {
        throw 'second launch did not restore the hidden main window'
    }
    [RunDockWindowSmoke]::PostMessage($mainWindow, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while ([RunDockWindowSmoke]::IsWindowVisible($mainWindow) -and [DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 100
    }
    if ([RunDockWindowSmoke]::IsWindowVisible($mainWindow)) { throw 'second WM_CLOSE did not hide the main window' }
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $trayMarker).Hash -ne $firstMarkerHash -or
        (Get-Item -LiteralPath $trayMarker).LastWriteTimeUtc -ne $firstMarkerWrite) {
        throw 'second close rewrote the one-time tray notice marker'
    }
    Start-Process -FilePath $shell -PassThru | Wait-Process
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while (-not [RunDockWindowSmoke]::IsWindowVisible($mainWindow) -and [DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 100
    }
    if (-not [RunDockWindowSmoke]::IsWindowVisible($mainWindow)) {
        throw 'desktop shell did not restore after the second close'
    }

    $runValues = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
    if (($runValues.PSObject.Properties.Value -join "`n") -notmatch 'rundock\.exe.*--background') {
        throw 'default login autostart was not registered with --background'
    }

    New-Item -ItemType Directory -Force -Path $dataDir | Out-Null
    $retentionMarker = Join-Path $dataDir 'ci-data-retention.txt'
    Set-Content -LiteralPath $retentionMarker -Value 'preserve'
    $ping = Join-Path $env:SystemRoot 'System32\PING.EXE'
    & $daemon start $ping --name ci-upgrade-survivor -- -t 127.0.0.1
    if ($LASTEXITCODE -ne 0) { throw 'failed to start upgrade survivor process' }
    $beforeUpgrade = (& $daemon --json list | ConvertFrom-Json).processes |
        Where-Object name -eq 'ci-upgrade-survivor'
    if (-not $beforeUpgrade.pid) { throw 'upgrade survivor has no PID' }
    $managedPid = [int]$beforeUpgrade.pid

    $upgrade = Start-Process -FilePath $installer -ArgumentList @(
        '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/DIR=$installDir"
    ) -Wait -PassThru
    if ($upgrade.ExitCode -ne 0) { throw "in-place upgrade failed: $($upgrade.ExitCode)" }
    if (-not (Get-Process -Id $managedPid -ErrorAction SilentlyContinue)) {
        throw 'in-place upgrade stopped the managed child process'
    }
    if (Get-InstalledProcess -ExecutablePath $shell) {
        throw 'silent upgrade unexpectedly left the desktop shell running'
    }

    Start-Process -FilePath $shell -ArgumentList '--background' | Out-Null
    Wait-RunDockHealth
    $afterUpgrade = (& $daemon --json list | ConvertFrom-Json).processes |
        Where-Object name -eq 'ci-upgrade-survivor'
    if ($afterUpgrade.id -ne $beforeUpgrade.id -or $afterUpgrade.pid -ne $beforeUpgrade.pid) {
        throw 'in-place upgrade did not re-adopt the exact managed process'
    }
    & $daemon stop ci-upgrade-survivor
    if ($LASTEXITCODE -ne 0) { throw 'failed to stop upgrade survivor before uninstall' }
    $managedPid = $null

    Start-Process -FilePath $shell -ArgumentList '--quit' -PassThru | Wait-Process
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while ((Get-InstalledProcess -ExecutablePath $shell) -and [DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 200
    }
    if (Get-InstalledProcess -ExecutablePath $shell) { throw 'desktop shell did not exit' }
    $healthAfterShellExit = Invoke-RestMethod 'http://127.0.0.1:2999/api/v1/system/health' -TimeoutSec 2
    if ($healthAfterShellExit.status -notin @('ok', 'degraded')) { throw 'daemon stopped with desktop shell' }
    if (Get-NetTCPConnection -State Listen -LocalPort 5173 -ErrorAction SilentlyContinue) {
        throw 'production package unexpectedly started a Vite listener'
    }

    $uninstaller = Get-Item -LiteralPath (Join-Path $installDir 'unins000.exe') -ErrorAction Stop
    $uninstall = Start-Process -FilePath $uninstaller.FullName -ArgumentList @(
        '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART'
    ) -Wait -PassThru
    if ($uninstall.ExitCode -ne 0) { throw "silent uninstall failed: $($uninstall.ExitCode)" }
    if (-not (Test-Path -LiteralPath $retentionMarker)) { throw 'uninstall removed user data' }
    if (Test-Path -LiteralPath $shell) { throw 'desktop shell remained after uninstall' }
} finally {
    if ($managedPid) { Stop-Process -Id $managedPid -Force -ErrorAction SilentlyContinue }
    foreach ($executable in @($shell, $daemon)) {
        foreach ($process in (Get-InstalledProcess -ExecutablePath $executable)) {
            Stop-Process -Id $process.ProcessId -Force -ErrorAction SilentlyContinue
        }
    }
}
