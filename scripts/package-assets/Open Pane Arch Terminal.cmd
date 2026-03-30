@echo off
setlocal
set "SCRIPT_DIR=%~dp0"
powershell -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%SCRIPT_DIR%Open Pane Arch Terminal.ps1" %*
exit /b %ERRORLEVEL%