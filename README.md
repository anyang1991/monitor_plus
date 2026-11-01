# Monitor_Plus

桌面悬浮监控小窗：同时盯住 **GLM Coding Plan 限额** 和 **多台服务器的 CPU/GPU 占用**。

![tech](https://img.shields.io/badge/Tauri-2-24C8DB) ![tech](https://img.shields.io/badge/Vue-3-42B883)

## 功能

- **GLM Coding Plan 限额**：5 小时限额 + 每周限额双进度条，70%/90% 变色预警，显示重置倒计时
  - API Key 自动读取本机 ZCode 登录凭据（无需手动配置），也可在设置中手动填写
  - 支持 bigmodel.cn（中国）与 api.z.ai（全球）端点
- **GPT Codex 订阅限额**：ChatGPT 订阅的 5 小时 / 每周限额进度条与重置倒计时
  - 设置里点「🔍 自动检测 Codex 凭据」：自动扫描本机 `CODEX_HOME` / `~/.codex/auth.json`，读出 token 并写入配置、自动测试，无需手动粘贴
  - 未检测到时会给出针对性提示（Codex 未安装 / 已安装未登录 / API key 模式无订阅额度）；token 过期后运行一次 `codex` 再点检测即可
- **服务器 CPU/GPU 监控**：每台服务器一张卡片——CPU 占用条、内存占用、每块 GPU 的算力利用率/显存/温度
  - 服务器端零依赖部署（一个 Python 脚本 + systemd），详见 [docs/AGENT_DEPLOY.md](docs/AGENT_DEPLOY.md)
  - 支持可选访问令牌鉴权；轮询间隔可调（2–10 秒）
- **悬浮小窗形态**：置顶、无边框、可拖动、位置记忆；可折叠成单行迷你条；窗口透明度可调
- **系统托盘**：显示/隐藏、退出；关闭按钮 = 隐藏到托盘；开机自启可选
- **一键更新**：上游（GitLab 仓库）发布新版本后，设置里点「检查更新 → 下载并安装」即完成升级；发布流程见 [docs/RELEASE.md](docs/RELEASE.md)

## 界面结构

```
┌─────────────────────────────┐
│ Monitor+            ⌄ ⚙ ✕  │   ← 拖动条 / 折叠 / 设置 / 隐藏
├─────────────────────────────┤
│ GLM Coding Plan      MAX    │
│ 5 小时限额        ▓▓░░  6%  │
│ 3时12分后重置                │
│ 每周限额          ▓▓▓░ 34%  │
├─────────────────────────────┤
│ ● gpu-server-01      12ms   │
│ CPU   ▓▓▓░░░░░░  32%        │
│ 内存  ▓▓░░░░░░░  10/16G     │
│ │ RTX 4090           67°C   │
│ │ 算力 ▓▓▓▓▓░░░  58%        │
│ │ 显存 ▓▓▓░░░░░  8.2/24G    │
└─────────────────────────────┘
```

## 更新通道配置

应用内置的默认更新通道是 GitHub Releases 模式（把下面地址里的 `anyang1991` 换成实际用户名后即可）：

```
https://github.com/anyang1991/monitor_plus/releases/latest/download/latest.json
```

自建 GitLab 等私有通道、或想改用其他清单地址时，在应用「设置 → 应用更新」里填写**清单地址**与**访问令牌**；也可以把下面这段提示词直接交给你的 AI 编程助手（ZCode / Codex / Claude Code 等）代为配置：

```text
请帮我配置 Monitor Plus 桌面应用的自动更新通道。

第 0 步：先彻底退出 Monitor Plus（系统托盘右键图标 → 退出），避免退出时用旧配置覆盖我们的修改。

应用的配置文件是 JSON，位置：
- Windows: %APPDATA%\com.monitorplus.app\config.json
- macOS: ~/Library/Application Support/com.monitorplus.app/config.json
- Linux: ~/.config/com.monitorplus.app/config.json

请读取该文件（若不存在则创建一个空 JSON 对象再填），修改以下两个字段，其余字段原样保留：
1. "update_manifest_url" 设置为：<更新清单 latest.json 的完整 URL>
2. "update_token" 设置为：<访问令牌；公开仓库没有令牌就删除这个键>

写回文件（UTF-8、2 空格缩进、chmod 600），然后告诉我写入了什么。
最后提醒我：重新打开 Monitor Plus → 设置 → 应用更新 → 检查更新 验证通道是否生效。
```

字段说明：

| 字段 | 说明 |
|---|---|
| `update_manifest_url` | 指向 `latest.json` 的完整地址；GitHub Releases 用户留空即用内置默认 |
| `update_token` | 私有仓库的访问令牌（GitLab Project Access Token，`read_repository`）；公开仓库不需要 |

修改后重新打开应用 → 设置 → 应用更新 → 检查更新，即可验证通道。

## 项目结构

```
├── src/                    # Vue 3 前端（悬浮窗 UI）
│   ├── App.vue             # 主窗口：标题栏/迷你条/路由设置面板
│   ├── components/
│   │   ├── GlmPanel.vue        # GLM 5h/每周限额进度条
│   │   ├── ServerCard.vue      # 服务器 CPU/内存/GPU 卡片
│   │   └── SettingsPanel.vue   # 设置：服务器增删改、API Key、间隔、透明度
│   └── types.ts            # 前后端共享的数据结构（camelCase 对齐）
├── src-tauri/              # Rust 后端
│   └── src/
│       ├── lib.rs          # 入口：托盘、窗口事件、轮询循环、Tauri 命令
│       ├── config.rs       # 配置持久化（app_config_dir/config.json）
│       ├── glm.rs          # ZCode 凭据解密（AES-256-GCM）+ 配额接口轮询
│       └── servers.rs      # 服务器 Agent 指标拉取
├── agent/
│   ├── agent.py            # 服务器采集 Agent（零依赖 Python3）
│   └── monitor-agent.service   # systemd 服务文件
└── docs/
    ├── WINDOWS_BUILD.md    # Windows 上构建安装包指南
    └── AGENT_DEPLOY.md     # 服务器 Agent 部署指南
```

## 快速开始

**Windows 构建**（主目标平台）：见 [docs/WINDOWS_BUILD.md](docs/WINDOWS_BUILD.md)。

**macOS / Linux 开发运行**：

```bash
pnpm install
pnpm tauri dev
```

> 首次运行会自动读取本机 ZCode 凭据查询 GLM 限额（需已登录 ZCode CLI）。

## 服务器侧快速接入

在每台要监控的 Linux 服务器上：

```bash
scp agent/agent.py user@server:/opt/monitor-agent/
scp agent/monitor-agent.service user@server:/etc/systemd/system/
ssh user@server "systemctl enable --now monitor-agent"
```

然后在小窗设置里添加服务器地址 `http://<服务器IP>:9101`。详细步骤与安全建议见 [docs/AGENT_DEPLOY.md](docs/AGENT_DEPLOY.md)。

## 数据来源说明

GLM 配额接口：`GET {base}/api/monitor/usage/quota/limit`，认证头为 `Authorization: <api-key>`（无 Bearer 前缀）。ZCode 凭据文件（`~/.zcode/v2/credentials.json`）中的 key 为 `enc:v1:` 加密格式，本应用按 ZCode CLI 相同的密钥派生逻辑（`zcode-credential-fallback:{platform}:{homedir}:{username}` → SHA-256 → AES-256-GCM）本地解密，凭据不出本机。
