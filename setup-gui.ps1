# CodeAtlas GUI Setup Wizard Launcher
# Usage: powershell -ExecutionPolicy Bypass -File .\setup-gui.ps1

$RepoRoot = (Resolve-Path $PSScriptRoot).Path
$WizardDir = Join-Path $RepoRoot "setup-wizard"

if (-not (Test-Path $WizardDir)) {
    Write-Host "Error: setup-wizard directory not found at $WizardDir" -ForegroundColor Red
    exit 1
}

# Install dependencies if needed
if (-not (Test-Path (Join-Path $WizardDir "node_modules"))) {
    Write-Host "Installing setup wizard dependencies..." -ForegroundColor Cyan
    Push-Location $WizardDir
    & npm install 2>&1 | Out-Null
    Pop-Location
}

# Build TypeScript
Write-Host "Building Setup Wizard..." -ForegroundColor Cyan
Push-Location $WizardDir
& npx tsc 2>&1 | Out-Null
Pop-Location

if (-not (Test-Path (Join-Path $WizardDir "electron-main.js"))) {
    Write-Host "Error: Build failed. Check TypeScript compilation." -ForegroundColor Red
    exit 1
}

# Launch Electron
Write-Host ""
Write-Host "Starting CodeAtlas Setup Wizard..." -ForegroundColor Green
Push-Location $WizardDir
& npx electron .
Pop-Location
