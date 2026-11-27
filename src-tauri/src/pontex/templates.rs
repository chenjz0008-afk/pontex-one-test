//! 五个 Agent 的 PonteX 配置。写法以 docs.ponte-x.com 的接入文档和上游投影器为准。

use serde_json::{json, Value};
use toml_edit::{value, DocumentMut, Item, Table};

use super::agents::AgentId;
use super::consts::{API_ORIGIN, PROVIDER_ID};
use super::key::{ModelInfo, Secret};
use super::models::Resolved;
use super::upstream::Row;

pub const DISPLAY: &str = "PonteX";
/// 网关没给上下文长度时的保守值：报小了只是早一点压缩，报大了长会话会被拒绝。
const FALLBACK_CONTEXT: u64 = 128_000;

pub fn build(id: AgentId, key: &Secret, models: &Resolved, catalog: &[ModelInfo]) -> Row {
    let settings = match id {
        AgentId::ClaudeCode => claude(key, models),
        AgentId::Codex => codex(key, models),
        AgentId::GeminiCli => gemini(key, models),
        AgentId::GrokBuild => grok(key, models),
        AgentId::Workbuddy => workbuddy(key, models, catalog),
    };
    row(PROVIDER_ID, DISPLAY, "custom", settings)
}

pub fn row(id: &str, name: &str, category: &str, settings: Value) -> Row {
    Row {
        id: id.into(),
        name: name.into(),
        category: category.into(),
        settings,
    }
}

/// 五个模型变量要写全：少写的会用 Claude Code 自带的默认名，并照那个模型计费。
/// 用 AUTH_TOKEN（Bearer）：API_KEY 在交互模式会先问用户「是否使用这个 Key」。
fn claude(key: &Secret, models: &Resolved) -> Value {
    json!({ "env": {
        "ANTHROPIC_BASE_URL": API_ORIGIN,
        "ANTHROPIC_AUTH_TOKEN": key.expose(),
        "ANTHROPIC_MODEL": models.main(),
        "ANTHROPIC_DEFAULT_OPUS_MODEL": models.role("opus"),
        "ANTHROPIC_DEFAULT_SONNET_MODEL": models.role("sonnet"),
        "ANTHROPIC_DEFAULT_HAIKU_MODEL": models.role("haiku"),
        "CLAUDE_CODE_SUBAGENT_MODEL": models.role("subagent"),
    }})
}

/// 自定义 provider。内置 `openai` 会探测 WebSocket 并用服务端压缩，PonteX 都不支持。
/// Key 由上游投影成 `experimental_bearer_token`，不进 `auth.json`。
fn codex(key: &Secret, models: &Resolved) -> Value {
    let mut doc = DocumentMut::new();
    doc["model_provider"] = value("custom");
    doc["model"] = value(models.main());
    let mut table = Table::new();
    table["name"] = value(DISPLAY);
    table["base_url"] = value(format!("{API_ORIGIN}/v1"));
    table["wire_api"] = value("responses");
    let mut providers = Table::new();
    providers.set_implicit(true);
    providers["custom"] = Item::Table(table);
    doc["model_providers"] = Item::Table(providers);
    json!({ "auth": { "OPENAI_API_KEY": key.expose() }, "config": doc.to_string() })
}

fn gemini(key: &Secret, models: &Resolved) -> Value {
    json!({ "env": {
        "GOOGLE_GEMINI_BASE_URL": API_ORIGIN,
        "GEMINI_API_KEY": key.expose(),
        "GEMINI_MODEL": models.main(),
    }})
}

fn grok(key: &Secret, models: &Resolved) -> Value {
    let id = models.main();
    let mut doc = DocumentMut::new();
    let mut defaults = Table::new();
    defaults["default"] = value(id);
    doc["models"] = Item::Table(defaults);
    let mut table = Table::new();
    table["model"] = value(id);
    table["base_url"] = value(format!("{API_ORIGIN}/v1"));
    table["name"] = value(DISPLAY);
    table["api_key"] = value(key.expose());
    table["api_backend"] = value("responses");
    table["context_window"] = value(models.context.unwrap_or(FALLBACK_CONTEXT) as i64);
    let mut profiles = Table::new();
    profiles.set_implicit(true);
    profiles.insert(id, Item::Table(table));
    doc["model"] = Item::Table(profiles);
    json!({ "config": doc.to_string() })
}

fn workbuddy(key: &Secret, models: &Resolved, catalog: &[ModelInfo]) -> Value {
    let entries: Vec<Value> = models
        .all()
        .into_iter()
        .map(|id| {
            let mut entry = json!({ "id": id, "supportsToolCall": true });
            if let Some(context) = catalog
                .iter()
                .find(|m| m.id == id)
                .and_then(|m| m.context_length)
            {
                entry["maxInputTokens"] = json!(context);
            }
            entry
        })
        .collect();
    json!({
        "vendor": DISPLAY,
        "url": format!("{API_ORIGIN}/chat/completions"),
        "apiKey": key.expose(),
        "models": entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn resolved(main: &str) -> Resolved {
        Resolved {
            roles: BTreeMap::from([("main".to_string(), main.to_string())]),
            context: None,
        }
    }

    #[test]
    fn no_template_doubles_the_version_segment_or_leaves_the_host() {
        let key = super::super::key::parse("px-AbCdEfGhIjKl").unwrap();
        for id in AgentId::ALL {
            let text = build(id, &key, &resolved("v/m-1.5"), &[]).settings.to_string();
            assert!(text.contains("https://api.ponte-x.com"), "{id:?}");
            assert!(!text.contains("/v1/v1"), "{id:?}");
            assert_eq!(text.matches("https://").count(), 1, "{id:?}");
        }
    }

    #[test]
    fn claude_sets_all_model_variables_and_no_api_key() {
        let key = super::super::key::parse("px-AbCdEfGhIjKl").unwrap();
        let env = claude(&key, &resolved("a/b"))["env"].clone();
        assert_eq!(env.as_object().unwrap().len(), 7);
        assert!(env.get("ANTHROPIC_API_KEY").is_none());
    }

    #[test]
    fn toml_templates_parse_with_dotted_model_names() {
        let key = super::super::key::parse("px-AbCdEfGhIjKl").unwrap();
        for id in [AgentId::Codex, AgentId::GrokBuild] {
            let row = build(id, &key, &resolved("x-ai/grok-4.7"), &[]);
            let text = row.settings["config"].as_str().unwrap().to_string();
            let doc: DocumentMut = text.parse().unwrap();
            if id == AgentId::GrokBuild {
                assert_eq!(doc["model"]["x-ai/grok-4.7"]["api_backend"].as_str(), Some("responses"));
            }
        }
    }
}
