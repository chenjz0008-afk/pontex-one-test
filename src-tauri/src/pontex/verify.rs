//! 用这个 Agent 的协议和主模型发一次最小请求。证明的是「Key + 模型 + 协议」网关接受。

use std::time::Duration;

use serde_json::{json, Value};

use super::agents::AgentId;
use super::consts::api_origin;
use super::key::{client_for, header, Secret};
use super::models::{manifest, Protocol};
use super::store::Verify;

pub async fn run(id: AgentId, key: &Secret, model: &str) -> Verify {
    let (reason, request_id) = match send(id, key, model).await {
        Ok(request_id) => (None, request_id),
        Err((reason, request_id)) => (Some(reason), request_id),
    };
    Verify {
        at: chrono::Utc::now().timestamp(),
        ok: reason.is_none(),
        reason,
        request_id,
        model: model.to_string(),
        key_hint: key.hint(),
    }
}

async fn send(
    id: AgentId,
    key: &Secret,
    model: &str,
) -> Result<Option<String>, (String, Option<String>)> {
    let protocol = manifest(id).protocol;
    let (path, body): (String, Value) = match protocol {
        Protocol::Messages => (
            "/v1/messages".into(),
            json!({"model": model, "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}]}),
        ),
        Protocol::Responses => (
            "/v1/responses".into(),
            json!({"model": model, "input": "hi", "max_output_tokens": 16, "store": false}),
        ),
        Protocol::Chat => (
            "/chat/completions".into(),
            json!({"model": model, "max_tokens": 16, "messages": [{"role": "user", "content": "hi"}]}),
        ),
        Protocol::Gemini => (
            format!("/v1beta/models/{model}:generateContent"),
            json!({"contents": [{"role": "user", "parts": [{"text": "hi"}]}], "generationConfig": {"maxOutputTokens": 16}}),
        ),
    };
    let url = format!("{}{path}", api_origin());
    let client = client_for(&url).map_err(|_| ("NETWORK".to_string(), None))?;
    let mut request = client.post(&url).timeout(Duration::from_secs(45)).json(&body);
    request = match protocol {
        Protocol::Gemini => request.header("x-goog-api-key", key.expose()),
        Protocol::Messages => request
            .bearer_auth(key.expose())
            .header("anthropic-version", "2023-06-01"),
        _ => request.bearer_auth(key.expose()),
    };
    let response = request.send().await.map_err(|error| {
        (if error.is_timeout() { "TIMEOUT" } else { "NETWORK" }.to_string(), None)
    })?;
    let request_id = header(&response, "x-pontex-request-id");
    let status = response.status();
    if status.is_success() {
        return Ok(request_id);
    }
    let reason = header(&response, "x-pontex-reason")
        .unwrap_or_else(|| format!("HTTP_{}", status.as_u16()));
    Err((reason, request_id))
}
