@echo off
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0run_dev.ps1" %*
exit /b %errorlevel%
