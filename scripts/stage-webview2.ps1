param(
    [string]$OutputPath = (Join-Path (Split-Path $PSScriptRoot -Parent) "target\installer-deps\MicrosoftEdgeWebview2Setup.exe")
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$BootstrapperUrl = "https://go.microsoft.com/fwlink/p/?LinkId=2124703"
$OutputPath = [IO.Path]::GetFullPath($OutputPath)
$OutputDirectory = Split-Path $OutputPath -Parent
$TemporaryPath = "$OutputPath.download-$PID"

function Assert-MicrosoftSignature([string]$Path) {
    $Signature = Get-AuthenticodeSignature -LiteralPath $Path
    if ($Signature.Status -ne [System.Management.Automation.SignatureStatus]::Valid) {
        throw "WebView2 bootstrapper signature is not valid: $($Signature.Status)"
    }
    if (-not $Signature.SignerCertificate -or
        $Signature.SignerCertificate.Subject -notmatch '(^|,\s*)O=Microsoft Corporation(,|$)') {
        throw "WebView2 bootstrapper is not signed by Microsoft Corporation"
    }
    $Length = (Get-Item -LiteralPath $Path).Length
    if ($Length -lt 100KB -or $Length -gt 20MB) {
        throw "WebView2 bootstrapper size is outside the expected range: $Length bytes"
    }
}

if (Test-Path -LiteralPath $OutputPath) {
    Assert-MicrosoftSignature $OutputPath
    Write-Host "WebView2 bootstrapper already staged and verified: $OutputPath"
    exit 0
}

New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
try {
    Invoke-WebRequest -Uri $BootstrapperUrl -OutFile $TemporaryPath -UseBasicParsing
    Assert-MicrosoftSignature $TemporaryPath
    Move-Item -LiteralPath $TemporaryPath -Destination $OutputPath -Force
}
finally {
    Remove-Item -LiteralPath $TemporaryPath -Force -ErrorAction SilentlyContinue
}

$Hash = (Get-FileHash -LiteralPath $OutputPath -Algorithm SHA256).Hash.ToLowerInvariant()
Write-Host "WebView2 bootstrapper verified: $OutputPath"
Write-Host "SHA256: $Hash"
