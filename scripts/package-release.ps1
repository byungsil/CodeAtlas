param(
    [string]$OutputRoot = (Join-Path $PSScriptRoot "..\\release"),
    [string]$BundleName,
    [switch]$SkipZip
)

$ErrorActionPreference = "Stop"

function Get-PackageVersion {
    param([string]$CargoTomlPath)
    $match = Select-String -Path $CargoTomlPath -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if (-not $match) {
        throw "Could not determine version from $CargoTomlPath"
    }
    return $match.Matches[0].Groups[1].Value
}

function Reset-Directory {
    param([string]$Path)
    if (Test-Path $Path) {
        Remove-Item -LiteralPath $Path -Recurse -Force
    }
    New-Item -ItemType Directory -Path $Path | Out-Null
}

function Copy-Tree {
    param(
        [string]$Source,
        [string]$Destination
    )
    if (-not (Test-Path $Source)) {
        throw "Missing source path: $Source"
    }
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    Copy-Item -LiteralPath $Source -Destination $Destination -Recurse -Force
}

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$IndexerRoot = Join-Path $RepoRoot "indexer"
$ServerRoot = Join-Path $RepoRoot "server"
$NodeRuntimeRoot = Join-Path $RepoRoot ".tools\\node"
$ReleaseAssetRoot = Join-Path $PSScriptRoot "release"
$Version = Get-PackageVersion -CargoTomlPath (Join-Path $IndexerRoot "Cargo.toml")

if ([string]::IsNullOrWhiteSpace($BundleName)) {
    $BundleName = "CodeAtlas-windows-x64-v$Version"
}

$resolvedOutputRoot = Resolve-Path $OutputRoot -ErrorAction SilentlyContinue
if ($resolvedOutputRoot) {
    $OutputRoot = $resolvedOutputRoot.Path
} else {
    $OutputRoot = [System.IO.Path]::GetFullPath($OutputRoot)
}
New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null

Push-Location $IndexerRoot
try {
    & "C:\Users\byung\.cargo\bin\cargo.exe" build --release
} finally {
    Pop-Location
}

Push-Location $ServerRoot
try {
    & (Join-Path $NodeRuntimeRoot "node.exe") ".\node_modules\typescript\bin\tsc" -p tsconfig.json
} finally {
    Pop-Location
}

$StagingRoot = Join-Path $OutputRoot $BundleName
Reset-Directory -Path $StagingRoot

New-Item -ItemType Directory -Force -Path (Join-Path $StagingRoot "bin") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $StagingRoot "server") | Out-Null

Copy-Item -LiteralPath (Join-Path $IndexerRoot "target\\release\\codeatlas-indexer.exe") -Destination (Join-Path $StagingRoot "bin\\codeatlas-indexer.exe") -Force
Copy-Tree -Source (Join-Path $ServerRoot "dist") -Destination (Join-Path $StagingRoot "server")
Copy-Tree -Source (Join-Path $ServerRoot "public") -Destination (Join-Path $StagingRoot "server")

Copy-Item -LiteralPath (Join-Path $ServerRoot "package.json") -Destination (Join-Path $StagingRoot "server\\package.json") -Force
Copy-Item -LiteralPath (Join-Path $ServerRoot "package-lock.json") -Destination (Join-Path $StagingRoot "server\\package-lock.json") -Force

Copy-Item -LiteralPath (Join-Path $ReleaseAssetRoot "README.txt") -Destination (Join-Path $StagingRoot "README.txt") -Force
Copy-Item -LiteralPath (Join-Path $ReleaseAssetRoot "setup-prereqs.ps1") -Destination (Join-Path $StagingRoot "setup-prereqs.ps1") -Force
Copy-Item -LiteralPath (Join-Path $ReleaseAssetRoot "setup-prereqs.cmd") -Destination (Join-Path $StagingRoot "setup-prereqs.cmd") -Force
Copy-Item -LiteralPath (Join-Path $ReleaseAssetRoot "mcp-config.example.json") -Destination (Join-Path $StagingRoot "mcp-config.example.json") -Force
Copy-Item -LiteralPath (Join-Path $ReleaseAssetRoot "index-workspace.cmd") -Destination (Join-Path $StagingRoot "index-workspace.cmd") -Force
Copy-Item -LiteralPath (Join-Path $ReleaseAssetRoot "watch-workspace.cmd") -Destination (Join-Path $StagingRoot "watch-workspace.cmd") -Force
Copy-Item -LiteralPath (Join-Path $ReleaseAssetRoot "start-dashboard.cmd") -Destination (Join-Path $StagingRoot "start-dashboard.cmd") -Force
Copy-Item -LiteralPath (Join-Path $ReleaseAssetRoot "start-mcp.cmd") -Destination (Join-Path $StagingRoot "start-mcp.cmd") -Force

$Metadata = [ordered]@{
    name = $BundleName
    version = $Version
    createdAt = (Get-Date).ToString("o")
    platform = "windows-x64"
    indexerBinary = "bin\\codeatlas-indexer.exe"
    dashboardEntry = "server\\dist\\index.js"
    mcpEntry = "server\\dist\\mcp.js"
}
$Metadata | ConvertTo-Json | Set-Content -Path (Join-Path $StagingRoot "bundle-manifest.json")

if (-not $SkipZip) {
    $ZipPath = Join-Path $OutputRoot "$BundleName.zip"
    if (Test-Path $ZipPath) {
        Remove-Item -LiteralPath $ZipPath -Force
    }
    Compress-Archive -Path (Join-Path $StagingRoot "*") -DestinationPath $ZipPath
    Write-Host "Created zip: $ZipPath"
}

Write-Host "Staging folder: $StagingRoot"
