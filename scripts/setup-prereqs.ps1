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
        "WARN"  { Write-Host $logEntry -ForegroundColor Yellow }
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
    $ec = $LASTEXITCODE
    # winget returns 1 when the package is already installed and no upgrade is available.
    # -1978335189 (0x8A150011) means "already installed" on some winget versions.
    if ($ec -ne 0 -and $ec -ne -1978335189) {
        Write-Log "winget exited with code $ec for $DisplayName" "WARN"
    }
}

function Ensure-Tool {
    param(
        [string]$CommandName,
        [string]$DisplayName,
        [string]$WingetId,
        [string[]]$FallbackPaths = @()
    )

    # Check PATH first
    if (Test-CommandAvailable $CommandName) {
        $version = try { & $CommandName --version 2>$null | Select-Object -First 1 } catch { $null }
        if ($version) {
            Write-Log "$DisplayName already available: $version" "INFO"
        } else {
            Write-Log "$DisplayName already available." "INFO"
        }
        return
    }

    # Check fallback absolute paths (e.g. LLVM not in PATH but installed)
    foreach ($fb in $FallbackPaths) {
        if (Test-Path $fb) {
            Write-Log "$DisplayName found at $fb (not in PATH)." "INFO"
            return
        }
    }

    Install-WithWinget -PackageId $WingetId -DisplayName $DisplayName

    if (-not (Test-CommandAvailable $CommandName)) {
        $foundViaFallback = $FallbackPaths | Where-Object { Test-Path $_ } | Select-Object -First 1
        if ($foundViaFallback) {
            Write-Log "$DisplayName installed at $foundViaFallback (not in PATH - open a new shell if needed)." "WARN"
        } else {
            Write-Warning "$DisplayName was installed, but the current shell may not see it yet. Open a new shell if follow-up commands fail."
        }
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

# 1. Node.js & npm
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

# LLVM/Clang (optional) - enhances C++ AST analysis quality.
# If unavailable, the indexer falls back to tree-sitter-only parsing.
try {
    $llvmFallbacks = @("C:\Program Files\LLVM\bin\clang.exe", "C:\Program Files (x86)\LLVM\bin\clang.exe")
    Ensure-Tool -CommandName "clang" -DisplayName "LLVM/Clang (C++ AST - optional)" -WingetId "LLVM.LLVM" -FallbackPaths $llvmFallbacks
} catch {
    Write-Log "LLVM/Clang not available - C++ will use tree-sitter fallback: $_" "WARN"
}

# 2. Server Dependencies (optional)
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

# 3. GUI Setup Wizard (optional)
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

    # Build TypeScript + copy assets
    Push-Location $WizardDir
    try {
        & npm run build 2>&1 | Out-Null
    } finally {
        Pop-Location
    }

    if (-not (Test-Path (Join-Path $WizardDir "main\electron-main.js"))) {
        Write-Log "Build failed - main/electron-main.js not found" "ERROR"
        exit 1
    }

    # Launch Electron (use local binary)
    $ElectronBin = Join-Path $WizardDir "node_modules\electron\dist\electron.exe"
    if (-not (Test-Path $ElectronBin)) {
        $ElectronBin = Join-Path $WizardDir "node_modules\.bin\electron.cmd"
    }
    Push-Location $WizardDir
    try {
        & $ElectronBin .
    } finally {
        Pop-Location
    }
    
    exit 0
}

# ==================== Done ====================

Write-Host ""
Write-Log "Prerequisite setup complete!" "INFO"
Write-Host ""
Write-Log "Next steps:" "INFO"
Write-Host "  1. Build indexer:  cd indexer; cargo build --release" -ForegroundColor White
Write-Host "  2. Start server:   cd server; npm run build" -ForegroundColor White
Write-Host ""
Write-Host "  Run GUI wizard:   powershell setup-gui.ps1" -ForegroundColor Gray
Write-Host ""

