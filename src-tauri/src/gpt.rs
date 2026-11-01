use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const CODEX_AUTH_FILE: &str = ".codex/auth.json";

/// chatgpt.com 直连不可达，固定走本机代理端口（个人自用，写死）
pub const PROXY_URL: &str = "http://127.0.0.1:7890";
const PROXY_PORT: u16 = 7890;
const PROBE_TIMEOUT: Duration = Duration::from_millis(400);
/// 探测结果缓存时长，避免每次轮询都白连一次
const PROBE_CACHE: Duration = Duration::from_secs(60);

/// 探测本机代理端口是否在线（TCP 连一下），结果缓存 60 秒
fn proxy_available() -> bool {
    use std::net::{IpAddr, SocketAddr, TcpStream};
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<Option<(bool, std::time::Instant)>>> = OnceLock::new();
    let mut cached = CACHE.get_or_init(|| Mutex::new(None)).lock().unwrap();
    if let Some((ok, at)) = *cached {
        if at.elapsed() < PROBE_CACHE {
            return ok;
        }
    }
    let addr = SocketAddr::new(IpAddr::from([127, 0, 0, 1]), PROXY_PORT);
    let ok = TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).is_ok();
    *cached = Some((ok, std::time::Instant::now()));
    ok
}

/// GPT 每次轮询选客户端：代理在线走代理，不在线回退直连
pub fn pick_client<'a>(
    proxy: &'a reqwest::Client,
    direct: &'a reqwest::Client,
) -> &'a reqwest::Client {
    if proxy_available() {
        proxy
    } else {
        direct
    }
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct WindowUsage {
    pub used_percent: f64,
    /// 重置时刻（unix 毫秒）；无法解析时为 None
    pub reset_at_ms: Option<u64>,
    pub window_secs: u64,
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct GptSnapshot {
    pub ok: bool,
    pub error: Option<String>,
    /// 凭据是否已配置（未配置时前端隐藏整个区块）
    pub configured: bool,
    pub key_source: Option<String>,
    /// 套餐展示名（Pro/Plus/Free/Team…），获取失败为 None
    pub plan: Option<String>,
    /// accounts/check 的原始响应摘要（套餐缺失时用于诊断）
    pub plan_debug: Option<String>,
    pub five_hour: Option<WindowUsage>,
    pub weekly: Option<WindowUsage>,
    pub credits_balance: Option<String>,
    pub fetched_at_ms: u64,
}

/// 解析 Codex CLI 的 auth.json，返回 (access_token, account_id, auth_mode)
pub fn parse_auth_json(raw: &str) -> Result<(String, Option<String>, String), String> {
    let v: Value = serde_json::from_str(raw).map_err(|e| format!("auth.json 解析失败: {e}"))?;
    let mode = v
        .get("auth_mode")
        .and_then(Value::as_str)
        .unwrap_or("chatgpt")
        .to_string();
    if mode != "chatgpt" {
        return Err(format!(
            "auth_mode 是 {mode}（API key 模式没有订阅限额可查）"
        ));
    }
    let tokens = v.get("tokens").ok_or("auth.json 缺少 tokens 字段")?;
    let access = tokens
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or("auth.json 缺少 tokens.access_token")?;
    let account_id = tokens
        .get("account_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Ok((access.to_string(), account_id, mode))
}

/// 手动粘贴的 token 优先；否则读本机 Codex CLI 凭据 ~/.codex/auth.json
pub fn resolve_token(manual: Option<&str>) -> Result<(String, Option<String>), String> {
    if let Some(t) = manual.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok((t.to_string(), None));
    }
    let home = dirs::home_dir().ok_or("无法定位用户主目录")?;
    let path = home.join(CODEX_AUTH_FILE);
    let raw = std::fs::read_to_string(&path)
        .map_err(|_| format!("本机无 {} 且未配置 token", path.display()))?;
    let (token, account_id, _) = parse_auth_json(&raw)?;
    Ok((token, account_id))
}

/// reset_at 兼容三种格式：unix 秒、unix 毫秒、ISO 字符串
fn parse_reset_at(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::Number(n) => {
            let x = n.as_f64()?;
            if x <= 0.0 {
                return None;
            }
            // < 10^12 视为秒，否则毫秒
            Some(if x < 1e12 { (x * 1000.0) as u64 } else { x as u64 })
        }
        Value::String(s) => parse_iso_ms(s),
        _ => None,
    }
}

/// 手写 ISO8601/UTC 解析，避免引入 chrono：
/// 支持 "2026-01-02T03:04:05Z" 与 "2026-01-02T03:04:05.678Z"（毫秒可选，时区仅 Z）
fn parse_iso_ms(s: &str) -> Option<u64> {
    let s = s.trim();
    let rest = s.strip_suffix('Z').or_else(|| s.strip_suffix('z'))?;
    let (date, time) = rest.split_once('T')?;
    let mut d = date.split('-');
    let (y, mo, da) = (
        d.next()?.parse::<i64>().ok()?,
        d.next()?.parse::<i64>().ok()?,
        d.next()?.parse::<i64>().ok()?,
    );
    let mut t = time.split(':');
    let (h, mi) = (t.next()?.parse::<i64>().ok()?, t.next()?.parse::<i64>().ok()?);
    let sec_part = t.next().unwrap_or("0");
    let (sec, ms) = match sec_part.split_once('.') {
        Some((a, frac)) => {
            let frac = format!("{:0<3}", &frac[..frac.len().min(3)]);
            (a.parse::<i64>().ok()?, frac.parse::<i64>().ok()?)
        }
        None => (sec_part.parse::<i64>().ok()?, 0),
    };
    // 天数累计（proleptic 公历），足够用于重置时间换算
    let days = days_from_civil(y, mo, da);
    Some(((days * 86400 + h * 3600 + mi * 60 + sec) as u64) * 1000 + ms as u64)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn window_from(v: &Value) -> Option<WindowUsage> {
    let used = v.get("used_percent").and_then(Value::as_f64)?;
    let secs = v
        .get("limit_window_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some(WindowUsage {
        used_percent: used,
        reset_at_ms: parse_reset_at(v.get("reset_at")),
        window_secs: secs,
    })
}

/// plan_type 转短展示名。wham/usage 返回无前缀小写（"plus"），
/// accounts/check 类接口返回 "chatgpt_plus" 形式，两者都兼容
fn plan_label(plan_type: &str) -> String {
    let p = plan_type.trim().to_ascii_lowercase();
    let p = p
        .trim_start_matches("chatgpt_")
        .trim_start_matches("codex_")
        .to_string();
    match p.as_str() {
        "pro" => "Pro".into(),
        "plus" => "Plus".into(),
        "free" => "Free".into(),
        "team" => "Team".into(),
        "enterprise" => "Ent".into(),
        "business" => "Business".into(),
        "edu" => "Edu".into(),
        other => other.to_string(),
    }
}

/// 拉取一次 Codex 订阅限额
pub async fn fetch_usage(
    client: &reqwest::Client,
    token: &str,
    account_id: Option<&str>,
) -> Result<GptSnapshot, String> {
    let mut req = client
        .get(USAGE_URL)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "codex-cli")
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(10));
    if let Some(a) = account_id.map(str::trim).filter(|s| !s.is_empty()) {
        req = req.header("ChatGPT-Account-Id", a);
    }
    let resp = req.send().await.map_err(|e| format!("请求失败: {e}"))?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| format!("响应解析失败: {e}"))?;
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(
            "token 已过期或无效：在登录过 Codex CLI 的机器上运行一次 codex 即会自动刷新，然后重新检测"
                .into(),
        );
    }
    if !status.is_success() {
        let msg = body
            .get("detail")
            .and_then(Value::as_str)
            .unwrap_or("未知错误");
        return Err(format!("接口返回错误 (HTTP {status}): {msg}"));
    }

    let rl = body.get("rate_limit");
    let mut five_hour = None;
    let mut weekly = None;
    let mut other = None;
    for key in ["primary_window", "secondary_window"] {
        if let Some(w) = rl.and_then(|r| r.get(key)).and_then(window_from) {
            match w.window_secs {
                18000 => five_hour = Some(w),
                604800 => weekly = Some(w),
                _ => {
                    if five_hour.is_none() {
                        five_hour = Some(w)
                    } else if weekly.is_none() {
                        other = Some(w)
                    }
                }
            }
        }
    }
    if weekly.is_none() {
        weekly = other.or(weekly);
    }
    let credits_balance = body
        .get("credits")
        .filter(|c| {
            c.get("has_credits").and_then(Value::as_bool).unwrap_or(false)
                && !c.get("unlimited").and_then(Value::as_bool).unwrap_or(false)
        })
        .and_then(|c| c.get("balance"))
        .map(|b| match b {
            Value::Number(n) => n.to_string(),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        });

    if five_hour.is_none() && weekly.is_none() {
        return Err("响应中没有 rate_limit 窗口数据".into());
    }
    // 套餐直接取 wham/usage 顶层 plan_type（accounts/check 会被 Cloudflare 拦，
    // 而 usage 端点已验证可正常通过）；取不到时 plan_debug 带原始响应供诊断
    let plan = body
        .get("plan_type")
        .and_then(Value::as_str)
        .map(plan_label);
    let plan_debug = if plan.is_none() {
        let raw = serde_json::to_string(&body).unwrap_or_default();
        Some(format!(
            "wham/usage 响应里没有 plan_type | {}",
            raw.chars().take(600).collect::<String>()
        ))
    } else {
        None
    };
    Ok(GptSnapshot {
        ok: true,
        error: None,
        configured: true,
        key_source: None,
        plan,
        plan_debug,
        five_hour,
        weekly,
        credits_balance,
        fetched_at_ms: now_ms(),
    })
}

/// 每次轮询前重新读取本机 Codex 凭据（CODEX_HOME / ~/.codex）。
/// Codex CLI 运行时会自动轮换 auth.json 里的 token，读文件才能跟上刷新。
pub fn read_auto_credentials() -> Result<(String, Option<String>), String> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(home) = std::env::var("CODEX_HOME") {
        candidates.push(PathBuf::from(home).join("auth.json"));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(CODEX_AUTH_FILE));
    }
    for path in candidates {
        if let Ok(raw) = std::fs::read_to_string(&path) {
            let (token, account, _) = parse_auth_json(&raw)?;
            return Ok((token, account));
        }
    }
    Err("本机未找到 Codex 凭据".into())
}

/// 解析 token + 拉取，任何失败都产出带 error 的快照。
/// token 来源优先级：本机 auth.json（每轮重读，跟随 Codex 自动轮换）→ 配置里缓存的 token → 未配置。
pub async fn poll_once(
    client: &reqwest::Client,
    manual_token: Option<&str>,
    manual_account: Option<&str>,
) -> GptSnapshot {
    let (token, account, source) = match read_auto_credentials() {
        Ok((t, a)) => (t, a, "codex"),
        Err(_) => match manual_token.map(str::trim).filter(|s| !s.is_empty()) {
            Some(t) => (t.to_string(), manual_account.map(str::to_string), "manual"),
            None => {
                return GptSnapshot {
                    configured: false,
                    error: None,
                    fetched_at_ms: now_ms(),
                    ..Default::default()
                }
            }
        },
    };
    match fetch_usage(client, &token, account.as_deref()).await {
        Ok(mut snap) => {
            snap.key_source = Some(source.into());
            snap
        }
        Err(e) => GptSnapshot {
            ok: false,
            configured: true,
            error: Some(e),
            key_source: Some(source.into()),
            fetched_at_ms: now_ms(),
            ..Default::default()
        },
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classify_windows_by_seconds() {
        let body = json!({
            "rate_limit": {
                "primary_window": {"used_percent": 42.0, "limit_window_seconds": 18000, "reset_at": 1793000000i64},
                "secondary_window": {"used_percent": 88.5, "limit_window_seconds": 604800, "reset_at": "2026-10-29T03:37:44Z"}
            },
            "credits": {"has_credits": true, "unlimited": false, "balance": 12.5}
        });
        let snap = futures_block(body.clone());
        assert!(snap.ok, "{:?}", snap.error);
        let f = snap.five_hour.unwrap();
        assert_eq!(f.used_percent, 42.0);
        assert_eq!(f.reset_at_ms, Some(1793000000000));
        let w = snap.weekly.unwrap();
        assert_eq!(w.used_percent, 88.5);
        assert!(w.reset_at_ms.is_some());
        assert_eq!(snap.credits_balance.as_deref(), Some("12.5"));
    }

    // 测试辅助：直接走 fetch 的解析逻辑（不发请求），把 body 塞进同一套解析
    fn futures_block(body: Value) -> GptSnapshot {
        let rl = body.get("rate_limit");
        let mut five_hour = None;
        let mut weekly = None;
        for key in ["primary_window", "secondary_window"] {
            if let Some(w) = rl.and_then(|r| r.get(key)).and_then(window_from) {
                match w.window_secs {
                    18000 => five_hour = Some(w),
                    604800 => weekly = Some(w),
                    _ => {}
                }
            }
        }
        let credits_balance = body
            .get("credits")
            .and_then(|c| c.get("balance"))
            .map(|b| b.to_string().trim_matches('"').to_string());
        GptSnapshot {
            ok: five_hour.is_some() || weekly.is_some(),
            five_hour,
            weekly,
            credits_balance,
            configured: true,
            ..Default::default()
        }
    }

    #[test]
    fn plan_from_usage_body() {
        // wham/usage 顶层直接带 plan_type（accounts/check 被 Cloudflare 拦，不可用）
        let body: Value = serde_json::from_str(
            r#"{"account_id":"a","email":"x@y.z","plan_type":"plus",
                "rate_limit":{"primary_window":{"used_percent":34,"limit_window_seconds":18000},"secondary_window":null}}"#,
        ).unwrap();
        let plan = body.get("plan_type").and_then(Value::as_str).map(plan_label);
        assert_eq!(plan.as_deref(), Some("Plus"));
        // 没有 plan_type 时视为缺失
        let body2: Value = serde_json::from_str(r#"{"rate_limit":{}}"#).unwrap();
        assert!(body2.get("plan_type").is_none());
    }

    #[test]
    fn plan_label_mapping() {
        assert_eq!(plan_label("chatgpt_pro"), "Pro");
        assert_eq!(plan_label("chatgpt_plus"), "Plus");
        assert_eq!(plan_label("chatgpt_free"), "Free");
        assert_eq!(plan_label("chatgpt_team"), "Team");
        assert_eq!(plan_label("chatgpt_unknown_thing"), "unknown_thing");
    }

    #[test]
    fn plan_from_accounts_check() {
        // 默认账号
        let v: Value = serde_json::from_str(
            r#"{"accounts":{"default":{"account":{"plan_type":"chatgpt_pro"}}}}"#,
        ).unwrap();
        let pt = v.get("accounts").and_then(|a| a.get("default"))
            .and_then(|d| d.get("account")).and_then(|a| a.get("plan_type"))
            .and_then(Value::as_str).unwrap();
        assert_eq!(plan_label(pt), "Pro");
        // 无 default 时取第一个账号
        let v2: Value = serde_json::from_str(
            r#"{"accounts":{"acc-1":{"account":{"plan_type":"chatgpt_plus"}}}}"#,
        ).unwrap();
        let accs = v2.get("accounts").unwrap().as_object().unwrap();
        let pt2 = accs.values().find_map(|a| a.get("account").unwrap().get("plan_type").unwrap().as_str()).unwrap();
        assert_eq!(plan_label(pt2), "Plus");
    }

    #[test]
    fn reset_at_formats() {
        assert_eq!(parse_reset_at(Some(&json!(1793245064u64))), Some(1793245064000)); // 秒
        assert_eq!(parse_reset_at(Some(&json!(1793245064999u64))), Some(1793245064999)); // 毫秒
        assert_eq!(
            parse_reset_at(Some(&json!("2026-10-29T03:37:44.999Z"))),
            parse_reset_at(Some(&json!(1793245064999u64)))
        );
        assert_eq!(parse_reset_at(Some(&json!("2026-10-29T03:37:44Z"))), Some(1793245064000));
        assert_eq!(parse_reset_at(Some(&json!(null))), None);
        assert_eq!(parse_reset_at(None), None);
    }

    #[test]
    fn resolve_token_prefers_manual() {
        let (t, a) = resolve_token(Some("  sk-manual  ")).unwrap();
        assert_eq!(t, "sk-manual");
        assert_eq!(a, None);
    }

    #[test]
    fn parse_auth_json_variants() {
        let (t, a, m) = parse_auth_json(
            r#"{"auth_mode":"chatgpt","tokens":{"access_token":"abc","account_id":"acc-1","refresh_token":"r"}}"#,
        )
        .unwrap();
        assert_eq!(t, "abc");
        assert_eq!(a.as_deref(), Some("acc-1"));
        assert_eq!(m, "chatgpt");

        // 缺省 auth_mode 视为 chatgpt
        let (t, _, m) = parse_auth_json(r#"{"tokens":{"access_token":"xyz"}}"#).unwrap();
        assert_eq!(t, "xyz");
        assert_eq!(m, "chatgpt");

        // API key 模式拒绝
        let e = parse_auth_json(r#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-x"}"#).unwrap_err();
        assert!(e.contains("apikey"));

        // 缺 token 报错
        assert!(parse_auth_json(r#"{"tokens":{}}"#).is_err());
    }
}
