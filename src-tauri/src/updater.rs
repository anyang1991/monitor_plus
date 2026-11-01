use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tauri::Emitter;

/// 更新清单默认走 GitHub Releases 的 latest 资产模式（公开仓库无需令牌）：
/// 把 anyang.1991 换成实际用户名，或在应用设置的「更新通道」里
/// 填入自定义清单地址（自建 GitLab 等私有通道）与访问令牌。
pub const DEFAULT_MANIFEST_URL: &str =
    "https://github.com/anyang.1991/monitor_plus/releases/latest/download/latest.json";

#[derive(Clone, Serialize, Debug)]
pub struct UpdateCheck {
    pub current_version: String,
    pub available: bool,
    pub latest_version: Option<String>,
    pub notes: Option<String>,
    pub asset_url: Option<String>,
    pub asset_size: Option<u64>,
    pub error: Option<String>,
}

impl UpdateCheck {
    fn err(current: &str, msg: impl Into<String>) -> Self {
        Self {
            current_version: current.into(),
            available: false,
            latest_version: None,
            notes: None,
            asset_url: None,
            asset_size: None,
            error: Some(msg.into()),
        }
    }
}

#[derive(Deserialize)]
struct Manifest {
    version: String,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    platforms: HashMap<String, PlatformAsset>,
}

#[derive(Deserialize)]
struct PlatformAsset {
    url: String,
    #[serde(default)]
    size: Option<u64>,
}

fn platform_key() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows-x86_64",
        "macos" => match std::env::consts::ARCH {
            "aarch64" => "darwin-aarch64",
            _ => "darwin-x86_64",
        },
        _ => "linux-x86_64",
    }
}

/// 数值型 semver 比较（按 '.' 分段补齐到 3 位），a > b 返回 true
pub fn version_gt(a: &str, b: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        let clean = v.trim().trim_start_matches('v').trim_start_matches('V');
        let mut p: Vec<u64> = clean
            .split('.')
            .map(|x| x.trim().parse().unwrap_or(0))
            .collect();
        while p.len() < 3 {
            p.push(0);
        }
        p
    }
    let (a, b) = (parts(a), parts(b));
    for i in 0..3 {
        if a[i] != b[i] {
            return a[i] > b[i];
        }
    }
    false
}

/// 认证：token 含 ':' 视为 Deploy Token（basic auth，用户名:令牌），
/// 否则视为 Project/Personal Access Token（PRIVATE-TOKEN 头）
fn authed(req: reqwest::RequestBuilder, token: Option<&str>) -> reqwest::RequestBuilder {
    match token.map(str::trim).filter(|s| !s.is_empty()) {
        Some(t) if t.contains(':') => {
            use base64::engine::general_purpose::STANDARD;
            use base64::Engine;
            let (user, pass) = t.split_once(':').unwrap_or(("oauth2", t));
            let cred = STANDARD.encode(format!("{user}:{pass}"));
            req.header("Authorization", format!("Basic {cred}"))
        }
        Some(t) => req.header("PRIVATE-TOKEN", t),
        None => req,
    }
}

/// GitLab 私有仓库未认证时会 302 到登录页，据此给出明确提示
fn is_redirect_to_login(resp: &reqwest::Response) -> bool {
    resp.url().as_str().contains("sign_in")
}

pub async fn check(
    client: &reqwest::Client,
    current: &str,
    manifest_url: Option<&str>,
    token: Option<&str>,
) -> UpdateCheck {
    let url = manifest_url
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_MANIFEST_URL);
    let resp = match authed(client.get(url), token)
        .timeout(Duration::from_secs(10))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return UpdateCheck::err(current, format!("检查更新失败: {e}")),
    };
    if is_redirect_to_login(&resp)
        || resp.status() == reqwest::StatusCode::UNAUTHORIZED
        || resp.status() == reqwest::StatusCode::FORBIDDEN
    {
        return UpdateCheck::err(
            current,
            "仓库是私有的：请在设置中填写 GitLab 访问令牌（Project Access Token，scope: read_repository）",
        );
    }
    let status = resp.status();
    if !status.is_success() {
        return UpdateCheck::err(current, format!("检查更新失败 (HTTP {status})"));
    }
    let m: Manifest = match resp.json().await {
        Ok(m) => m,
        Err(e) => return UpdateCheck::err(current, format!("更新清单解析失败: {e}")),
    };

    let asset = m.platforms.get(platform_key());
    let available = version_gt(&m.version, current);
    if available && asset.is_none() {
        return UpdateCheck {
            current_version: current.into(),
            available: false,
            latest_version: Some(m.version),
            notes: Some(format!(
                "上游已有新版本，但还没有 {} 的安装包；请先在对应平台运行发布脚本",
                platform_key()
            )),
            asset_url: None,
            asset_size: None,
            error: None,
        };
    }
    UpdateCheck {
        current_version: current.into(),
        available,
        latest_version: Some(m.version),
        notes: m.notes,
        asset_url: asset.map(|a| a.url.clone()),
        asset_size: asset.and_then(|a| a.size),
        error: None,
    }
}

fn spawn_detached(cmd: &mut std::process::Command) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn().map(|_| ()).map_err(|e| format!("启动安装器失败: {e}"))
}

/// 下载安装包到临时目录并落地安装，随后退出当前应用
/// 简单 percent-decode（%2F → / 等），用于从 API URL 段里还原文件名
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let hex = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 从下载 URL 推导临时文件名。
/// API raw 端点形如 .../files/updates%2Fxxx.exe/raw?ref=main：
/// 去掉 query、解码、跳过尾部的 "raw"，再过滤 Windows 非法字符。
fn temp_file_name(url: &str) -> String {
    let path_only = url.split('?').next().unwrap_or("");
    let decoded = percent_decode(path_only);
    const ILLEGAL: [char; 9] = ['<', '>', ':', '"', '|', '?', '*', '/', '\\'];
    let name = decoded
        .rsplit('/')
        .find(|s| !s.is_empty() && *s != "raw" && s.contains('.'))
        .unwrap_or("monitor-plus-update.bin")
        .chars()
        .map(|c| if ILLEGAL.contains(&c) { '_' } else { c })
        .collect::<String>();
    if name.is_empty() {
        "monitor-plus-update.bin".into()
    } else {
        name
    }
}

async fn download(
    client: &reqwest::Client,
    url: &str,
    token: Option<&str>,
    app: &tauri::AppHandle,
) -> Result<PathBuf, String> {
    let resp = authed(client.get(url), token)
        .timeout(Duration::from_secs(600))
        .send()
        .await
        .map_err(|e| format!("下载失败: {e}"))?;
    if is_redirect_to_login(&resp) {
        return Err("下载需要认证：请在设置中填写 GitLab 访问令牌".into());
    }
    if !resp.status().is_success() {
        return Err(format!("下载失败 (HTTP {})", resp.status()));
    }
    let total = resp.content_length();
    let tmp = std::env::temp_dir().join(temp_file_name(url));

    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::File::create(&tmp)
        .await
        .map_err(|e| format!("写入临时文件失败: {e}"))?;
    let mut stream = resp;
    let mut received: u64 = 0;
    let mut last_pct: i64 = -1;
    while let Some(chunk) = stream.chunk().await.map_err(|e| format!("下载中断: {e}"))? {
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("写入临时文件失败: {e}"))?;
        received += chunk.len() as u64;
        let pct = total.map(|t| (received * 100 / t) as i64).unwrap_or(-1);
        if pct != last_pct {
            last_pct = pct;
            let _ = app.emit(
                "update-progress",
                serde_json::json!({"received": received, "total": total, "percent": pct}),
            );
        }
    }
    file.flush().await.map_err(|e| e.to_string())?;
    drop(file);
    Ok(tmp)
}

pub async fn download_and_install(
    client: &reqwest::Client,
    url: &str,
    token: Option<&str>,
    app: &tauri::AppHandle,
) -> Result<(), String> {
    let tmp = download(client, url, token, app).await?;

    match std::env::consts::OS {
        "windows" => {
            // 写临时批处理再执行：cmd /C 直接传带嵌套引号的命令行会被 cmd 的
            // 引号解析搅乱（start 收到 \\ 导致"找不到文件"），批处理文件完全绕开
            let batch = tmp.with_file_name("monitor-plus-update.cmd");
            let script = format!(
                "@echo off\r\ntimeout /t 2 /nobreak >nul\r\nstart \"\" \"{}\" /S\r\ndel \"%~f0\"\r\n",
                tmp.display()
            );
            std::fs::write(&batch, script).map_err(|e| format!("写入安装脚本失败: {e}"))?;
            spawn_detached(std::process::Command::new("cmd").args(["/C", &batch.display().to_string()]))?;
            app.exit(0);
            Ok(())
        }
        "macos" => {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let bundle = exe
                .ancestors()
                .find(|p| p.extension().is_some_and(|e| e == "app"))
                .map(|p| p.to_path_buf());
            match bundle {
                Some(bundle) => {
                    let parent = bundle
                        .parent()
                        .ok_or("无法定位 .app 所在目录")?
                        .display()
                        .to_string();
                    let script = format!(
                        "sleep 1; tar -xzf '{}' -C '{}'; open '{}'",
                        tmp.display(),
                        parent,
                        bundle.display()
                    );
                    spawn_detached(std::process::Command::new("sh").arg("-c").arg(script))?;
                    app.exit(0);
                    Ok(())
                }
                None => Err(
                    "开发模式（tauri dev）无法自动覆盖安装：请在项目目录 git pull 后重启 tauri dev"
                        .to_string(),
                ),
            }
        }
        other => Err(format!("暂不支持 {other} 平台自动更新")),
    }
}

use std::path::PathBuf;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_file_names() {
        // API raw 端点：文件名藏在编码段里，query 要剥掉
        let url = "https://gitlab.example.com/api/v4/projects/team%2Fapp/repository/files/updates%2FMonitorPlus_0.2.1_x64-setup.exe/raw?ref=main";
        assert_eq!(temp_file_name(url), "MonitorPlus_0.2.1_x64-setup.exe");
        // Web raw 风格
        assert_eq!(
            temp_file_name("https://x/y/updates/setup.exe"),
            "setup.exe"
        );
        // 兜底
        assert_eq!(temp_file_name("https://x/raw?ref=main"), "monitor-plus-update.bin");
        assert_eq!(percent_decode("a%2Fb%20c"), "a/b c");
    }

    #[test]
    fn semver_compare() {
        assert!(version_gt("0.2.0", "0.1.0"));
        assert!(version_gt("1.0.0", "0.9.9"));
        assert!(version_gt("0.1.10", "0.1.9"));
        assert!(version_gt("v1.2.3", "1.2.2"));
        assert!(!version_gt("0.1.0", "0.1.0"));
        assert!(!version_gt("0.1.0", "0.2.0"));
    }

    #[test]
    fn parse_manifest() {
        let m: Manifest = serde_json::from_str(
            r#"{
                "version": "0.2.0",
                "notes": "修复若干问题",
                "pub_date": "2026-10-25T12:00:00Z",
                "platforms": {
                    "windows-x86_64": {"url": "https://x/setup.exe", "size": 123}
                }
            }"#,
        )
        .unwrap();
        assert_eq!(m.version, "0.2.0");
        assert_eq!(m.platforms.get("windows-x86_64").unwrap().url, "https://x/setup.exe");
        assert!(m.platforms.get("darwin-aarch64").is_none());
    }
}
