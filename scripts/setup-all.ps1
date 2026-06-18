# CodeAtlas Setup - CLI bootstrap and GUI compatibility entry point
# Use setup-gui.ps1 as the canonical GUI launcher; this script supports CLI setup and delegates -Gui.

param(
    [switch]$Gui,
    [Alias("g")]
    [switch]$LaunchGui
)

$ErrorActionPreference = "Stop"

function Write-Log {
    param([string]$Message, [string]$Level = "INFO")
    
    $timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
    switch ($Level) {
        "ERROR" { Write-Host "[$timestamp] [$Level] $Message" -ForegroundColor Red }
        "WARN"  { Write-Host "[$timestamp] [$Level] $Message" -ForegroundColor Yellow }
        default { Write-Host "[$timestamp] [$Level] $Message" -ForegroundColor White }
    }
}

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

Write-Host ""
Write-Host "==================================================" -ForegroundColor Cyan
Write-Host " CodeAtlas Full Setup"
Write-Host "==================================================" -ForegroundColor Cyan
Write-Host ""

# Check if running in GUI mode
if ($Gui -or $LaunchGui) {
    $guiLauncher = Join-Path $RepoRoot "setup-gui.ps1"

    if (-not (Test-Path $guiLauncher)) {
        Write-Log "GUI launcher not found: $guiLauncher" "ERROR"
        exit 1
    }

    Write-Log "Delegating GUI setup to setup-gui.ps1..." "INFO"
    & $guiLauncher
    if (-not $?) {
        exit 1
    }

    exit 0
}

# ==================== CLI Mode (Legacy) ====================

Write-Log "Running in CLI bootstrap mode. Recommended GUI entry point: .\\setup-gui.ps1" "INFO"
Write-Host ""

$scriptPath = Join-Path $PSScriptRoot "setup-prereqs.ps1"
if (Test-Path $scriptPath) {
    Write-Log "Running prerequisite setup..." "INFO"
    & $scriptPath
} else {
    Write-Log "setup-prereqs.ps1 not found at: $scriptPath" "ERROR"
    exit 1
}

Write-Host ""
Write-Log "Prerequisites installed. Would you like to build the indexer and server now?" "INFO"
$answer = Read-Host "Build now? (Y/n)"

if ($answer -ne 'n' -and $answer -ne 'N') {
    Write-Host ""
    
    # Build indexer
    Write-Log "Building Rust indexer..." "INFO"
    Push-Location (Join-Path $RepoRoot "indexer")
    try {
        & cargo build --release 2>&1 | Out-Null
        Write-Log "Indexer built successfully" "INFO"
    } catch {
        Write-Log "Failed to build indexer: $_" "ERROR"
    } finally {
        Pop-Location
    }

    # Build server
    Write-Log "Building TypeScript server..." "INFO"
    Push-Location (Join-Path $RepoRoot "server")
    try {
        & npx tsc 2>&1 | Out-Null
        Write-Log "Server built successfully" "INFO"
    } catch {
        Write-Log "Failed to build server: $_" "ERROR"
    } finally {
        Pop-Location
    }
}

Write-Host ""
Write-Log "Setup complete!" "INFO"
Write-Host ""
Write-Host "Next steps:" -ForegroundColor Cyan
Write-Host "  1. Run 'powershell -ExecutionPolicy Bypass -File .\\setup-gui.ps1' for the GUI setup wizard" -ForegroundColor White
Write-Host "  2. Or run server: cd server && npm start" -ForegroundColor White
Write-Host ""
