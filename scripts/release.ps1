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
$ISCC      = "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"

Write-Host "==> RunDock release build v$Version" -ForegroundColor Cyan

$CargoVersion = (Select-String -Path (Join-Path $Root "Cargo.toml") -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
if ($Version -ne $CargoVersion) {
    throw "Requested version $Version does not match Cargo.toml $CargoVersion"
}
if (-not (Test-Path $ISCC)) {
    throw "Inno Setup not found at $ISCC. Install it before running the release build."
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

    # ── 3. Build release binary ────────────────────────────────────────────────
    Write-Host "--> Building release binary..."
    Push-Location $Root
    try {
        cargo build --release --locked
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }
    }
    finally { Pop-Location }

    # ── 4. Create installer ────────────────────────────────────────────────────
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
