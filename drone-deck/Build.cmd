@echo off
rem Builds Drone Deck. Windows blocks .ps1 scripts by default, so run Build.ps1 with a one-off policy bypass.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0Build.ps1" %*
exit /b %ERRORLEVEL%
