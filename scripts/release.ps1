# release.ps1 — Build a release installer for RunDock
# Usage:  .\scripts\release.ps1 -Version <Cargo.toml version>
# Requires: Rust (cargo), Inno Setup 6 installed at default path

param(
    [Parameter(Mandatory)]
    [string]$Version
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$Root      = Split-Path $PSScriptRoot -Parent
$ISSFile   = Join-Path $Root "installer\alter-setup.iss"
$DistDir   = Join-Path $Root "dist"
$SystemISCC = "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
$PortableISCC = Join-Path $Root "target\tools\inno\ISCC.exe"
$ISCC = if (Test-Path -LiteralPath $SystemISCC) { $SystemISCC } else { $PortableISCC }

Write-Host "==> RunDock release build v$Version" -ForegroundColor Cyan

$CargoVersion = (Select-String -Path (Join-Path $Root "Cargo.toml") -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
if ($Version -ne $CargoVersion) {
    throw "Requested version $Version does not match Cargo.toml $CargoVersion"
}
$ShellCargoVersion = (Select-String -Path (Join-Path $Root "desktop-shell\Cargo.toml") -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
if ($Version -ne $ShellCargoVersion) {
    throw "Requested version $Version does not match desktop-shell/Cargo.toml $ShellCargoVersion"
}
$TauriVersion = (Get-Content (Join-Path $Root "desktop-shell\tauri.conf.json") -Raw | ConvertFrom-Json).version
if ($Version -ne $TauriVersion) {
    throw "Requested version $Version does not match desktop-shell/tauri.conf.json $TauriVersion"
}
if (-not (Test-Path $ISCC)) {
    throw "Inno Setup not found. Install it normally or place a portable copy at $PortableISCC."
}

$OriginalIss = Get-Content $ISSFile -Raw
$InstallerFile = $null
$Hash = $null
try {
    # ── 1. Stage a version without leaving the source file modified ────────────
    Write-Host "--> Staging Inno Setup version..."
    $VersionPattern = '#define AppVersion\s+"[^"]+"'
    $VersionMatches = [regex]::Matches($OriginalIss, $VersionPattern)
    if ($VersionMatches.Count -ne 1) {
        throw "Expected exactly one AppVersion definition, found $($VersionMatches.Count)"
    }
    $PatchedIss = [regex]::Replace(
        $OriginalIss,
        $VersionPattern,
        "#define AppVersion  `"$Version`""
    )
    Set-Content -LiteralPath $ISSFile -Value $PatchedIss -NoNewline

    # Remove only artifacts for this exact version so a failed build cannot be
    # mistaken for a successful fresh installer.
    if (Test-Path $DistDir) {
        Get-ChildItem -LiteralPath $DistDir -Filter "RunDock-$Version-*.exe" -File |
            Remove-Item -Force
    }

    # ── 2. Build the exact dashboard embedded by Rust ──────────────────────────
    Write-Host "--> Building web UI..."
    Push-Location (Join-Path $Root "web-ui")
    try {
        npm ci
        if ($LASTEXITCODE -ne 0) { throw "npm ci failed with exit code $LASTEXITCODE" }
        npm run build
        if ($LASTEXITCODE -ne 0) { throw "web UI build failed with exit code $LASTEXITCODE" }
    }
    finally { Pop-Location }

    # ── 3. Stage the official Evergreen WebView2 bootstrapper ─────────────────
    Write-Host "--> Staging and verifying WebView2 bootstrapper..."
    & (Join-Path $Root "scripts\stage-webview2.ps1")
    if ($LASTEXITCODE -ne 0) { throw "WebView2 staging failed with exit code $LASTEXITCODE" }

    # ── 4. Build both release binaries ────────────────────────────────────────
    Write-Host "--> Building release binaries..."
    Push-Location $Root
    try {
        cargo build --release --locked
        if ($LASTEXITCODE -ne 0) { throw "alter release build failed with exit code $LASTEXITCODE" }
        cargo build --manifest-path desktop-shell\Cargo.toml --release --locked
        if ($LASTEXITCODE -ne 0) { throw "desktop shell release build failed with exit code $LASTEXITCODE" }
        $Loader = Join-Path $Root "desktop-shell\target\release\WebView2Loader.dll"
        if (-not (Test-Path -LiteralPath $Loader)) {
            $LoaderSource = Get-ChildItem (Join-Path $Root "desktop-shell\target\release\build") -Filter WebView2Loader.dll -File -Recurse |
                Where-Object FullName -Match '[\\/]out[\\/]x64[\\/]WebView2Loader\.dll$' |
                Sort-Object LastWriteTimeUtc -Descending |
                Select-Object -First 1
            if (-not $LoaderSource) {
                throw "WebView2Loader.dll was not produced by the desktop build"
            }
            Copy-Item -LiteralPath $LoaderSource.FullName -Destination $Loader
        }
        $LoaderSignature = Get-AuthenticodeSignature -LiteralPath $Loader
        if ($LoaderSignature.Status -ne [System.Management.Automation.SignatureStatus]::Valid -or
            -not $LoaderSignature.SignerCertificate -or
            $LoaderSignature.SignerCertificate.Subject -notmatch '(^|,\s*)O=Microsoft Corporation(,|$)') {
            throw "WebView2Loader.dll is not signed by Microsoft Corporation"
        }
    }
    finally { Pop-Location }

    # ── 5. Create installer ────────────────────────────────────────────────────
    Write-Host "--> Building Inno Setup installer..."
    New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
    & $ISCC $ISSFile
    if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed with exit code $LASTEXITCODE" }

    $InstallerFile = @(Get-ChildItem -LiteralPath $DistDir -Filter "RunDock-$Version-*.exe" -File)
    if ($InstallerFile.Count -ne 1) {
        throw "Expected exactly one fresh installer for $Version, found $($InstallerFile.Count)"
    }
    $InstallerFile = $InstallerFile[0]
    # This is the hash of the local, unsigned build only. The release workflow
    # signs the installer and computes the authoritative hash afterwards.
    $Hash = (Get-FileHash $InstallerFile.FullName -Algorithm SHA256).Hash.ToLower()
}
finally {
    Set-Content -LiteralPath $ISSFile -Value $OriginalIss -NoNewline
}

# ── 6. Summary ────────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "══════════════════════════════════════════════════════" -ForegroundColor Green
Write-Host "  Release build complete!" -ForegroundColor Green
Write-Host "  Installer : $($InstallerFile.FullName)" -ForegroundColor Green
Write-Host "  Local unsigned SHA256: $Hash" -ForegroundColor Green
Write-Host "══════════════════════════════════════════════════════" -ForegroundColor Green
Write-Host ""
Write-Host "Next steps:" -ForegroundColor Yellow
Write-Host "  1. Tag the release:  git tag v$Version && git push origin v$Version"
Write-Host "  2. GitHub Actions will create the GitHub Release automatically."
Write-Host "  3. For WinGet, use only the signed GitHub Release asset and its CI-published SHA256."
Write-Host "     Update the canonical microsoft/winget-pkgs manifest in a separate reviewed PR."
