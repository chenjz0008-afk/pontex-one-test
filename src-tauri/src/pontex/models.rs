//! 给每个 Agent 选模型：随包清单的候选 ∩ 这把 Key 能用的模型。

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::agents::AgentId;
use super::key::ModelInfo;

#[derive(Debug, Deserialize)]
struct Manifest {
    agents: BTreeMap<AgentId, AgentManifest>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Protocol {
    Messages,
    Responses,
    Gemini,
    Chat,
}

#[derive(Debug, Deserialize)]
pub struct AgentManifest {
    pub protocol: Protocol,
    roles: indexmap::IndexMap<String, Role>,
}

#[derive(Debug, Deserialize)]
struct Role {
    prefer: Vec<String>,
    family: Vec<String>,
    #[serde(default)]
    fallback: Option<String>,
    #[serde(default)]
    optional: bool,
}

pub fn manifest(id: AgentId) -> &'static AgentManifest {
    static MANIFEST: OnceLock<Manifest> = OnceLock::new();
    &MANIFEST
        .get_or_init(|| serde_json::from_str(include_str!("manifest.json")).expect("bundled manifest"))
        .agents[&id]
}

/// 角色 → 模型 id。一定有 `main`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolved {
    pub roles: BTreeMap<String, String>,
    /// 主模型的上下文长度，来自网关；没有就不写。
    pub context: Option<u64>,
}

impl Resolved {
    pub fn main(&self) -> &str {
        &self.roles["main"]
    }
    pub fn role(&self, name: &str) -> &str {
        self.roles.get(name).unwrap_or(&self.roles["main"])
    }
    /// 去重后的全部模型，主模型在最前。
    pub fn all(&self) -> Vec<String> {
        let mut out = vec![self.main().to_string()];
        for id in self.roles.values() {
            if !out.contains(id) {
                out.push(id.clone());
            }
        }
        out
    }
}

/// 这个 Agent 能用的文本模型。
pub fn usable<'a>(
    id: AgentId,
    models: &'a [ModelInfo],
    gemini: Option<&[String]>,
) -> Vec<&'a ModelInfo> {
    models
        .iter()
        .filter(|m| m.is_text())
        .filter(|m| match (manifest(id).protocol, gemini) {
            (Protocol::Gemini, Some(native)) => native.iter().any(|n| n == &m.id),
            _ => true,
        })
        .collect()
}

/// `models` 为 `None`：没拿到列表（没有额度），按清单第一候选写，状态会标「未验证」。
/// 返回 `None`：这把 Key 没有这个 Agent 能用的模型，不写配置。
pub fn resolve(
    id: AgentId,
    models: Option<&[ModelInfo]>,
    gemini: Option<&[String]>,
    chosen: Option<&str>,
) -> Option<Resolved> {
    let spec = manifest(id);
    let available = models.map(|list| usable(id, list, gemini));
    let mut roles = BTreeMap::new();
    for (name, role) in &spec.roles {
        let picked = match &available {
            Some(list) => pick(role, list),
            None => role.prefer.first().cloned(),
        };
        let picked = if name == "main" {
            let valid = chosen.filter(|id| {
                available
                    .as_ref()
                    .is_none_or(|list| list.iter().any(|m| m.id == *id))
            });
            valid.map(str::to_string).or(picked)
        } else {
            picked.or_else(|| {
                role.fallback
                    .as_ref()
                    .and_then(|from| roles.get(from).cloned())
            })
        };
        match picked {
            Some(model) => {
                roles.insert(name.clone(), model);
            }
            None if role.optional => {}
            None => return None,
        }
    }
    let context = models
        .into_iter()
        .flatten()
        .find(|m| Some(&m.id) == roles.get("main"))
        .and_then(|m| m.context_length);
    Some(Resolved { roles, context })
}

fn pick(role: &Role, list: &[&ModelInfo]) -> Option<String> {
    if let Some(found) = role.prefer.iter().find_map(|want| {
        list.iter()
            .find(|m| m.id == *want || m.aliases.iter().any(|a| a == want))
    }) {
        return Some(found.id.clone());
    }
    role.family.iter().find_map(|prefix| {
        list.iter()
            .map(|m| m.id.as_str())
            .filter(|id| id.starts_with(prefix.as_str()))
            .max_by(|a, b| by_version(a, b))
            .map(str::to_string)
    })
}

/// 版本高的在后；版本相同，名字短的在后（主版本优先于 `-mini`、`-preview` 这类变体）。
fn by_version(a: &str, b: &str) -> Ordering {
    let numbers = |id: &str| -> Vec<u32> {
        id.split(|c: char| !c.is_ascii_digit())
            .filter(|part| !part.is_empty() && part.len() < 6)
            .filter_map(|part| part.parse().ok())
            .collect()
    };
    numbers(a)
        .cmp(&numbers(b))
        .then_with(|| b.len().cmp(&a.len()))
        .then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(ids: &[&str]) -> Vec<ModelInfo> {
        ids.iter()
            .map(|id| ModelInfo {
                id: id.to_string(),
                aliases: vec![],
                modality: Some("text".into()),
                context_length: Some(200_000),
            })
            .collect()
    }

    #[test]
    fn every_agent_is_in_the_manifest() {
        for id in AgentId::ALL {
            assert!(manifest(id).roles.contains_key("main"), "{id:?}");
        }
    }

    #[test]
    fn claude_fills_all_five_roles_from_what_the_key_can_use() {
        let models = list(&["anthropic/claude-opus-5.5", "anthropic/claude-sonnet-5"]);
        let r = resolve(AgentId::ClaudeCode, Some(&models), None, None).unwrap();
        assert_eq!(r.main(), "anthropic/claude-opus-5.5");
        assert_eq!(r.role("sonnet"), "anthropic/claude-sonnet-5");
        assert_eq!(r.role("haiku"), "anthropic/claude-sonnet-5");
        assert_eq!(r.role("subagent"), "anthropic/claude-sonnet-5");
        assert_eq!(r.context, Some(200_000));
    }

    #[test]
    fn missing_family_writes_nothing() {
        let models = list(&["openai/gpt-6-luna"]);
        assert!(resolve(AgentId::ClaudeCode, Some(&models), None, None).is_none());
        assert!(resolve(AgentId::GrokBuild, Some(&models), None, None).is_none());
        assert!(resolve(AgentId::GrokBuild, None, None, None).is_none());
    }

    #[test]
    fn family_prefers_newer_then_plain_names() {
        let models = list(&["x-ai/grok-4", "x-ai/grok-4.7-mini", "x-ai/grok-4.7", "x-ai/grok-3"]);
        let r = resolve(AgentId::GrokBuild, Some(&models), None, None).unwrap();
        assert_eq!(r.main(), "x-ai/grok-4.7");
    }

    #[test]
    fn gemini_only_uses_natively_callable_models() {
        let models = list(&["google/gemini-3.5-pro", "google/gemini-3.5-flash"]);
        let native = vec!["google/gemini-3.5-flash".to_string()];
        let r = resolve(AgentId::GeminiCli, Some(&models), Some(&native), None).unwrap();
        assert_eq!(r.main(), "google/gemini-3.5-flash");
        assert!(resolve(AgentId::GeminiCli, Some(&models), Some(&[]), None).is_none());
    }

    #[test]
    fn user_choice_wins_only_while_the_key_still_allows_it() {
        let models = list(&["openai/gpt-6-luna", "openai/gpt-5.5"]);
        let pick = |c| resolve(AgentId::Codex, Some(&models), None, Some(c)).unwrap();
        assert_eq!(pick("openai/gpt-5.5").main(), "openai/gpt-5.5");
        assert_eq!(pick("openai/gone").main(), "openai/gpt-6-luna");
    }
}
