param(
    [switch]$SkipServerDeps,
    [switch]$SkipServerBuild,
    [switch]$SkipIndexerBuild
)

$ErrorActionPreference = "Stop"

function Test-CommandAvailable {
    param([string]$Name)
    return $null -ne (Get-Command $Name -ErrorAction SilentlyContinue)
}

function Invoke-Step {
    param(
        [string]$Name,
        [scriptblock]$Action
    )

    Write-Host ""
    Write-Host "==> $Name"
    & $Action
}

function Install-WithWinget {
    param(
        [string]$PackageId,
        [string]$DisplayName,
        [string]$Override
    )

    if (-not (Test-CommandAvailable "winget")) {
        throw "winget is not available. Please install $DisplayName manually."
    }

    Write-Host "Installing $DisplayName with winget..."
    if ($Override) {
        & winget install --exact --id $PackageId --override $Override --accept-source-agreements --accept-package-agreements
    } else {
        & winget install --exact --id $PackageId --accept-source-agreements --accept-package-agreements
    }
}

function Ensure-VisualCppToolchain {
    # If cl exists in PATH, assume MSVC build tools are ready for this shell.
    if (Test-CommandAvailable "cl") {
        Write-Host "MSVC toolchain already available in current shell."
        return
    }

    # Fallback: detect Visual Studio Build Tools install via vswhere.
    $vsWherePath = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vsWherePath) {
        $installPath = & $vsWherePath -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
        if ($LASTEXITCODE -eq 0 -and $installPath) {
            Write-Warning "Visual Studio Build Tools are installed, but cl.exe is not in current PATH."
            Write-Warning "Continuing anyway. If indexer build fails later, rerun in 'Developer PowerShell for VS'."
            return
        }
    }

    $vsOverride = "--wait --quiet --add Microsoft.VisualStudio.Workload.VCTools --add Microsoft.VisualStudio.Component.Windows11SDK.22621"
    Install-WithWinget -PackageId "Microsoft.VisualStudio.2022.BuildTools" -DisplayName "Visual Studio 2022 Build Tools (MSVC + Windows SDK)" -Override $vsOverride

    if (-not (Test-CommandAvailable "cl")) {
        Write-Warning "Build Tools installation finished, but cl.exe is still not visible in current shell."
        Write-Warning "Continuing anyway. If indexer build fails later, rerun in a fresh 'Developer PowerShell for VS'."
    }
}

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$IndexerRoot = Join-Path $RepoRoot "indexer"
$ServerRoot = Join-Path $RepoRoot "server"
$PrereqScript = Join-Path $PSScriptRoot "setup-prereqs.ps1"

if (-not (Test-Path $PrereqScript)) {
    throw "Missing script: $PrereqScript"
}

Invoke-Step -Name "Install base prerequisites (Node.js, npm, Rust, server deps optional)" -Action {
    if ($SkipServerDeps) {
        & powershell -ExecutionPolicy Bypass -File $PrereqScript -SkipServerDeps
    } else {
        & powershell -ExecutionPolicy Bypass -File $PrereqScript
    }

    if ($LASTEXITCODE -ne 0) {
        throw "setup-prereqs.ps1 failed with exit code $LASTEXITCODE"
    }
}

Invoke-Step -Name "Validate/install Windows C/C++ toolchain (MSVC + Windows SDK)" -Action {
    Ensure-VisualCppToolchain
}

if (-not $SkipIndexerBuild) {
    Invoke-Step -Name "Build indexer (cargo build --release)" -Action {
        Push-Location $IndexerRoot
        try {
            & cargo build --release
            if ($LASTEXITCODE -ne 0) {
                throw "Indexer build failed with exit code $LASTEXITCODE"
            }
        } finally {
            Pop-Location
        }
    }
}

Invoke-Step -Name "Install server dependencies (npm install)" -Action {
    if ($SkipServerDeps) {
        Write-Host "Skipped (requested by -SkipServerDeps)."
        return
    }

    Push-Location $ServerRoot
    try {
        & npm install
        if ($LASTEXITCODE -ne 0) {
            throw "npm install failed with exit code $LASTEXITCODE"
        }
    } finally {
        Pop-Location
    }
}

if (-not $SkipServerBuild) {
    Invoke-Step -Name "Build server (npm run build)" -Action {
        Push-Location $ServerRoot
        try {
            & npm run build
            if ($LASTEXITCODE -ne 0) {
                throw "Server build failed with exit code $LASTEXITCODE"
            }
        } finally {
            Pop-Location
        }
    }
}

Write-Host ""
Write-Host "Setup complete."
Write-Host "Artifacts:"
Write-Host "  - indexer: indexer/target/release/codeatlas-indexer.exe"
Write-Host "  - server build: server/dist (if build script outputs dist)"
