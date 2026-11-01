use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct GpuInfo {
    pub index: u32,
    pub name: String,
    pub util: f64,
    pub mem_used_mb: f64,
    pub mem_total_mb: f64,
    pub temp_c: Option<f64>,
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct AgentMetrics {
    pub hostname: String,
    pub cpu_percent: f64,
    #[serde(default)]
    pub cpu_cores: Vec<f64>,
    pub mem_used_gb: Option<f64>,
    pub mem_total_gb: Option<f64>,
    #[serde(default)]
    pub gpus: Vec<GpuInfo>,
    pub agent_ts_ms: u64,
}

#[derive(Clone, Serialize, Debug)]
pub struct ServerUpdate {
    pub id: String,
    pub online: bool,
    pub error: Option<String>,
    pub metrics: Option<AgentMetrics>,
    pub latency_ms: Option<u64>,
}

fn normalize_endpoint(url: &str) -> String {
    let u = url.trim().trim_end_matches('/');
    if u.contains("://") {
        u.to_string()
    } else {
        format!("http://{u}")
    }
}

pub async fn fetch_metrics(
    client: &reqwest::Client,
    url: &str,
    token: Option<&str>,
) -> Result<AgentMetrics, String> {
    let endpoint = format!("{}/metrics", normalize_endpoint(url));
    let mut req = client.get(&endpoint).timeout(Duration::from_secs(4));
    if let Some(t) = token.map(str::trim).filter(|s| !s.is_empty()) {
        req = req.header("X-Agent-Token", t);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("连接失败: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    resp.json::<AgentMetrics>()
        .await
        .map_err(|e| format!("响应解析失败: {e}"))
}
