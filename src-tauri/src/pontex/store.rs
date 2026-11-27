//! PonteX 自己的状态（`~/.pontex-one/pontex-state.json`）。不进上游数据库，不占它的 schema 版本号。

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::agents::AgentId;
use super::baseline::Baseline;

const VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub version: u32,
    pub key_hint: Option<String>,
    pub connected_at: Option<i64>,
    /// 最近一次网关返回的模型列表，用来离线显示可选模型。
    pub models: Vec<super::key::ModelInfo>,
    pub gemini: Option<Vec<String>>,
    pub agents: BTreeMap<AgentId, AgentState>,
    pub prefs: Prefs,
    /// 已经替用户打开过一次开机启动。之后开关都听他的。
    pub autostart_set: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentState {
    /// 用户自己的原生状态（官方登录、官方 Key、Bedrock 等），最近一次离开它时的样子。
    pub native: Option<Baseline>,
    /// 用户原来用的别家网关。
    pub previous: Option<Baseline>,
    /// 第一次写之前是哪一种，断开时回到这里。
    pub origin: Option<Target>,
    /// 我们最后一次写完时各文件的哈希：没变过就能按字节还原。
    pub applied: BTreeMap<String, Option<String>>,
    /// 最后一次在这里选的线路。
    pub target: Option<Target>,
    pub verify: Option<Verify>,
    /// 用户选的主模型。
    pub model: Option<String>,
    /// 连接时还没装、配置已备好，等装好后应用。
    pub waiting: bool,
    /// 最近一次失败，成功后清掉。
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Target {
    Pontex,
    Native,
    Previous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verify {
    pub at: i64,
    pub ok: bool,
    pub reason: Option<String>,
    pub request_id: Option<String>,
    pub model: String,
    pub key_hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub language: Option<String>,
    pub close_to_tray: bool,
    pub coexist_notice_seen: bool,
    /// 新建的文件夹默认放在哪；没设就是个人文件夹里的 PonteX。
    pub folders_dir: Option<String>,
    /// 在文件夹里打开 Agent 时，先在访达 / 资源管理器里打开这个文件夹。
    pub open_folder: bool,
    /// 用哪个终端（`places::terminals()` 里的一个）；没设就用系统自带的。
    pub terminal: Option<String>,
    /// 第一次在文件夹里打开 Agent 之后不再显示说明。
    pub launched_once: bool,
    /// 看过「已读入各 Agent 里现有的 MCP」。
    pub toolbox_notice_seen: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            language: None,
            close_to_tray: true,
            coexist_notice_seen: false,
            folders_dir: None,
            open_folder: true,
            terminal: None,
            launched_once: false,
            toolbox_notice_seen: false,
        }
    }
}

pub fn path() -> PathBuf {
    super::upstream::device_dir().join("pontex-state.json")
}

impl Store {
    /// 文件坏了就改名留底、从空状态开始：基线副本还在 `baseline/` 目录里。
    pub fn load() -> Self {
        let file = path();
        let Ok(bytes) = std::fs::read(&file) else {
            return Self { version: VERSION, ..Self::default() };
        };
        match serde_json::from_slice::<Self>(&bytes) {
            Ok(store) if store.version <= VERSION => Self { version: VERSION, ..store },
            _ => {
                let kept = file.with_extension(format!("json.bad-{}", chrono::Utc::now().timestamp()));
                log::error!("pontex-state.json 读不出来，已改名保留");
                let _ = std::fs::rename(&file, kept);
                Self { version: VERSION, ..Self::default() }
            }
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        super::upstream::write_private(&path(), &bytes)
    }

    pub fn agent(&self, id: AgentId) -> AgentState {
        self.agents.get(&id).cloned().unwrap_or_default()
    }

    pub fn agent_mut(&mut self, id: AgentId) -> &mut AgentState {
        self.agents.entry(id).or_default()
    }
}
