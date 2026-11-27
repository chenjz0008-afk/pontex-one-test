//! 开机自动启动。默认开：装好后第一次打开时设一次，之后开关都听用户的。

use std::path::{Path, PathBuf};

use auto_launch::{AutoLaunch, AutoLaunchBuilder};

use super::core::Core;

const NAME: &str = "PonteX One";
/// 随开机启动时不弹主窗口，只留在菜单栏 / 托盘。
pub const HIDDEN: &str = "--hidden";

/// 写进启动项的是**可执行文件**：macOS 的启动项不能执行 `.app` 目录；Windows 按命令行解析，
/// 路径里有空格要加引号。
fn command(exe: &Path, windows: bool) -> String {
    if windows {
        format!("\"{}\"", exe.display())
    } else {
        exe.display().to_string()
    }
}

/// 会一直在这里的位置。直接从 DMG 里打开、或被系统隔离运行时，路径下次就没了。
fn lasting(exe: &Path) -> bool {
    !exe.starts_with("/Volumes") && !exe.components().any(|c| c.as_os_str() == "AppTranslocation")
}

fn launcher(exe: &Path) -> Result<AutoLaunch, String> {
    let mut builder = AutoLaunchBuilder::new();
    builder
        .set_app_name(NAME)
        .set_app_path(&command(exe, cfg!(windows)))
        .set_args(&[HIDDEN]);
    #[cfg(target_os = "macos")]
    builder.set_use_launch_agent(true);
    builder.build().map_err(|e| e.to_string())
}

/// `None` 只看现在开没开。
pub fn set(enable: Option<bool>) -> Result<bool, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let launcher = launcher(&exe)?;
    match enable {
        Some(true) => launcher.enable().map_err(|e| e.to_string())?,
        Some(false) => launcher.disable().map_err(|e| e.to_string())?,
        None => {}
    }
    launcher.is_enabled().map_err(|e| e.to_string())
}

/// macOS：启动项里写的不是现在这个位置（软件挪过地方，或旧版本写成了 `.app` 目录）。
fn stale(plist: &str, exe: &Path) -> bool {
    !plist.contains(&format!("<string>{}</string>", exe.display()))
}

fn plist() -> PathBuf {
    super::upstream::home().join(format!("Library/LaunchAgents/{NAME}.plist"))
}

/// 启动时调用。
pub fn by_default(core: &Core) {
    let Ok(exe) = std::env::current_exe() else { return };
    if cfg!(debug_assertions) || !lasting(&exe) {
        return;
    }
    let repair = cfg!(target_os = "macos")
        && std::fs::read_to_string(plist()).is_ok_and(|text| stale(&text, &exe));
    if core.first_autostart() || repair {
        if let Err(error) = set(Some(true)) {
            log::warn!("[PONTEX] 设置开机启动失败: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_is_something_the_system_can_run() {
        let mac = Path::new("/Applications/PonteX One.app/Contents/MacOS/pontex-one");
        assert_eq!(command(mac, false), mac.display().to_string());
        let win = Path::new(r"C:\Users\a b\AppData\Local\Programs\PonteX One\pontex-one.exe");
        assert_eq!(command(win, true), format!("\"{}\"", win.display()));
    }

    #[test]
    fn only_lasting_places_are_registered() {
        assert!(lasting(Path::new("/Applications/PonteX One.app/Contents/MacOS/pontex-one")));
        assert!(!lasting(Path::new("/Volumes/PonteX One/PonteX One.app/Contents/MacOS/pontex-one")));
        assert!(!lasting(Path::new("/private/var/folders/x/T/AppTranslocation/ID/d/PonteX One.app/Contents/MacOS/pontex-one")));
    }

    #[test]
    fn an_entry_pointing_at_the_bundle_is_repaired() {
        let exe = Path::new("/Applications/PonteX One.app/Contents/MacOS/pontex-one");
        assert!(stale("<string>/Applications/PonteX One.app</string><string>--hidden</string>", exe));
        assert!(!stale(&format!("<string>{}</string>", exe.display()), exe));
    }
}
