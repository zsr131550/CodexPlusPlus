param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ColdRunCount = 5
$ColdExitAfterMs = 3000
$IdleSampleSeconds = 30
$IdleExitAfterMs = 32000
$FirstFrameLimitMs = 1500.0
$CpuP95LimitMs = 16.7
$MaximumStallLimitMs = 50.0
$PrivateMemoryLimitBytes = 157286400L

$RepositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$BinaryPath = Join-Path $RepositoryRoot 'target\release\codex-plus-plus-manager-native.exe'
$PerfRoot = Join-Path $RepositoryRoot 'target\perf\native-manager'
$RunDirectory = Join-Path $PerfRoot ((Get-Date -Format 'yyyyMMdd-HHmmss-fff') + "-$PID")

function Invoke-CargoBuild {
    Push-Location $RepositoryRoot
    try {
        & cargo build -p codex-plus-manager-native --release
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }
}

function Wait-ForReport {
    param(
        [Parameter(Mandatory)]
        [string] $Path
    )

    $Deadline = [DateTime]::UtcNow.AddSeconds(5)
    while (-not (Test-Path -LiteralPath $Path)) {
        if ([DateTime]::UtcNow -ge $Deadline) {
            throw "performance report was not written: $Path"
        }
        Start-Sleep -Milliseconds 50
    }
    Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
}

function Stop-OwnedProcess {
    param(
        [Parameter(Mandatory)]
        [System.Diagnostics.Process] $Process
    )

    if ($Process.HasExited) {
        return
    }
    $null = $Process.CloseMainWindow()
    if (-not $Process.WaitForExit(3000)) {
        $Process.Kill()
        $Process.WaitForExit()
    }
}

function Invoke-NativeSample {
    param(
        [Parameter(Mandatory)]
        [string] $Name,

        [Parameter(Mandatory)]
        [int] $ExitAfterMs,

        [int] $MemorySampleAfterSeconds = 0
    )

    $SampleDirectory = Join-Path $RunDirectory $Name
    $StateDirectory = Join-Path $SampleDirectory 'state'
    $ReportPath = Join-Path $SampleDirectory 'report.json'
    New-Item -ItemType Directory -Path $SampleDirectory | Out-Null

    $env:CODEX_PLUS_NATIVE_STATE_DIR = $StateDirectory
    $env:CODEX_PLUS_NATIVE_PERF_REPORT = $ReportPath
    $env:CODEX_PLUS_NATIVE_PERF_EXIT_AFTER_MS = $ExitAfterMs.ToString(
        [Globalization.CultureInfo]::InvariantCulture
    )

    $Process = Start-Process `
        -FilePath $BinaryPath `
        -WorkingDirectory $RepositoryRoot `
        -PassThru
    $PrivateMemoryBytes = $null

    try {
        if ($MemorySampleAfterSeconds -gt 0) {
            Start-Sleep -Seconds $MemorySampleAfterSeconds
            $LiveProcess = Get-Process -Id $Process.Id -ErrorAction Stop
            $LiveProcess.Refresh()
            $PrivateMemoryBytes = [long] $LiveProcess.PrivateMemorySize64
        }

        $RemainingWaitMs = [Math]::Max(
            15000,
            $ExitAfterMs + 10000 - ($MemorySampleAfterSeconds * 1000)
        )
        if (-not $Process.WaitForExit($RemainingWaitMs)) {
            throw "$Name did not exit within the expected interval"
        }
        if ($Process.ExitCode -ne 0) {
            throw "$Name exited with code $($Process.ExitCode)"
        }

        $Report = Wait-ForReport -Path $ReportPath
        if ($null -eq $Report.first_ui_frame_ms) {
            throw "$Name did not record first_ui_frame_ms"
        }

        [pscustomobject]@{
            Name = $Name
            FirstUiFrameMs = [double] $Report.first_ui_frame_ms
            OverviewReadyMs = if ($null -eq $Report.overview_ready_ms) {
                $null
            }
            else {
                [double] $Report.overview_ready_ms
            }
            CpuSamplesMs = @($Report.cpu_frame_samples_ms | ForEach-Object { [double] $_ })
            InputSamplesMs = @($Report.input_latency_samples_ms | ForEach-Object { [double] $_ })
            PrivateMemoryBytes = $PrivateMemoryBytes
            ReportPath = $ReportPath
        }
    }
    finally {
        Stop-OwnedProcess -Process $Process
    }
}

function Get-Percentile {
    param(
        [Parameter(Mandatory)]
        [double[]] $Values,

        [Parameter(Mandatory)]
        [double] $Percentile
    )

    if ($Values.Count -eq 0) {
        return $null
    }
    $Sorted = @($Values | Sort-Object)
    $Index = [Math]::Ceiling($Sorted.Count * $Percentile) - 1
    $Index = [Math]::Max(0, [Math]::Min($Sorted.Count - 1, $Index))
    [double] $Sorted[$Index]
}

function Get-Maximum {
    param(
        [Parameter(Mandatory)]
        [double[]] $Values
    )

    if ($Values.Count -eq 0) {
        return $null
    }
    [double] (($Values | Measure-Object -Maximum).Maximum)
}

$PreviousEnvironment = @{}
foreach ($Name in @(
    'CODEX_PLUS_NATIVE_STATE_DIR',
    'CODEX_PLUS_NATIVE_PERF_REPORT',
    'CODEX_PLUS_NATIVE_PERF_EXIT_AFTER_MS'
)) {
    $PreviousEnvironment[$Name] = [Environment]::GetEnvironmentVariable($Name, 'Process')
}

try {
    New-Item -ItemType Directory -Path $RunDirectory | Out-Null
    Invoke-CargoBuild

    $ColdSamples = @()
    for ($Index = 1; $Index -le $ColdRunCount; $Index += 1) {
        $ColdSamples += Invoke-NativeSample `
            -Name "cold-$Index" `
            -ExitAfterMs $ColdExitAfterMs
    }

    $IdleSample = Invoke-NativeSample `
        -Name 'idle-30s' `
        -ExitAfterMs $IdleExitAfterMs `
        -MemorySampleAfterSeconds $IdleSampleSeconds

    $ColdFirstFrames = [double[]] @($ColdSamples | ForEach-Object { $_.FirstUiFrameMs })
    $CpuSamples = [double[]] @($IdleSample.CpuSamplesMs)
    $InputSamples = [double[]] @($IdleSample.InputSamplesMs)
    $AllStalls = [double[]] @($CpuSamples + $InputSamples)

    if ($CpuSamples.Count -eq 0) {
        throw 'the 30-second sample did not contain CPU frame samples'
    }
    if ($InputSamples.Count -ne 5) {
        throw "expected 5 scripted input samples, got $($InputSamples.Count)"
    }
    if ($null -eq $IdleSample.PrivateMemoryBytes) {
        throw 'the 30-second sample did not record private memory'
    }

    $ColdMaximumMs = Get-Maximum -Values $ColdFirstFrames
    $CpuP95Ms = Get-Percentile -Values $CpuSamples -Percentile 0.95
    $MaximumStallMs = Get-Maximum -Values $AllStalls

    Write-Output "Evidence directory: $RunDirectory"
    Write-Output ("Cold first UI frames (ms): " + (($ColdFirstFrames | ForEach-Object {
        $_.ToString('F3', [Globalization.CultureInfo]::InvariantCulture)
    }) -join ', '))
    Write-Output ("CPU frame samples (ms): " + (($CpuSamples | ForEach-Object {
        $_.ToString('F3', [Globalization.CultureInfo]::InvariantCulture)
    }) -join ', '))
    Write-Output ("Input latency samples (ms): " + (($InputSamples | ForEach-Object {
        $_.ToString('F3', [Globalization.CultureInfo]::InvariantCulture)
    }) -join ', '))
    Write-Output ("Cold maximum: {0:F3} ms (limit {1:F1} ms)" -f $ColdMaximumMs, $FirstFrameLimitMs)
    Write-Output ("CPU p95: {0:F3} ms (limit {1:F1} ms)" -f $CpuP95Ms, $CpuP95LimitMs)
    Write-Output ("Maximum CPU/input stall: {0:F3} ms (limit < {1:F1} ms)" -f $MaximumStallMs, $MaximumStallLimitMs)
    Write-Output ("Private memory at 30s: {0} bytes (limit {1} bytes)" -f $IdleSample.PrivateMemoryBytes, $PrivateMemoryLimitBytes)

    $Failures = @()
    if ($ColdMaximumMs -gt $FirstFrameLimitMs) {
        $Failures += "cold maximum $ColdMaximumMs ms exceeds $FirstFrameLimitMs ms"
    }
    if ($CpuP95Ms -gt $CpuP95LimitMs) {
        $Failures += "CPU p95 $CpuP95Ms ms exceeds $CpuP95LimitMs ms"
    }
    if ($MaximumStallMs -ge $MaximumStallLimitMs) {
        $Failures += "maximum stall $MaximumStallMs ms is not below $MaximumStallLimitMs ms"
    }
    if ($IdleSample.PrivateMemoryBytes -gt $PrivateMemoryLimitBytes) {
        $Failures += "private memory $($IdleSample.PrivateMemoryBytes) exceeds $PrivateMemoryLimitBytes bytes"
    }

    if ($Failures.Count -gt 0) {
        $Failures | ForEach-Object { Write-Error $_ }
        exit 1
    }

    Write-Output 'Native manager performance gate passed.'
}
finally {
    foreach ($Name in $PreviousEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable($Name, $PreviousEnvironment[$Name], 'Process')
    }
}
