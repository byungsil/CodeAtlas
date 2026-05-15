param()

$ErrorActionPreference = "Stop"
$ScriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$ServerRoot = Join-Path $ScriptRoot "server"
$IndexerPath = Join-Path $ScriptRoot "bin\codeatlas-indexer.exe"
$PrereqScript = Join-Path $ScriptRoot "setup-prereqs.ps1"

function Invoke-Step {
    param(
        [string]$Name,
        [scriptblock]$Action
    )

    Write-Host ""
    Write-Host "==> $Name"
    & $Action
}

Invoke-Step -Name "Install runtime prerequisites (Node.js + npm runtime deps)" -Action {
    if (-not (Test-Path $PrereqScript)) {
        throw "Missing script: $PrereqScript"
    }

    & powershell -NoProfile -ExecutionPolicy Bypass -File $PrereqScript
    if ($LASTEXITCODE -ne 0) {
        throw "setup-prereqs.ps1 failed with exit code $LASTEXITCODE"
    }
}

Invoke-Step -Name "Validate bundled release artifacts" -Action {
    if (-not (Test-Path $IndexerPath)) {
        throw "Missing indexer binary: $IndexerPath"
    }

    if (-not (Test-Path (Join-Path $ServerRoot "package.json"))) {
        throw "Missing server package.json under $ServerRoot"
    }

    if (-not (Test-Path (Join-Path $ServerRoot "dist"))) {
        throw "Missing server dist bundle under $ServerRoot"
    }

    if (-not (Test-Path (Join-Path $ServerRoot "public"))) {
        throw "Missing server public assets under $ServerRoot"
    }
}

Write-Host ""
Write-Host "Release setup complete."
Write-Host "Next:"
Write-Host "  1. index-workspace.cmd <workspace-root> --full --workspace-name <display-name>"
Write-Host "  2. Configure MCP using mcp-config.example.json"
