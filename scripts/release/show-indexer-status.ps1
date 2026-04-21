param(
    [Parameter(Mandatory = $true)]
    [string]$DataDir,
    [string]$Prefix = "Active CodeAtlas indexer status:"
)

if (-not (Test-Path -LiteralPath $DataDir)) {
    exit 0
}

$statusFiles = Get-ChildItem -LiteralPath $DataDir -Filter "indexer-status-*.json" -ErrorAction SilentlyContinue |
    Sort-Object Name

if (-not $statusFiles -or $statusFiles.Count -eq 0) {
    exit 0
}

Write-Host $Prefix
foreach ($statusFile in $statusFiles) {
    try {
        $status = Get-Content -LiteralPath $statusFile.FullName -Raw | ConvertFrom-Json
        Write-Host ("  pid={0} mode={1} acquired_at={2} workspace={3}" -f $status.pid, $status.mode, $status.acquired_at, $status.workspace_root)
    } catch {
        Write-Host ("  unreadable status file: {0}" -f $statusFile.Name)
    }
}
