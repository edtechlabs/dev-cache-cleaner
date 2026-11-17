@echo off
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0deploy_windows.ps1" %*
exit /b %errorlevel%
