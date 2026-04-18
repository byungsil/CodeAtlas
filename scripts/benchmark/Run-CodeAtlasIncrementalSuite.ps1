[CmdletBinding()]
param(
    [string]$IndexerPath = "E:\Dev\CodeAtlas\indexer\target\debug\codeatlas-indexer.exe",
    [string]$OutputPath,
    [switch]$KeepWorkspace
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Quote-ProcessArgument {
    param([Parameter(Mandatory = $true)][string]$Value)

    if ($Value -notmatch '[\s"]') {
        return $Value
    }

    return '"' + ($Value -replace '"', '\"') + '"'
}

function Parse-KeyValueMsLine {
    param(
        [Parameter(Mandatory = $true)][string]$Line,
        [Parameter(Mandatory = $true)][string]$Prefix
    )

    $result = [ordered]@{}
    $content = $Line.Substring($Prefix.Length).Trim()
    foreach ($segment in ($content -split '\|')) {
        $trimmed = $segment.Trim()
        if ($trimmed -match '^(?<name>.+?)\s+(?<value>\d+)ms(?:\s+\([^)]+\))?$') {
            $name = $Matches.name.Trim() -replace '\s+', '_'
            $result[$name] = [int]$Matches.value
        }
    }
    return $result
}

function Parse-CountLine {
    param([Parameter(Mandatory = $true)][string]$Line)

    if ($Line -match 'Symbols:\s+(?<symbols>\d+)\s+\|\s+Calls:\s+(?<calls>\d+)\s+\|\s+Propagation:\s+(?<propagation>\d+)\s+\|\s+Files:\s+(?<files>\d+)') {
        return [ordered]@{
            symbols = [int]$Matches.symbols
            calls = [int]$Matches.calls
            propagation = [int]$Matches.propagation
            files = [int]$Matches.files
        }
    }
    return $null
}

function Parse-ModeLine {
    param([Parameter(Mandatory = $true)][string]$Line)

    if ($Line -match '^Mode:\s+incremental\s+\((?<toIndex>\d+)\s+to index,\s+(?<unchanged>\d+)\s+unchanged,\s+(?<toDelete>\d+)\s+to delete\)$') {
        return [ordered]@{
            kind = "incremental"
            toIndex = [int]$Matches.toIndex
            unchanged = [int]$Matches.unchanged
            toDelete = [int]$Matches.toDelete
        }
    }
    if ($Line -match '^Mode:\s+full rebuild$') {
        return [ordered]@{
            kind = "full_rebuild"
        }
    }
    return $null
}

function Parse-TotalLine {
    param([Parameter(Mandatory = $true)][string]$Line)

    if ($Line -match 'Done in\s+(?<ms>\d+)ms') {
        return [int]$Matches.ms
    }
    return $null
}

function Invoke-IndexerRun {
    param(
        [Parameter(Mandatory = $true)][string]$IndexerExecutable,
        [Parameter(Mandatory = $true)][string]$WorkspaceRoot,
        [switch]$Full
    )

    $argList = @($WorkspaceRoot)
    if ($Full) {
        $argList += "--full"
    }

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $IndexerExecutable
    $psi.WorkingDirectory = Split-Path -Parent $IndexerExecutable
    $psi.Arguments = (($argList | ForEach-Object { Quote-ProcessArgument $_ }) -join ' ')
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true

    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $psi
    $captured = [System.Collections.Generic.List[string]]::new()

    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        [void]$process.Start()
        while (-not $process.StandardOutput.EndOfStream) {
            $captured.Add($process.StandardOutput.ReadLine())
        }
        while (-not $process.StandardError.EndOfStream) {
            $captured.Add($process.StandardError.ReadLine())
        }
        $process.WaitForExit()
        $peakWorkingSet = [int64]$process.PeakWorkingSet64
        $exitCode = $process.ExitCode
    }
    finally {
        $stopwatch.Stop()
        $process.Dispose()
    }

    $counts = $null
    $mode = $null
    $timings = [ordered]@{}
    $incrementalTimings = [ordered]@{}
    $parseBreakdown = [ordered]@{}
    $resolveBreakdown = [ordered]@{}
    $reportedTotalMs = $null
    $escalation = $null
    $modeOverrideFull = $false
    $outputPath = $null

    foreach ($line in $captured) {
        if ([string]::IsNullOrWhiteSpace($line)) {
            continue
        }
        if (-not $counts) {
            $counts = Parse-CountLine -Line $line
        }
        if (-not $mode) {
            $mode = Parse-ModeLine -Line $line
        }
        if ($line.StartsWith("  Timings:")) {
            $timings = Parse-KeyValueMsLine -Line $line -Prefix "  Timings:"
            continue
        }
        if ($line.StartsWith("  Incremental timings:")) {
            $incrementalTimings = Parse-KeyValueMsLine -Line $line -Prefix "  Incremental timings:"
            continue
        }
        if ($line.StartsWith("  Parse breakdown:")) {
            $parseBreakdown = Parse-KeyValueMsLine -Line $line -Prefix "  Parse breakdown:"
            continue
        }
        if ($line.StartsWith("  Incremental parse breakdown:")) {
            $parseBreakdown = Parse-KeyValueMsLine -Line $line -Prefix "  Incremental parse breakdown:"
            continue
        }
        if ($line.StartsWith("  Resolve breakdown:")) {
            $resolveBreakdown = Parse-KeyValueMsLine -Line $line -Prefix "  Resolve breakdown:"
            continue
        }
        if ($line.StartsWith("  Escalation:")) {
            $escalation = $line.Substring("  Escalation:".Length).Trim()
            continue
        }
        if ($line -eq "Mode override: full rebuild") {
            $modeOverrideFull = $true
            continue
        }
        if ($line.StartsWith("Done in")) {
            $reportedTotalMs = Parse-TotalLine -Line $line
            continue
        }
        if ($line.StartsWith("  Output:")) {
            $outputPath = $line.Substring("  Output:".Length).Trim()
        }
    }

    return [ordered]@{
        exitCode = $exitCode
        wallClockMs = [int]$stopwatch.ElapsedMilliseconds
        reportedTotalMs = $reportedTotalMs
        peakWorkingSetBytes = $peakWorkingSet
        mode = $mode
        escalation = $escalation
        modeOverrideFull = $modeOverrideFull
        counts = $counts
        timings = $timings
        incrementalTimings = $incrementalTimings
        parseBreakdown = $parseBreakdown
        resolveBreakdown = $resolveBreakdown
        outputPath = $outputPath
        rawOutput = @($captured)
    }
}

function Reset-WorkspaceSnapshot {
    param(
        [Parameter(Mandatory = $true)][string]$WorkspaceRoot,
        [Parameter(Mandatory = $true)][string]$SnapshotRoot
    )

    Get-ChildItem -LiteralPath $WorkspaceRoot -Force | Where-Object { $_.Name -ne ".codeatlas" } | Remove-Item -Recurse -Force
    Get-ChildItem -LiteralPath $SnapshotRoot -Force | ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination $WorkspaceRoot -Recurse -Force
    }
}

function Replace-WorkspaceWithSnapshot {
    param(
        [Parameter(Mandatory = $true)][string]$WorkspaceRoot,
        [Parameter(Mandatory = $true)][string]$SnapshotRoot
    )

    Get-ChildItem -LiteralPath $WorkspaceRoot -Force | Where-Object { $_.Name -ne ".codeatlas" } | Remove-Item -Recurse -Force
    Get-ChildItem -LiteralPath $SnapshotRoot -Force | ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination $WorkspaceRoot -Recurse -Force
    }
}

function New-TempWorkspace {
    param(
        [Parameter(Mandatory = $true)][string]$BaseRoot,
        [Parameter(Mandatory = $true)][string]$ScenarioName,
        [Parameter(Mandatory = $true)][string]$BeforeSnapshot
    )

    $workspace = Join-Path $BaseRoot $ScenarioName
    if (Test-Path -LiteralPath $workspace) {
        Remove-Item -LiteralPath $workspace -Recurse -Force
    }
    New-Item -ItemType Directory -Path $workspace | Out-Null
    Get-ChildItem -LiteralPath $BeforeSnapshot -Force | ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination $workspace -Recurse -Force
    }
    return $workspace
}

function Get-DatabaseFileSize {
    param([Parameter(Mandatory = $true)][string]$WorkspaceRoot)

    $dbPath = Join-Path $WorkspaceRoot ".codeatlas\index.db"
    if (Test-Path -LiteralPath $dbPath) {
        return (Get-Item -LiteralPath $dbPath).Length
    }
    return $null
}

function Initialize-SyntheticBranchWorkspace {
    param([Parameter(Mandatory = $true)][string]$WorkspaceRoot)

    if (Test-Path -LiteralPath $WorkspaceRoot) {
        Remove-Item -LiteralPath $WorkspaceRoot -Recurse -Force
    }
    New-Item -ItemType Directory -Path $WorkspaceRoot | Out-Null

    for ($i = 1; $i -le 30; $i++) {
        $fileName = ("unit_{0:d2}.cpp" -f $i)
        $functionName = ("Unit{0:d2}" -f $i)
        Set-Content -LiteralPath (Join-Path $WorkspaceRoot $fileName) -Value @"
int $functionName() {
    return $i;
}
"@ -Encoding UTF8
    }
}

function Apply-SyntheticBranchLikeChurn {
    param([Parameter(Mandatory = $true)][string]$WorkspaceRoot)

    for ($i = 1; $i -le 12; $i++) {
        $fileName = ("unit_{0:d2}.cpp" -f $i)
        $functionName = ("Unit{0:d2}" -f $i)
        Set-Content -LiteralPath (Join-Path $WorkspaceRoot $fileName) -Value @"
int $functionName() {
    return $($i * 10);
}
"@ -Encoding UTF8
    }
}

function Invoke-IncrementalScenario {
    param(
        [Parameter(Mandatory = $true)][string]$IndexerExecutable,
        [Parameter(Mandatory = $true)][string]$BaseRoot,
        [Parameter(Mandatory = $true)][string]$ScenarioName,
        [Parameter(Mandatory = $true)][string]$BeforeSnapshot,
        [string]$AfterSnapshot,
        [scriptblock]$AfterAction
    )

    $workspace = New-TempWorkspace -BaseRoot $BaseRoot -ScenarioName $ScenarioName -BeforeSnapshot $BeforeSnapshot

    $fullSeed = Invoke-IndexerRun -IndexerExecutable $IndexerExecutable -WorkspaceRoot $workspace -Full
    $noOp = Invoke-IndexerRun -IndexerExecutable $IndexerExecutable -WorkspaceRoot $workspace

    if ($AfterSnapshot) {
        Replace-WorkspaceWithSnapshot -WorkspaceRoot $workspace -SnapshotRoot $AfterSnapshot
    }
    if ($AfterAction) {
        & $AfterAction $workspace
    }

    $scenarioRun = Invoke-IndexerRun -IndexerExecutable $IndexerExecutable -WorkspaceRoot $workspace

    return [ordered]@{
        scenario = $ScenarioName
        fullSeed = $fullSeed
        noOpIncremental = $noOp
        scenarioRun = $scenarioRun
        databaseSizeBytes = Get-DatabaseFileSize -WorkspaceRoot $workspace
        workspace = $workspace
    }
}

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$samplesRoot = Join-Path $repoRoot "samples\incremental"
$resultsRoot = Join-Path $repoRoot "dev_docs\benchmark_results"
$tempBase = Join-Path $repoRoot "storage\benchmark_workspaces"

if (-not (Test-Path -LiteralPath $IndexerPath)) {
    throw "Indexer executable does not exist: $IndexerPath"
}

if (-not (Test-Path -LiteralPath $resultsRoot)) {
    New-Item -ItemType Directory -Path $resultsRoot | Out-Null
}
if (-not (Test-Path -LiteralPath $tempBase)) {
    New-Item -ItemType Directory -Path $tempBase | Out-Null
}

if (-not $OutputPath) {
    $OutputPath = Join-Path $resultsRoot "incremental-suite-samples.json"
}

$gitCommit = ""
try {
    $gitCommit = (git -C $repoRoot rev-parse HEAD 2>$null | Select-Object -First 1).Trim()
}
catch {
    $gitCommit = ""
}

$results = @()

$results += Invoke-IncrementalScenario `
    -IndexerExecutable $IndexerPath `
    -BaseRoot $tempBase `
    -ScenarioName "no_change_rerun" `
    -BeforeSnapshot (Join-Path $samplesRoot "edit_symbol_rename\before")

$results += Invoke-IncrementalScenario `
    -IndexerExecutable $IndexerPath `
    -BaseRoot $tempBase `
    -ScenarioName "single_cpp_edit" `
    -BeforeSnapshot (Join-Path $samplesRoot "edit_symbol_rename\before") `
    -AfterSnapshot (Join-Path $samplesRoot "edit_symbol_rename\after")

$results += Invoke-IncrementalScenario `
    -IndexerExecutable $IndexerPath `
    -BaseRoot $tempBase `
    -ScenarioName "header_edit" `
    -BeforeSnapshot (Join-Path $samplesRoot "header_comment_change\before") `
    -AfterSnapshot (Join-Path $samplesRoot "header_comment_change\after")

$burstWorkspace = New-TempWorkspace -BaseRoot $tempBase -ScenarioName "repeated_file_burst" -BeforeSnapshot (Join-Path $samplesRoot "edit_symbol_rename\before")
$burstSeed = Invoke-IndexerRun -IndexerExecutable $IndexerPath -WorkspaceRoot $burstWorkspace -Full
$burstRuns = @()
for ($i = 1; $i -le 5; $i++) {
    $workerCpp = Join-Path $burstWorkspace "worker.cpp"
    Add-Content -LiteralPath $workerCpp -Value "`n// burst $i"
    $burstRuns += Invoke-IndexerRun -IndexerExecutable $IndexerPath -WorkspaceRoot $burstWorkspace
}
$results += [ordered]@{
    scenario = "repeated_file_burst"
    fullSeed = $burstSeed
    burstRuns = $burstRuns
    databaseSizeBytes = Get-DatabaseFileSize -WorkspaceRoot $burstWorkspace
    workspace = $burstWorkspace
}

$results += Invoke-IncrementalScenario `
    -IndexerExecutable $IndexerPath `
    -BaseRoot $tempBase `
    -ScenarioName "branch_like_mass_change" `
    -BeforeSnapshot (Join-Path $samplesRoot "mass_churn\before") `
    -AfterSnapshot (Join-Path $samplesRoot "mass_churn\after")

$syntheticWorkspace = Join-Path $tempBase "branch_like_percentage_churn_synthetic"
Initialize-SyntheticBranchWorkspace -WorkspaceRoot $syntheticWorkspace
$syntheticSeed = Invoke-IndexerRun -IndexerExecutable $IndexerPath -WorkspaceRoot $syntheticWorkspace -Full
$syntheticNoOp = Invoke-IndexerRun -IndexerExecutable $IndexerPath -WorkspaceRoot $syntheticWorkspace
Apply-SyntheticBranchLikeChurn -WorkspaceRoot $syntheticWorkspace
$syntheticRun = Invoke-IndexerRun -IndexerExecutable $IndexerPath -WorkspaceRoot $syntheticWorkspace
$results += [ordered]@{
    scenario = "branch_like_percentage_churn_synthetic"
    fullSeed = $syntheticSeed
    noOpIncremental = $syntheticNoOp
    scenarioRun = $syntheticRun
    databaseSizeBytes = Get-DatabaseFileSize -WorkspaceRoot $syntheticWorkspace
    workspace = $syntheticWorkspace
}

$summary = [ordered]@{
    generatedScenarios = $results.Count
}

$payload = [ordered]@{
    schemaVersion = 1
    recordedAt = (Get-Date).ToString("o")
    gitCommit = $gitCommit
    indexerPath = $IndexerPath
    buildProfile = if ($IndexerPath -match '\\release\\') { "release" } else { "debug" }
    tempWorkspaceRoot = $tempBase
    results = $results
    summary = $summary
}

$json = $payload | ConvertTo-Json -Depth 8
Set-Content -LiteralPath $OutputPath -Value $json -Encoding UTF8

if (-not $KeepWorkspace) {
    foreach ($result in $results) {
        if ($result.workspace -and (Test-Path -LiteralPath $result.workspace)) {
            Remove-Item -LiteralPath $result.workspace -Recurse -Force
        }
    }
}

Write-Host "Incremental benchmark suite written to $OutputPath"
