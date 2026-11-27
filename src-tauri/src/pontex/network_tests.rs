//! 联网部分对着本机假网关测：原因码、跳转、四种协议的路径和请求体。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};

use super::agents::AgentId;
use super::{key, verify};

type Seen = Arc<Mutex<Vec<(String, HashMap<String, String>, Value)>>>;

struct Gateway {
    seen: Seen,
    _env: crate::test_support::EnvVars,
}

async fn gateway(reply: impl Fn(&str) -> Response + Clone + Send + Sync + 'static) -> Gateway {
    let seen: Seen = Arc::default();
    let record = seen.clone();
    let app = axum::Router::new().fallback(move |uri: Uri, headers: HeaderMap, body: Bytes| {
        let (record, reply) = (record.clone(), reply.clone());
        async move {
            let headers = headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string()))
                .collect();
            let body = serde_json::from_slice(&body).unwrap_or(Value::Null);
            record.lock().unwrap().push((uri.path().to_string(), headers, body));
            reply(uri.path())
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Gateway {
        seen,
        _env: crate::test_support::EnvVars::new()
            .set("PONTEX_ONE_DEV_API", origin)
            .set("NO_PROXY", "127.0.0.1"),
    }
}

fn refuse(status: u16, reason: &'static str) -> Response {
    (
        StatusCode::from_u16(status).unwrap(),
        [("x-pontex-reason", reason), ("x-pontex-request-id", "px-req")],
        "{}",
    )
        .into_response()
}

fn secret() -> key::Secret {
    key::parse("px-AbCdEfGhIjKl").unwrap()
}

#[tokio::test]
#[serial_test::serial]
async fn key_reasons_come_from_the_gateway_header() {
    for (status, reason, usable) in [
        (401, "INVALID_API_KEY", false),
        (403, "API_KEY_EXPIRED", false),
        (403, "IP_NOT_ALLOWED", false),
        (402, "INSUFFICIENT_BALANCE", true),
        (402, "PERIOD_LIMIT_EXCEEDED", true),
    ] {
        let _g = gateway(move |_| refuse(status, reason)).await;
        let report = key::check(&secret()).await;
        assert_eq!(report.reason.as_deref(), Some(reason));
        assert_eq!(report.usable, usable, "{reason}");
        assert_eq!(report.request_id.as_deref(), Some("px-req"));
        assert!(!report.listed);
    }
}

#[tokio::test]
#[serial_test::serial]
async fn a_good_key_lists_models_and_native_gemini_models() {
    let g = gateway(|path| match path {
        "/v1/models" => (
            [("x-pontex-key-rotated", "2026-12-01T00:00:00Z")],
            json!({"data": [{"id": "google/gemini-3.5-flash", "modality": "text"}]}).to_string(),
        )
            .into_response(),
        _ => json!({"models": [{"name": "models/google/gemini-3.5-flash"}]}).to_string().into_response(),
    })
    .await;
    let report = key::check(&secret()).await;
    assert!(report.usable && report.listed, "{report:?}");
    assert_eq!(report.gemini.unwrap(), vec!["google/gemini-3.5-flash"]);
    assert!(report.rotated_until.is_some());
    let seen = g.seen.lock().unwrap();
    assert_eq!(seen[0].1["authorization"], "Bearer px-AbCdEfGhIjKl");
    assert_eq!(seen[1].0, "/v1beta/models");
}

#[tokio::test]
#[serial_test::serial]
async fn the_key_never_follows_a_redirect() {
    let elsewhere = Arc::new(Mutex::new(0));
    let count = elsewhere.clone();
    let app = axum::Router::new().fallback(move || {
        *count.lock().unwrap() += 1;
        async { "{}" }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = format!("http://{}/steal", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let _g = gateway(move |_| (StatusCode::FOUND, [("location", target.clone())]).into_response()).await;
    let report = key::check(&secret()).await;
    assert!(!report.usable);
    assert_eq!(report.reason.as_deref(), Some("HTTP_302"));
    assert_eq!(*elsewhere.lock().unwrap(), 0);
}

#[tokio::test]
#[serial_test::serial]
async fn every_agent_is_verified_over_its_own_protocol() {
    let g = gateway(|_| "{}".into_response()).await;
    for (id, path) in [
        (AgentId::ClaudeCode, "/v1/messages"),
        (AgentId::Codex, "/v1/responses"),
        (AgentId::GeminiCli, "/v1beta/models/v/m:generateContent"),
        (AgentId::GrokBuild, "/v1/responses"),
        (AgentId::Workbuddy, "/chat/completions"),
    ] {
        assert!(verify::run(id, &secret(), "v/m").await.ok, "{id:?}");
        let seen = g.seen.lock().unwrap();
        let (got, headers, body) = seen.last().unwrap();
        assert_eq!(got, path);
        let credential = if id == AgentId::GeminiCli { "x-goog-api-key" } else { "authorization" };
        assert!(headers[credential].contains("px-AbCdEfGhIjKl"), "{id:?}");
        // PonteX 不保存对话状态，带 store:true 会被拒绝
        assert_ne!(body["store"], json!(true));
    }
}

#[tokio::test]
#[serial_test::serial]
async fn a_rejected_request_is_not_reported_as_verified() {
    let _g = gateway(|_| refuse(403, "MODEL_NOT_AVAILABLE")).await;
    let result = verify::run(AgentId::Codex, &secret(), "v/m").await;
    assert!(!result.ok);
    assert_eq!(result.reason.as_deref(), Some("MODEL_NOT_AVAILABLE"));
}
