@echo off
setlocal
if not exist "%~dp0src-tauri\target\release\dev-cache-cleaner.exe" (
    call "%~dp0build_app.bat"
    if errorlevel 1 exit /b 1
)
start "" "%~dp0src-tauri\target\release\dev-cache-cleaner.exe"
exit /b %errorlevel%
