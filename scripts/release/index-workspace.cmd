@echo off
setlocal
set "SCRIPT_DIR=%~dp0"
set "WORKSPACE_ROOT=%~f1"
if not "%WORKSPACE_ROOT%"=="" (
  set "DATA_DIR=%WORKSPACE_ROOT%\.codeatlas"
  powershell -NoProfile -ExecutionPolicy Bypass -File "%SCRIPT_DIR%show-indexer-status.ps1" -DataDir "%DATA_DIR%" -Prefix "Existing CodeAtlas indexer status before index-workspace:"
)
"%SCRIPT_DIR%bin\codeatlas-indexer.exe" %*

