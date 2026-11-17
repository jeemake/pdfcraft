@echo off
rem Installs Revue PDF for this account. No administrator rights needed.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0Install-Revue.ps1" %*
echo.
pause
