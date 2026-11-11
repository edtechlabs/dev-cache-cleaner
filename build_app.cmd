@echo off
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0build_app.ps1" %*
exit /b %errorlevel%
