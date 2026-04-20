@echo off
setlocal
set "SCRIPT_DIR=%~dp0"
"%SCRIPT_DIR%bin\codeatlas-indexer.exe" %*

