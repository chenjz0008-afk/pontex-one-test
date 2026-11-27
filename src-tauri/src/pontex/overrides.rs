//! 会盖过配置文件的环境变量。只报变量名和所在文件，不读出值。

use serde::Serialize;

use super::agents::AgentId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Override {
    pub name: String,
    pub source: String,
    /// 会让 PonteX 配置不生效；否则只是提醒。
    pub blocking: bool,
}

/// Gemini CLI 的 `.env` 不覆盖已有的环境变量，所以同名变量会挡住配置。Claude Code 以
/// `settings.json` 的 `env` 为准，Codex 和 Grok 的 Key 写在表里，环境变量优先级更低。
fn blocking(id: AgentId, name: &str) -> bool {
    id == AgentId::GeminiCli
        && matches!(
            name,
            "GEMINI_API_KEY" | "GOOGLE_GEMINI_BASE_URL" | "GEMINI_MODEL" | "GOOGLE_GENAI_USE_VERTEXAI"
        )
}

pub fn scan(id: AgentId) -> Vec<Override> {
    if id == AgentId::Workbuddy {
        return Vec::new();
    }
    let mut out: Vec<Override> = super::upstream::env_conflicts(id)
        .into_iter()
        .map(|(name, source)| Override {
            blocking: blocking(id, &name),
            name,
            source,
        })
        .collect();
    out.sort_by(|a, b| b.blocking.cmp(&a.blocking).then_with(|| a.name.cmp(&b.name)));
    out.dedup_by(|a, b| a.name == b.name && a.source == b.source);
    out
}
