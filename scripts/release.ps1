# Monitor Plus 发布脚本（Windows）
# 用法：powershell -File scripts\release.ps1 -Version 0.2.0 -Notes "更新说明"
# 完成后自动提交并推送 updates/ 清单与安装包，已安装的旧版本即可在小窗里一键更新。

param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$Notes = ""
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "版本号需为 x.y.z 格式" }

# 1. 同步三处版本号
(Get-Content src-tauri\tauri.conf.json -Raw) `
    -replace '"version": "[^"]*"', "`"version`": `"$Version`"" | Set-Content src-tauri\tauri.conf.json
(Get-Content src-tauri\Cargo.toml -Raw) `
    -replace '(?m)^version = "[^"]*"', "version = `"$Version`"" | Set-Content src-tauri\Cargo.toml
(Get-Content package.json -Raw) `
    -replace '"version": "[^"]*"', "`"version`": `"$Version`"" | Set-Content package.json

# 2. 构建（NSIS 安装包）
pnpm install
pnpm tauri build
if ($LASTEXITCODE -ne 0) { throw "构建失败" }

# 3. 收集产物
$nsis = Get-ChildItem "src-tauri\target\release\bundle\nsis\*-setup.exe" | Select-Object -First 1
if (-not $nsis) { throw "未找到 NSIS 安装包" }
New-Item -ItemType Directory -Force -Path updates | Out-Null
$assetName = "MonitorPlus_${Version}_x64-setup.exe"
Copy-Item $nsis.FullName "updates\$assetName" -Force

# 4. 更新清单（保留其他平台条目）
$manifest = [ordered]@{
    version  = $Version
    pub_date = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    notes    = $Notes
    platforms = [ordered]@{}
}
$manifestPath = "updates\latest.json"
if (Test-Path $manifestPath) {
    try {
        $old = Get-Content $manifestPath -Raw | ConvertFrom-Json
        if ($old.platforms) { $old.platforms.PSObject.Properties | ForEach-Object { $manifest.platforms[$_.Name] = $_.Value } }
    } catch { }
}
# 更新通道：默认 GitHub Releases（公开）；私有通道用环境变量覆盖
#   $env:RELEASE_BASE = "https://your.gitlab/api/v4/projects/ID/repository/files"  +  $env:RELEASE_TOKEN
$base = if ($env:RELEASE_BASE) { $env:RELEASE_BASE } else { "https://github.com/YOUR_GITHUB_USERNAME/monitor_plus/releases/download/v$Version" }
$manifest.platforms["windows-x86_64"] = [ordered]@{
    url  = "$base/updates%2F$assetName/raw?ref=main"
    size = (Get-Item "updates\$assetName").Length
}
$manifest | ConvertTo-Json -Depth 5 | Set-Content $manifestPath -Encoding utf8

# 5. 提交推送
git add updates src-tauri/tauri.conf.json src-tauri/Cargo.toml package.json src-tauri/Cargo.lock
git commit -m "release v$Version"
git push origin main

Write-Host "`n✅ v$Version 已发布。已安装的旧版本点『检查更新』即可升级。" -ForegroundColor Green
