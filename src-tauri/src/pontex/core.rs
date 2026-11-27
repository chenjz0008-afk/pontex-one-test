//! 连接、切换、换 Key、断开。每个 Agent 独立一步、独立结果。
//!
//! 写之前的固定顺序：读现场 → 该留底就留底 → **先把留底记录落盘** → 才调上游引擎写文件。

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::agents::{self, AgentId, Install};
use super::baseline;
use super::consts::{NATIVE_ID, PREVIOUS_ID, PROVIDER_ID};
use super::key::{KeyReport, Secret};
use super::live::{self, Route};
use super::models;
use super::overrides::{self, Override};
use super::status::{self, Status};
use super::store::{AgentState, Prefs, Store, Target, Verify};
use super::templates;
use super::upstream::{self, Engine};

pub struct Core {
    engine: Engine,
    store: Mutex<Store>,
    installs: RwLock<HashMap<AgentId, Install>>,
    overrides: RwLock<HashMap<AgentId, Vec<Override>>>,
    updates: Mutex<BTreeMap<AgentId, String>>,
    /// 连接、切换、断开一次只做一个。
    pub op: tokio::sync::Mutex<()>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub agent: AgentId,
    pub result: &'static str,
    pub error: Option<String>,
    /// Codex：切换后另一组会话会变成只读，值是受影响的会话数。
    pub locked_sessions: Option<usize>,
}

impl Outcome {
    fn new(agent: AgentId, result: &'static str) -> Self {
        Self { agent, result, error: None, locked_sessions: None }
    }
    fn failed(agent: AgentId, error: String) -> Self {
        Self { error: Some(error), ..Self::new(agent, "failed") }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentView {
    pub id: AgentId,
    pub label: &'static str,
    pub install: Install,
    pub route: Route,
    pub current: Option<Target>,
    pub switchable: bool,
    /// 用户原来的别家网关；有就多一格「原配置」。
    pub previous: Option<String>,
    pub status: Status,
    pub model: Option<String>,
    pub options: Vec<String>,
    pub overrides: Vec<Override>,
    pub verify: Option<Verify>,
    pub waiting: bool,
    pub connected: bool,
    /// 没装、而且我们能装。
    pub installable: bool,
    /// 自己动手时用的镜像安装命令。
    pub mirror: Option<String>,
    /// 由我们装着的版本。
    pub managed: Option<String>,
    /// 我们装的、有新版。
    pub update: Option<String>,
    /// Codex 桌面版：装着的版本。
    pub app: Option<String>,
    /// Codex 桌面版：这台电脑能装、还没装。
    pub app_installable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub version: String,
    pub engine: &'static str,
    pub connected: bool,
    pub key_hint: Option<String>,
    pub agents: Vec<AgentView>,
    pub prefs: Prefs,
    pub cc_switch_present: bool,
    pub data_dir: String,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Choice {
    pub agent: AgentId,
    pub apply: bool,
}

impl Core {
    pub fn new(engine: Engine) -> Self {
        Self {
            engine,
            store: Mutex::new(Store::load()),
            installs: RwLock::default(),
            overrides: RwLock::default(),
            updates: Mutex::default(),
            op: tokio::sync::Mutex::new(()),
        }
    }

    fn locked(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn store(&self) -> Store {
        self.locked().clone()
    }

    fn edit(&self, change: impl FnOnce(&mut Store)) -> Result<(), String> {
        let mut store = self.locked();
        let mut next = store.clone();
        change(&mut next);
        next.save()?;
        *store = next;
        Ok(())
    }

    pub fn prefs(&self) -> Prefs {
        self.locked().prefs.clone()
    }

    pub fn set_prefs(&self, prefs: Prefs) -> Result<(), String> {
        self.edit(|store| store.prefs = prefs)
    }

    // ---------- 检测 ----------

    pub fn update(&self, id: AgentId) -> Option<String> {
        self.updates.lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned()
    }

    pub fn set_update(&self, id: AgentId, version: Option<String>) {
        let mut updates = self.updates.lock().unwrap_or_else(|e| e.into_inner());
        match version {
            Some(version) => updates.insert(id, version),
            None => updates.remove(&id),
        };
    }

    /// 第一次打开时为真，只真一次。
    pub fn first_autostart(&self) -> bool {
        !self.locked().autostart_set && self.edit(|store| store.autostart_set = true).is_ok()
    }

    /// 会启动子进程，只在后台线程调用。返回有没有变化。
    pub fn detect_all(&self) -> bool {
        let found: Vec<(AgentId, Install, Vec<Override>)> = std::thread::scope(|scope| {
            let jobs: Vec<_> = AgentId::ALL
                .into_iter()
                .map(|id| scope.spawn(move || (id, agents::detect(id), overrides::scan(id))))
                .collect();
            jobs.into_iter().filter_map(|job| job.join().ok()).collect()
        });
        let mut changed = false;
        for (id, install, list) in found {
            changed |= self.set_install(id, install);
            let mut all = self.overrides.write().unwrap_or_else(|e| e.into_inner());
            changed |= all.insert(id, list.clone()).as_ref() != Some(&list);
        }
        changed
    }

    pub fn set_install(&self, id: AgentId, install: Install) -> bool {
        let mut installs = self.installs.write().unwrap_or_else(|e| e.into_inner());
        installs.insert(id, install.clone()).as_ref() != Some(&install)
    }

    pub fn install(&self, id: AgentId) -> Install {
        self.installs
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
            .unwrap_or(Install::Checking)
    }

    // ---------- 读 ----------

    pub fn snapshot(&self) -> Snapshot {
        let store = self.store();
        let connected = store.key_hint.is_some();
        let views = AgentId::ALL
            .into_iter()
            .map(|id| self.view(id, &store))
            .collect();
        Snapshot {
            version: String::new(),
            engine: upstream::VERSION,
            connected,
            key_hint: store.key_hint.clone(),
            agents: views,
            prefs: store.prefs,
            cc_switch_present: upstream::home()
                .join(".cc-switch/cc-switch.db")
                .exists(),
            data_dir: upstream::device_dir().display().to_string(),
        }
    }

    fn view(&self, id: AgentId, store: &Store) -> AgentView {
        let state = store.agent(id);
        let install = self.install(id);
        let app = (id == AgentId::Codex).then(super::agents::app_version).flatten();
        let route = live::read(id);
        let found = self
            .overrides
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
            .unwrap_or_default();
        let status = status::derive(&status::Facts {
            id,
            install: &install,
            route: &route,
            state: &state,
            overrides: &found,
            key_hint: store.key_hint.as_deref(),
        });
        let previous = match (&state.previous, &route) {
            (_, Route::Other { name }) => Some(name.clone()),
            (Some(kept), _) => match &kept.route {
                Route::Other { name } => Some(name.clone()),
                _ => None,
            },
            _ => None,
        };
        let model = match &route {
            Route::Pontex { model, .. } => model.clone(),
            _ => state.model.clone(),
        };
        let options = models::usable(id, &store.models, store.gemini.as_deref())
            .into_iter()
            .map(|m| m.id.clone())
            .collect();
        AgentView {
            id,
            label: id.label(),
            current: status::current(&route),
            switchable: id.switchable(),
            previous,
            status,
            model,
            options,
            overrides: found,
            verify: state.verify,
            waiting: state.waiting,
            connected: self.engine.row_exists(id, PROVIDER_ID),
            installable: install == Install::Missing && super::installer::supported(id),
            mirror: super::installer::spec::spec(id).map(|s| s.mirror_command()),
            managed: super::installer::layout::Layout::new(upstream::device_dir()).managed(id),
            update: self.update(id),
            app: app.clone(),
            app_installable: app.is_none() && id == AgentId::Codex && super::installer::app_supported(),
            install,
            route,
        }
    }

    /// 已连接的 Key，从 PonteX 行里取。
    pub fn stored_key(&self) -> Option<Secret> {
        AgentId::ALL.into_iter().find_map(|id| {
            let s = self.engine.row_settings(id, PROVIDER_ID)?;
            let text = |pointer: &str| s.pointer(pointer).and_then(Value::as_str).map(str::to_string);
            let key = match id {
                AgentId::ClaudeCode => text("/env/ANTHROPIC_AUTH_TOKEN"),
                AgentId::Codex => text("/auth/OPENAI_API_KEY"),
                AgentId::GeminiCli => text("/env/GEMINI_API_KEY"),
                AgentId::GrokBuild => text("/config").and_then(|config| {
                    let doc: toml_edit::DocumentMut = config.parse().ok()?;
                    let profiles = doc.get("model")?.as_table_like()?;
                    let key = profiles.iter().next()?.1.get("api_key")?.as_str()?;
                    Some(key.to_string())
                }),
                AgentId::Workbuddy => text("/apiKey"),
            }?;
            super::key::parse(&key).ok()
        })
    }

    // ---------- 写 ----------

    /// 写之前留底。现场是我们自己上次写下的就不用再留。
    fn keep_user_state(&self, id: AgentId) -> Result<(), String> {
        let route = live::read(id);
        if let Route::Unreadable { file } = route {
            return Err(format!("UNREADABLE:{file}"));
        }
        let Some(slot) = baseline::slot_of(&route) else {
            return Ok(());
        };
        let state = self.store().agent(id);
        let kept = match slot {
            Target::Previous => &state.previous,
            _ => &state.native,
        };
        if kept.is_some() && self.unchanged_files(id, &state).len() == live::managed_files(id).len() {
            return Ok(());
        }
        let taken = baseline::capture(&self.engine, id, slot, route)?;
        self.edit(|store| {
            let agent = store.agent_mut(id);
            agent.origin.get_or_insert(slot);
            match slot {
                Target::Previous => agent.previous = Some(taken),
                _ => agent.native = Some(taken),
            }
        })
    }

    /// 我们写完之后没人动过的文件。
    fn unchanged_files(&self, id: AgentId, state: &AgentState) -> Vec<PathBuf> {
        live::managed_files(id)
            .into_iter()
            .filter(|path| {
                state
                    .applied
                    .get(&path.display().to_string())
                    .is_some_and(|recorded| *recorded == live::digest(path))
            })
            .collect()
    }

    fn record(&self, id: AgentId, target: Option<Target>) -> Result<(), String> {
        let applied: BTreeMap<String, Option<String>> = live::managed_files(id)
            .into_iter()
            .map(|path| (path.display().to_string(), live::digest(&path)))
            .collect();
        self.edit(|store| {
            let agent = store.agent_mut(id);
            agent.applied = applied;
            agent.target = target;
            agent.error = None;
            agent.waiting = false;
        })
    }

    /// 建或更新 PonteX 行，不碰任何 Agent 的文件。
    fn stage(&self, id: AgentId, key: &Secret, resolved: &models::Resolved) -> Result<(), String> {
        let store = self.store();
        let mut row = templates::build(id, key, resolved, &store.models);
        if id == AgentId::Workbuddy {
            drop_taken_models(&mut row.settings)?;
        }
        self.engine.put_row(id, row)
    }

    /// 单独还原过的 Agent 没有 PonteX 行了，用已连接的 Key 重新备一份。
    fn restage(&self, id: AgentId) -> Result<(), String> {
        let key = self.stored_key().ok_or("NOT_CONNECTED")?;
        let store = self.store();
        let list = (!store.models.is_empty()).then_some(store.models.as_slice());
        let chosen = store.agent(id).model;
        let resolved = models::resolve(id, list, store.gemini.as_deref(), chosen.as_deref())
            .ok_or("NO_MODELS")?;
        self.stage(id, &key, &resolved)
    }

    fn write_pontex(&self, id: AgentId) -> Result<Option<usize>, String> {
        if !self.install(id).usable() {
            return Err("NOT_INSTALLED".into());
        }
        if !self.engine.row_exists(id, PROVIDER_ID) {
            self.restage(id)?;
        }
        self.keep_user_state(id)?;
        let locked = self.engine.switch(id, PROVIDER_ID)?;
        if !live::read(id).is_pontex() {
            return Err("NOT_EFFECTIVE".into());
        }
        self.record(id, Some(Target::Pontex))?;
        Ok(locked)
    }

    /// 回到用户自己的配置。`purge` 见 [`baseline::restore_key_fields`]。
    fn write_user(&self, id: AgentId, target: Target, purge: bool) -> Result<Option<usize>, String> {
        let official = upstream::official_row(id).ok_or("NOT_SWITCHABLE")?;
        self.keep_user_state(id)?;
        let state = self.store().agent(id);
        let (kept, row) = match target {
            Target::Previous => (state.previous, PREVIOUS_ID),
            _ => (state.native, NATIVE_ID),
        };
        if target == Target::Previous && kept.is_none() {
            return Err("NO_PREVIOUS".into());
        }
        let row = match &kept {
            Some(kept) if kept.has_row => row,
            _ => official,
        };
        let locked = self.engine.switch(id, row)?;
        if let Some(kept) = &kept {
            baseline::restore_key_fields(&self.engine, id, kept, purge)?;
        }
        if live::read(id).is_pontex() {
            return Err("NOT_EFFECTIVE".into());
        }
        self.record(id, Some(target))?;
        Ok(locked)
    }

    fn finish(&self, id: AgentId, result: &'static str, done: Result<Option<usize>, String>) -> Outcome {
        match done {
            Ok(locked_sessions) => Outcome { locked_sessions, ..Outcome::new(id, result) },
            Err(error) => {
                log::warn!("[PONTEX] {} {result} 失败: {error}", id.key());
                let _ = self.edit(|store| store.agent_mut(id).error = Some(error.clone()));
                Outcome::failed(id, error)
            }
        }
    }

    /// 首次连接或换 Key。调用方已经验证过 Key。
    ///
    /// - `choices` 里勾选的：写成 PonteX；
    /// - 没勾选、但现在就走 PonteX 的：换成新 Key；
    /// - 其余：只把配置备好，不碰文件。
    pub fn connect(&self, key: &Secret, report: &KeyReport, choices: &[Choice]) -> Vec<Outcome> {
        if report.listed {
            let saved = self.edit(|store| {
                store.models = report.models.clone();
                store.gemini = report.gemini.clone();
            });
            if let Err(error) = saved {
                return AgentId::ALL.map(|id| Outcome::failed(id, error.clone())).to_vec();
            }
        }
        let list = report.listed.then_some(report.models.as_slice());
        let mut outcomes = Vec::new();
        for id in AgentId::ALL {
            let wanted = choices.iter().any(|c| c.agent == id && c.apply);
            let state = self.store().agent(id);
            let Some(resolved) =
                models::resolve(id, list, report.gemini.as_deref(), state.model.as_deref())
            else {
                outcomes.push(Outcome::new(id, "no-model"));
                continue;
            };
            if let Err(error) = self.stage(id, key, &resolved) {
                outcomes.push(self.finish(id, "stage", Err(error)));
                continue;
            }
            let usable = self.install(id).usable();
            let in_use = live::read(id).is_pontex();
            outcomes.push(if usable && (wanted || in_use) {
                self.finish(id, "applied", self.write_pontex(id))
            } else {
                let _ = self.edit(|store| {
                    let agent = store.agent_mut(id);
                    // 换 Key 时不带勾选：原来在等的继续等。
                    agent.waiting = !usable && (wanted || agent.waiting);
                    agent.error = None;
                });
                Outcome::new(id, if usable { "staged" } else { "waiting" })
            });
        }
        if outcomes.iter().any(|o| matches!(o.result, "applied" | "staged" | "waiting")) {
            let hint = key.hint();
            let _ = self.edit(|store| {
                store.key_hint = Some(hint);
                store.connected_at = Some(chrono::Utc::now().timestamp());
            });
        }
        outcomes
    }

    pub fn switch(&self, id: AgentId, target: Target) -> Outcome {
        match target {
            Target::Pontex => self.finish(id, "applied", self.write_pontex(id)),
            other => self.finish(id, "restored", self.write_user(id, other, false)),
        }
    }

    /// 换主模型：更新那一行；正在走 PonteX 才写文件。
    pub fn set_model(&self, id: AgentId, model: &str) -> Outcome {
        let Some(key) = self.stored_key() else {
            return Outcome::failed(id, "NOT_CONNECTED".into());
        };
        let store = self.store();
        let list = (!store.models.is_empty()).then_some(store.models.as_slice());
        let Some(resolved) = models::resolve(id, list, store.gemini.as_deref(), Some(model))
            .filter(|r| r.main() == model)
        else {
            return Outcome::failed(id, "MODEL_NOT_ALLOWED".into());
        };
        let done = self
            .edit(|store| store.agent_mut(id).model = Some(model.to_string()))
            .and_then(|()| self.stage(id, &key, &resolved))
            .and_then(|()| {
                if live::read(id).is_pontex() {
                    self.write_pontex(id)
                } else {
                    Ok(None)
                }
            });
        self.finish(id, "applied", done)
    }

    /// 记下「装好就接入」：连接时勾选的，或在这里点了安装的。
    pub fn expect(&self, id: AgentId) {
        if self.locked().key_hint.is_none() {
            return;
        }
        if let Err(error) = self.edit(|store| store.agent_mut(id).waiting = true) {
            log::warn!("[PONTEX] 记录待接入失败: {error}");
        }
    }

    /// 装好就接入，原来的配置先留底。只自动做一次：之后用户切到哪里、失败不失败，都不再替他动。
    pub fn apply_waiting(&self) -> Vec<Outcome> {
        let store = self.store();
        AgentId::ALL
            .into_iter()
            .filter(|id| store.agent(*id).waiting && self.install(*id).usable())
            .map(|id| {
                let outcome = self.finish(id, "applied", self.write_pontex(id));
                if outcome.error.is_some() {
                    let _ = self.edit(|store| store.agent_mut(id).waiting = false);
                }
                outcome
            })
            .collect()
    }

    /// 现在要验证的：走 PonteX、用的是已连接的 Key。
    pub fn verify_target(&self, id: AgentId) -> Option<(Secret, String)> {
        let key = self.stored_key()?;
        match live::read(id) {
            Route::Pontex { model: Some(model), key_hint } if key_hint == Some(key.hint()) => {
                Some((key, model))
            }
            _ => None,
        }
    }

    pub fn set_verify(&self, id: AgentId, verify: Verify) {
        let _ = self.edit(|store| store.agent_mut(id).verify = Some(verify));
    }

    /// 断开并还原。`id` 为空是全部；全部断开时从本机删掉 Key。
    pub fn disconnect(&self, only: Option<AgentId>) -> Vec<Outcome> {
        let targets: Vec<AgentId> = AgentId::ALL
            .into_iter()
            .filter(|id| only.is_none_or(|one| one == *id))
            .collect();
        let mut outcomes = Vec::new();
        for id in targets {
            let done = self.disconnect_one(id);
            outcomes.push(self.finish(id, "restored", done.map(|()| None)));
        }
        // 一个个还原到最后一个，和全部断开是一回事。
        let last = !AgentId::ALL.into_iter().any(|id| self.engine.row_exists(id, PROVIDER_ID));
        if last && outcomes.iter().all(|o| o.error.is_none()) {
            let _ = self.edit(|store| {
                store.key_hint = None;
                store.connected_at = None;
                store.models.clear();
                store.gemini = None;
            });
            forget_backups();
        }
        outcomes
    }

    fn disconnect_one(&self, id: AgentId) -> Result<(), String> {
        let state = self.store().agent(id);
        let route = live::read(id);
        if id == AgentId::Workbuddy {
            self.engine.workbuddy_remove(PROVIDER_ID)?;
        } else if route.is_pontex() {
            let origin = state.origin.unwrap_or(Target::Native);
            // 我们写完之后没人动过的文件，字段级还原完再按字节盖回去。Claude 例外：开着的会话
            // 要靠引擎先写空值再删，整份盖回去它们就收不到。
            let untouched = match state.target {
                Some(Target::Pontex) if id != AgentId::ClaudeCode => self.unchanged_files(id, &state),
                _ => Vec::new(),
            };
            self.write_user(id, origin, true)?;
            let kept = match origin {
                Target::Previous => state.previous.as_ref(),
                _ => state.native.as_ref(),
            };
            if let Some(kept) = kept {
                baseline::restore_bytes(&self.engine, id, kept, &untouched)?;
                self.record(id, Some(origin))?;
            }
        }
        self.engine.delete_row(id, PROVIDER_ID)?;
        self.edit(|store| {
            let agent = store.agent_mut(id);
            agent.verify = None;
            agent.waiting = false;
            agent.error = None;
            agent.target = None;
        })
    }
}

/// WorkBuddy 里同一个模型 id 只能有一条，别人的不覆盖。
fn drop_taken_models(settings: &mut Value) -> Result<(), String> {
    let taken: Vec<String> = upstream::workbuddy_owners()?
        .into_iter()
        .filter(|(_, owner)| owner.as_deref() != Some(PROVIDER_ID))
        .map(|(id, _)| id)
        .collect();
    let Some(list) = settings.get_mut("models").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    list.retain(|m| {
        m.get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| !taken.iter().any(|t| t == id))
    });
    if list.is_empty() {
        return Err("WORKBUDDY_MODELS_TAKEN".into());
    }
    Ok(())
}

/// 数据库升级前的备份里也有 Key。
fn forget_backups() {
    let dir = upstream::backups_dir();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "db") {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub type Shared = Arc<Core>;
