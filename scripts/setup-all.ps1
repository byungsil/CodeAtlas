# CodeAtlas Setup - Full Installation Script
# Run this to install all prerequisites and build everything

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
    Write-Log "Launching GUI Setup Wizard..." "INFO"
    
    $wizardDir = Join-Path $RepoRoot "setup-wizard"
    
    if (-not (Test-Path $wizardDir)) {
        Write-Log "Setup wizard directory not found: $wizardDir" "ERROR"
        exit 1
    }

    # Install npm dependencies for setup-wizard
    if (-not (Test-Path (Join-Path $wizardDir "node_modules"))) {
        Write-Host ""
        Write-Log "Installing setup wizard dependencies..." "INFO"
        Push-Location $wizardDir
        try {
            & npm install 2>&1 | Out-Null
            Write-Log "Setup wizard dependencies installed" "INFO"
        } catch {
            Write-Log "Failed to install setup wizard dependencies: $_" "ERROR"
            throw
        } finally {
            Pop-Location
        }
    }

    # Build Setup Wizard
    Write-Host ""
    Write-Log "Building Setup Wizard..." "INFO"
    Push-Location $wizardDir
    try {
        & npm run build
        Write-Log "Setup wizard build complete" "INFO"
    } catch {
        Write-Log "Failed to build setup wizard: $_" "ERROR"
        throw
    } finally {
        Pop-Location
    }

    if (-not (Test-Path (Join-Path $wizardDir "main/electron-main.js"))) {
        Write-Log "Build failed - electron-main.js not found" "ERROR"
        exit 1
    }

    # Launch Electron
    Write-Host ""
    Write-Log "Starting GUI Setup Wizard..." "INFO"
    Start-Sleep -Seconds 1
    
    Push-Location $wizardDir
    try {
        & npx electron .
    } finally {
        Pop-Location
    }
    
    exit 0
}

# ==================== CLI Mode (Legacy) ====================

Write-Log "Running in CLI mode. Use -Gui or -g flag for GUI." "INFO"
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
        & cargo build --release
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
        & npx tsc
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
Write-Host "  1. Run 'setup-all.ps1 -Gui' for GUI setup wizard" -ForegroundColor White
Write-Host "  2. Or run server: cd server && npm start" -ForegroundColor White
Write-Host ""
