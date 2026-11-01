# Monitor Plus agent Windows 守护安装
# 用法：右键"使用 PowerShell 运行"，或 cmd 里执行 windows-install.cmd
# 效果：pythonw.exe（无窗口）运行 agent.py，计划任务每 5 分钟拉起一次（已运行则自动跳过）
param([switch]$Uninstall)

$ErrorActionPreference = "Stop"
$taskName = "MonitorPlusAgent"

if ($Uninstall) {
    schtasks /Delete /TN $taskName /F 2>$null
    Get-CimInstance Win32_Process -Filter "Name like 'python%'" |
        Where-Object { $_.CommandLine -match 'agent\.py' } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    Write-Host "已删除守护任务并停止 agent 进程" -ForegroundColor Green
    exit 0
}

# 定位脚本目录（agent.py 必须与本脚本同目录）
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$agentPy = Join-Path $scriptDir "agent.py"
if (-not (Test-Path $agentPy)) { Write-Host "未找到 $agentPy" -ForegroundColor Red; exit 1 }

# 找 pythonw.exe：优先 PATH 里的 pythonw，其次由 python.exe 同目录推导（排除商店占位 stub）
$pyw = $null
$pywCmd = Get-Command pythonw -All -ErrorAction SilentlyContinue |
    Where-Object { $_.Source -notmatch 'WindowsApps' } | Select-Object -First 1
if ($pywCmd) { $pyw = $pywCmd.Source }
if (-not $pyw) {
    $pyCmd = Get-Command python -All -ErrorAction SilentlyContinue |
        Where-Object { $_.Source -notmatch 'WindowsApps' } | Select-Object -First 1
    if ($pyCmd) {
        $candidate = Join-Path (Split-Path -Parent $pyCmd.Source) "pythonw.exe"
        if (Test-Path $candidate) { $pyw = $candidate }
    }
}
if (-not $pyw) {
    Write-Host "未找到可用的 pythonw.exe，请确认 Python 已安装且不在 WindowsApps 商店占位目录" -ForegroundColor Red
    exit 1
}
Write-Host "使用解释器: $pyw"

# 计划任务：立即触发一次 + 每 5 分钟重复 + 每次登录时触发
$action = New-ScheduledTaskAction -Execute $pyw -Argument "`"$agentPy`" --port 9101"
$triggerRep = New-ScheduledTaskTrigger -Once -At (Get-Date) -RepetitionInterval (New-TimeSpan -Minutes 5) -RepetitionDuration (New-TimeSpan -Days 3650)
$triggerLogon = New-ScheduledTaskTrigger -AtLogOn
Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $triggerRep, $triggerLogon -Force | Out-Null

Start-ScheduledTask -TaskName $taskName
Start-Sleep -Seconds 2

# 验证
$proc = Get-CimInstance Win32_Process -Filter "Name like 'python%'" |
    Where-Object { $_.CommandLine -match 'agent\.py' }
if ($proc) {
    Write-Host "✅ 守护已安装且 agent 正在运行（PID $($proc.ProcessId -join ', ')，无窗口）" -ForegroundColor Green
    Write-Host "   计划任务每 5 分钟检查一次：不在运行则自动拉起；以后开机登录也会自动启动"
} else {
    Write-Host "任务已创建但 agent 未见运行，稍等 5 分钟守护会再次拉起；或手动执行: Start-ScheduledTask $taskName" -ForegroundColor Yellow
}
