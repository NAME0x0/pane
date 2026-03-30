@echo off
setlocal
set "SCRIPT_DIR=%~dp0"
powershell -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%SCRIPT_DIR%Collect Pane Support Bundle.ps1" %*
exit /b %ERRORLEVEL%
