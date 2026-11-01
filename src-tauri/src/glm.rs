use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::Duration;

const QUOTA_PATH: &str = "/api/monitor/usage/quota/limit";
const CRED_FILE: &str = ".zcode/v2/credentials.json";
const ENC_PREFIX: &str = "enc:v1:";

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct LimitEntry {
    pub percentage: f64,
    pub next_reset_ms: Option<u64>,
    pub usage: Option<f64>,
    pub current: Option<f64>,
    pub remaining: Option<f64>,
    #[serde(default)]
    pub details: BTreeMap<String, f64>,
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct QuotaSnapshot {
    pub ok: bool,
    pub error: Option<String>,
    pub level: Option<String>,
    pub five_hour: Option<LimitEntry>,
    pub weekly: Option<LimitEntry>,
    pub mcp: Option<LimitEntry>,
    pub key_source: Option<String>,
    pub fetched_at_ms: u64,
}

/// node os.platform() 的取值，ZCode 凭据派生密钥里用到
fn node_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

fn credential_secret() -> String {
    if let Ok(s) = std::env::var("ZCODE_CREDENTIAL_SECRET") {
        let s = s.trim();
        if !s.is_empty() {
            return s.to_string();
        }
    }
    let home = dirs::home_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!(
        "zcode-credential-fallback:{}:{}:{}",
        node_platform(),
        home,
        whoami::username()
    )
}

/// 解密 ZCode 的 enc:v1:<iv>.<tag>.<ct> 凭据（AES-256-GCM, key=sha256(secret)）
pub fn decrypt_credential(value: &str) -> Result<String, String> {
    let rest = value
        .strip_prefix(ENC_PREFIX)
        .ok_or_else(|| "凭据不是 enc:v1 加密格式".to_string())?;
    let parts: Vec<&str> = rest.split('.').collect();
    if parts.len() != 3 {
        return Err("凭据密文格式无效".into());
    }
    let decode = |s: &str| URL_SAFE_NO_PAD.decode(s).map_err(|e| e.to_string());
    let iv = decode(parts[0])?;
    let tag = decode(parts[1])?;
    let ct = decode(parts[2])?;
    if iv.len() != 12 {
        return Err("凭据 IV 长度异常".into());
    }
    let key = Sha256::digest(credential_secret().as_bytes());
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| format!("初始化解密器失败: {e}"))?;
    let mut payload = ct;
    payload.extend_from_slice(&tag);
    let plain = cipher
        .decrypt(Nonce::from_slice(&iv), payload.as_ref())
        .map_err(|_| "凭据解密失败（密钥不匹配或密文损坏）".to_string())?;
    String::from_utf8(plain).map_err(|e| e.to_string())
}

/// 手动配置优先；否则读 ZCode 凭据文件中 coding-plan 的 api-key
pub fn resolve_api_key(manual: Option<&str>) -> Result<(String, &'static str), String> {
    if let Some(key) = manual.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok((key.to_string(), "manual"));
    }
    let home = dirs::home_dir().ok_or("无法定位用户主目录")?;
    let path = home.join(CRED_FILE);
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("读取 {} 失败: {e}", path.display()))?;
    let creds: serde_json::Map<String, Value> =
        serde_json::from_str(&raw).map_err(|e| format!("凭据文件解析失败: {e}"))?;
    for (k, v) in &creds {
        if k.starts_with("account-provider:coding-plan:account:") && k.ends_with(":api-key") {
            if let Some(s) = v.as_str() {
                let key = if s.starts_with(ENC_PREFIX) {
                    decrypt_credential(s)?
                } else {
                    s.to_string()
                };
                if !key.is_empty() {
                    return Ok((key, "zcode"));
                }
            }
        }
    }
    Err("ZCode 凭据中未找到 coding-plan api-key，请在设置中手动填写".into())
}

fn entry_from(v: &Value) -> LimitEntry {
    let f = |name: &str| v.get(name).and_then(Value::as_f64);
    let mut details = BTreeMap::new();
    if let Some(list) = v.get("usageDetails").and_then(Value::as_array) {
        for item in list {
            if let (Some(code), Some(usage)) = (
                item.get("modelCode").and_then(Value::as_str),
                item.get("usage").and_then(Value::as_f64),
            ) {
                details.insert(code.to_string(), usage);
            }
        }
    }
    LimitEntry {
        percentage: f("percentage").unwrap_or(0.0),
        next_reset_ms: v.get("nextResetTime").and_then(Value::as_u64),
        usage: f("usage"),
        current: f("currentValue"),
        remaining: f("remaining"),
        details,
    }
}

fn classify(data: &Value) -> (Option<LimitEntry>, Option<LimitEntry>, Option<LimitEntry>) {
    let mut token_limits: Vec<&Value> = data
        .get("limits")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter(|l| l.get("type").and_then(Value::as_str) == Some("TOKENS_LIMIT"))
                .collect()
        })
        .unwrap_or_default();
    // 5 小时窗口的重置时间一定早于每周窗口，以此区分
    token_limits.sort_by_key(|l| l.get("nextResetTime").and_then(Value::as_u64).unwrap_or(u64::MAX));
    let five_hour = token_limits.first().map(|v| entry_from(v));
    let weekly = token_limits.get(1).map(|v| entry_from(v));
    let mcp = data
        .get("limits")
        .and_then(Value::as_array)
        .and_then(|arr| {
            arr.iter()
                .find(|l| l.get("type").and_then(Value::as_str) == Some("TIME_LIMIT"))
        })
        .map(entry_from);
    (five_hour, weekly, mcp)
}

/// 拉取一次配额快照（不含 key 解析错误时的降级逻辑，由 poll_once 包装）
pub async fn fetch_quota(
    client: &reqwest::Client,
    base_url: &str,
    key: &str,
) -> Result<QuotaSnapshot, String> {
    let url = format!("{}{}", base_url.trim_end_matches('/'), QUOTA_PATH);
    let resp = client
        .get(&url)
        .header("Authorization", key)
        .header("Accept-Language", "en-US,en")
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| format!("响应解析失败: {e}"))?;
    if body.get("success").and_then(Value::as_bool) != Some(true) {
        let msg = body
            .get("msg")
            .and_then(Value::as_str)
            .unwrap_or("未知错误")
            .to_string();
        return Err(format!("接口返回错误 (HTTP {status}): {msg}"));
    }
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    let data = body.get("data").cloned().unwrap_or(Value::Null);
    let (five_hour, weekly, mcp) = classify(&data);
    Ok(QuotaSnapshot {
        ok: true,
        error: None,
        level: data.get("level").and_then(Value::as_str).map(str::to_string),
        five_hour,
        weekly,
        mcp,
        key_source: None,
        fetched_at_ms: now_ms(),
    })
}

/// 解析 key + 拉取配额，任何失败都产出带 error 的快照（前端展示用）
pub async fn poll_once(
    client: &reqwest::Client,
    base_url: &str,
    manual_key: Option<&str>,
) -> QuotaSnapshot {
    let (key, source) = match resolve_api_key(manual_key) {
        Ok(kv) => kv,
        Err(e) => {
            return QuotaSnapshot {
                ok: false,
                error: Some(e),
                key_source: None,
                fetched_at_ms: now_ms(),
                ..Default::default()
            }
        }
    };
    match fetch_quota(client, base_url, &key).await {
        Ok(mut snap) => {
            snap.key_source = Some(source.to_string());
            snap
        }
        Err(e) => QuotaSnapshot {
            ok: false,
            error: Some(e),
            key_source: Some(source.to_string()),
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

    /// 与 zcode.cjs 中 createZCodeCredentialCipher 对应的 node 加密器（测试用镜像实现）
    #[test]
    fn decrypt_roundtrip_matches_node_cipher() {
        let secret = credential_secret();
        // node 侧输出样例由本仓库 docs 中的脚本生成：
        // node -e 'const c=require("crypto");const k=c.createHash("sha256").update(process.argv[1]).digest();const iv=c.randomBytes(12);const ci=c.createCipheriv("aes-256-gcm",k,iv);const ct=Buffer.concat([ci.update("hello-key","utf8"),ci.final()]);const tag=ci.getAuthTag();console.log(["enc:v1:",iv.toString("base64url"),tag.toString("base64url"),ct.toString("base64url")].join("."))' <secret>
        // 这里直接用相同算法在 Rust 侧自证往返：
        let key = Sha256::digest(secret.as_bytes());
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let iv = [7u8; 12];
        let mut payload = b"test-api-key-123".to_vec();
        let tag_len = 16;
        let sealed = cipher.encrypt(Nonce::from_slice(&iv), payload.as_ref()).unwrap();
        let (ct, tag) = sealed.split_at(sealed.len() - tag_len);
        payload = ct.to_vec();
        let enc = format!(
            "{ENC_PREFIX}{}.{}.{}",
            URL_SAFE_NO_PAD.encode(iv),
            URL_SAFE_NO_PAD.encode(tag),
            URL_SAFE_NO_PAD.encode(payload)
        );
        assert_eq!(decrypt_credential(&enc).unwrap(), "test-api-key-123");
    }

    #[test]
    fn decrypt_rejects_garbage() {
        assert!(decrypt_credential("enc:v1:bad.bad.bad").is_err());
        assert!(decrypt_credential("plaintext").is_err());
    }

    /// 端到端联调：真实凭据 + 真实接口（仅手动运行：cargo test -- --ignored）
    #[tokio::test]
    #[ignore]
    async fn real_api_poll() {
        let client = reqwest::Client::new();
        let snap = poll_once(&client, "https://open.bigmodel.cn", None).await;
        assert!(snap.ok, "poll failed: {:?}", snap.error);
        assert!(snap.five_hour.is_some(), "missing 5h limit");
        assert!(snap.weekly.is_some(), "missing weekly limit");
        println!(
            "5h {}% / weekly {}% / level {:?} / key {:?}",
            snap.five_hour.as_ref().unwrap().percentage,
            snap.weekly.as_ref().unwrap().percentage,
            snap.level,
            snap.key_source
        );
    }

    #[test]
    fn classify_identifies_windows() {        let data = serde_json::json!({
            "limits": [
                {"type":"TOKENS_LIMIT","unit":3,"number":5,"percentage":2,"nextResetTime":1792866930498i64},
                {"type":"TOKENS_LIMIT","unit":6,"number":1,"percentage":33,"nextResetTime":1793245064999i64},
                {"type":"TIME_LIMIT","unit":5,"number":1,"usage":4000,"currentValue":4000,"remaining":0,"percentage":100,
                 "nextResetTime":1792980025934i64,
                 "usageDetails":[{"modelCode":"search-prime","usage":3955},{"modelCode":"web-reader","usage":45}]}
            ],
            "level":"max"
        });
        let (five, weekly, mcp) = classify(&data);
        assert_eq!(five.unwrap().percentage, 2.0);
        assert_eq!(weekly.unwrap().percentage, 33.0);
        let mcp = mcp.unwrap();
        assert_eq!(mcp.percentage, 100.0);
        assert_eq!(mcp.details.get("search-prime"), Some(&3955.0));
    }
}
