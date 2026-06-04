param(
    [switch]$SkipServerDeps,
    [switch]$Gui,
    [Alias("g")]
    [switch]$LaunchGui
)

$ErrorActionPreference = "Stop"

function Write-Log {
    param([string]$Message, [string]$Level = "INFO")
    
    $timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
    $logEntry = "[$timestamp] [$Level] $Message"
    
    switch ($Level) {
        "ERROR" { Write-Host $logEntry -ForegroundColor Red }
        "WARN"  { Write- Host $logEntry -ForegroundColor Yellow }
        default { Write-Host $logEntry -ForegroundColor White }
    }
}

function Test-CommandAvailable {
    param([string]$Name)
    return $null -ne (Get-Command $Name -ErrorAction SilentlyContinue)
}

function Install-WithWinget {
    param(
        [string]$PackageId,
        [string]$DisplayName
    )

    if (-not (Test-CommandAvailable "winget")) {
        throw "winget is not available. Please install $DisplayName manually."
    }

    Write-Host "Installing $DisplayName with winget..." -ForegroundColor Cyan
    & winget install --exact --id $PackageId --accept-source-agreements --accept-package-agreements
}

function Ensure-Tool {
    param(
        [string]$CommandName,
        [string]$DisplayName,
        [string]$WingetId
    )

    if (Test-CommandAvailable $CommandName) {
        $version = try { & $CommandName --version 2>$null | Select-Object -First 1 } catch { $null }
        if ($version) {
            Write-Log "$DisplayName already available: $version" "INFO"
        } else {
            Write-Log "$DisplayName already available." "INFO"
        }
        return
    }

    Install-WithWinget -PackageId $WingetId -DisplayName $DisplayName

    if (-not (Test-CommandAvailable $CommandName)) {
        Write-Warning "$DisplayName was installed, but the current shell may not see it yet. Open a new shell if follow-up commands fail."
    }
}

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$ServerRoot = Join-Path $RepoRoot "server"
$WizardDir = Join-Path $RepoRoot "setup-wizard"

Write-Host ""
Write-Host "==================================================" -ForegroundColor Cyan
Write-Host " CodeAtlas Prerequisite Setup" -ForegroundColor Cyan
Write-Host "==================================================" -ForegroundColor Cyan
Write-Host ""

# 1. 필수 도구 설치
try {
    Ensure-Tool -CommandName "node" -DisplayName "Node.js LTS" -WingetId "OpenJS.NodeJS.LTS"
} catch {
    Write-Log "Failed to install Node.js: $_" "ERROR"
    throw
}

try {
    Ensure-Tool -CommandName "npm" -DisplayName "npm" -WingetId "OpenJS.NodeJS.LTS"
} catch {
    Write-Log "Failed to install npm: $_" "ERROR"
    throw
}

try {
    Ensure-Tool -CommandName "cargo" -DisplayName "Rust toolchain" -WingetId "Rustlang.Rustup"
} catch {
    Write-Log "Failed to install Rust: $_" "ERROR"
    throw
}

# 2. 서버 의존성 설치
if (-not $SkipServerDeps) {
    if (Test-Path (Join-Path $ServerRoot "package-lock.json")) {
        Write-Host ""
        Write-Log "Installing server npm dependencies..." "INFO"
        Push-Location $ServerRoot
        try {
            & npm install 2>&1 | Out-Null
            Write-Log "Server dependencies installed" "INFO"
        } catch {
            Write-Log "Failed to install server dependencies: $_" "ERROR"
            throw
        } finally {
            Pop-Location
        }
    }
}

# 3. GUI 마법사 실행 (선택사항)
if ($Gui -or $LaunchGui) {
    Write-Host ""
    Write-Log "Launching GUI Setup Wizard..." "INFO"
    
    if (-not (Test-Path $WizardDir)) {
        Write-Log "Setup wizard directory not found: $WizardDir" "ERROR"
        exit 1
    }

    # Install setup-wizard dependencies
    if (-not (Test-Path (Join-Path $WizardDir "node_modules"))) {
        Push-Location $WizardDir
        try {
            & npm install 2>&1 | Out-Null
        } finally {
            Pop-Location
        }
    }

    # Build TypeScript
    Push-Location $WizardDir
    try {
        & npx tsc 2>&1 | Out-Null
    } finally {
        Pop-Location
    }

    if (-not (Test-Path (Join-Path $WizardDir "electron-main.js"))) {
        Write-Log "Build failed - electron-main.js not found" "ERROR"
        exit 1
    }

    # Launch Electron
    Push-Location $WizardDir
    try {
        & npx electron .
    } finally {
        Pop-Location
    }
    
    exit 0
}

# ==================== 완료 메시지 ====================

Write-Host ""
Write-Log "Prerequisite setup complete!" "INFO"
Write-Host ""
Write-Log "Next steps:" "INFO"
Write-Host "  ${WHITE}1. Build indexer: cd indexer && cargo build --release${NC}" -ForegroundColor White
Write-Host "  ${WHITE}2. Start server:   cd server && npm run build${NC}" -ForegroundColor White
Write-Host ""

# GUI 옵션 안내
Write-Host "${GRAY}GUI Setup Wizard를 사용하려면:${NC}" -ForegroundColor Gray
Write-Host "${GRAY}  powershell scripts/setup-all.ps1 -Gui${NC}" -ForegroundColor Gray
Write-Host ""
