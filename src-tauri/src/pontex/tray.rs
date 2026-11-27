//! 托盘：五个 Agent，每个只有「PonteX / 原生 / 原配置」。内容来自同一份快照。

use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
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
    let open = if zh { format!("打开 {name}") } else { format!("Open {name}") };
    menu.append(&MenuItem::with_id(app, "open", open, true, None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    for agent in &snapshot.agents {
        let label = format!("{}  ·  {}", agent.label, text(zh, agent.status.code));
        let usable = agent.install.usable();
        let mut items: Vec<Box<dyn IsMenuItem<tauri::Wry>>> = Vec::new();
        if agent.switchable {
            let mut add = |target: Target, title: &str, enabled: bool| -> tauri::Result<()> {
                items.push(Box::new(CheckMenuItem::with_id(
                    app,
                    format!("switch:{}:{}", agent.id.key(), target_key(target)),
                    title,
                    enabled && usable,
                    agent.current == Some(target),
                    None::<&str>,
                )?));
                Ok(())
            };
            add(Target::Pontex, "PonteX", snapshot.connected)?;
            add(Target::Native, say("原生", "Native"), true)?;
            if agent.previous.is_some() {
                add(Target::Previous, say("其他", "Other"), true)?;
            }
        } else {
            let added = agent.current == Some(Target::Pontex);
            items.push(Box::new(MenuItem::with_id(
                app,
                format!("apply:{}", agent.id.key()),
                say("添加 PonteX 模型", "Add PonteX models"),
                usable && snapshot.connected && !added,
                None::<&str>,
            )?));
            items.push(Box::new(MenuItem::with_id(
                app,
                format!("launch:{}", agent.id.key()),
                say("打开 WorkBuddy", "Open WorkBuddy"),
                usable,
                None::<&str>,
            )?));
        }
        let refs: Vec<&dyn IsMenuItem<tauri::Wry>> = items.iter().map(|i| i.as_ref()).collect();
        menu.append(&Submenu::with_items(app, label, true, &refs)?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    let quit = if zh { format!("退出 {name}") } else { format!("Quit {name}") };
    menu.append(&MenuItem::with_id(app, "quit", quit, true, None::<&str>)?)?;
    Ok(menu)
}

fn target_key(target: Target) -> &'static str {
    match target {
        Target::Pontex => "pontex",
        Target::Native => "native",
        Target::Previous => "previous",
    }
}

fn on_menu(app: &AppHandle, id: &str) {
    let mut parts = id.split(':');
    match (parts.next(), parts.next().and_then(AgentId::parse), parts.next()) {
        (Some("open"), ..) => super::app::show_main(app),
        (Some("quit"), ..) => app.exit(0),
        (Some("switch"), Some(agent), Some(target)) => {
            let target = match target {
                "pontex" => Target::Pontex,
                "native" => Target::Native,
                _ => Target::Previous,
            };
            spawn_switch(app, agent, target);
        }
        (Some("apply"), Some(agent), _) => spawn_switch(app, agent, Target::Pontex),
        (Some("launch"), Some(_), _) => {
            tauri::async_runtime::spawn(async {
                if let Err(error) = super::upstream::open_workbuddy().await {
                    log::warn!("[PONTEX] 打开 WorkBuddy 失败: {error}");
                }
            });
        }
        _ => {}
    }
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
