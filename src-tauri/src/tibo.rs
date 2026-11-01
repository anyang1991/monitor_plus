use serde::Serialize;
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Tibo（Codex 负责人 @thsottiaux）重置官宣追踪，数据来自公共聚合源
/// （yurenpai/tibo-reset-live 项目同款默认源）
const FEED_URL: &str = "https://codex-reset.com/api/feed";

#[derive(Clone, Serialize, Default, Debug)]
pub struct TiboSnapshot {
    pub ok: bool,
    pub error: Option<String>,
    /// 下一次重置日期短格式 "MM-DD"；无计划为 None
    pub next_date: Option<String>,
    /// 完整日期 "YYYY-MM-DD"
    pub next_date_full: Option<String>,
    /// Tibo 原话摘要
    pub summary: Option<String>,
    /// 推文链接
    pub url: Option<String>,
    /// 官宣时间（ISO）
    pub announced_at: Option<String>,
    pub confidence: Option<String>,
    /// 无确定日期但有预告帖时的说明（前端显示"预告"而非"无计划"）
    pub preview_note: Option<String>,
    /// 最近一条明确时间信号（如 "Lands tomorrow before 10am PST"）
    pub signal_summary: Option<String>,
    /// 聚合源是否标记数据过期
    pub stale: bool,
    pub fetched_at_ms: u64,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 今天 UTC 日期 "YYYY-MM-DD"
fn today_utc() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    civil_from_days(secs.div_euclid(86400))
}

/// Howard Hinnant civil_from_days：天数 → "YYYY-MM-DD"
fn civil_from_days(days: i64) -> String {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// 从 feed 事件数组里选出"下一次重置"：type=reset 且 date >= 今天 UTC，取最近的一条
fn pick_next_reset<'a>(body: &'a Value, today: &str) -> Option<&'a Value> {
    let events = body.get("events")?.as_array()?;
    events
        .iter()
        .filter(|e| e.get("type").and_then(Value::as_str) == Some("reset"))
        .filter_map(|e| e.get("date").and_then(Value::as_str).map(|d| (d, e)))
        .filter(|(date, _)| *date >= today)
        .min_by_key(|(date, _)| *date)
        .map(|(_, e)| e)
}

pub async fn poll_once(client: &reqwest::Client) -> TiboSnapshot {
    let resp = match client
        .get(FEED_URL)
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(15))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return TiboSnapshot {
                ok: false,
                error: Some(format!("请求失败: {e}")),
                fetched_at_ms: now_ms(),
                ..Default::default()
            }
        }
    };
    if !resp.status().is_success() {
        return TiboSnapshot {
            ok: false,
            error: Some(format!("HTTP {}", resp.status())),
            fetched_at_ms: now_ms(),
            ..Default::default()
        };
    }
    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            return TiboSnapshot {
                ok: false,
                error: Some(format!("解析失败: {e}")),
                fetched_at_ms: now_ms(),
                ..Default::default()
            }
        }
    };

    let today = today_utc();
    let stale = body.get("stale").and_then(Value::as_bool).unwrap_or(false);
    let signal_summary = body
        .get("signal")
        .and_then(|s| s.get("summary"))
        .and_then(Value::as_str)
        .map(str::to_string);

    match pick_next_reset(&body, &today) {
        Some(e) => {
            let full = e
                .get("date")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            TiboSnapshot {
                ok: true,
                error: None,
                next_date: Some(full.get(5..).unwrap_or("").to_string()),
                next_date_full: Some(full),
                summary: e
                    .get("summary")
                    .and_then(Value::as_str)
                    .map(|s| s.chars().take(160).collect()),
                url: e.get("url").and_then(Value::as_str).map(str::to_string),
                announced_at: e
                    .get("announced_at")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                confidence: e.get("confidence").and_then(Value::as_str).map(str::to_string),
                preview_note: None,
                signal_summary,
                stale,
                fetched_at_ms: now_ms(),
            }
        }
        None => {
            // 无确定事件时的预告回退：找最新的"重置相关"帖（reset_related lane），
            // 其文本含"未来重置"预告字样（如 next two days will be resets）则显示预告。
            // 这类帖子未经确证（explicit_reset_claim=false），所以用 preview 语义展示。
            const HINTS: &[&str] = &[
                "will be resets",
                "is a reset",
                "resets tomorrow",
                "reset tomorrow",
                "resets today",
                "reset today",
                "banked",
            ];
            let preview = body
                .get("tweets")
                .and_then(Value::as_array)
                .and_then(|tweets| {
                    tweets.iter().find(|t| {
                        t.get("tibo_lane").and_then(Value::as_str) == Some("reset_related")
                            && t.get("explicit_reset_claim").and_then(Value::as_bool) != Some(true)
                            && t.get("text")
                                .and_then(Value::as_str)
                                .map(|s| HINTS.iter().any(|h| s.to_ascii_lowercase().contains(h)))
                                .unwrap_or(false)
                    })
                })
                .map(|t| {
                    let summary = t
                        .get("text")
                        .and_then(Value::as_str)
                        .map(|s| s.chars().take(160).collect::<String>());
                    (t.get("url").and_then(Value::as_str).map(str::to_string), summary)
                });
            let (url, summary) = match preview {
                Some((u, s)) => (u, s),
                None => (None, None),
            };
            TiboSnapshot {
                ok: true,
                error: None,
                next_date: None,
                next_date_full: None,
                // 有预告帖时用它的摘要占位（前端据此显示"预告：1-2 天内"而非"无计划"）
                preview_note: url.as_ref().map(|_| "Tibo 预告近期将重置（未给出确切时间）".to_string()),
                summary,
                url,
                announced_at: None,
                confidence: Some("preview".into()),
                signal_summary,
                stale,
                fetched_at_ms: now_ms(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn picks_nearest_future_reset() {
        let body = json!({
            "events": [
                {"type":"reset","date":"2026-11-05","summary":"far"},
                {"type":"feature","date":"2026-11-01","summary":"not a reset"},
                {"type":"reset","date":"2026-10-31","summary":"today"},
                {"type":"reset","date":"2026-10-20","summary":"past"}
            ]
        });
        let picked = pick_next_reset(&body, "2026-10-31").unwrap();
        assert_eq!(picked.get("summary").unwrap(), "today");
    }

    #[test]
    fn none_when_all_past() {
        let body = json!({
            "events": [{"type":"reset","date":"2026-10-20","summary":"past"}]
        });
        assert!(pick_next_reset(&body, "2026-10-31").is_none());
    }

    #[test]
    fn civil_roundtrip() {
        assert_eq!(civil_from_days(0), "1970-01-01");
        assert_eq!(civil_from_days(20_000), "2024-10-04");
    }
}
