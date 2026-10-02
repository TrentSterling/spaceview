param(
    [string]$LivePath = '',
    [string]$Output = '',
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
if (-not $Output) { $Output = Join-Path $repo ('test-results/' + (Get-Date -Format 'yyyyMMdd-HHmmss')) }
$Output = [System.IO.Path]::GetFullPath($Output)
New-Item -ItemType Directory -Path $Output -Force | Out-Null
$previousPrefs = $env:SPACEVIEW_PREFS_DIR
$env:SPACEVIEW_PREFS_DIR = Join-Path $Output 'prefs'

function Invoke-NativeCheck([string]$Name, [string[]]$Arguments, [int]$TimeoutSeconds = 90) {
    $stdout = Join-Path $Output ($Name + '-stdout.txt')
    $stderr = Join-Path $Output ($Name + '-stderr.txt')
    # These are the app's visible native UI checks. The read-only probe is hidden.
    $windowStyle = if ($Name -eq 'probe') { 'Hidden' } else { 'Normal' }
    $process = Start-Process -FilePath (Join-Path $repo 'target/release/spaceview.exe') `
        -ArgumentList $Arguments -WorkingDirectory $Output -WindowStyle $windowStyle `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru
    # Keep the handle open before waiting; Windows PowerShell otherwise loses
    # ExitCode for short-lived native GUI processes.
    $nativeHandle = $process.Handle
    if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
        $process.Kill()
        throw "$Name timed out after $TimeoutSeconds seconds"
    }
    $nativeExitCode = $process.ExitCode
    if ($null -eq $nativeExitCode -or $nativeExitCode -ne 0) {
        Get-Content -LiteralPath $stderr
        throw "$Name failed with exit code $nativeExitCode (handle $nativeHandle)"
    }
    Write-Host "[SpaceViewGauntlet] PASS $Name"
}

Push-Location $repo
try {
    $ErrorActionPreference = 'Continue'
    cargo test --locked --offline 2>&1 | ForEach-Object { "$_" } | Tee-Object -FilePath (Join-Path $Output 'unit-tests.txt')
    $testExit = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($testExit -ne 0) { throw 'Rust regression tests failed' }
    if (-not $SkipBuild) {
        $ErrorActionPreference = 'Continue'
        cargo build --release --locked --offline 2>&1 | ForEach-Object { "$_" } | Tee-Object -FilePath (Join-Path $Output 'build.txt')
        $buildExit = $LASTEXITCODE
        $ErrorActionPreference = 'Stop'
        if ($buildExit -ne 0) { throw 'Release build failed' }
    }

    foreach ($size in @(@{ Name = 'laptop'; W = 1024; H = 700 }, @{ Name = 'wide'; W = 1400; H = 860 })) {
        $shots = Join-Path $Output $size.Name
        Invoke-NativeCheck $size.Name @('--shots', ('"' + $shots + '"'), '--shot-width', $size.W, '--shot-height', $size.H)
        $pngs = @(Get-ChildItem -LiteralPath $shots -Filter '*.png')
        if ($pngs.Count -ne 18) { throw "$($size.Name): expected 18 screenshots, got $($pngs.Count)" }
        foreach ($png in $pngs) {
            $bytes = [System.IO.File]::ReadAllBytes($png.FullName)
            if ($bytes.Length -lt 4096 -or $bytes[0] -ne 137 -or $bytes[1] -ne 80) {
                throw "Invalid screenshot: $($png.FullName)"
            }
        }
    }

    if ($LivePath) {
        if (-not (Test-Path -LiteralPath $LivePath -PathType Container)) { throw "Missing live scan folder: $LivePath" }
        $probe = Join-Path $Output 'probe'
        Invoke-NativeCheck 'probe' @('--scan-probe', ('"' + $LivePath + '"'), '--report-dir', ('"' + $probe + '"'), '--probe-seconds', '5')
        Get-Content -LiteralPath (Join-Path $probe 'scan-probe.txt')
        $live = Join-Path $Output 'live'
        Invoke-NativeCheck 'live' @('--live-shots', ('"' + $live + '"'), '--scan', ('"' + $LivePath + '"'), '--shot-width', '1024', '--shot-height', '700')
        if (@(Get-ChildItem -LiteralPath $live -Filter '*.png').Count -ne 5) { throw 'Missing live scan screenshots' }
        $liveReport = Get-Content -LiteralPath (Join-Path $live 'live-report.txt') -Raw
        if ($liveReport -notmatch 'COMPLETE native live scan checks passed') { throw 'Live scan checks incomplete' }
        Write-Host $liveReport
    }

    Invoke-NativeCheck 'stress' @('--synthetic', '500000', '--stress', '8')
    $metrics = Import-Csv -LiteralPath (Join-Path $Output 'stress_log.csv')
    if ($metrics.Count -lt 5) { throw 'Stress test did not record enough samples' }
    if (@($metrics | Where-Object { [int]$_.layout_node_count -gt 252048 }).Count -gt 0) { throw 'Layout budget exceeded' }
    if ((Get-Content -LiteralPath (Join-Path $Output 'stress-stderr.txt') -Raw) -match 'drift|panicked') {
        throw 'Stress test reported an invariant failure'
    }

    $binary = Get-Item -LiteralPath (Join-Path $repo 'target/release/spaceview.exe')
    $receipt = [ordered]@{
        timestampUtc = (Get-Date).ToUniversalTime().ToString('o')
        binary = $binary.FullName
        sha256 = (Get-FileHash -LiteralPath $binary.FullName -Algorithm SHA256).Hash
        bytes = $binary.Length
        livePath = $LivePath
        screenshots = 36 + $(if ($LivePath) { 5 } else { 0 })
        visualReview = 'pending human or agent image inspection'
    }
    $receipt | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $Output 'receipt.json') -Encoding UTF8
    Write-Host "[SpaceViewGauntlet] COMPLETE all automated checks passed; inspect PNGs in $Output"
} finally {
    $env:SPACEVIEW_PREFS_DIR = $previousPrefs
    Pop-Location
}
