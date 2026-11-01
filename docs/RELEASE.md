# 发布与一键更新

Monitor Plus 采用「清单 + 安装包进仓库」的自更新方案：安装包和版本清单（`updates/latest.json`）提交到 GitLab 仓库，已安装的旧版本在小窗设置里点「检查更新 → 下载并安装」即可完成升级（自动退出 → 静默安装 → 手动重新打开）。

## 发布新版本

**Windows（主平台）**：

```powershell
powershell -File scripts\release.ps1 -Version 0.2.0 -Notes "修复xx，新增xx"
```

**macOS**：

```bash
./scripts/release.sh 0.2.0 "修复xx，新增xx"
```

脚本自动完成：三处版本号同步（tauri.conf.json / Cargo.toml / package.json）→ `pnpm tauri build` → 安装包复制到 `updates/` → 生成/合并 `updates/latest.json` → git 提交推送。

## 更新按钮如何工作

1. 应用 GET `https://github.com/anyang1991/monitor_plus/-/raw/main/updates/latest.json`
2. 比对 `version` 与当前版本（数值比较，高才提示）
3. 用户点「下载并安装」→ 下载安装包（设置里有进度）→ 应用退出：
   - **Windows**：2 秒后静默运行 NSIS 安装器（`/S`，安装到用户目录，无需管理员）
   - **macOS**：解包 `.app.tar.gz` 覆盖现有 .app 并重新打开
   - 开发模式（`tauri dev` 运行）不支持覆盖安装，会提示用 `git pull` 更新

## 私有仓库的令牌

仓库是私有的，清单与安装包走 **GitLab API raw 端点**（`/api/v4/projects/.../repository/files/.../raw`）。更新令牌已**内置在程序里**（`src-tauri/src/updater.rs` 的 `UPDATE_TOKEN` 常量，Project Access Token / `read_repository` / 2027-10-25 到期），无需在设置里填写；令牌轮换或过期时改常量后重新发版即可。

注意：Web 的 `/-/raw/...` 路由和 Deploy Token 都**不适用**（前者不认 PRIVATE-TOKEN，后者不支持 REST API）。

## 注意

- 清单里没有当前平台的安装包时，检查更新会提示「尚未提供当前平台的安装包」，不会误装
- 版本比较是数值型 semver（x.y.z），请保持 `x.y.z` 格式
- 安装包直接提交进 git 仓库（每个版本 5–10MB），介意体积可定期清理 `updates/` 下的旧文件
