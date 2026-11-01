use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::AppHandle;
use tauri::Manager;

#[derive(Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub token: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GlmConfig {
    #[serde(default = "default_glm_base")]
    pub base_url: String,
    /// 手动配置的 API key；None 时自动从本机 ZCode 凭据读取
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct GptConfig {
    /// 手动粘贴的 ChatGPT access_token；None 时自动读本机 ~/.codex/auth.json
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub account_id: Option<String>,
}

fn default_glm_base() -> String {
    "https://open.bigmodel.cn".into()
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub servers: Vec<ServerConfig>,
    #[serde(default)]
    pub glm: GlmConfig,
    #[serde(default)]
    pub gpt: GptConfig,
    /// 更新清单地址（覆盖默认 GitHub 模式，自建 GitLab 等私有通道用）
    #[serde(default)]
    pub update_manifest_url: Option<String>,
    /// 更新通道访问令牌（私有通道需要；公开仓库留空）
    #[serde(default)]
    pub update_token: Option<String>,
    #[serde(default = "default_glm_refresh")]
    pub refresh_glm_secs: u64,
    #[serde(default = "default_server_refresh")]
    pub refresh_server_secs: u64,
    #[serde(default = "default_opacity")]
    pub opacity: f64,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub window_pos: Option<(i32, i32)>,
}

fn default_glm_refresh() -> u64 {
    60
}

fn default_server_refresh() -> u64 {
    3
}

fn default_opacity() -> f64 {
    1.0
}

impl Default for GlmConfig {
    fn default() -> Self {
        Self {
            base_url: default_glm_base(),
            api_key: None,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            servers: Vec::new(),
            glm: GlmConfig::default(),
            gpt: GptConfig::default(),
            update_manifest_url: None,
            update_token: None,
            refresh_glm_secs: default_glm_refresh(),
            refresh_server_secs: default_server_refresh(),
            opacity: default_opacity(),
            collapsed: false,
            window_pos: None,
        }
    }
}

impl AppConfig {
    pub fn sanitized(mut self) -> Self {
        self.refresh_glm_secs = self.refresh_glm_secs.clamp(30, 3600);
        self.refresh_server_secs = self.refresh_server_secs.clamp(1, 60);
        self.opacity = if self.opacity.is_finite() {
            self.opacity.clamp(0.4, 1.0)
        } else {
            1.0
        };
        self.glm.base_url = self.glm.base_url.trim().trim_end_matches('/').to_string();
        if self.glm.base_url.is_empty() {
            self.glm.base_url = default_glm_base();
        }
        self.glm.api_key = self
            .glm
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        self.gpt.access_token = self
            .gpt
            .access_token
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        self.gpt.account_id = self
            .gpt
            .account_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        self.update_manifest_url = self
            .update_manifest_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        self.update_token = self
            .update_token
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        self.servers = self
            .servers
            .into_iter()
            .filter(|s| !s.url.trim().is_empty())
            .map(|mut s| {
                s.name = s.name.trim().to_string();
                s.url = s.url.trim().to_string();
                s.token = s
                    .token
                    .as_deref()
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(str::to_string);
                s
            })
            .collect();
        self
    }
}

pub fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("无法确定配置目录: {e}"))?;
    Ok(dir.join("config.json"))
}

pub fn load(app: &AppHandle) -> AppConfig {
    let Ok(path) = config_path(app) else {
        return AppConfig::default();
    };
    match fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str::<AppConfig>(&raw) {
            Ok(cfg) => cfg.sanitized(),
            Err(e) => {
                // 解析失败（多半是写入中断导致的半截文件）：坏文件留档而非覆盖，
                // 避免下一次 flush 把用户配置彻底抹掉
                eprintln!("[monitor-plus] config.json 解析失败: {e}，已留档为 config.json.bad");
                let _ = fs::rename(&path, path.with_extension("json.bad"));
                AppConfig::default()
            }
        },
        Err(_) => AppConfig::default(),
    }
}

pub fn save(app: &AppHandle, config: &AppConfig) -> Result<(), String> {
    let path = config_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let raw = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    // 原子写入：先写临时文件再 rename，应用被杀也不会留下半截 config.json
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, raw).map_err(|e| format!("写入配置失败: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("替换配置失败: {e}"))
}

/// 全局共享状态：配置 + 窗口位置缓存 + 轮询唤醒 + 共享 HTTP 客户端
pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub pos: Mutex<Option<(i32, i32)>>,
    pub reload: tokio::sync::Notify,
    /// GLM 请求用：继承环境代理（全球端点可能需要）
    pub client: reqwest::Client,
    /// 服务器 Agent 请求用：强制直连（LAN 地址不能走代理）
    pub direct_client: reqwest::Client,
    /// GPT 请求用：强制走本机 7890（chatgpt.com 直连不可达）
    pub gpt_proxy_client: reqwest::Client,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        let pos = config.window_pos;
        let direct_client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap_or_default();
        let gpt_proxy_client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(crate::gpt::PROXY_URL).expect("内置 GPT 代理地址非法"))
            .build()
            .unwrap_or_default();
        Self {
            config: Mutex::new(config),
            pos: Mutex::new(pos),
            reload: tokio::sync::Notify::new(),
            client: reqwest::Client::new(),
            direct_client,
            gpt_proxy_client,
        }
    }

    pub fn current_config(&self) -> AppConfig {
        self.config.lock().unwrap().clone()
    }

    pub fn replace_config(&self, config: AppConfig) {
        *self.config.lock().unwrap() = config;
    }

    /// 合并窗口位置后落盘；失败仅记日志（供诊断），不打断调用方
    pub fn flush(&self, app: &AppHandle) {
        let mut cfg = self.current_config();
        cfg.window_pos = *self.pos.lock().unwrap();
        if let Err(e) = save(app, &cfg) {
            eprintln!("[monitor-plus] 配置落盘失败: {e}");
        }
    }
}
