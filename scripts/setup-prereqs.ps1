param(
    [switch]$SkipServerDeps
)

$ErrorActionPreference = "Stop"

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

    Write-Host "Installing $DisplayName with winget..."
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
            Write-Host "$DisplayName already available: $version"
        } else {
            Write-Host "$DisplayName already available."
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

Ensure-Tool -CommandName "node" -DisplayName "Node.js LTS" -WingetId "OpenJS.NodeJS.LTS"
Ensure-Tool -CommandName "npm" -DisplayName "npm" -WingetId "OpenJS.NodeJS.LTS"
Ensure-Tool -CommandName "cargo" -DisplayName "Rust toolchain" -WingetId "Rustlang.Rustup"

if (-not $SkipServerDeps) {
    if (Test-Path (Join-Path $ServerRoot "package-lock.json")) {
        Write-Host "Installing server npm dependencies..."
        Push-Location $ServerRoot
        try {
            & npm install
        } finally {
            Pop-Location
        }
    }
}

Write-Host ""
Write-Host "Prerequisite setup complete."
Write-Host "Next steps:"
Write-Host "  1. cd indexer && cargo build --release"
Write-Host "  2. cd server && npm run build"
