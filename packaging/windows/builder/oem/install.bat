@echo off
powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:\OEM\bootstrap.ps1 >> C:\oem-bootstrap.log 2>&1
exit /b %errorlevel%
