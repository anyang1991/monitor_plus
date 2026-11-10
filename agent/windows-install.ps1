# Monitor Plus agent Windows 守护安装
# 用法：
#   .\windows-install.ps1
#   .\windows-install.ps1 -Port 9101 -Bind 0.0.0.0 -Interval 2 -Token "secret"
#   .\windows-install.ps1 -Uninstall
# 本脚本不修改防火墙；安装和检测只针对本目录中的 agent.py。
param(
    [string]$Token = "",
    [string]$Bind = "0.0.0.0",
    [int]$Port = 9101,
    [double]$Interval = 2.0,
    [switch]$Uninstall
)

$ErrorActionPreference = "Stop"
$taskName = "MonitorPlusAgent"

if ($Bind -notmatch '^[A-Za-z0-9._:-]{1,253}$') {
    throw "-Bind 只支持主机名或 IPv4/IPv6 文本形式，且不能包含空白或命令分隔符"
}
if ($Port -lt 1 -or $Port -gt 65535) {
    throw "-Port 必须在 1-65535 之间"
}
if ([double]::IsNaN($Interval) -or [double]::IsInfinity($Interval) -or
    $Interval -lt 0.5 -or $Interval -gt 3600) {
    throw "-Interval 必须是有限数值，且在 0.5-3600 秒之间"
}
if ($Token.Length -gt 512 -or $Token -match '[\r\n\0]') {
    throw "-Token 长度不能超过 512，且不能包含换行或 NUL"
}

$scriptDir = Split-Path -Parent -Path $PSCommandPath
$agentPy = Join-Path -Path $scriptDir -ChildPath "agent.py"
if (-not (Test-Path -LiteralPath $agentPy -PathType Leaf)) {
    Write-Host "未找到 $agentPy" -ForegroundColor Red
    exit 1
}

function ConvertTo-TaskArgument([string]$Value) {
    # 按 CommandLineToArgvW 规则加引号，安全处理空格、引号和尾部反斜杠。
    if ($null -eq $Value -or $Value.Length -eq 0) {
        return '""'
    }

    $escaped = $Value -replace '(\\*)"', '$1$1\"'
    if ($escaped.EndsWith('\')) {
        $escaped += '\'
    }
    return '"' + $escaped + '"'
}

function Test-AgentHealth {
    param(
        [string]$HostName,
        [int]$ListenPort,
        [string]$AccessToken
    )

    $headers = @{}
    if ($AccessToken) {
        $headers["X-Agent-Token"] = $AccessToken
    }

    try {
        $builder = [System.UriBuilder]::new("http", $HostName, $ListenPort, "/health")
        $health = Invoke-RestMethod -Uri $builder.Uri.AbsoluteUri -Headers $headers -TimeoutSec 2 -ErrorAction Stop
        return $null -ne $health -and $true -eq $health.ok
    }
    catch {
        return $false
    }
}

if ($Uninstall) {
    $existingTask = Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
    if ($existingTask) {
        Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
    }

    # 只匹配安装器写入的完整 agent.py 路径，不停止其他目录里的 Python/agent 进程。
    $agentPathNeedle = '"' + $agentPy + '"'
    Get-CimInstance -ClassName Win32_Process -Filter "Name like 'python%'" |
        Where-Object {
            $_.CommandLine -and
            $_.CommandLine.IndexOf($agentPathNeedle, [System.StringComparison]::OrdinalIgnoreCase) -ge 0
        } |
        ForEach-Object {
            Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
        }

    Write-Host "已删除守护任务，并只停止本目录 agent" -ForegroundColor Green
    exit 0
}

# 找 pythonw.exe：优先 PATH 里的 pythonw，其次由 python.exe 同目录推导（排除商店占位 stub）
$pyw = $null
$pywCmd = Get-Command pythonw -All -ErrorAction SilentlyContinue |
    Where-Object { $_.Source -notmatch 'WindowsApps' } | Select-Object -First 1
if ($pywCmd) { $pyw = $pywCmd.Source }
if (-not $pyw) {
    $pyCmd = Get-Command python -All -ErrorAction SilentlyContinue |
        Where-Object { $_.Source -notmatch 'WindowsApps' } | Select-Object -First 1
    if ($pyCmd) {
        $candidate = Join-Path -Path (Split-Path -Parent $pyCmd.Source) -ChildPath "pythonw.exe"
        if (Test-Path -LiteralPath $candidate) { $pyw = $candidate }
    }
}
if (-not $pyw) {
    Write-Host "未找到可用的 pythonw.exe，请确认 Python 已安装且不在 WindowsApps 商店占位目录" -ForegroundColor Red
    exit 1
}
Write-Host "使用解释器: $pyw"

$argumentParts = @(
    (ConvertTo-TaskArgument $agentPy),
    "--port", "$Port",
    "--bind", (ConvertTo-TaskArgument $Bind),
    "--interval", $Interval.ToString([System.Globalization.CultureInfo]::InvariantCulture)
)
if ($Token) {
    $argumentParts += @("--token", (ConvertTo-TaskArgument $Token))
}
$agentArguments = $argumentParts -join " "

$action = New-ScheduledTaskAction -Execute $pyw -Argument $agentArguments
$currentUser = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
$triggerRep = New-ScheduledTaskTrigger -Once -At (Get-Date) -RepetitionInterval (New-TimeSpan -Minutes 5) -RepetitionDuration (New-TimeSpan -Days 3650)
$triggerLogon = New-ScheduledTaskTrigger -AtLogOn -User $currentUser

# 电池供电时允许启动且切换电池不中断；零执行时限表示没有 72 小时终止限制。
$settings = New-ScheduledTaskSettingsSet `
    -AllowStartIfOnBatteries `
    -DontStopIfGoingOnBatteries `
    -ExecutionTimeLimit ([System.TimeSpan]::Zero) `
    -MultipleInstances IgnoreNew

Register-ScheduledTask -TaskName $taskName -Action $action `
    -Trigger $triggerRep, $triggerLogon -Settings $settings -Force | Out-Null

Start-ScheduledTask -TaskName $taskName

# 只用本安装参数对应的 /health 验证；令牌不匹配时不会误报成功。
$probeHost = if ($Bind -eq "0.0.0.0" -or $Bind -eq "::") { "127.0.0.1" } else { $Bind }
$healthy = $false
for ($attempt = 0; $attempt -lt 20 -and -not $healthy; $attempt++) {
    Start-Sleep -Milliseconds 250
    $healthy = Test-AgentHealth -HostName $probeHost -ListenPort $Port -AccessToken $Token
}

if ($healthy) {
    Write-Host "✅ 守护已安装，健康检查通过：http://$(if ($probeHost -eq $Bind) { $Bind } else { $probeHost }):$Port/health" -ForegroundColor Green
    Write-Host "   计划任务每 5 分钟守护一次；使用电池也会启动/继续运行，且没有执行时限终止。"
} else {
    Write-Host "任务已创建但 /health 未通过。请检查端口、令牌和 agent 日志，或执行: Start-ScheduledTask $taskName" -ForegroundColor Yellow
}
