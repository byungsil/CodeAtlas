[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$WorkspaceRoot,

    [ValidateSet("full", "incremental")]
    [string]$Mode = "full",

    [string]$IndexerPath = "E:\Dev\CodeAtlas\indexer\target\debug\codeatlas-indexer.exe",

    [string]$OutputPath,

    [int]$RepeatCount = 1,

    [int]$RetryCount = 3,

    [int]$RetryDelayMs = 1000,

    [switch]$VerboseIndexer,

    [switch]$JsonIndexerOutput
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Parse-KeyValueMsLine {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Line,
        [Parameter(Mandatory = $true)]
        [string]$Prefix
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

function Quote-ProcessArgument {
    param([Parameter(Mandatory = $true)][string]$Value)

    if ($Value -notmatch '[\s"]') {
        return $Value
    }

    return '"' + ($Value -replace '"', '\"') + '"'
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

function Parse-TotalLine {
    param([Parameter(Mandatory = $true)][string]$Line)

    if ($Line -match 'Done in\s+(?<ms>\d+)ms') {
        return [int]$Matches.ms
    }
    return $null
}

function Invoke-BenchmarkRun {
    param(
        [Parameter(Mandatory = $true)][int]$RunIndex,
        [Parameter(Mandatory = $true)][string]$IndexerExecutable,
        [Parameter(Mandatory = $true)][string]$Workspace,
        [Parameter(Mandatory = $true)][string]$SelectedMode,
        [switch]$IndexerVerbose,
        [switch]$IndexerJson
    )

    $argList = @($Workspace)
    if ($SelectedMode -eq "full") {
        $argList += "--full"
    }
    if ($IndexerVerbose) {
        $argList += "--verbose"
    }
    if ($IndexerJson) {
        $argList += "--json"
    }

    $attemptsUsed = 0
    $captured = $null
    $exitCode = 1
    $wallClockMs = 0

    for ($attempt = 1; $attempt -le ($RetryCount + 1); $attempt++) {
        $attemptsUsed = $attempt
        $capturedLines = [System.Collections.Generic.List[string]]::new()
        $psi = New-Object System.Diagnostics.ProcessStartInfo
        $psi.FileName = $IndexerExecutable
        $psi.WorkingDirectory = Split-Path -Parent $IndexerExecutable
        $psi.RedirectStandardOutput = $true
        $psi.RedirectStandardError = $true
        $psi.UseShellExecute = $false
        $psi.CreateNoWindow = $true
        $psi.Arguments = (($argList | ForEach-Object { Quote-ProcessArgument $_ }) -join ' ')

        $process = New-Object System.Diagnostics.Process
        $process.StartInfo = $psi

        $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
        try {
            [void]$process.Start()
            while (-not $process.StandardOutput.EndOfStream) {
                $capturedLines.Add($process.StandardOutput.ReadLine())
            }
            while (-not $process.StandardError.EndOfStream) {
                $capturedLines.Add($process.StandardError.ReadLine())
            }
            $process.WaitForExit()
            $exitCode = $process.ExitCode
        }
        finally {
            $stopwatch.Stop()
            $wallClockMs = [int]$stopwatch.ElapsedMilliseconds
            $process.Dispose()
        }

        $captured = $capturedLines
        if ($exitCode -eq 0) {
            break
        }
        if ($attempt -le $RetryCount) {
            Start-Sleep -Milliseconds $RetryDelayMs
        }
    }

    $counts = $null
    $timings = [ordered]@{}
    $parseBreakdown = [ordered]@{}
    $resolveBreakdown = [ordered]@{}
    $reportedTotalMs = $null
    $outputPath = $null

    foreach ($line in $captured) {
        if (-not $counts) {
            $counts = Parse-CountLine -Line $line
        }
        if ($line.StartsWith("  Timings:")) {
            $timings = Parse-KeyValueMsLine -Line $line -Prefix "  Timings:"
            continue
        }
        if ($line.StartsWith("  Parse breakdown:")) {
            $parseBreakdown = Parse-KeyValueMsLine -Line $line -Prefix "  Parse breakdown:"
            continue
        }
        if ($line.StartsWith("  Resolve breakdown:")) {
            $resolveBreakdown = Parse-KeyValueMsLine -Line $line -Prefix "  Resolve breakdown:"
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
        runIndex = $RunIndex
        attemptsUsed = $attemptsUsed
        exitCode = $exitCode
        wallClockMs = $wallClockMs
        reportedTotalMs = $reportedTotalMs
        counts = $counts
        timings = $timings
        parseBreakdown = $parseBreakdown
        resolveBreakdown = $resolveBreakdown
        outputPath = $outputPath
        rawOutput = @($captured)
    }
}

if (-not (Test-Path -LiteralPath $WorkspaceRoot)) {
    throw "Workspace root does not exist: $WorkspaceRoot"
}

if (-not (Test-Path -LiteralPath $IndexerPath)) {
    throw "Indexer executable does not exist: $IndexerPath"
}

if ($RepeatCount -lt 1) {
    throw "RepeatCount must be at least 1."
}

if ($RetryCount -lt 0) {
    throw "RetryCount must be 0 or greater."
}

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$resultDir = Join-Path $repoRoot "dev_docs\benchmark_results"
if (-not (Test-Path -LiteralPath $resultDir)) {
    New-Item -ItemType Directory -Path $resultDir | Out-Null
}

if (-not $OutputPath) {
    $workspaceName = Split-Path -Leaf $WorkspaceRoot
    $timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $OutputPath = Join-Path $resultDir "$workspaceName-$Mode-$timestamp.json"
}

$gitCommit = ""
try {
    $gitCommit = (git -C $repoRoot rev-parse HEAD 2>$null | Select-Object -First 1).Trim()
}
catch {
    $gitCommit = ""
}

$runs = @()
for ($i = 1; $i -le $RepeatCount; $i++) {
    $runs += Invoke-BenchmarkRun -RunIndex $i -IndexerExecutable $IndexerPath -Workspace $WorkspaceRoot -SelectedMode $Mode -IndexerVerbose:$VerboseIndexer -IndexerJson:$JsonIndexerOutput
}

$successfulRuns = @($runs | Where-Object { $_.exitCode -eq 0 -and $_.reportedTotalMs })
$reportedTotals = @($successfulRuns | ForEach-Object { [int]$_.reportedTotalMs })

$summary = [ordered]@{
    successfulRuns = $successfulRuns.Count
    totalRuns = $runs.Count
}

if ($reportedTotals.Count -gt 0) {
    $summary.averageReportedTotalMs = [int][Math]::Round((($reportedTotals | Measure-Object -Average).Average), 0)
    $summary.minReportedTotalMs = [int]($reportedTotals | Measure-Object -Minimum).Minimum
    $summary.maxReportedTotalMs = [int]($reportedTotals | Measure-Object -Maximum).Maximum
}

$result = [ordered]@{
    schemaVersion = 1
    recordedAt = (Get-Date).ToString("o")
    workspaceRoot = (Resolve-Path -LiteralPath $WorkspaceRoot).Path
    workspaceName = (Split-Path -Leaf $WorkspaceRoot)
    mode = $Mode
    repeatCount = $RepeatCount
    indexerPath = (Resolve-Path -LiteralPath $IndexerPath).Path
    gitCommit = $gitCommit
    buildProfile = if ($IndexerPath -match '\\release\\') { "release" } else { "debug" }
    machine = [ordered]@{
        computerName = $env:COMPUTERNAME
        os = [System.Environment]::OSVersion.VersionString
        powershell = $PSVersionTable.PSVersion.ToString()
    }
    runs = $runs
    summary = $summary
}

$json = $result | ConvertTo-Json -Depth 8
Set-Content -LiteralPath $OutputPath -Value $json -Encoding UTF8

Write-Host "Benchmark written to $OutputPath"
