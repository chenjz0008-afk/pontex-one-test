//! 用户 Key 的检查。只读 `GET /v1/models`，按网关的 `X-Pontex-Reason` 判断原因。

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::consts::api_origin;

/// 用户的 Key。`Debug` 只显示前缀，不会被日志带出去。
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
    pub fn hint(&self) -> String {
        super::live::key_hint(&self.0)
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Secret({}…)", self.hint())
    }
}

/// 格式检查，不联网。返回的错误码和 [`KeyReport::reason`] 同一套。
pub fn parse(raw: &str) -> Result<Secret, &'static str> {
    let key = raw.trim();
    if key.starts_with("pxp-") {
        return Err("PROVISIONING_KEY_NOT_FOR_INFERENCE");
    }
    let body = key.strip_prefix("px-").ok_or("KEY_FORMAT")?;
    if body.len() < 5
        || body.len() > 256
        || !body.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err("KEY_FORMAT");
    }
    Ok(Secret(key.to_string()))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub modality: Option<String>,
    #[serde(default)]
    pub context_length: Option<u64>,
}

impl ModelInfo {
    pub fn is_text(&self) -> bool {
        self.modality.as_deref().is_none_or(|m| m == "text")
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyReport {
    /// Key 本身可用（余额不足也算可用，只是没有额度）。
    pub usable: bool,
    /// 拿到了模型列表。
    pub listed: bool,
    pub reason: Option<String>,
    pub request_id: Option<String>,
    /// 这把是轮换前的旧 Key，值是失效时间。
    pub rotated_until: Option<String>,
    pub hint: String,
    #[serde(skip)]
    pub models: Vec<ModelInfo>,
    /// Gemini 原生入口能调用的模型；`None` 是没查到。
    #[serde(skip)]
    pub gemini: Option<Vec<String>>,
}

/// 这些 402 说明 Key 是对的，只是现在没有额度。
const QUOTA: &[&str] = &[
    "INSUFFICIENT_BALANCE",
    "WALLET_FROZEN",
    "MEMBER_QUOTA_EXCEEDED",
    "PERIOD_LIMIT_EXCEEDED",
];

pub(super) fn client_for(url: &str) -> Result<reqwest::Client, String> {
    // 跟随用户的代理设置，但不跟随跳转：Key 只能到这一个主机。
    super::upstream::http(url)
}

pub(super) fn header(response: &reqwest::Response, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

pub async fn check(key: &Secret) -> KeyReport {
    let mut report = KeyReport {
        usable: false,
        listed: false,
        reason: None,
        request_id: None,
        rotated_until: None,
        hint: key.hint(),
        models: Vec::new(),
        gemini: None,
    };
    let url = format!("{}/v1/models", api_origin());
    let sent = match client_for(&url) {
        Ok(client) => {
            client
                .get(&url)
                .bearer_auth(key.expose())
                .timeout(Duration::from_secs(20))
                .send()
                .await
        }
        Err(_) => {
            report.reason = Some("NETWORK".into());
            return report;
        }
    };
    let response = match sent {
        Ok(response) => response,
        Err(error) => {
            report.reason = Some(if error.is_timeout() { "TIMEOUT" } else { "NETWORK" }.into());
            return report;
        }
    };
    report.request_id = header(&response, "x-pontex-request-id");
    report.rotated_until = header(&response, "x-pontex-key-rotated");
    let status = response.status().as_u16();
    let reason = header(&response, "x-pontex-reason");
    if (200..300).contains(&status) {
        match response.json::<Value>().await {
            Ok(body) => {
                report.models = parse_models(&body);
                report.listed = !report.models.is_empty();
                report.usable = true;
                if !report.listed {
                    report.reason = Some("NO_MODELS".into());
                }
            }
            Err(_) => report.reason = Some("BAD_RESPONSE".into()),
        }
    } else {
        let reason = reason.unwrap_or_else(|| format!("HTTP_{status}"));
        report.usable = QUOTA.contains(&reason.as_str());
        report.reason = Some(reason);
    }
    if report.listed {
        report.gemini = gemini_models(key).await;
    }
    report
}

pub(super) fn parse_models(body: &Value) -> Vec<ModelInfo> {
    body.get("data")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|item| serde_json::from_value::<ModelInfo>(item.clone()).ok())
                .filter(|m| !m.id.trim().is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// `GET /v1beta/models` 只列出能从 Gemini 原生入口调用的模型，不产生费用。
async fn gemini_models(key: &Secret) -> Option<Vec<String>> {
    let url = format!("{}/v1beta/models", api_origin());
    let response = client_for(&url)
        .ok()?
        .get(&url)
        .header("x-goog-api-key", key.expose())
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body: Value = response.json().await.ok()?;
    Some(
        body.get("models")?
            .as_array()?
            .iter()
            .filter_map(|m| m.get("name")?.as_str())
            .map(|name| name.trim_start_matches("models/").to_string())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(parse("pxp-abcdefgh"), Err("PROVISIONING_KEY_NOT_FOR_INFERENCE"));
        assert_eq!(parse("sk-ant-1"), Err("KEY_FORMAT"));
        assert_eq!(parse("px-a"), Err("KEY_FORMAT"));
        assert_eq!(parse("px-ab cd ef"), Err("KEY_FORMAT"));
        let key = parse("  px-AbCdEfGhIjKl\n").unwrap();
        assert_eq!(key.hint(), "px-AbCdEfGh");
        assert!(!format!("{key:?}").contains("IjKl"));
    }

    #[test]
    fn models_tolerate_extra_fields() {
        let body = serde_json::json!({"data": [
            {"id": "openai/gpt-6-luna", "modality": "text", "context_length": 400000, "x": 1},
            {"id": "openai/gpt-image-1", "modality": "image"}, {"object": "model"}
        ]});
        let models = parse_models(&body);
        assert_eq!(models.len(), 2);
        assert!(models[0].is_text() && !models[1].is_text());
    }
}
