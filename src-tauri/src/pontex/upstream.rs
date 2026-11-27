//! 防腐层：整个 `pontex` 模块里**唯一**允许引用上游代码的文件（`architecture` 测试强制）。
//!
//! 上游改了签名、挪了位置，只在这里对齐；其余代码面对的是这里给出的稳定接口。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use super::agents::AgentId;
use crate::app_config::AppType;
use crate::live::engine::LiveFile;
use crate::live::patch::json::JsonPatch;
use crate::live::patch::toml::TomlDocPatch;
use crate::live::patch::{KeyPath, LivePatch, LiveWriteError, WholeFile};
use crate::mode::operation::{AppWrite, FileChange};
use crate::mode::state::{op, PendingTarget};
use crate::provider::Provider;
use crate::services::ProviderService;
use crate::store::AppState;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const WORKBUDDY_OWNER: &str = crate::workbuddy_config::OWNER_FIELD;

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

// ---------- 身份 ----------

fn app(id: AgentId) -> AppType {
    match id {
        AgentId::ClaudeCode => AppType::Claude,
        AgentId::Codex => AppType::Codex,
        AgentId::GeminiCli => AppType::Gemini,
        AgentId::GrokBuild => AppType::GrokBuild,
        AgentId::Workbuddy => AppType::WorkBuddy,
    }
}

/// 上游官方卡的 id。WorkBuddy 是累加式，没有「当前」。
pub fn official_row(id: AgentId) -> Option<&'static str> {
    match id {
        AgentId::ClaudeCode => Some("claude-official"),
        AgentId::Codex => Some("codex-official"),
        AgentId::GeminiCli => Some("gemini-official"),
        AgentId::GrokBuild => Some("grokbuild-official"),
        AgentId::Workbuddy => None,
    }
}

/// 本机装的版本；没装或跑不起来是 `None`。会启动子进程。
pub fn tool_version(id: AgentId) -> Option<String> {
    crate::commands::local_tool_version(tool_name(id)?)
}

/// 可执行文件名，也是上游工具表里的 id（Grok 是 `grok`，不是 `grokbuild`）。
pub fn tool_name(id: AgentId) -> Option<&'static str> {
    match id {
        AgentId::ClaudeCode => Some("claude"),
        AgentId::Codex => Some("codex"),
        AgentId::GeminiCli => Some("gemini"),
        AgentId::GrokBuild => Some("grok"),
        AgentId::Workbuddy => None,
    }
}

/// shell 配置里的相关环境变量：（变量名，所在文件）。不返回值。
pub fn env_conflicts(id: AgentId) -> Vec<(String, String)> {
    crate::services::env_checker::check_env_conflicts(app(id).as_str())
        .unwrap_or_default()
        .into_iter()
        .map(|c| (c.var_name, c.source_path))
        .collect()
}

pub async fn open_workbuddy() -> Result<(), String> {
    crate::commands::workbuddy_account_open_app().await
}

// ---------- 路径 ----------

pub fn home() -> PathBuf {
    crate::config::get_home_dir()
}
pub fn device_dir() -> PathBuf {
    crate::config::device_dir()
}
pub fn backups_dir() -> PathBuf {
    crate::config::get_app_config_dir().join("backups")
}
pub fn claude_settings() -> PathBuf {
    crate::config::get_claude_settings_path()
}
pub fn codex_config() -> PathBuf {
    crate::codex_config::get_codex_config_path()
}
pub fn codex_auth() -> PathBuf {
    crate::codex_config::get_codex_auth_path()
}
pub fn gemini_env_file() -> PathBuf {
    crate::gemini_config::get_gemini_env_path()
}
pub fn gemini_settings() -> PathBuf {
    crate::gemini_config::get_gemini_settings_path()
}
pub fn grok_config() -> PathBuf {
    crate::grok_config::get_grok_config_path()
}
pub fn workbuddy_models() -> PathBuf {
    crate::workbuddy_config::config_path()
}
pub fn skills_dir(id: AgentId) -> Result<PathBuf, String> {
    crate::services::SkillService::get_app_skills_dir(&app(id)).map_err(err)
}

pub fn write_private(path: &Path, data: &[u8]) -> Result<(), String> {
    crate::config::atomic_write_private(path, data).map_err(err)
}

/// 跟随用户的代理设置，不跟随跳转。
pub fn http(url: &str) -> Result<reqwest::Client, String> {
    crate::proxy::http_client::no_redirect_for_url(url)
}

/// 只走 https 的客户端，会跟随跳转。下载内容另有签名和哈希校验，可以跟着镜像跳到 CDN。
pub fn https() -> Result<reqwest::Client, String> {
    crate::proxy::http_client::https_only()
}

// ---------- 读 ----------

pub fn gemini_env() -> Result<HashMap<String, String>, String> {
    crate::gemini_config::read_gemini_env().map_err(err)
}

/// WorkBuddy 已有的模型：（模型 id，归属）。归属为空是用户自己加的。
pub fn workbuddy_owners() -> Result<Vec<(String, Option<String>)>, String> {
    Ok(crate::workbuddy_config::model_owners()
        .map_err(err)?
        .into_iter()
        .map(|m| (m.id, m.owner))
        .collect())
}

/// 「关键字段」：请求发到哪、凭什么鉴权、哪个模型、哪种协议。清单由上游维护。
pub mod key_fields {
    use crate::live::floor;

    pub fn claude_top(key: &str) -> bool {
        floor::claude_floor_top(key)
    }
    pub fn claude_env(key: &str) -> bool {
        floor::claude_floor_env(key) || floor::claude_exclusive_env(key)
    }
    pub fn gemini_env(key: &str) -> bool {
        floor::gemini_floor_env(key)
    }
    pub const CODEX_TOP: &[&str] = floor::CODEX_FLOOR_TOP;
    pub const GEMINI_SETTINGS: &[&[&str]] = floor::GEMINI_FLOOR_SETTINGS;
}

// ---------- 引擎 ----------

/// 上游引擎：供应商行、写配置、崩溃恢复。
pub struct Engine {
    state: AppState,
}

/// 一行供应商里我们用得到的部分。
pub struct Row {
    pub id: String,
    pub name: String,
    pub category: String,
    pub settings: Value,
}

impl Engine {
    /// 只做写配置需要的几步，不跑上游的导入、迁移、同步、路由服务。
    pub fn open() -> Result<Self, String> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut settings = crate::settings::get_settings();
        if !settings.preserve_codex_official_auth_on_switch {
            // Codex 的官方登录不归我们管：切到 PonteX 也原样留在 auth.json。
            settings.preserve_codex_official_auth_on_switch = true;
            crate::settings::update_settings(settings).map_err(err)?;
        }
        let db = Arc::new(crate::database::Database::init().map_err(err)?);
        let state = AppState::new(db);
        // 上次写到一半的客户端文件先补完，再做别的。
        crate::mode::operation::recover_on_startup(&state.db);
        crate::services::provider::claude_direct::resume_tombstone_sweep();
        state.db.init_default_official_providers().map_err(err)?;
        Ok(Self { state })
    }

    /// 数据库是更新的版本写的：先于任何写操作检查，返回（库的版本，支持的版本）。
    pub fn too_new() -> Option<(i32, i32)> {
        let path = crate::config::get_app_config_dir().join("cc-switch.db");
        crate::database::Database::stored_user_version_exceeds_supported(&path)
            .ok()
            .flatten()
            .map(|found| (found, crate::database::SCHEMA_VERSION))
    }

    pub fn row_exists(&self, id: AgentId, row: &str) -> bool {
        self.state.db.provider_exists(row, &app(id)).unwrap_or(false)
    }

    pub fn row_settings(&self, id: AgentId, row: &str) -> Option<Value> {
        self.state
            .db
            .get_provider_by_id(row, &app(id))
            .ok()?
            .map(|provider| provider.settings_config)
    }

    /// 建或更新一行，**不碰** Agent 的文件。
    pub fn put_row(&self, id: AgentId, row: Row) -> Result<(), String> {
        let app = app(id);
        let existing = self.state.db.get_provider_by_id(&row.id, &app).map_err(err)?;
        let fresh = existing.is_none();
        let mut provider: Provider = serde_json::from_value(serde_json::json!({
            "id": row.id,
            "name": row.name,
            "category": row.category,
            "websiteUrl": "https://docs.ponte-x.com",
            "settingsConfig": row.settings,
        }))
        .map_err(err)?;
        if let Some(existing) = existing {
            provider.meta = existing.meta;
            provider.created_at = existing.created_at;
            provider.sort_index = existing.sort_index;
        }
        if fresh && official_row(id).is_none() {
            // 累加式应用要带上「没写进文件」的标记。
            return ProviderService::add_without_switching(&self.state, app, provider)
                .map(|_| ())
                .map_err(err);
        }
        self.state.db.save_provider(&app, &provider).map_err(err)
    }

    pub fn delete_row(&self, id: AgentId, row: &str) -> Result<(), String> {
        self.state.db.delete_provider(&app(id), row).map_err(err)
    }

    /// 切到这一行并写文件。返回 Codex 因此变成只读的会话数。
    pub fn switch(&self, id: AgentId, row: &str) -> Result<Option<usize>, String> {
        let before = (id == AgentId::Codex)
            .then(crate::codex_history_migration::codex_live_history_bucket)
            .flatten();
        ProviderService::switch(&self.state, app(id), row).map_err(err)?;
        Ok((id == AgentId::Codex)
            .then(|| crate::codex_history_migration::codex_history_split_hint(before.as_deref()))
            .flatten()
            .map(|hint| hint.locked_threads))
    }

    pub fn workbuddy_remove(&self, row: &str) -> Result<(), String> {
        crate::workbuddy_config::remove_provider(row).map_err(err)
    }

    /// 用上游的写锁、写前意图、崩溃恢复写一组文件。
    pub fn write(&self, id: AgentId, edits: &[Edit]) -> Result<(), String> {
        if edits.is_empty() {
            return Ok(());
        }
        let app = app(id);
        let session = AppWrite::begin(self.state.db.as_ref(), app.as_str()).map_err(err)?;
        let patches: Vec<(LiveFile, Box<dyn LivePatch + '_>)> =
            edits.iter().map(Edit::patch).collect();
        let changes: Vec<FileChange<'_>> = patches
            .iter()
            .map(|(file, patch)| FileChange {
                file: file.clone(),
                patch: patch.as_ref(),
            })
            .collect();
        session
            .run(op::APPLY, &changes, PendingTarget::default())
            .map(|_| ())
            .map_err(err)
    }
}

// ---------- MCP（照搬 CC Switch 的统一 MCP） ----------

/// 一个 MCP 服务器，和它在哪些 Agent 里启用。
#[derive(Debug, Clone, serde::Serialize)]
pub struct Mcp {
    pub id: String,
    pub name: String,
    /// 原样的服务器定义：`{type, command, args, env}` 或 `{type, url, headers}`。
    pub server: Value,
    pub on: Vec<AgentId>,
}

impl Engine {
    /// 把各 Agent 里现有的 MCP 读进清单（只读，不改它们的配置），返回新读进来的个数。
    pub fn mcp_import(&self) -> Result<usize, String> {
        crate::services::McpService::import_from_all_apps(&self.state).map_err(err)
    }

    pub fn mcp_list(&self) -> Result<Vec<Mcp>, String> {
        let all = crate::services::McpService::get_all_servers(&self.state).map_err(err)?;
        Ok(all
            .into_values()
            .map(|s| Mcp {
                on: AgentId::ALL.into_iter().filter(|id| s.apps.is_enabled_for(&app(*id))).collect(),
                id: s.id,
                name: s.name,
                server: s.server,
            })
            .collect())
    }

    /// 在一个 Agent 里启用或停用：写进 / 移出它的配置文件。
    pub fn mcp_toggle(&self, id: &str, agent: AgentId, enabled: bool) -> Result<(), String> {
        crate::services::McpService::toggle_app(&self.state, id, app(agent), enabled).map_err(err)
    }

    /// 新增或修改；`on` 里的 Agent 写进去，其余的移出来。返回 id。
    pub fn mcp_save(&self, id: Option<String>, name: &str, server: Value, on: &[AgentId]) -> Result<String, String> {
        let existing = crate::services::McpService::get_all_servers(&self.state).map_err(err)?;
        let id = match id {
            Some(id) => id,
            None => {
                let base: String = name
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '-' })
                    .collect();
                let base = if base.trim_matches('-').is_empty() { "mcp".to_string() } else { base };
                (0..)
                    .map(|n| if n == 0 { base.clone() } else { format!("{base}-{n}") })
                    .find(|candidate| !existing.contains_key(candidate))
                    .unwrap_or(base)
            }
        };
        let mut server_row = existing.get(&id).cloned().unwrap_or(crate::app_config::McpServer {
            id: id.clone(),
            name: name.to_string(),
            server: Value::Null,
            apps: Default::default(),
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        });
        server_row.name = name.to_string();
        server_row.server = server;
        for agent in AgentId::ALL {
            server_row.apps.set_enabled_for(&app(agent), on.contains(&agent));
        }
        crate::services::McpService::upsert_server(&self.state, server_row).map_err(err)?;
        Ok(id)
    }

    /// 从清单和所有 Agent 的配置里删掉。
    pub fn mcp_delete(&self, id: &str) -> Result<(), String> {
        crate::services::McpService::delete_server(&self.state, id).map(|_| ()).map_err(err)
    }

    pub fn mcp_server(&self, id: &str) -> Option<Value> {
        crate::services::McpService::get_all_servers(&self.state)
            .ok()?
            .get(id)
            .map(|s| s.server.clone())
    }
}

/// 试着连一下每个 MCP：能用、要先登录、连不上。
pub async fn mcp_check(servers: Vec<(String, Value)>) -> Vec<(String, &'static str)> {
    use crate::services::mcp_connectivity::{start_checks, ConnectivityStatus};
    let total = servers.len();
    let rows = servers
        .into_iter()
        .map(|(id, server)| crate::app_config::McpServer {
            id: id.clone(),
            name: id,
            server,
            apps: Default::default(),
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        })
        .collect();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    start_checks(rows, move |result| tx.send(result).is_ok());
    let mut out = Vec::with_capacity(total);
    while out.len() < total {
        let Some(result) = rx.recv().await else { break };
        let status = match result.status {
            ConnectivityStatus::Connected => "ok",
            ConnectivityStatus::AuthRequired => "login",
            ConnectivityStatus::Failed => "failed",
        };
        out.push((result.server_id, status));
    }
    out
}

/// 退出前把引擎安排的延迟清理（Claude 的空值）做完。
pub fn settle() {
    if let Err(error) = crate::services::provider::claude_direct::settle_claude_tombstones() {
        log::warn!("[PONTEX] 清理 Claude 空值失败，下次启动继续: {error}");
    }
}

type TomlEdit = dyn Fn(&mut toml_edit::DocumentMut) + Send + Sync;

/// 对一个文件的改动。
pub enum Edit {
    /// 整份写入。
    Write(PathBuf, Vec<u8>),
    Delete(PathBuf),
    /// 在现有文档上改，其余字节不动；文件解析不了就报错、不写。
    Toml(PathBuf, Box<TomlEdit>),
    /// 按键路径设值或删除，保留注释和顺序。
    Json(PathBuf, Vec<(Vec<String>, Option<Value>)>),
}

struct TomlFn<'a>(&'a TomlEdit);

impl TomlDocPatch for TomlFn<'_> {
    fn apply_to(
        &self,
        _path: &Path,
        doc: &mut toml_edit::DocumentMut,
    ) -> Result<(), LiveWriteError> {
        (self.0)(doc);
        Ok(())
    }
}

impl Edit {
    fn patch(&self) -> (LiveFile, Box<dyn LivePatch + '_>) {
        match self {
            Edit::Write(path, bytes) => (
                LiveFile::private(path),
                Box::new(WholeFile::Write(bytes.clone())),
            ),
            Edit::Delete(path) => (LiveFile::private(path), Box::new(WholeFile::Delete)),
            Edit::Toml(path, change) => (LiveFile::private(path), Box::new(TomlFn(change.as_ref()))),
            Edit::Json(path, values) => {
                let mut patch = JsonPatch::default();
                for (keys, value) in values {
                    let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
                    let key_path = KeyPath::new(&keys);
                    match value {
                        Some(value) => patch.set.push((key_path, value.clone())),
                        None => patch.remove.push(key_path),
                    }
                }
                (LiveFile::shared(path), Box::new(patch))
            }
        }
    }
}
