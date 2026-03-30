@echo off
setlocal
set "SCRIPT_DIR=%~dp0"
powershell -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%SCRIPT_DIR%Install Pane Shortcuts.ps1" %*
exit /b %ERRORLEVEL%
