//! 从 Agent 自己的配置文件读出「现在走哪条线路」。界面只信这里，不信数据库指针。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use toml_edit::DocumentMut;

use super::agents::AgentId;
use super::consts::API_HOST;
use super::upstream;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Route {
    Pontex {
        key_hint: Option<String>,
        model: Option<String>,
    },
    /// 官方账号登录，或还没有任何配置。
    Official,
    /// 官方 Key、Bedrock、Vertex 等：直连官方，但凭据写在配置里。
    OfficialKey { via: String },
    /// 别家网关。
    Other { name: String },
    /// WorkBuddy：PonteX 模型在不在列表里。
    Added {
        models: usize,
        key_hint: Option<String>,
    },
    NotAdded,
    Unreadable { file: String },
}

impl Route {
    pub fn is_pontex(&self) -> bool {
        matches!(self, Route::Pontex { .. } | Route::Added { .. })
    }
}

/// 我们会写的文件。登录文件（`auth.json`）不在内：登录不归我们管。
pub fn managed_files(id: AgentId) -> Vec<PathBuf> {
    match id {
        AgentId::ClaudeCode => vec![upstream::claude_settings()],
        AgentId::Codex => vec![upstream::codex_config()],
        AgentId::GeminiCli => vec![
            upstream::gemini_env_file(),
            upstream::gemini_settings(),
        ],
        AgentId::GrokBuild => vec![upstream::grok_config()],
        AgentId::Workbuddy => vec![upstream::workbuddy_models()],
    }
}

pub fn digest(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}

/// Key 只露前缀，和控制台列表里显示的一样（`px-` 加 8 位）。
pub fn key_hint(key: &str) -> String {
    key.trim().chars().take(11).collect()
}

pub fn read(id: AgentId) -> Route {
    let result = match id {
        AgentId::ClaudeCode => read_claude(),
        AgentId::Codex => read_codex(),
        AgentId::GeminiCli => read_gemini(),
        AgentId::GrokBuild => read_grok(),
        AgentId::Workbuddy => read_workbuddy(),
    };
    result.unwrap_or_else(|path| Route::Unreadable {
        file: path.display().to_string(),
    })
}

fn text(path: &Path) -> Result<Option<String>, PathBuf> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text.trim_start_matches('\u{feff}').to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(path.to_path_buf()),
    }
}

fn json(path: &Path) -> Result<Value, PathBuf> {
    match text(path)? {
        Some(text) if !text.trim().is_empty() => {
            // Gemini CLI 允许注释，其余客户端写的都是严格 JSON；宽松解析只用来读。
            json5::from_str(&text).map_err(|_| path.to_path_buf())
        }
        _ => Ok(Value::Null),
    }
}

fn toml(path: &Path) -> Result<DocumentMut, PathBuf> {
    text(path)?
        .unwrap_or_default()
        .parse::<DocumentMut>()
        .map_err(|_| path.to_path_buf())
}

fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url.trim())
        .ok()?
        .host_str()
        .map(str::to_ascii_lowercase)
}

fn filled(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

fn read_claude() -> Result<Route, PathBuf> {
    let root = json(&upstream::claude_settings())?;
    let env = |name: &str| filled(root.pointer(&format!("/env/{name}")).and_then(Value::as_str));
    let truthy = |name: &str| env(name).is_some_and(|v| v != "0" && !v.eq_ignore_ascii_case("false"));
    for (flag, via) in [
        ("CLAUDE_CODE_USE_BEDROCK", "Amazon Bedrock"),
        ("CLAUDE_CODE_USE_VERTEX", "Google Vertex AI"),
        ("CLAUDE_CODE_USE_FOUNDRY", "Microsoft Foundry"),
    ] {
        if truthy(flag) {
            return Ok(Route::OfficialKey { via: via.into() });
        }
    }
    let credential = env("ANTHROPIC_AUTH_TOKEN").or_else(|| env("ANTHROPIC_API_KEY"));
    if let Some(base) = env("ANTHROPIC_BASE_URL") {
        return Ok(match host_of(base) {
            Some(host) if host == API_HOST => Route::Pontex {
                key_hint: credential.map(key_hint),
                model: env("ANTHROPIC_MODEL").map(str::to_string),
            },
            Some(host) if host == "api.anthropic.com" => Route::OfficialKey {
                via: "API Key".into(),
            },
            Some(host) => Route::Other { name: host },
            None => Route::Other { name: base.into() },
        });
    }
    let helper = filled(root.get("apiKeyHelper").and_then(Value::as_str));
    Ok(if credential.is_some() || helper.is_some() {
        Route::OfficialKey { via: "API Key".into() }
    } else {
        Route::Official
    })
}

fn read_codex() -> Result<Route, PathBuf> {
    let doc = toml(&upstream::codex_config())?;
    let top = |name: &str| filled(doc.get(name).and_then(|item| item.as_str()));
    let provider = top("model_provider").unwrap_or("openai");
    if provider != "openai" {
        let table = doc.get("model_providers").and_then(|t| t.get(provider));
        let field = |name: &str| filled(table.and_then(|t| t.get(name)).and_then(|v| v.as_str()));
        return Ok(match field("base_url").and_then(host_of) {
            Some(host) if host == API_HOST => Route::Pontex {
                key_hint: field("experimental_bearer_token").map(key_hint),
                model: top("model").map(str::to_string),
            },
            Some(host) => Route::Other { name: host },
            None => Route::Other { name: provider.into() },
        });
    }
    if let Some(host) = top("openai_base_url").and_then(host_of) {
        if host != "api.openai.com" {
            return Ok(Route::Other { name: host });
        }
    }
    let auth = json(&upstream::codex_auth()).unwrap_or(Value::Null);
    let has_key = filled(auth.get("OPENAI_API_KEY").and_then(Value::as_str)).is_some();
    let has_login = auth.get("tokens").is_some_and(|t| !t.is_null());
    Ok(if has_key && !has_login {
        Route::OfficialKey { via: "API Key".into() }
    } else {
        Route::Official
    })
}

pub fn gemini_env() -> Result<std::collections::HashMap<String, String>, PathBuf> {
    upstream::gemini_env().map_err(|_| upstream::gemini_env_file())
}

fn read_gemini() -> Result<Route, PathBuf> {
    let env = gemini_env()?;
    let settings = json(&upstream::gemini_settings())?;
    let get = |name: &str| filled(env.get(name).map(String::as_str));
    let key = get("GEMINI_API_KEY").or_else(|| get("GOOGLE_API_KEY"));
    if let Some(base) = get("GOOGLE_GEMINI_BASE_URL") {
        return Ok(match host_of(base) {
            Some(host) if host == API_HOST => Route::Pontex {
                key_hint: key.map(key_hint),
                model: get("GEMINI_MODEL").map(str::to_string),
            },
            Some(host) if host.ends_with(".googleapis.com") => Route::OfficialKey {
                via: "API Key".into(),
            },
            Some(host) => Route::Other { name: host },
            None => Route::Other { name: base.into() },
        });
    }
    let selected = settings
        .pointer("/security/auth/selectedType")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Ok(if selected == "vertex-ai" || get("GOOGLE_GENAI_USE_VERTEXAI").is_some() {
        Route::OfficialKey { via: "Google Vertex AI".into() }
    } else if key.is_some() && selected != "oauth-personal" {
        Route::OfficialKey { via: "API Key".into() }
    } else {
        Route::Official
    })
}

fn read_grok() -> Result<Route, PathBuf> {
    let doc = toml(&upstream::grok_config())?;
    let default = filled(
        doc.get("models")
            .and_then(|m| m.get("default"))
            .and_then(|v| v.as_str()),
    );
    let Some(profile) = default else {
        return Ok(Route::Official);
    };
    let table = doc.get("model").and_then(|m| m.get(profile));
    let field = |name: &str| filled(table.and_then(|t| t.get(name)).and_then(|v| v.as_str()));
    Ok(match field("base_url").map(|url| (url, host_of(url))) {
        None => Route::Official,
        Some((_, Some(host))) if host == API_HOST => Route::Pontex {
            key_hint: field("api_key").map(key_hint),
            model: field("model").map(str::to_string),
        },
        Some((_, Some(host))) if host == "api.x.ai" => Route::OfficialKey {
            via: "API Key".into(),
        },
        Some((_, Some(host))) => Route::Other { name: host },
        Some((url, None)) => Route::Other { name: url.into() },
    })
}

fn read_workbuddy() -> Result<Route, PathBuf> {
    let root = json(&upstream::workbuddy_models())?;
    let models = match &root {
        Value::Array(list) => list.as_slice(),
        Value::Object(map) => match map.get("models") {
            Some(Value::Array(list)) => list.as_slice(),
            None | Some(Value::Null) => &[],
            Some(_) => return Err(upstream::workbuddy_models()),
        },
        _ => &[],
    };
    let ours: Vec<&Value> = models
        .iter()
        .filter(|m| {
            m.get(upstream::WORKBUDDY_OWNER).and_then(Value::as_str)
                == Some(super::consts::PROVIDER_ID)
        })
        .collect();
    Ok(if ours.is_empty() {
        Route::NotAdded
    } else {
        Route::Added {
            models: ours.len(),
            key_hint: ours[0].get("apiKey").and_then(Value::as_str).map(key_hint),
        }
    })
}
