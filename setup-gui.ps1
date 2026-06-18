# CodeAtlas GUI Setup Wizard Launcher
# Canonical Windows entry point for the interactive setup wizard.
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

# Build TypeScript + copy assets
Write-Host "Building Setup Wizard..." -ForegroundColor Cyan
Push-Location $WizardDir
& npm run build 2>&1 | Out-Null
Pop-Location

if (-not (Test-Path (Join-Path $WizardDir "main\electron-main.js"))) {
    Write-Host "Error: Build failed. Check TypeScript compilation." -ForegroundColor Red
    exit 1
}

# Launch Electron (use local binary to avoid npx download prompt)
$ElectronBin = Join-Path $WizardDir "node_modules\electron\dist\electron.exe"
if (-not (Test-Path $ElectronBin)) {
    $ElectronBin = Join-Path $WizardDir "node_modules\.bin\electron.cmd"
}

Write-Host ""
Write-Host "Starting CodeAtlas Setup Wizard..." -ForegroundColor Green
Push-Location $WizardDir
if ($ElectronBin -like "*.cmd") {
    # .cmd shims can detach from PowerShell; run via cmd.exe explicitly and wait
    $proc = Start-Process -FilePath "cmd.exe" -ArgumentList "/c `"$ElectronBin`" ." -WorkingDirectory $WizardDir -PassThru -NoNewWindow
} else {
    $proc = Start-Process -FilePath $ElectronBin -ArgumentList "." -WorkingDirectory $WizardDir -PassThru
}
$proc.WaitForExit()
Pop-Location
