//! 在文件夹里打开 Agent：用过的文件夹、新建文件夹、先打开文件夹再打开终端。
//!
//! 给不会用终端的人用：选一个文件夹，PonteX 在系统的文件管理器里打开它，
//! 再打开终端、进到这个文件夹、把 Agent 启动好。

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::agents::{self, AgentId};
use super::upstream;

/// 记住的文件夹最多这么多个；更早的丢掉。
const KEEP: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recent {
    pub path: String,
    pub agent: AgentId,
    pub at: i64,
}

/// 给界面的一行：被删掉或移走的文件夹不出现。
#[derive(Debug, Clone, Serialize)]
pub struct Folder {
    pub path: String,
    pub name: String,
    pub parent: String,
    pub agent: AgentId,
}

static LOCK: Mutex<()> = Mutex::new(());

fn file() -> PathBuf {
    upstream::device_dir().join("pontex-folders.json")
}

fn load() -> Vec<Recent> {
    std::fs::read(file())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save(list: &[Recent]) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(list).map_err(|e| e.to_string())?;
    upstream::write_private(&file(), &bytes)
}

fn guard() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// 用过的文件夹，最近的在前。
pub fn list() -> Vec<Folder> {
    let _lock = guard();
    load()
        .into_iter()
        .filter(|r| Path::new(&r.path).is_dir())
        .map(|r| {
            let path = Path::new(&r.path);
            Folder {
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| r.path.clone()),
                parent: path
                    .parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                path: r.path,
                agent: r.agent,
            }
        })
        .collect()
}

/// 打开过：排到最前面。
pub fn remember(path: &Path, agent: AgentId) -> Result<(), String> {
    let _lock = guard();
    let path = path.display().to_string();
    let mut list = load();
    list.retain(|r| r.path != path);
    list.insert(
        0,
        Recent {
            path,
            agent,
            at: chrono::Utc::now().timestamp(),
        },
    );
    list.truncate(KEEP);
    save(&list)
}

/// 从列表里拿掉（文件夹不动）；返回原来的位置和内容，用来撤销。
pub fn forget(path: &str) -> Result<Option<(usize, Recent)>, String> {
    let _lock = guard();
    let mut list = load();
    let Some(index) = list.iter().position(|r| r.path == path) else {
        return Ok(None);
    };
    let recent = list.remove(index);
    save(&list)?;
    Ok(Some((index, recent)))
}

/// 撤销「拿掉」：放回原来的位置。
pub fn restore(index: usize, recent: Recent) -> Result<(), String> {
    let _lock = guard();
    let mut list = load();
    list.retain(|r| r.path != recent.path);
    list.insert(index.min(list.len()), recent);
    save(&list)
}

/// 新建的文件夹默认放在这里：个人文件夹里的 PonteX。
/// 不放进「文稿 / 文档」：那里常被 iCloud / OneDrive 同步，Agent 产生的大量小文件会拖慢同步、还可能被挪到云端。
pub fn default_dir(custom: Option<&str>) -> PathBuf {
    match custom.map(str::trim).filter(|s| !s.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => upstream::home().join("PonteX"),
    }
}

/// 名字有问题时返回错误码，界面翻成人话。
pub fn check_name(name: &str) -> Result<&str, &'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Err("FOLDER_NAME_EMPTY");
    }
    if name.chars().any(|c| {
        matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()
    }) {
        return Err("FOLDER_NAME_CHARS");
    }
    if name.starts_with('.') {
        return Err("FOLDER_NAME_DOT");
    }
    if name.chars().count() > 120 {
        return Err("FOLDER_NAME_LONG");
    }
    Ok(name)
}

/// 在 `parent` 里新建一个叫 `name` 的文件夹；`parent` 不在就先建好。
pub fn make(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let name = check_name(name)?;
    std::fs::create_dir_all(parent).map_err(|e| format!("FOLDER_CREATE: {e}"))?;
    let path = parent.join(name);
    if path.exists() {
        return Err("FOLDER_EXISTS".into());
    }
    std::fs::create_dir(&path).map_err(|e| format!("FOLDER_CREATE: {e}"))?;
    Ok(path)
}

/// 这台电脑上能用的终端。第一个是默认的。
pub fn terminals() -> Vec<&'static str> {
    #[cfg(target_os = "macos")]
    {
        let mut out = vec!["terminal"];
        let home = upstream::home();
        if ["/Applications/iTerm.app", "Applications/iTerm.app"]
            .iter()
            .any(|p| Path::new(p).exists() || home.join(p).exists())
        {
            out.push("iterm");
        }
        out
    }
    #[cfg(target_os = "windows")]
    {
        let mut out = Vec::new();
        if windows_terminal().is_some() {
            out.push("wt");
        }
        out.push("powershell");
        out
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        vec!["terminal"]
    }
}

/// Agent 的命令行在哪：先找 PATH 和常见位置，再找我们自己装的。
fn executable(id: AgentId) -> Result<PathBuf, String> {
    let tool = upstream::tool_name(id).ok_or("NOT_A_CLI")?;
    if let Some(path) = agents::find_executable(tool) {
        return Ok(path);
    }
    let bin = upstream::device_dir().join("bin");
    let names: Vec<String> = if cfg!(windows) {
        vec![format!("{tool}.cmd"), format!("{tool}.exe")]
    } else {
        vec![tool.to_string()]
    };
    names
        .iter()
        .map(|n| bin.join(n))
        .find(|p| p.is_file())
        .ok_or_else(|| "NOT_INSTALLED".to_string())
}

/// 先在文件管理器里打开文件夹，再打开终端、在里面启动 Agent。会等一小会儿，只在后台线程调用。
pub fn launch(
    id: AgentId,
    dir: &Path,
    open_folder: bool,
    terminal: Option<&str>,
) -> Result<(), String> {
    if !dir.is_dir() {
        return Err("FOLDER_MISSING".into());
    }
    let exe = executable(id)?;
    if open_folder {
        // 文件夹打不开不耽误启动 Agent。
        if let Err(e) = reveal(dir) {
            log::warn!("打开文件夹失败：{e}");
        }
        // 让终端出现在文件夹窗口前面。
        std::thread::sleep(Duration::from_millis(350));
    }
    let terminal = terminal
        .filter(|t| terminals().contains(t))
        .unwrap_or(terminals()[0]);
    open_terminal(terminal, dir, &exe)?;
    remember(dir, id)
}

fn reveal(dir: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("/usr/bin/open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        std::process::Command::new(Path::new(&root).join("explorer.exe"))
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = std::process::Command::new("xdg-open");
    // explorer.exe 成功时也可能返回 1：只看能不能启动。
    command
        .arg(dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// 单引号里的字符串（sh / zsh）。
fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// 单引号里的字符串（PowerShell）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn ps_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// 终端里跑的脚本：进到文件夹、启动 Agent；Agent 退出后留在这个文件夹的 shell 里。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn command_script(dir: &Path, exe: &Path) -> String {
    format!(
        "#!/bin/zsh -il\ncd -- {dir} || exit 1\nclear\n{exe}\nexec \"${{SHELL:-/bin/zsh}}\" -l\n",
        dir = sh_quote(&dir.display().to_string()),
        exe = sh_quote(&exe.display().to_string()),
    )
}

#[cfg(target_os = "macos")]
fn open_terminal(terminal: &str, dir: &Path, exe: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    // 用 .command 文件交给「终端」：不需要「自动化」权限，也不会弹授权框。
    let scripts = upstream::device_dir().join("launch");
    std::fs::create_dir_all(&scripts).map_err(|e| e.to_string())?;
    sweep(&scripts);
    let script = scripts.join(format!("{}.command", uuid::Uuid::new_v4().simple()));
    std::fs::write(&script, command_script(dir, exe)).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| e.to_string())?;
    let app = if terminal == "iterm" {
        "iTerm"
    } else {
        "Terminal"
    };
    let status = std::process::Command::new("/usr/bin/open")
        .args(["-a", app])
        .arg(&script)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("TERMINAL_FAILED: open -a {app}"))
    }
}

/// 一天前的启动脚本删掉。
#[cfg(target_os = "macos")]
fn sweep(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > Duration::from_secs(24 * 3600));
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_terminal() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let path = Path::new(&local).join(r"Microsoft\WindowsApps\wt.exe");
    path.exists().then_some(path)
}

#[cfg(target_os = "windows")]
fn open_terminal(terminal: &str, dir: &Path, exe: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    let run = format!("& {}", ps_quote(&exe.display().to_string()));
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    let powershell = Path::new(&root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let spawned = match (terminal, windows_terminal()) {
        ("wt", Some(wt)) => std::process::Command::new(wt)
            .arg("-d")
            .arg(dir)
            .arg(&powershell)
            // Windows 终端把分号当成命令分隔符。
            .args(["-NoExit", "-Command", &run.replace(';', r"\;")])
            .spawn(),
        // 没有 Windows 终端：直接开一个新的 PowerShell 窗口。
        _ => std::process::Command::new(&powershell)
            .args(["-NoExit", "-Command", &run])
            .current_dir(dir)
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn(),
    };
    spawned
        .map(|_| ())
        .map_err(|e| format!("TERMINAL_FAILED: {e}"))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn open_terminal(_terminal: &str, dir: &Path, exe: &Path) -> Result<(), String> {
    std::process::Command::new("x-terminal-emulator")
        .current_dir(dir)
        .arg("-e")
        .arg(exe)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("TERMINAL_FAILED: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked_before_anything_is_created() {
        assert_eq!(check_name("  官网改版 "), Ok("官网改版"));
        assert_eq!(check_name(""), Err("FOLDER_NAME_EMPTY"));
        assert_eq!(check_name("a/b"), Err("FOLDER_NAME_CHARS"));
        assert_eq!(check_name("a:b"), Err("FOLDER_NAME_CHARS"));
        assert_eq!(check_name(".hidden"), Err("FOLDER_NAME_DOT"));
        assert_eq!(check_name(&"长".repeat(121)), Err("FOLDER_NAME_LONG"));
    }

    #[test]
    fn make_refuses_to_reuse_an_existing_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().join("PonteX");
        let made = make(&parent, "合同整理").unwrap();
        assert!(made.is_dir());
        assert_eq!(make(&parent, "合同整理").unwrap_err(), "FOLDER_EXISTS");
    }

    #[test]
    fn the_script_quotes_paths_with_spaces_and_quotes() {
        let script = command_script(
            Path::new("/Users/me/My Work/it's"),
            Path::new("/opt/homebrew/bin/claude"),
        );
        assert!(script.contains("cd -- '/Users/me/My Work/it'\\''s' || exit 1"));
        assert!(script.contains("\n'/opt/homebrew/bin/claude'\n"));
        assert!(script.starts_with("#!/bin/zsh -il\n"));
        assert_eq!(ps_quote("C:\\it's"), "'C:\\it''s'");
    }

    #[test]
    fn the_default_folder_is_in_home_not_documents() {
        assert_eq!(default_dir(None), upstream::home().join("PonteX"));
        assert_eq!(default_dir(Some("  ")), upstream::home().join("PonteX"));
        assert_eq!(default_dir(Some("/tmp/x")), PathBuf::from("/tmp/x"));
    }

    /// 真机上跑：终端真的打开了、进到了文件夹、把命令启动了。会弹窗口，只在 CI 的真机上跑。
    #[test]
    #[ignore = "打开真的终端窗口，只在 CI 真机上跑"]
    fn a_real_terminal_starts_the_agent_in_the_folder() {
        let tmp = tempfile::tempdir().unwrap();
        // 路径里有空格（Windows 上 cmd 写中文会走代码页，只在 macOS 上加中文和引号）。
        let work = if cfg!(windows) {
            tmp.path().join("My Work").join("site v2")
        } else {
            tmp.path().join("My Work's").join("官网改版")
        };
        std::fs::create_dir_all(&work).unwrap();
        let marker = tmp.path().join("marker.txt");
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        #[cfg(windows)]
        let exe = {
            let exe = bin.join("claude.cmd");
            std::fs::write(&exe, format!("@echo off\r\ncd > \"{}\"\r\n", marker.display())).unwrap();
            exe
        };
        #[cfg(not(windows))]
        let exe = {
            use std::os::unix::fs::PermissionsExt;
            let exe = bin.join("claude");
            std::fs::write(&exe, format!("#!/bin/sh\npwd -P > {}\n", sh_quote(&marker.display().to_string()))).unwrap();
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
            exe
        };
        reveal(&work).expect("打开文件夹");
        let terminal = terminals()[0];
        println!("终端：{terminal}");
        open_terminal(terminal, &work, &exe).expect("打开终端");
        let deadline = std::time::Instant::now() + Duration::from_secs(90);
        while !marker.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(500));
        }
        let seen = std::fs::read_to_string(&marker).expect("终端里的命令没有跑起来");
        let seen = PathBuf::from(seen.trim());
        println!("命令在这里跑的：{}", seen.display());
        let want = std::fs::canonicalize(&work).unwrap();
        let strip = |p: &Path| p.display().to_string().trim_start_matches(r"\\?\").to_lowercase();
        assert_eq!(strip(&seen), strip(&want), "命令不是在选的文件夹里跑的");
    }
}
