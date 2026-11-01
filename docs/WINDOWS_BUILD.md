# Windows 构建指南

Monitor Plus 主目标平台是 Windows。在一台 Windows 10/11 机器上完成一次性环境安装后，构建只需三条命令。

## 1. 安装环境（仅首次）

### 1.1 Microsoft C++ 构建工具（MSVC）

Tauri 在 Windows 上依赖 MSVC 编译器与链接器：

1. 下载 [Visual Studio 2022 Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
2. 安装时勾选 **"使用 C++ 的桌面开发"** 工作负载（含 MSVC v143 + Windows 11 SDK）

### 1.2 WebView2 运行时

Windows 11 与较新的 Windows 10 已内置。若没有，从 [Microsoft 官网](https://developer.microsoft.com/microsoft-edge/webview2/) 安装 Evergreen Runtime。

### 1.3 Rust（rustup，MSVC 目标）

PowerShell 执行：

```powershell
winget install Rustlang.Rustup
# 或下载 https://win.rustup.rs 运行
rustup default stable-msvc
```

### 1.4 Node.js 与 pnpm

```powershell
winget install OpenJS.NodeJS.LTS
npm install -g pnpm
```

## 2. 构建

```powershell
git clone https://github.com/anyang1991/monitor_plus.git
cd monitor_plus
pnpm install
pnpm tauri build
```

产物在 `src-tauri\target\release\bundle\` 下：

- `msi\Monitor Plus_0.1.0_x64_zh-CN.msi` — 安装包（推荐）
- `nsis\Monitor Plus_0.1.0_x64-setup.exe` — NSIS 安装器

直接运行绿色版可执行文件：`src-tauri\target\release\Monitor Plus.exe`。

## 3. 开发调试

```powershell
pnpm tauri dev
```

## 常见问题

| 问题 | 处理 |
|---|---|
| `link.exe not found` | 未装 VS Build Tools 或没勾 C++ 工作负载，重装 1.1 步骤 |
| `error: Microsoft Visual C++ 14.0 or greater is required` | 同上 |
| 下载 crate 超时 | 配置 rsproxy：见下方 |
| pnpm 安装慢 | `npm config set registry https://registry.npmmirror.com` |

### Rust 镜像（国内网络）

`%USERPROFILE%\.cargo\config.toml`：

```toml
[source.crates-io]
replace-with = 'rsproxy-sparse'
[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"
[registries.rsproxy]
index = "sparse+https://rsproxy.cn/index/"
[net]
git-fetch-with-cli = true
```

## 首次运行配置

1. 启动 Monitor Plus，悬浮窗出现（置顶，可拖动）
2. 点标题栏 ⚙ 进入设置：
   - **GLM**：默认自动读取本机 ZCode 凭据（需先在终端登录过 `zcode`）；也可手动粘贴 API Key；国内选 `bigmodel.cn`，走全球版选 `api.z.ai`
   - **服务器**：点"＋ 添加"，填名称与地址（如 `192.168.1.10:9101`），点"测试"验证
3. 点 ⌄ 可折叠成单行迷你条，双击迷你条展开
4. 点 ✕ 隐藏到托盘（不是退出），托盘右键可退出
