param()

$ErrorActionPreference = "Stop"
$ScriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$ServerRoot = Join-Path $ScriptRoot "server"

function Test-CommandAvailable {
    param([string]$Name)
    return $null -ne (Get-Command $Name -ErrorAction SilentlyContinue)
}

function Install-NodeWithWinget {
    if (-not (Test-CommandAvailable "winget")) {
        throw "winget is not available. Please install Node.js LTS manually."
    }

    Write-Host "Installing Node.js LTS with winget..."
    & winget install --exact --id OpenJS.NodeJS.LTS --accept-source-agreements --accept-package-agreements
}

if (Test-CommandAvailable "node") {
    $version = try { & node --version 2>$null | Select-Object -First 1 } catch { $null }
    if ($version) {
        Write-Host "Node.js already available: $version"
    } else {
        Write-Host "Node.js already available."
    }
} else {
    Install-NodeWithWinget
    if (-not (Test-CommandAvailable "node")) {
        Write-Warning "Node.js was installed, but the current shell may not see it yet. Open a new shell if follow-up commands fail."
    }
}

if (-not (Test-Path (Join-Path $ServerRoot "package.json"))) {
    throw "Missing bundled server package.json under $ServerRoot"
}

if (-not (Test-CommandAvailable "npm")) {
    throw "npm is not available after installing Node.js. Open a new shell and re-run setup-prereqs.cmd."
}

Write-Host ""
Write-Host "Installing CodeAtlas server runtime dependencies..."
Push-Location $ServerRoot
try {
    & npm ci --omit=dev
} finally {
    Pop-Location
}

Write-Host ""
Write-Host "Runtime setup complete."
Write-Host "You can now use MCP or start the dashboard from this folder."
