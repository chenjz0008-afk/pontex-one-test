//! 托盘：一层菜单。上面每个 Agent 一行，打勾 = 已启用 PonteX；下面是最近的文件夹。内容来自同一份快照。

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

use super::agents::AgentId;
use super::app::Runtime;
use super::consts::{PRODUCT_EN, PRODUCT_ZH};
use super::core::Snapshot;
use super::status::Code;
use super::store::Target;

pub const ID: &str = "pontex-one";

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id(ID)
        .icon(tauri::image::Image::from_bytes(icon())?)
        .icon_as_template(cfg!(target_os = "macos"))
        .menu(&Menu::new(app)?)
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()));
    if cfg!(not(target_os = "macos")) {
        builder = builder.tooltip(PRODUCT_ZH).on_tray_icon_event(|tray, event| {
            use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                super::app::show_main(tray.app_handle());
            }
        });
    }
    builder.build(app)?;
    Ok(())
}

fn icon() -> &'static [u8] {
    if cfg!(target_os = "macos") {
        include_bytes!("../../pontex/icons/tray/statusTemplate@2x.png")
    } else {
        include_bytes!("../../pontex/icons/32x32.png")
    }
}

fn text(zh: bool, code: Code) -> &'static str {
    match (code, zh) {
        (Code::Checking, true) => "检测中",
        (Code::Checking, false) => "Checking",
        (Code::NotInstalled, true) => "未安装",
        (Code::NotInstalled, false) => "Not installed",
        (Code::Waiting, true) => "装好后自动接入",
        (Code::Waiting, false) => "Connects once installed",
        (Code::Verified | Code::Written, true) => "PonteX",
        (Code::Verified | Code::Written, false) => "PonteX",
        (Code::Native, true) => "原生",
        (Code::Native, false) => "Native",
        (Code::Other, true) => "其他",
        (Code::Other, false) => "Other",
        (Code::Added, true) => "已添加",
        (Code::Added, false) => "Added",
        (Code::NotAdded, true) => "未添加",
        (Code::NotAdded, false) => "Not added",
        (_, true) => "需要处理",
        (_, false) => "Needs attention",
    }
}

/// 按快照重建菜单。在主线程上调用。
pub fn refresh(app: &AppHandle, snapshot: &Snapshot) {
    let Some(tray) = app.tray_by_id(ID) else {
        return;
    };
    match build(app, snapshot) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
        }
        Err(error) => log::warn!("[PONTEX] 重建托盘菜单失败: {error}"),
    }
}

fn build(app: &AppHandle, snapshot: &Snapshot) -> tauri::Result<Menu<tauri::Wry>> {
    let zh = super::app::chinese(&snapshot.prefs);
    let say = |cn: &'static str, en: &'static str| if zh { cn } else { en };
    let menu = Menu::new(app)?;
    let name = if zh { PRODUCT_ZH } else { PRODUCT_EN };
    // 一层：打勾 = 已启用 PonteX，点一下就切；没勾的在名字后面写着现在是什么。
    // 系统菜单的勾选项不能带图标、右边也放不了灰字，所以状态写进名字里。
    menu.append(&MenuItem::with_id(app, "heading", say("已启用 PonteX 的 Agent", "Agents using PonteX"), false, None::<&str>)?)?;
    for agent in &snapshot.agents {
        let usable = agent.install.usable();
        let on = agent.current == Some(Target::Pontex);
        let side = if !usable {
            Some(text(zh, agent.status.code))
        } else if on {
            None
        } else {
            Some(text(zh, agent.status.code))
        };
        let label = match side {
            Some(side) => format!("{}  ·  {side}", agent.label),
            None => agent.label.to_string(),
        };
        menu.append(&CheckMenuItem::with_id(
            app,
            format!("toggle:{}", agent.id.key()),
            label,
            usable && (snapshot.connected || on),
            on,
            None::<&str>,
        )?)?;
    }
    // 最近的文件夹：点一下就用上次那个 Agent 在那里打开。
    let recents: Vec<_> = super::places::list().into_iter().take(5).collect();
    if !recents.is_empty() {
        menu.append(&PredefinedMenuItem::separator(app)?)?;
        menu.append(&MenuItem::with_id(app, "heading-folders", say("最近的文件夹", "Recent folders"), false, None::<&str>)?)?;
        for (index, folder) in recents.iter().enumerate() {
            menu.append(&MenuItem::with_id(app, format!("folder:{index}"), &folder.name, true, None::<&str>)?)?;
        }
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    if let super::update::View::Ready { version, .. } = &snapshot.update.view {
        let label = if zh { format!("重启以更新到 {version}") } else { format!("Restart to update to {version}") };
        menu.append(&MenuItem::with_id(app, "update", label, true, None::<&str>)?)?;
    }
    let open = if zh { format!("打开 {name}") } else { format!("Open {name}") };
    menu.append(&MenuItem::with_id(app, "open", open, true, None::<&str>)?)?;
    let quit = if zh { format!("退出 {name}") } else { format!("Quit {name}") };
    menu.append(&MenuItem::with_id(app, "quit", quit, true, None::<&str>)?)?;
    Ok(menu)
}

fn on_menu(app: &AppHandle, id: &str) {
    let mut parts = id.split(':');
    match (parts.next(), parts.next()) {
        (Some("open"), _) => super::app::show_main(app),
        (Some("quit"), _) => app.exit(0),
        (Some("update"), _) => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let runtime = app.state::<Runtime>();
                if let Err(error) = super::app::restart_to_update(&app, &runtime).await {
                    log::warn!("[PONTEX] 从菜单栏更新失败: {error}");
                    super::app::show_main(&app);
                }
            });
        }
        (Some("toggle"), Some(agent)) => {
            let Some(agent) = AgentId::parse(agent) else { return };
            let runtime = app.state::<Runtime>();
            let Ok(core) = runtime.core() else { return };
            let on = core.snapshot().agents.iter().any(|a| a.id == agent && a.current == Some(Target::Pontex));
            if agent.switchable() {
                spawn_switch(app, agent, if on { Target::Native } else { Target::Pontex });
            } else if on {
                spawn_disconnect(app, agent);
            } else {
                spawn_switch(app, agent, Target::Pontex);
            }
        }
        (Some("folder"), Some(index)) => {
            let Some(folder) = index.parse::<usize>().ok().and_then(|i| super::places::list().into_iter().nth(i)) else { return };
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = super::app::launch(&app, folder.agent, folder.path).await {
                    log::warn!("[PONTEX] 从菜单栏打开文件夹失败: {error}");
                    super::app::show_main(&app);
                }
            });
        }
        _ => {}
    }
}

fn spawn_disconnect(app: &AppHandle, agent: AgentId) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if super::app::disconnect(&app, Some(agent)).await.is_err() {
            super::app::show_main(&app);
        }
    });
}

fn spawn_switch(app: &AppHandle, agent: AgentId, target: Target) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let runtime = app.state::<Runtime>();
        let outcome = super::app::switch(&app, &runtime, agent, target).await;
        // 托盘里看不到错误原因：失败就把主窗口请出来。
        if outcome.is_ok_and(|o| o.error.is_some()) {
            super::app::show_main(&app);
        }
    });
}
