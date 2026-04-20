@echo off
setlocal

if "%~1"=="" (
  echo Usage: start-mcp.cmd ^<workspace-root^>
  exit /b 1
)

set "SCRIPT_DIR=%~dp0"
set "WORKSPACE_ROOT=%~f1"
set "DATA_DIR=%WORKSPACE_ROOT%\.codeatlas"

where node >nul 2>nul
if errorlevel 1 (
  echo Missing Node.js runtime. Run setup-prereqs.cmd first.
  exit /b 1
)

if not exist "%DATA_DIR%" (
  echo Missing data directory: %DATA_DIR%
  exit /b 1
)

set "CODEATLAS_WORKSPACE=%WORKSPACE_ROOT%"
set "CODEATLAS_INDEXER_PATH=%SCRIPT_DIR%bin\codeatlas-indexer.exe"

node "%SCRIPT_DIR%server\dist\mcp.js" "%DATA_DIR%"
