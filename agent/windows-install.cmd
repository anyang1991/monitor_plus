@echo off
rem Monitor Plus agent 守护安装（双击运行即可）
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0windows-install.ps1"
pause
