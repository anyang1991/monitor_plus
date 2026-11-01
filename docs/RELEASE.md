# 发布与应用更新

Monitor Plus 使用「版本清单 + 安装包」检查与安装更新。公开发行使用 GitHub Releases；自建 GitLab 通道可以在应用设置中配置。

## 使用公开更新通道

在「设置 → 应用更新」填写下面的清单地址，访问令牌留空。保存后重新打开设置并检查更新。

```text
https://github.com/anyang1991/monitor_plus/releases/latest/download/latest.json
```

0.3.0 内置地址中的用户名尚未修正，需要手动填写上面的地址。0.2.x 的更新地址和令牌固定在旧程序中，请手动下载安装新版。

## 发布到 GitHub Releases

1. 同步 `package.json`、`src-tauri/Cargo.toml` 和 `src-tauri/tauri.conf.json` 的版本号。
2. 运行 `pnpm install` 与 `pnpm tauri build`，确认构建成功。
3. 从 `src-tauri/target/release/bundle/nsis/` 取出 Windows 安装包，使用 `MonitorPlus_<版本>_x64-setup.exe` 作为发布文件名。
4. 以 [`updates/latest.example.json`](../updates/latest.example.json) 为基础创建 `latest.json`，填写版本、说明、安装包下载地址与实际字节大小。
5. 在 GitHub 创建对应版本的 Release，并上传安装包和 `latest.json` 两个资产。
6. 验证清单和安装包均能公开下载，再在旧版本的应用设置中检查更新。

清单示例：

```json
{
  "version": "0.3.0",
  "notes": "本次更新说明",
  "platforms": {
    "windows-x86_64": {
      "url": "https://github.com/anyang1991/monitor_plus/releases/download/v0.3.0/MonitorPlus_0.3.0_x64-setup.exe",
      "size": 2328842
    }
  }
}
```

上例的字节大小对应当前 0.3.0 资产，发布新包时请替换。版本号使用 `x.y.z`；当前客户端按三个数字段比较版本。

`scripts/release.ps1` 和 `scripts/release.sh` 仍包含旧 GitLab 发布流程，尚未实现完整的 GitHub Release 资产上传。GitHub 发行请按上面的步骤操作。

## 自建 GitLab 私有更新通道

将安装包和清单放入自己的私有通道。在应用「设置 → 应用更新」填写：

- **清单地址**：GitLab Repository Files API 的 `latest.json` raw 地址。
- **访问令牌**：有权读取目标文件的 Project / Personal Access Token。请确认该 GitLab 版本及令牌 scope 允许读取所使用的 API 端点。

应用将普通令牌通过 `PRIVATE-TOKEN` 请求头发送。包含 `:` 的值按 `用户名:令牌` 进行 Basic 认证；Deploy Token 对 GitLab REST API 的支持与普通访问令牌不同，不能直接等同使用。

访问令牌属于客户端的本机配置，不写入公开源码、更新清单或安装包。Windows 配置文件位于 `%APPDATA%\com.monitorplus.app\config.json`。

## 安装行为

- **Windows**：下载安装包后退出应用，再启动 NSIS 静默安装；安装完成后可重新打开应用。
- **macOS**：支持相应 `.app.tar.gz` 清单资产的解包安装流程；当前公开 Release 提供 Windows 包。
- **Linux**：当前客户端尚未实现自动安装更新。

客户端通过清单选择当前平台的资产；没有相应平台文件时会提示安装包未提供。发布前请验证真实下载地址与安装结果。
