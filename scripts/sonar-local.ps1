[CmdletBinding()]
param(
    [string]$HostUrl = 'http://127.0.0.1:9000',
    [string]$ScannerPath,
    [string]$RustLcovPath,
    [string]$RustLcovRevision
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$projectKey = 'rundock-alter-v1-audit'

if ([string]::IsNullOrWhiteSpace($env:SONAR_TOKEN)) {
    throw 'SONAR_TOKEN must be set in the current terminal session.'
}

function Resolve-SonarScanner {
    param([string]$ExplicitPath)

    if (-not [string]::IsNullOrWhiteSpace($ExplicitPath)) {
        return (Resolve-Path -LiteralPath $ExplicitPath).Path
    }
    $command = Get-Command sonar-scanner -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }
    if (-not [string]::IsNullOrWhiteSpace($env:SONAR_SCANNER_HOME)) {
        foreach ($name in @('sonar-scanner.bat', 'sonar-scanner')) {
            $candidate = Join-Path $env:SONAR_SCANNER_HOME "bin\$name"
            if (Test-Path -LiteralPath $candidate -PathType Leaf) {
                return (Resolve-Path -LiteralPath $candidate).Path
            }
        }
    }
    throw 'SonarScanner was not found in PATH or SONAR_SCANNER_HOME. Pass -ScannerPath explicitly.'
}

function Invoke-SonarApi {
    param([Parameter(Mandatory)][string]$Path)

    $credential = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes("$($env:SONAR_TOKEN):"))
    Invoke-RestMethod -Uri "$($HostUrl.TrimEnd('/'))$Path" -Headers @{ Authorization = "Basic $credential" }
}

$status = Invoke-RestMethod -Uri "$($HostUrl.TrimEnd('/'))/api/system/status"
if ($status.status -ne 'UP') {
    throw "SonarQube is not ready at $HostUrl (status: $($status.status))."
}
$authentication = Invoke-SonarApi -Path '/api/authentication/validate'
if (-not $authentication.valid) {
    throw 'SONAR_TOKEN was rejected by the local SonarQube instance.'
}
$scanner = Resolve-SonarScanner -ExplicitPath $ScannerPath

Push-Location $repositoryRoot
try {
    $dirtyPaths = @(git status --porcelain --untracked-files=normal)
    if ($LASTEXITCODE -ne 0) { throw 'Unable to inspect the Git working tree.' }
    if ($dirtyPaths.Count -ne 0) {
        throw 'Local SonarQube requires a clean working tree so the analysis can be attributed to the exact HEAD revision.'
    }
    $revision = (git rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($revision)) {
        throw 'Unable to resolve the current Git revision.'
    }

    npm --prefix web-ui ci
    if ($LASTEXITCODE -ne 0) { throw 'Frontend dependency installation failed.' }
    npm --prefix web-ui run test:coverage
    if ($LASTEXITCODE -ne 0) { throw 'Frontend LCOV generation failed.' }
    npm --prefix web-ui run build
    if ($LASTEXITCODE -ne 0) { throw 'Embedded frontend build failed.' }

    New-Item -ItemType Directory -Force -Path coverage | Out-Null
    $rustLcovTarget = Join-Path $repositoryRoot 'coverage\rust-lcov.info'
    if ([string]::IsNullOrWhiteSpace($RustLcovPath)) {
        cargo llvm-cov --version *> $null
        if ($LASTEXITCODE -ne 0) {
            throw 'cargo-llvm-cov is required. Install the pinned project version before running this gate.'
        }
        cargo llvm-cov --all-targets --locked --lcov --output-path $rustLcovTarget --fail-under-lines 20
        if ($LASTEXITCODE -ne 0) { throw 'Rust LCOV generation failed.' }
    } else {
        if ($RustLcovRevision -ne $revision) {
            throw 'An external Rust LCOV file must declare the exact current HEAD with -RustLcovRevision.'
        }
        $resolvedRustLcov = (Resolve-Path -LiteralPath $RustLcovPath).Path
        if ((Get-Item -LiteralPath $resolvedRustLcov).Length -eq 0) {
            throw 'The supplied Rust LCOV file is empty.'
        }
        if ($resolvedRustLcov -ne $rustLcovTarget) {
            Copy-Item -LiteralPath $resolvedRustLcov -Destination $rustLcovTarget -Force
        }
        Write-Host "Using externally generated Rust LCOV for revision $revision"
    }

    & $scanner "-Dsonar.host.url=$HostUrl" "-Dsonar.scm.revision=$revision"
    if ($LASTEXITCODE -ne 0) { throw 'SonarScanner failed.' }

    $taskFile = Join-Path $repositoryRoot '.scannerwork\report-task.txt'
    if (-not (Test-Path -LiteralPath $taskFile -PathType Leaf)) {
        throw 'SonarScanner did not produce report-task.txt.'
    }
    $taskProperties = @{}
    foreach ($line in Get-Content -LiteralPath $taskFile) {
        if ($line -match '^([^=]+)=(.*)$') { $taskProperties[$Matches[1]] = $Matches[2] }
    }
    if ([string]::IsNullOrWhiteSpace($taskProperties.ceTaskId)) {
        throw 'SonarScanner report does not contain a compute-engine task ID.'
    }

    $deadline = [DateTime]::UtcNow.AddMinutes(5)
    do {
        Start-Sleep -Seconds 2
        $task = (Invoke-SonarApi -Path "/api/ce/task?id=$($taskProperties.ceTaskId)").task
    } until ($task.status -in @('SUCCESS', 'FAILED', 'CANCELED') -or [DateTime]::UtcNow -ge $deadline)
    if ($task.status -ne 'SUCCESS' -or [string]::IsNullOrWhiteSpace($task.analysisId)) {
        throw "SonarQube compute task did not succeed (status: $($task.status))."
    }
    $gate = (Invoke-SonarApi -Path "/api/qualitygates/project_status?analysisId=$($task.analysisId)").projectStatus
    if ($gate.status -ne 'OK') {
        throw "SonarQube Quality Gate failed for $revision (status: $($gate.status))."
    }
    $metrics = Invoke-SonarApi -Path "/api/measures/component?component=$projectKey&metricKeys=bugs,vulnerabilities,code_smells,duplicated_lines_density,coverage,reliability_rating,security_rating"
    Write-Host "SonarQube Quality Gate: OK"
    Write-Host "Revision: $revision"
    Write-Host "Task: $($taskProperties.dashboardUrl)"
    $metrics.component.measures | Sort-Object metric | ForEach-Object {
        Write-Host "$($_.metric)=$($_.value)"
    }
} finally {
    Pop-Location
}
