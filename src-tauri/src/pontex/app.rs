//! 入口、命令、窗口。界面是一份快照的投影：命令改状态，状态变了广播 `px://state`。

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

use super::agents::AgentId;
use super::consts::{api_origin, API_ORIGIN, SCHEME};
use super::core::{Choice, Core, Outcome, Snapshot};
use super::installer::{self, path as install_path, Installer, Phase, Planned, Progress};
use super::key::{self, KeyReport};
use super::links::{self, Link};
use super::skills::{self, Installed, Preview, Ready};
use super::store::{Prefs, Target, Verify};
use super::upstream::{self, Engine};
use super::{autostart, diagnose, places, toolbox, tray, verify};

const EVENT_STATE: &str = "px://state";
const EVENT_SKILL: &str = "px://skill";
const EVENT_INSTALL: &str = "px://install";
/// 多久自己检查一次装没装、配置有没有被改。只读本机，不联网。
const REFRESH: Duration = Duration::from_secs(600);
/// 问一次「我们装的有没有新版」。
const UPDATE_CHECK: Duration = Duration::from_secs(6 * 3600);

pub struct Runtime {
    core: RwLock<Result<Arc<Core>, StartupError>>,
    pending_skill: Mutex<Option<String>>,
    ready_skill: Mutex<Option<Ready>>,
    last_detect: Mutex<Option<Instant>>,
    last_update_check: Mutex<Option<Instant>>,
    jobs: Jobs,
}

/// 安装一次只做一个，其余排队。
#[derive(Default)]
struct Jobs {
    queue: Mutex<VecDeque<AgentId>>,
    running: AtomicBool,
    cancel: Arc<AtomicBool>,
    current: Mutex<Option<AgentId>>,
    progress: Mutex<BTreeMap<AgentId, Progress>>,
    /// 这几个要装桌面版（目前只有 Codex）。
    app: Mutex<BTreeSet<AgentId>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StartupError {
    code: &'static str,
    detail: String,
}

impl Runtime {
    fn started(&self) -> Result<Arc<Core>, StartupError> {
        self.core.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub(crate) fn core(&self) -> Result<Arc<Core>, String> {
        self.started().map_err(|e| e.code.to_string())
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn chinese(prefs: &Prefs) -> bool {
    match prefs.language.as_deref() {
        Some(language) => language.starts_with("zh"),
        None => sys_locale::get_locale().is_none_or(|l| l.to_lowercase().starts_with("zh")),
    }
}

/// 界面要的快照。Core 没起来时给出原因。
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum View {
    Ready { snapshot: Box<Snapshot> },
    Failed { error: StartupError },
}

fn snapshot_of(app: &AppHandle, core: &Core) -> Snapshot {
    let mut snapshot = core.snapshot();
    snapshot.version = app.package_info().version.to_string();
    snapshot
}

fn view(app: &AppHandle, runtime: &Runtime) -> View {
    match runtime.started() {
        Ok(core) => View::Ready { snapshot: Box::new(snapshot_of(app, &core)) },
        Err(error) => View::Failed { error },
    }
}

/// 广播现在的状态，主窗口和托盘一起更新。
fn publish(app: &AppHandle) {
    let runtime = app.state::<Runtime>();
    let current = view(app, &runtime);
    let _ = app.emit(EVENT_STATE, &current);
    if let View::Ready { snapshot } = current {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || tray::refresh(&handle, &snapshot));
    }
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| e.to_string())
}

/// 重新检测；装好了等着应用的顺便应用。
async fn refresh(app: &AppHandle, core: &Arc<Core>) {
    *lock(&app.state::<Runtime>().last_detect) = Some(Instant::now());
    let worker = core.clone();
    if blocking(move || worker.detect_all()).await.is_err() {
        return;
    }
    let applied = {
        let _guard = core.op.lock().await;
        let worker = core.clone();
        blocking(move || worker.apply_waiting()).await.unwrap_or_default()
    };
    publish(app);
    for outcome in applied.iter().filter(|o| o.error.is_none()) {
        verify_one(app, core, outcome.agent).await;
    }
}

/// 只查由我们装着、而且终端里用的正是这一份的。没有就不联网。
async fn check_updates(app: &AppHandle, core: &Arc<Core>, force: bool) {
    {
        let runtime = app.state::<Runtime>();
        let mut last = lock(&runtime.last_update_check);
        if !force && last.is_some_and(|at| at.elapsed() < UPDATE_CHECK) {
            return;
        }
        *last = Some(Instant::now());
    }
    let Ok(installer) = Installer::system() else { return };
    for id in AgentId::ALL {
        let Some(managed) = installer.layout.managed(id) else {
            core.set_update(id, None);
            continue;
        };
        let ours = matches!(
            core.install(id),
            super::agents::Install::Installed { version: Some(v) } if v.contains(&managed)
        );
        let newer = if ours { installer.newer(id).await } else { None };
        core.set_update(id, newer);
    }
    publish(app);
}

async fn verify_one(app: &AppHandle, core: &Arc<Core>, id: AgentId) -> Option<Verify> {
    let (key, model) = core.verify_target(id)?;
    let result = verify::run(id, &key, &model).await;
    core.set_verify(id, result.clone());
    publish(app);
    Some(result)
}

/// 主窗口和托盘共用的切换。
pub async fn switch(
    app: &AppHandle,
    runtime: &Runtime,
    id: AgentId,
    target: Target,
) -> Result<Outcome, String> {
    let core = runtime.core()?;
    let outcome = {
        let _guard = core.op.lock().await;
        let worker = core.clone();
        blocking(move || worker.switch(id, target)).await?
    };
    publish(app);
    if outcome.error.is_none() && target == Target::Pontex {
        let (app, core) = (app.clone(), core);
        tauri::async_runtime::spawn(async move {
            verify_one(&app, &core, id).await;
        });
    }
    Ok(outcome)
}

// ---------- 命令 ----------

#[tauri::command]
async fn px_snapshot(app: AppHandle) -> Result<serde_json::Value, String> {
    let handle = app.clone();
    let value = blocking(move || serde_json::to_value(view(&handle, &handle.state::<Runtime>()))).await?;
    value.map_err(|e| e.to_string())
}

#[tauri::command]
async fn px_refresh(app: AppHandle, force: bool, runtime: State<'_, Runtime>) -> Result<(), String> {
    let core = runtime.core()?;
    let recent = lock(&runtime.last_detect).is_some_and(|at| at.elapsed() < Duration::from_secs(30));
    if recent && !force {
        publish(&app);
    } else {
        refresh(&app, &core).await;
        if force {
            let (app, core) = (app.clone(), core.clone());
            tauri::async_runtime::spawn(async move { check_updates(&app, &core, true).await });
        }
    }
    Ok(())
}

#[tauri::command]
async fn px_check_key(key: String) -> Result<KeyReport, String> {
    let secret = key::parse(&key).map_err(str::to_string)?;
    Ok(key::check(&secret).await)
}

#[derive(Serialize)]
struct Connected {
    report: KeyReport,
    outcomes: Vec<Outcome>,
}

#[tauri::command]
async fn px_connect(
    app: AppHandle,
    runtime: State<'_, Runtime>,
    key: String,
    choices: Vec<Choice>,
) -> Result<Connected, String> {
    let core = runtime.core()?;
    let secret = key::parse(&key).map_err(str::to_string)?;
    // 以这一刻的检查为准，不采信界面传来的结果。
    let report = key::check(&secret).await;
    if !report.usable {
        return Err(report.reason.unwrap_or_else(|| "KEY_FORMAT".into()));
    }
    let outcomes = {
        let _guard = core.op.lock().await;
        let (worker, checked) = (core.clone(), report.clone());
        blocking(move || worker.connect(&secret, &checked, &choices)).await?
    };
    publish(&app);
    let applied: Vec<AgentId> = outcomes
        .iter()
        .filter(|o| o.result == "applied" && o.error.is_none())
        .map(|o| o.agent)
        .collect();
    for id in applied {
        let (app, core) = (app.clone(), core.clone());
        tauri::async_runtime::spawn(async move {
            verify_one(&app, &core, id).await;
        });
    }
    Ok(Connected { report, outcomes })
}

#[tauri::command]
async fn px_switch(
    app: AppHandle,
    runtime: State<'_, Runtime>,
    agent: AgentId,
    target: Target,
) -> Result<Outcome, String> {
    switch(&app, &runtime, agent, target).await
}

#[tauri::command]
async fn px_set_model(
    app: AppHandle,
    runtime: State<'_, Runtime>,
    agent: AgentId,
    model: String,
) -> Result<Outcome, String> {
    let core = runtime.core()?;
    let outcome = {
        let _guard = core.op.lock().await;
        let worker = core.clone();
        blocking(move || worker.set_model(agent, &model)).await?
    };
    publish(&app);
    if outcome.error.is_none() {
        let (app, core) = (app.clone(), core);
        tauri::async_runtime::spawn(async move {
            verify_one(&app, &core, agent).await;
        });
    }
    Ok(outcome)
}

#[tauri::command]
async fn px_verify(
    app: AppHandle,
    runtime: State<'_, Runtime>,
    agent: AgentId,
) -> Result<Option<Verify>, String> {
    let core = runtime.core()?;
    Ok(verify_one(&app, &core, agent).await)
}

#[tauri::command]
async fn px_disconnect(app: AppHandle, agent: Option<AgentId>) -> Result<Vec<Outcome>, String> {
    disconnect(&app, agent).await
}

/// 断开（还原到接入前）。托盘里取消 WorkBuddy 的勾也走这里。
pub async fn disconnect(app: &AppHandle, agent: Option<AgentId>) -> Result<Vec<Outcome>, String> {
    let core = app.state::<Runtime>().core()?;
    let outcomes = {
        let _guard = core.op.lock().await;
        let worker = core.clone();
        blocking(move || worker.disconnect(agent)).await?
    };
    publish(app);
    Ok(outcomes)
}

#[tauri::command]
async fn px_set_prefs(app: AppHandle, runtime: State<'_, Runtime>, prefs: Prefs) -> Result<(), String> {
    runtime.core()?.set_prefs(prefs)?;
    publish(&app);
    Ok(())
}

#[tauri::command]
fn px_autostart(enable: Option<bool>) -> Result<bool, String> {
    autostart::set(enable)
}

/// 界面只能打开这几个固定的地方，不能传任意地址。
#[tauri::command]
async fn px_open(app: AppHandle, target: String, agent: Option<AgentId>) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let url = match target.as_str() {
        "key" => "https://docs.ponte-x.com/admin/api-keys",
        "docs" => "https://docs.ponte-x.com",
        "download" => "https://docs.ponte-x.com/pontex-one",
        "console" => API_ORIGIN,
        "skills" => "https://skills.ponte-x.com",
        "upstream" => "https://github.com/farion1231/cc-switch",
        "install" => agent.ok_or("missing agent")?.install_url(),
        "workbuddy" => return upstream::open_workbuddy().await,
        "codex-app" => return blocking(installer::desktop::open).await?,
        "data" => {
            let dir = upstream::device_dir();
            return app
                .opener()
                .open_path(dir.to_string_lossy(), None::<&str>)
                .map_err(|e| e.to_string());
        }
        _ => return Err("unknown target".into()),
    };
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

// ---------- 在文件夹里打开 ----------

#[derive(Serialize)]
struct Folders {
    recents: Vec<places::Folder>,
    /// 新建的文件夹默认放在这里。
    default_dir: String,
    /// 这台电脑上能用的终端，第一个是默认的。
    terminals: Vec<&'static str>,
}

#[tauri::command]
async fn px_folders(runtime: State<'_, Runtime>) -> Result<Folders, String> {
    let custom = runtime.core()?.prefs().folders_dir;
    blocking(move || Folders {
        recents: places::list(),
        default_dir: places::default_dir(custom.as_deref()).display().to_string(),
        terminals: places::terminals(),
    })
    .await
}

#[tauri::command]
async fn px_launch(app: AppHandle, agent: AgentId, path: String) -> Result<(), String> {
    launch(&app, agent, path).await
}

/// 先打开文件夹，再打开终端、在里面启动 Agent；记进最近的文件夹。托盘也走这里。
pub async fn launch(app: &AppHandle, agent: AgentId, path: String) -> Result<(), String> {
    let core = app.state::<Runtime>().core()?;
    let prefs = core.prefs();
    let (open_folder, terminal) = (prefs.open_folder, prefs.terminal.clone());
    blocking(move || places::launch(agent, std::path::Path::new(&path), open_folder, terminal.as_deref())).await??;
    if !prefs.launched_once {
        core.set_prefs(Prefs { launched_once: true, ..prefs })?;
    }
    publish(app);
    Ok(())
}

/// 在默认位置（或 `parent`）里新建文件夹，返回它的路径。
#[tauri::command]
async fn px_new_folder(runtime: State<'_, Runtime>, name: String, parent: Option<String>) -> Result<String, String> {
    let custom = runtime.core()?.prefs().folders_dir;
    let parent = parent.map(std::path::PathBuf::from).unwrap_or_else(|| places::default_dir(custom.as_deref()));
    blocking(move || places::make(&parent, &name).map(|p| p.display().to_string())).await?
}

/// 系统的「选择文件夹」窗口；取消了返回空。
#[tauri::command]
async fn px_pick_folder(app: AppHandle, start: Option<String>) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    blocking(move || {
        let mut dialog = app.dialog().file();
        if let Some(dir) = start.filter(|d| std::path::Path::new(d).is_dir()) {
            dialog = dialog.set_directory(dir);
        }
        dialog
            .blocking_pick_folder()
            .and_then(|picked| picked.into_path().ok())
            .map(|path| path.display().to_string())
    })
    .await
}

#[tauri::command]
async fn px_forget_folder(app: AppHandle, path: String) -> Result<Option<(usize, places::Recent)>, String> {
    let removed = blocking(move || places::forget(&path)).await??;
    publish(&app);
    Ok(removed)
}

#[tauri::command]
async fn px_restore_folder(app: AppHandle, index: usize, recent: places::Recent) -> Result<(), String> {
    blocking(move || places::restore(index, recent)).await??;
    publish(&app);
    Ok(())
}

// ---------- AI 工具箱 ----------

#[derive(Serialize)]
struct Toolbox {
    skills: Vec<toolbox::Skill>,
    /// 技能能启用到的 Agent（装着的、有技能目录的）。
    skill_agents: Vec<AgentId>,
    mcps: Vec<upstream::Mcp>,
    /// MCP 能启用到的 Agent（装着的）。
    mcp_agents: Vec<AgentId>,
    /// 这次从各 Agent 里新读进来的 MCP 个数。
    imported: usize,
}

#[tauri::command]
async fn px_toolbox(runtime: State<'_, Runtime>) -> Result<Toolbox, String> {
    let core = runtime.core()?;
    let installed: Vec<AgentId> = core.snapshot().agents.iter().filter(|a| a.install.usable()).map(|a| a.id).collect();
    let _guard = core.op.lock().await;
    let worker = core.clone();
    blocking(move || {
        let engine = worker.engine();
        let imported = engine.mcp_import().unwrap_or_else(|e| {
            log::warn!("[PONTEX] 读入各 Agent 的 MCP 有失败: {e}");
            0
        });
        let skill_agents: Vec<AgentId> = toolbox::AGENTS.into_iter().filter(|a| installed.contains(a)).collect();
        Ok(Toolbox {
            skills: toolbox::list(&skill_agents),
            skill_agents,
            mcps: engine.mcp_list()?,
            mcp_agents: installed,
            imported,
        })
    })
    .await?
}

#[tauri::command]
async fn px_skill_toggle(dir: String, agent: AgentId, enabled: bool) -> Result<(), String> {
    blocking(move || toolbox::toggle(&dir, agent, enabled)).await?
}

#[tauri::command]
async fn px_mcp_toggle(runtime: State<'_, Runtime>, id: String, agent: AgentId, enabled: bool) -> Result<(), String> {
    let core = runtime.core()?;
    let _guard = core.op.lock().await;
    let worker = core.clone();
    blocking(move || worker.engine().mcp_toggle(&id, agent, enabled)).await?
}

#[tauri::command]
async fn px_mcp_save(
    runtime: State<'_, Runtime>,
    id: Option<String>,
    name: String,
    server: serde_json::Value,
    on: Vec<AgentId>,
) -> Result<String, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("MCP_NAME_EMPTY".into());
    }
    let core = runtime.core()?;
    let _guard = core.op.lock().await;
    let worker = core.clone();
    blocking(move || worker.engine().mcp_save(id, &name, server, &on)).await?
}

#[tauri::command]
async fn px_mcp_delete(runtime: State<'_, Runtime>, id: String) -> Result<(), String> {
    let core = runtime.core()?;
    let _guard = core.op.lock().await;
    let worker = core.clone();
    blocking(move || worker.engine().mcp_delete(&id)).await?
}

/// 试着连一下：`ok` 能用、`login` 要先登录、`failed` 连不上。
#[tauri::command]
async fn px_mcp_check(runtime: State<'_, Runtime>, ids: Vec<String>) -> Result<Vec<(String, &'static str)>, String> {
    let core = runtime.core()?;
    let worker = core.clone();
    let servers = blocking(move || {
        ids.into_iter()
            .filter_map(|id| worker.engine().mcp_server(&id).map(|server| (id, server)))
            .collect::<Vec<_>>()
    })
    .await?;
    Ok(upstream::mcp_check(servers).await)
}

#[tauri::command]
async fn px_diagnose(app: AppHandle, runtime: State<'_, Runtime>) -> Result<String, String> {
    use tauri_plugin_opener::OpenerExt;
    let (current, version) = (view(&app, &runtime), app.package_info().version.to_string());
    let path = blocking(move || diagnose::write(&version, &current)).await??;
    let _ = app.opener().reveal_item_in_dir(&path);
    Ok(path.display().to_string())
}

/// 启动失败后再试一次。成功了就和正常启动一样往下走。
#[tauri::command]
async fn px_retry(app: AppHandle, runtime: State<'_, Runtime>) -> Result<(), String> {
    if runtime.started().is_err() {
        let started = blocking(start_core).await?;
        *runtime.core.write().unwrap_or_else(|e| e.into_inner()) = started.clone();
        if let Ok(core) = started {
            run_core(&app, core);
        }
    }
    publish(&app);
    Ok(())
}

#[tauri::command]
fn px_pending_skill(runtime: State<'_, Runtime>) -> Option<String> {
    lock(&runtime.pending_skill).take()
}

#[tauri::command]
async fn px_skill_info(slug: String) -> Result<skills::Info, String> {
    skills::info(&slug).await
}

#[tauri::command]
async fn px_skills() -> Result<Vec<skills::Owned>, String> {
    blocking(skills::owned).await
}

#[tauri::command]
async fn px_skill_prepare(runtime: State<'_, Runtime>, slug: String) -> Result<Preview, String> {
    let core = runtime.core()?;
    let ready = skills::prepare(&slug).await?;
    let preview = ready.preview(|id| core.install(id).usable());
    *lock(&runtime.ready_skill) = Some(ready);
    Ok(preview)
}

/// 只能装刚刚校验过、给用户看过的那一个。
#[tauri::command]
async fn px_skill_commit(
    runtime: State<'_, Runtime>,
    slug: String,
    agents: Vec<AgentId>,
) -> Result<Vec<Installed>, String> {
    let ready = lock(&runtime.ready_skill)
        .take()
        .filter(|ready| ready.slug() == slug)
        .ok_or("SKILL_NOT_READY")?;
    blocking(move || ready.install(&agents)).await
}

#[tauri::command]
async fn px_skill_remove(slug: String, agent: AgentId) -> Result<(), String> {
    blocking(move || skills::remove(&slug, agent)).await?
}

// ---------- 安装 ----------

fn report(app: &AppHandle, progress: Progress) {
    let runtime = app.state::<Runtime>();
    lock(&runtime.jobs.progress).insert(progress.agent, progress.clone());
    let _ = app.emit(EVENT_INSTALL, progress);
}

/// 这几个 Agent 的安装计划：版本、占用、装在哪、会不会改 shell 配置。
#[tauri::command]
async fn px_install_plan(agents: Vec<AgentId>) -> Result<Vec<Planned>, String> {
    let installer = Installer::system()?;
    // PATH 的结论对所有 Agent 一样，只问一次 shell。
    let path = installer.path_plan().await?;
    let each = agents.into_iter().map(|agent| {
        let (installer, path) = (&installer, path.clone());
        async move {
            let want_app = agent == AgentId::Codex && installer::app_supported() && super::agents::app_version().is_none();
            let (cli, app) = futures::future::join(installer.plan(agent, path), async {
                if want_app { Some(installer.app_plan().await) } else { None }
            })
            .await;
            let (plan, error) = match cli {
                Ok(plan) => (Some(plan), None),
                Err(error) => (None, Some(error)),
            };
            let (app, app_error) = match app {
                Some(Ok(plan)) => (Some(plan), None),
                Some(Err(error)) => (None, Some(error)),
                None => (None, None),
            };
            Planned { agent, plan, error, app, app_error }
        }
    });
    Ok(futures::future::join_all(each).await)
}

#[tauri::command]
async fn px_install(
    app: AppHandle,
    runtime: State<'_, Runtime>,
    agents: Vec<AgentId>,
    desktop: Option<bool>,
) -> Result<(), String> {
    let core = runtime.core()?;
    // Codex 装桌面版还是命令行：这一次点的为准。没点 Codex 就不动它（它可能正排着队装桌面版）。
    if agents.contains(&AgentId::Codex) && !lock(&runtime.jobs.queue).contains(&AgentId::Codex) {
        let mut wanted = lock(&runtime.jobs.app);
        wanted.remove(&AgentId::Codex);
        if desktop == Some(true) && installer::app_supported() {
            wanted.insert(AgentId::Codex);
        }
    }
    let mut queued = Vec::new();
    {
        let mut queue = lock(&runtime.jobs.queue);
        for id in agents {
            let busy = queue.contains(&id)
                || lock(&runtime.jobs.progress).get(&id).is_some_and(|p| {
                    !matches!(p.phase, Phase::Done | Phase::Failed)
                });
            // 只装没装的、只更新我们装的。界面可能过时，以这一刻的检测为准。
            let app = lock(&runtime.jobs.app).contains(&id);
            // 命令行已经装着、再要桌面版：只要桌面版还没装，也可以装
            let allowed = core.install(id) == super::agents::Install::Missing
                || core.update(id).is_some()
                || (app && super::agents::app_version().is_none());
            let can = installer::supported(id) || app;
            if !busy && allowed && can {
                queue.push_back(id);
                queued.push(id);
            }
        }
    }
    for id in queued {
        // 在这里点了安装，就是要用 PonteX：装好直接接入。更新已经装着的不算。
        if core.install(id) == super::agents::Install::Missing {
            core.expect(id);
        }
        report(&app, Progress::new(id, Phase::Queued));
    }
    if !runtime.jobs.running.swap(true, Ordering::SeqCst) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { work(&app, &core).await });
    }
    Ok(())
}

async fn work(app: &AppHandle, core: &Arc<Core>) {
    let runtime = app.state::<Runtime>();
    loop {
        let next = lock(&runtime.jobs.queue).pop_front();
        let Some(id) = next else { break };
        *lock(&runtime.jobs.current) = Some(id);
        runtime.jobs.cancel.store(false, Ordering::SeqCst);
        let result = install_one(app, &runtime, id).await;
        *lock(&runtime.jobs.current) = None;
        match result {
            Ok(version) => {
                core.set_update(id, None);
                log::info!("[PONTEX] 已安装 {} {version}", id.key());
            }
            Err(error) => {
                log::warn!("[PONTEX] 安装 {} 失败: {error}", id.key());
                report(app, Progress { error: Some(error), ..Progress::new(id, Phase::Failed) });
            }
        }
        // 装好一个就认一个：检测、接入、验证都在这里面。
        refresh(app, core).await;
    }
    runtime.jobs.running.store(false, Ordering::SeqCst);
    // 收尾的瞬间又排进来的，接着做。
    if !lock(&runtime.jobs.queue).is_empty() && !runtime.jobs.running.swap(true, Ordering::SeqCst) {
        Box::pin(work(app, core)).await;
    }
}

async fn install_one(app: &AppHandle, runtime: &Runtime, id: AgentId) -> Result<String, String> {
    let installer = Installer::system()?;
    let handle = app.clone();
    if lock(&runtime.jobs.app).remove(&id) {
        // 桌面版装进系统的应用目录，不经过我们的 bin，也不改 PATH。
        return installer.install_app(&runtime.jobs.cancel, &move |p| report(&handle, p)).await;
    }
    let version = installer
        .install(id, &runtime.jobs.cancel, &move |p| report(&handle, p))
        .await?;
    // PATH 放在最后：前面任何一步失败都不会动用户的 shell 配置。
    let (bin, home) = (installer.layout.bin(), installer.home.clone());
    let done = blocking(move || {
        let plan = install_path::plan(&bin, &home);
        install_path::apply(&plan, &bin)
    })
    .await?;
    if let Err(error) = done {
        log::warn!("[PONTEX] 加入 PATH 失败: {error}");
    }
    Ok(version)
}

/// 用户自己去装（复制了镜像命令）：检测到装好就接入。
#[tauri::command]
fn px_expect(runtime: State<'_, Runtime>, agent: AgentId) -> Result<(), String> {
    let core = runtime.core()?;
    if !core.install(agent).usable() {
        core.expect(agent);
    }
    Ok(())
}

/// 只取消这一个；排在后面的照常装。
#[tauri::command]
fn px_install_cancel(app: AppHandle, runtime: State<'_, Runtime>, agent: AgentId) {
    let waiting = {
        let mut queue = lock(&runtime.jobs.queue);
        let before = queue.len();
        queue.retain(|id| *id != agent);
        queue.len() != before
    };
    if waiting {
        lock(&runtime.jobs.app).remove(&agent);
    }
    if waiting {
        report(&app, Progress {
            error: Some(installer::CANCELLED.into()),
            ..Progress::new(agent, Phase::Failed)
        });
    } else if *lock(&runtime.jobs.current) == Some(agent) {
        runtime.jobs.cancel.store(true, Ordering::SeqCst);
    }
}

#[tauri::command]
fn px_install_state(runtime: State<'_, Runtime>) -> Vec<Progress> {
    lock(&runtime.jobs.progress).values().cloned().collect()
}

/// 移除我们装的。先把它的配置还原，再删程序。
#[tauri::command]
async fn px_uninstall(app: AppHandle, runtime: State<'_, Runtime>, agent: AgentId) -> Result<(), String> {
    let core = runtime.core()?;
    {
        let _guard = core.op.lock().await;
        if *lock(&runtime.jobs.current) == Some(agent) {
            return Err("INSTALL_BUSY".into());
        }
        let worker = core.clone();
        blocking(move || {
            let installer = Installer::system()?;
            if installer.layout.managed(agent).is_none() {
                return Err(installer::EXISTS.to_string());
            }
            // 配置还原不了就不删程序：留着还能再试。
            if let Some(error) = worker.disconnect(Some(agent)).into_iter().find_map(|o| o.error) {
                return Err(error);
            }
            installer.uninstall(agent)
        })
        .await??;
    }
    lock(&runtime.jobs.progress).remove(&agent);
    refresh(&app, &core).await;
    Ok(())
}

// ---------- 窗口与生命周期 ----------

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        #[cfg(target_os = "macos")]
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// 链接进来：只记下 slug 并请出主窗口，其余一律忽略。
fn on_link(app: &AppHandle, raw: &str) {
    match links::parse(raw) {
        Some(Link::InstallSkill { slug }) => {
            log::info!("[PONTEX] 收到技能链接: {slug}");
            *lock(&app.state::<Runtime>().pending_skill) = Some(slug);
            show_main(app);
            let _ = app.emit(EVENT_SKILL, ());
        }
        None => log::warn!("[PONTEX] 忽略不认识的链接"),
    }
}

fn links_in(args: &[String]) -> Option<&String> {
    args.iter().find(|a| a.starts_with(&format!("{SCHEME}:")))
}

/// 数据库打不开的几种常见原因，界面据此说人话。
fn startup_code(detail: &str) -> &'static str {
    let detail = detail.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| detail.contains(w));
    if has(&["is locked", "database is busy"]) {
        "DB_LOCKED"
    } else if has(&["malformed", "not a database", "corrupt"]) {
        "DB_CORRUPT"
    } else if has(&["full", "no space"]) {
        "DB_FULL"
    } else if has(&["unable to open", "permission denied", "readonly", "read-only", "access is denied"]) {
        "DB_ACCESS"
    } else {
        "STARTUP"
    }
}

/// 后台循环：检测、接入、查新版。
fn run_core(app: &AppHandle, core: Arc<Core>) {
    autostart::by_default(&core);
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            refresh(&handle, &core).await;
            check_updates(&handle, &core, false).await;
            tokio::time::sleep(REFRESH).await;
        }
    });
}

fn start_core() -> Result<Arc<Core>, StartupError> {
    if let Some((found, supported)) = Engine::too_new() {
        return Err(StartupError {
            code: "DB_TOO_NEW",
            detail: format!("{found} > {supported}"),
        });
    }
    Engine::open()
        .map(|engine| Arc::new(Core::new(engine)))
        .map_err(|detail| {
            log::error!("[PONTEX] 启动失败: {detail}");
            StartupError { code: startup_code(&detail), detail }
        })
}

pub fn run() {
    let dir = upstream::device_dir();
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }

    tauri::Builder::default()
        // 必须是第一个插件。
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            match links_in(&args) {
                Some(link) => on_link(app, link),
                None => show_main(app),
            }
        }))
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .clear_targets()
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Folder {
                        path: diagnose::logs_dir(),
                        file_name: Some("pontex-one".into()),
                    },
                ))
                .max_file_size(2 << 20)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepOne)
                .build(),
        )
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_window_state::Builder::default().with_state_flags(
            tauri_plugin_window_state::StateFlags::POSITION | tauri_plugin_window_state::StateFlags::SIZE,
        ).build())
        .setup(|app| {
            let runtime = Runtime {
                core: RwLock::new(start_core()),
                pending_skill: Mutex::default(),
                ready_skill: Mutex::default(),
                last_detect: Mutex::default(),
                last_update_check: Mutex::default(),
                jobs: Jobs::default(),
            };
            let core = runtime.started().ok();
            app.manage(runtime);
            log::info!(
                "[PONTEX] PonteX One {} · CC Switch {} · {}",
                app.package_info().version,
                upstream::VERSION,
                api_origin()
            );

            let handle = app.handle().clone();
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                // 开发构建和 Linux 没有安装器替我们注册协议。
                #[cfg(any(target_os = "linux", all(debug_assertions, windows)))]
                let _ = app.deep_link().register_all();
                let on_open = handle.clone();
                app.deep_link().on_open_url(move |event| {
                    for url in event.urls() {
                        on_link(&on_open, url.as_str());
                    }
                });
                let args: Vec<String> = std::env::args().collect();
                if let Some(link) = links_in(&args) {
                    on_link(&handle, link);
                }
            }

            tray::create(&handle)?;
            publish(&handle);
            let hidden = std::env::args().any(|a| a == autostart::HIDDEN);
            if hidden {
                // 开机自启只留菜单栏图标，Dock 里不出现。
                #[cfg(target_os = "macos")]
                let _ = handle.set_activation_policy(tauri::ActivationPolicy::Accessory);
            } else {
                show_main(&handle);
            }
            if let Ok(installer) = Installer::system() {
                std::thread::spawn(move || installer.layout.sweep());
            }
            if let Some(core) = core {
                run_core(&handle, core);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let to_tray = app
                    .state::<Runtime>()
                    .started()
                    .is_ok_and(|core| core.prefs().close_to_tray);
                api.prevent_close();
                if to_tray {
                    let _ = window.hide();
                    #[cfg(target_os = "macos")]
                    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                } else {
                    app.exit(0);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            px_snapshot,
            px_refresh,
            px_check_key,
            px_connect,
            px_switch,
            px_set_model,
            px_verify,
            px_disconnect,
            px_set_prefs,
            px_autostart,
            px_open,
            px_diagnose,
            px_retry,
            px_pending_skill,
            px_skill_info,
            px_skills,
            px_skill_prepare,
            px_skill_commit,
            px_skill_remove,
            px_install_plan,
            px_install,
            px_expect,
            px_install_cancel,
            px_install_state,
            px_uninstall,
            px_folders,
            px_launch,
            px_new_folder,
            px_pick_folder,
            px_forget_folder,
            px_restore_folder,
            px_toolbox,
            px_skill_toggle,
            px_mcp_toggle,
            px_mcp_save,
            px_mcp_delete,
            px_mcp_check,
        ])
        .build(tauri::generate_context!())
        .expect("PonteX One failed to start")
        .run(|app, event| match event {
            tauri::RunEvent::Exit => upstream::settle(),
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => show_main(app),
            _ => {
                let _ = app;
            }
        });
}
