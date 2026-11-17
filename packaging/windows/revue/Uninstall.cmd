@echo off
rem Removes Revue PDF installed for this account.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0Uninstall-Revue.ps1" %*
echo.
pause
