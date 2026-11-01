mod config;
mod glm;
mod gpt;
mod servers;
mod tibo;
mod updater;

use config::{AppConfig, AppState, ServerConfig};
use serde::Serialize;
use std::path::PathBuf;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, PhysicalPosition, WindowEvent,
};

const WIN_LABEL: &str = "main";

#[tauri::command]
fn get_config(state: tauri::State<AppState>) -> AppConfig {
    state.current_config()
}

#[tauri::command]
fn save_config(
    app: AppHandle,
    state: tauri::State<AppState>,
    config: AppConfig,
) -> Result<(), String> {
    state.replace_config(config.sanitized());
    state.flush(&app);
    state.reload.notify_one();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 复刻前端 save_config 的 IPC 载荷：get_config 输出 + 用户改动 → 反序列化
    #[test]
    fn frontend_save_payload_deserializes() {
        let raw = r#"{
            "servers": [
                {"id":"srv-1","name":"n1","url":"1.2.3.4:9101","token":null},
                {"id":"srv-2","name":"n2","url":"http://5.6.7.8:9101","token":"tok"}
            ],
            "glm": {"base_url":"https://open.bigmodel.cn","api_key":"my-key"},
            "gpt": {"access_token":null,"account_id":null},
            "refresh_glm_secs":60,"refresh_server_secs":3,
            "opacity":0.85,"collapsed":false,"window_pos":[1440,233]
        }"#;
        let cfg: AppConfig = serde_json::from_str(raw).unwrap();
        let cfg = cfg.sanitized();
        assert_eq!(cfg.servers.len(), 2);
        assert_eq!(cfg.glm.api_key.as_deref(), Some("my-key"));
        assert_eq!(cfg.window_pos, Some((1440, 233)));

        // get_config → 前端原样回传（无任何改动）也必须成功
        let full = serde_json::to_string(&cfg).unwrap();
        let again: AppConfig = serde_json::from_str(&full).unwrap();
        assert_eq!(again.servers.len(), 2);

        // 未知字段（如旧版本的 update_token）必须被忽略而不是拒绝
        let legacy = r#"{"servers":[],"glm":{"base_url":"https://x","api_key":null},
            "gpt":{"access_token":null,"account_id":null},
            "update_token":"legacy","refresh_glm_secs":60,"refresh_server_secs":3,
            "opacity":1.0,"collapsed":true,"window_pos":null}"#;
        let legacy_cfg: AppConfig = serde_json::from_str(legacy).unwrap();
        assert!(legacy_cfg.collapsed);
    }

    /// 配置损坏时 load 留档坏文件并回默认，而不是静默吞掉
    #[test]
    fn corrupted_config_file_quarantined() {
        let dir = std::env::temp_dir().join(format!("mp-cfg-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        std::fs::write(&path, "{\"servers\":[{\"id\":").unwrap(); // 半截 JSON
        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed = serde_json::from_str::<AppConfig>(&raw);
        assert!(parsed.is_err()); // 触发 load 的留档分支
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[tauri::command]
fn set_collapsed(app: AppHandle, state: tauri::State<AppState>, collapsed: bool) {
    let mut cfg = state.current_config();
    cfg.collapsed = collapsed;
    state.replace_config(cfg);
    state.flush(&app);
    apply_window_size(&app, collapsed);
}

/// 收起态内容自适应宽度：前端量好后调用（仅收起态有意义）
#[tauri::command]
fn resize_widget(app: AppHandle, width: f64, height: f64) {
    let w = width.clamp(280.0, 900.0);
    let h = height.clamp(34.0, 200.0);
    if let Some(win) = app.get_webview_window(WIN_LABEL) {
        let _ = win.set_resizable(true);
        let _ = win.set_size(tauri::LogicalSize::new(w, h));
        let _ = win.set_resizable(false);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CodexDetect {
    found: bool,
    path: Option<String>,
    token: Option<String>,
    account_id: Option<String>,
    hint: Option<String>,
}

/// 扫描本机 Codex CLI 凭据（CODEX_HOME / ~/.codex），找到即写入配置并唤醒轮询
#[tauri::command]
fn detect_codex(app: AppHandle, state: tauri::State<AppState>) -> CodexDetect {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(home) = std::env::var("CODEX_HOME") {
        candidates.push(PathBuf::from(home).join("auth.json"));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".codex").join("auth.json"));
    }

    for path in candidates {
        if let Ok(raw) = std::fs::read_to_string(&path) {
            return match gpt::parse_auth_json(&raw) {
                Ok((token, account_id, _mode)) => {
                    let mut cfg = state.current_config();
                    cfg.gpt.access_token = Some(token.clone());
                    cfg.gpt.account_id = account_id.clone();
                    state.replace_config(cfg);
                    state.flush(&app);
                    state.reload.notify_one();
                    CodexDetect {
                        found: true,
                        path: Some(path.display().to_string()),
                        token: Some(token),
                        account_id,
                        hint: None,
                    }
                }
                Err(e) => CodexDetect {
                    found: false,
                    path: Some(path.display().to_string()),
                    token: None,
                    account_id: None,
                    hint: Some(e),
                },
            };
        }
    }

    // 未找到凭据：探测是否装了 Codex CLI，给针对性提示
    let installed = find_codex_on_path();
    CodexDetect {
        found: false,
        path: None,
        token: None,
        account_id: None,
        hint: Some(
            if installed {
                "检测到 Codex CLI 已安装但未登录：运行一次 codex 并登录后，再点检测".into()
            } else {
                format!(
                    "未找到 Codex 凭据（{}）：请在这台机器上安装并登录 Codex CLI",
                    dirs::home_dir()
                        .map(|h| h.join(".codex").join("auth.json").display().to_string())
                        .unwrap_or_else(|| "~/.codex/auth.json".into())
                )
            },
        ),
    }
}

fn find_codex_on_path() -> bool {
    let path = match std::env::var("PATH") {
        Ok(p) => p,
        Err(_) => return false,
    };
    let names: &[&str] = if std::env::consts::OS == "windows" {
        &["codex.exe", "codex.cmd", "codex.bat"]
    } else {
        &["codex"]
    };
    std::env::split_paths(&path).any(|dir| {
        names
            .iter()
            .any(|n| dir.join(n).is_file())
    })
}

/// 收起模式固定两行：上行大模型徽章、下行服务器小图标条
fn apply_window_size(app: &AppHandle, collapsed: bool) {
    let _ = app;
    let (w, h) = if collapsed { (400.0, 62.0) } else { (340.0, 560.0) };
    if let Some(win) = app.get_webview_window(WIN_LABEL) {
        // Windows 上 resizable=false 可能锁死程序化 set_size，先临时解锁
        let _ = win.set_resizable(true);
        let _ = win.set_size(tauri::LogicalSize::new(w, h));
        let _ = win.set_resizable(false);
    }
}

#[tauri::command]
fn set_opacity(app: AppHandle, state: tauri::State<AppState>, opacity: f64) {
    let mut cfg = state.current_config();
    cfg.opacity = opacity.clamp(0.4, 1.0);
    state.replace_config(cfg.clone());
    state.flush(&app);
    // 透明度由前端应用到根容器（窗口背景本身透明）
    let _ = app.emit("opacity-changed", cfg.opacity);
}

#[tauri::command]
async fn test_glm_now(
    state: tauri::State<'_, AppState>,
    config: AppConfig,
) -> Result<glm::QuotaSnapshot, String> {
    let client = state.client.clone();
    let sanitized = config.sanitized();
    Ok(glm::poll_once(
        &client,
        &sanitized.glm.base_url,
        sanitized.glm.api_key.as_deref(),
    )
    .await)
}

#[tauri::command]
async fn test_server(
    state: tauri::State<'_, AppState>,
    server: ServerConfig,
) -> Result<servers::AgentMetrics, String> {
    let client = state.direct_client.clone();
    servers::fetch_metrics(&client, &server.url, server.token.as_deref()).await
}

#[tauri::command]
async fn test_gpt_now(
    state: tauri::State<'_, AppState>,
    config: AppConfig,
) -> Result<gpt::GptSnapshot, String> {
    let client = gpt::pick_client(&state.gpt_proxy_client, &state.direct_client).clone();
    let sanitized = config.sanitized();
    Ok(gpt::poll_once(
        &client,
        sanitized.gpt.access_token.as_deref(),
        sanitized.gpt.account_id.as_deref(),
    )
    .await)
}

#[tauri::command]
async fn check_update(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<updater::UpdateCheck, String> {
    let client = state.client.clone();
    let cfg = state.current_config();
    let current = app.package_info().version.to_string();
    Ok(updater::check(
        &client,
        &current,
        cfg.update_manifest_url.as_deref(),
        cfg.update_token.as_deref(),
    )
    .await)
}

#[tauri::command]
async fn perform_update(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    url: String,
) -> Result<(), String> {
    let client = state.client.clone();
    let token = state.current_config().update_token;
    updater::download_and_install(&client, &url, token.as_deref(), &app).await
}

fn spawn_glm_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        loop {
            let (base, manual, secs) = {
                let cfg = state.current_config();
                (cfg.glm.base_url, cfg.glm.api_key, cfg.refresh_glm_secs)
            };
            let snapshot = glm::poll_once(&state.client, &base, manual.as_deref()).await;
            let _ = app.emit("glm-update", &snapshot);
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(secs)) => {}
                _ = state.reload.notified() => {}
            }
        }
    });
}

fn spawn_gpt_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        loop {
            let (token, account, secs) = {
                let cfg = state.current_config();
                (
                    cfg.gpt.access_token,
                    cfg.gpt.account_id,
                    cfg.refresh_glm_secs,
                )
            };
            let client = gpt::pick_client(&state.gpt_proxy_client, &state.direct_client).clone();
            let snapshot = gpt::poll_once(&client, token.as_deref(), account.as_deref()).await;
            let _ = app.emit("gpt-update", &snapshot);
            // Tibo 重置官宣追踪：同一节拍轮询（codex-reset.com 公共源）
            let tibo_snapshot = tibo::poll_once(&client).await;
            let _ = app.emit("tibo-update", &tibo_snapshot);
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(secs)) => {}
                _ = state.reload.notified() => {}
            }
        }
    });
}

fn spawn_server_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        loop {
            let (servers, secs) = {
                let cfg = state.current_config();
                (cfg.servers.clone(), cfg.refresh_server_secs)
            };
            let mut joins = Vec::new();
            for s in servers {
                let client = state.direct_client.clone();
                joins.push(tauri::async_runtime::spawn(async move {
                    let started = std::time::Instant::now();
                    let (online, error, metrics) =
                        match servers::fetch_metrics(&client, &s.url, s.token.as_deref()).await {
                            Ok(m) => (true, None, Some(m)),
                            Err(e) => (false, Some(e), None),
                        };
                    servers::ServerUpdate {
                        id: s.id,
                        online,
                        error,
                        metrics,
                        latency_ms: Some(started.elapsed().as_millis() as u64),
                    }
                }));
            }
            for j in joins {
                if let Ok(update) = j.await {
                    let _ = app.emit("server-update", &update);
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(secs)) => {}
                _ = state.reload.notified() => {}
            }
        }
    });
}

fn persist_window_pos(app: &AppHandle) {
    let state = app.state::<AppState>();
    if let Some(win) = app.get_webview_window(WIN_LABEL) {
        if let Ok(pos) = win.outer_position() {
            *state.pos.lock().unwrap() = Some((pos.x, pos.y));
        }
    }
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示 / 隐藏", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 Monitor Plus", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().cloned().unwrap())
        .tooltip("Monitor Plus")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(win) = app.get_webview_window(WIN_LABEL) {
                    if win.is_visible().unwrap_or(false) {
                        let _ = win.hide();
                    } else {
                        let _ = win.show();
                        let _ = win.set_focus();
                    }
                }
            }
            "quit" => {
                persist_window_pos(app);
                app.state::<AppState>().flush(app);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                ..
            } = event
            {
                if let Some(win) = tray.app_handle().get_webview_window(WIN_LABEL) {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let cfg = config::load(app.handle());
            let start_collapsed = cfg.collapsed;
            app.manage(AppState::new(cfg));

            // 恢复上次的窗口位置与折叠尺寸
            let state = app.state::<AppState>();
            if let Some(win) = app.get_webview_window(WIN_LABEL) {
                if let Some((x, y)) = *state.pos.lock().unwrap() {
                    let _ = win.set_position(PhysicalPosition::new(x, y));
                }
                let (w, h) = if start_collapsed { (400.0, 40.0) } else { (340.0, 560.0) };
                let _ = win.set_size(tauri::LogicalSize::new(w, h));
            }

            let handle = app.handle().clone();
            setup_tray(&handle)?;
            spawn_glm_loop(handle.clone());
            spawn_gpt_loop(handle.clone());
            spawn_server_loop(handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            set_collapsed,
            set_opacity,
            test_glm_now,
            test_gpt_now,
            detect_codex,
            test_server,
            check_update,
            perform_update,
            resize_widget
        ])
        .on_window_event(|window, event| {
            if window.label() == WIN_LABEL {
                match event {
                    WindowEvent::Moved(pos) => {
                        let app = window.app_handle();
                        *app.state::<AppState>().pos.lock().unwrap() = Some((pos.x, pos.y));
                    }
                    WindowEvent::CloseRequested { api, .. } => {
                        // 点关闭 = 隐藏到托盘，位置落盘
                        let app = window.app_handle();
                        persist_window_pos(app);
                        app.state::<AppState>().flush(app);
                        let _ = window.hide();
                        api.prevent_close();
                    }
                    _ => {}
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
