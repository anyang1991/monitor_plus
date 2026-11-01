@echo off
rem 停止并卸载 Monitor Plus agent 守护（双击运行即可）
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0windows-install.ps1" -Uninstall
pause
