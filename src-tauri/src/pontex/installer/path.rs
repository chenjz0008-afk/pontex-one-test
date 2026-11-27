//! 让新终端找得到装好的命令。我们的目录排在 PATH **最后**：任何已有的安装都优先，不会被盖住。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

const BEGIN: &str = "# >>> PonteX One >>>";
const END: &str = "# <<< PonteX One <<<";

/// 会改哪里。给用户在安装前看。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PathPlan {
    /// 已经找得到，什么都不改。
    Ready,
    /// 在这个文件末尾加一段带标记的内容。
    Profile { file: PathBuf },
    /// Windows：当前用户的 Path。
    UserPath,
}

pub fn block(dir: &Path) -> String {
    let dir = dir.display().to_string().replace('"', "\\\"");
    format!(
        "{BEGIN}\ncase \":$PATH:\" in *\":{dir}:\"*) ;; *) export PATH=\"$PATH:{dir}\" ;; esac\n{END}\n"
    )
}

fn fish_block(dir: &Path) -> String {
    let dir = dir.display().to_string().replace('\'', "\\'");
    format!("{BEGIN}\nif not contains -- '{dir}' $PATH\n    set -gx PATH $PATH '{dir}'\nend\n{END}\n")
}

fn csh_block(dir: &Path) -> String {
    format!("{BEGIN}\nsetenv PATH \"${{PATH}}:{}\"\n{END}\n", dir.display())
}

fn pwsh_block(dir: &Path) -> String {
    let dir = dir.display().to_string().replace('\'', "''");
    format!("{BEGIN}\nif (($env:PATH -split ':') -notcontains '{dir}') {{ $env:PATH += ':{dir}' }}\n{END}\n")
}

fn nu_block(dir: &Path) -> String {
    let dir = dir.display().to_string().replace('\\', "\\\\").replace('"', "\\\"");
    format!("{BEGIN}\n$env.PATH = ($env.PATH | split row ':' | append \"{dir}\" | uniq)\n{END}\n")
}

/// 按文件名决定用哪种语法。
fn block_for(file: &Path, dir: &Path) -> String {
    match file.file_name().and_then(|n| n.to_str()).unwrap_or_default() {
        ".cshrc" | ".tcshrc" => csh_block(dir),
        "profile.ps1" => pwsh_block(dir),
        name if name.ends_with(".fish") => fish_block(dir),
        name if name.ends_with(".nu") => nu_block(dir),
        _ => block(dir),
    }
}

/// 每种 shell 打开新终端时真正会读的那个文件。`shell` 是登录 shell 的路径，`zdotdir` 是 zsh 的配置目录。
/// 认不出的都按 POSIX 登录 shell 处理，读 `.profile`。
pub fn profile_for(shell: &str, home: &Path, zdotdir: Option<&Path>, macos: bool) -> PathBuf {
    let name = Path::new(shell).file_name().and_then(|n| n.to_str()).unwrap_or_default();
    match name {
        "zsh" => zdotdir.unwrap_or(home).join(".zshrc"),
        // macOS 的终端开的是登录 shell，只读 .bash_profile。
        "bash" if macos || home.join(".bash_profile").exists() => home.join(".bash_profile"),
        "bash" => home.join(".bashrc"),
        "fish" => home.join(".config/fish/conf.d/pontex-one.fish"),
        "tcsh" if home.join(".tcshrc").exists() => home.join(".tcshrc"),
        "tcsh" | "csh" => home.join(".cshrc"),
        "pwsh" => home.join(".config/powershell/profile.ps1"),
        "nu" if macos && !home.join(".config/nushell").exists() => {
            home.join("Library/Application Support/nushell/config.nu")
        }
        "nu" => home.join(".config/nushell/config.nu"),
        _ => home.join(".profile"),
    }
}

/// 首选的文件写不进去（比如由 dotfiles 工具管着、只读）时的后备。
fn fallback(file: &Path) -> Option<PathBuf> {
    (file.file_name()? == ".zshrc").then(|| file.with_file_name(".zprofile"))
}

/// 在文件末尾加上标记块。跟随软链（很多人的配置是软链到 dotfiles 仓库的），保留权限，原子替换。
pub fn add_block(file: &Path, dir: &Path) -> std::io::Result<()> {
    let body = block_for(file, dir);
    let target = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let existing = match std::fs::read(&target) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e),
    };
    let mut next = without_block(&existing);
    if next == existing && contains(&existing, BEGIN.as_bytes()) {
        // 有开头没结尾：用户动过，不猜。
        return Err(std::io::Error::other("marker damaged"));
    }
    if !next.is_empty() && !next.ends_with(b"\n") {
        next.push(b'\n');
    }
    next.extend_from_slice(body.as_bytes());
    replace(&target, &next)
}

pub fn remove_block(file: &Path) -> std::io::Result<()> {
    let target = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let existing = match std::fs::read(&target) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let next = without_block(&existing);
    if next == existing {
        return Ok(());
    }
    if next.is_empty() && file.extension().is_some_and(|e| e == "fish") {
        return std::fs::remove_file(&target);
    }
    replace(&target, &next)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    find(haystack, needle).is_some()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// 按字节处理：配置文件不一定是 UTF-8。
fn without_block(text: &[u8]) -> Vec<u8> {
    let Some(start) = find(text, BEGIN.as_bytes()) else {
        return text.to_vec();
    };
    let Some(end) = find(&text[start..], END.as_bytes()).map(|i| start + i + END.len()) else {
        return text.to_vec();
    };
    let end = if text.get(end) == Some(&b'\n') { end + 1 } else { end };
    [&text[..start], &text[end..]].concat()
}

fn replace(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let parent = target.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.flush()?;
    match std::fs::metadata(target) {
        Ok(meta) => std::fs::set_permissions(temp.path(), meta.permissions())?,
        #[cfg(unix)]
        Err(_) => {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o644))?;
        }
        #[cfg(not(unix))]
        Err(_) => {}
    }
    temp.persist(target).map(|_| ()).map_err(|e| e.error)
}

// ---------- 现在是什么样 ----------

/// 登录 shell 里的 PATH 和 ZDOTDIR（GUI 应用自己的环境不算数）。
#[cfg(unix)]
fn shell_env(shell: &str) -> Option<(String, Option<PathBuf>)> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    const MARK: &str = "__PONTEX_PATH__";
    let mut child = Command::new(shell)
        .args(["-lic", &format!("printf '%s%s%s%s%s' {MARK} \"$PATH\" {MARK} \"$ZDOTDIR\" {MARK}")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut text = Vec::new();
        let _ = out.read_to_end(&mut text);
        String::from_utf8_lossy(&text).into_owned()
    });
    let until = Instant::now() + Duration::from_secs(8);
    while Instant::now() < until {
        if child.try_wait().ok()?.is_some() {
            let text = reader.join().ok()?;
            let mut parts = text.split(MARK).skip(1);
            let path = parts.next()?.to_string();
            let zdotdir = parts.next().filter(|d| !d.is_empty()).map(PathBuf::from);
            return Some((path, zdotdir.filter(|d| d.is_absolute())));
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    let _ = child.kill();
    let _ = child.wait();
    None
}

#[cfg(unix)]
pub fn plan(dir: &Path, home: &Path) -> PathPlan {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let (path, zdotdir) = shell_env(&shell).unwrap_or_default();
    if std::env::split_paths(&path).any(|p| p == dir) {
        return PathPlan::Ready;
    }
    PathPlan::Profile {
        file: profile_for(&shell, home, zdotdir.as_deref(), cfg!(target_os = "macos")),
    }
}

#[cfg(unix)]
pub fn apply(plan: &PathPlan, dir: &Path) -> Result<(), String> {
    match plan {
        PathPlan::Profile { file } => match (add_block(file, dir), fallback(file)) {
            (Ok(()), _) => Ok(()),
            (Err(_), Some(next)) => add_block(&next, dir).map_err(|e| e.to_string()),
            (Err(error), None) => Err(error.to_string()),
        },
        _ => Ok(()),
    }
}

#[cfg(unix)]
pub fn remove(_dir: &Path, home: &Path) {
    let mut files: Vec<PathBuf> = [
        ".zshrc",
        ".zprofile",
        ".bash_profile",
        ".bashrc",
        ".profile",
        ".cshrc",
        ".tcshrc",
        ".config/fish/conf.d/pontex-one.fish",
        ".config/powershell/profile.ps1",
        ".config/nushell/config.nu",
        "Library/Application Support/nushell/config.nu",
    ]
    .map(|rel| home.join(rel))
    .to_vec();
    let shell = std::env::var("SHELL").unwrap_or_default();
    if let Some((_, Some(dir))) = shell_env(&shell) {
        files.extend([dir.join(".zshrc"), dir.join(".zprofile")]);
    }
    for file in files {
        if let Err(error) = remove_block(&file) {
            log::warn!("[PONTEX] 清理 {} 失败: {error}", file.display());
        }
    }
}

#[cfg(windows)]
mod registry {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::types::FromRegValue;
    use winreg::{RegKey, RegValue};

    fn key() -> std::io::Result<RegKey> {
        RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
    }

    /// 原始值连同类型一起取：展开过再写回会把用户的 `%VAR%` 写死。
    pub fn read() -> std::io::Result<(Option<RegValue>, String)> {
        match key()?.get_raw_value("Path") {
            Ok(value) => {
                let text = String::from_reg_value(&value)?;
                Ok((Some(value), text))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((None, String::new())),
            Err(e) => Err(e),
        }
    }

    pub fn write(previous: Option<RegValue>, text: &str) -> std::io::Result<()> {
        use winreg::enums::REG_EXPAND_SZ;
        let bytes = text.encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes).collect();
        let vtype = previous.map_or(REG_EXPAND_SZ, |p| p.vtype);
        key()?.set_raw_value("Path", &RegValue { bytes, vtype })?;
        broadcast();
        Ok(())
    }

    /// 通知资源管理器重新读环境变量，新开的终端立刻生效。
    fn broadcast() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
        };
        let name: Vec<u16> = "Environment".encode_utf16().chain(Some(0)).collect();
        // SAFETY: 字符串以 NUL 结尾，在调用期间有效；结果指针可以为空。
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                name.as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                3000,
                std::ptr::null_mut(),
            );
        }
    }
}

#[cfg(windows)]
fn same_dir(entry: &str, dir: &Path) -> bool {
    entry
        .trim()
        .trim_end_matches('\\')
        .eq_ignore_ascii_case(&dir.display().to_string())
}

#[cfg(windows)]
pub fn plan(dir: &Path, _home: &Path) -> PathPlan {
    match registry::read() {
        Ok((_, text)) if text.split(';').any(|e| same_dir(e, dir)) => PathPlan::Ready,
        _ => PathPlan::UserPath,
    }
}

#[cfg(windows)]
pub fn apply(plan: &PathPlan, dir: &Path) -> Result<(), String> {
    if *plan != PathPlan::UserPath {
        return Ok(());
    }
    let (previous, text) = registry::read().map_err(|e| e.to_string())?;
    let sep = if text.is_empty() || text.ends_with(';') { "" } else { ";" };
    registry::write(previous, &format!("{text}{sep}{}", dir.display())).map_err(|e| e.to_string())
}

#[cfg(windows)]
pub fn remove(dir: &Path, _home: &Path) {
    let Ok((previous, text)) = registry::read() else { return };
    let kept: Vec<&str> = text.split(';').filter(|e| !same_dir(e, dir)).collect();
    if kept.len() != text.split(';').count() {
        let _ = registry::write(previous, &kept.join(";"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_is_added_once_and_removed_cleanly() {
        let home = tempfile::tempdir().unwrap();
        let rc = home.path().join(".zshrc");
        std::fs::write(&rc, "alias a=b").unwrap();
        let dir = Path::new("/h/.pontex-one/bin");
        add_block(&rc, dir).unwrap();
        add_block(&rc, dir).unwrap();
        let text = std::fs::read_to_string(&rc).unwrap();
        assert_eq!(text.matches(BEGIN).count(), 1);
        assert!(text.starts_with("alias a=b\n"));
        assert!(text.contains("export PATH=\"$PATH:/h/.pontex-one/bin\""), "排在最后");
        // 用户后来在后面加了东西
        std::fs::write(&rc, format!("{text}alias c=d\n")).unwrap();
        remove_block(&rc).unwrap();
        assert_eq!(std::fs::read_to_string(&rc).unwrap(), "alias a=b\nalias c=d\n");
        remove_block(&rc).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn follows_symlinks_and_keeps_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let real = home.path().join("dotfiles/zshrc");
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, b"# caf\xe9\n").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
        let rc = home.path().join(".zshrc");
        std::os::unix::fs::symlink(&real, &rc).unwrap();
        add_block(&rc, Path::new("/x")).unwrap();
        assert!(std::fs::symlink_metadata(&rc).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(std::fs::read(&real).unwrap().starts_with(b"# caf\xe9\n"));
        remove_block(&rc).unwrap();
        assert_eq!(std::fs::read(&real).unwrap(), b"# caf\xe9\n");
    }

    #[test]
    fn a_damaged_marker_is_left_alone() {
        let home = tempfile::tempdir().unwrap();
        let rc = home.path().join(".zshrc");
        let text = format!("a\n{BEGIN}\nsomething\n");
        std::fs::write(&rc, &text).unwrap();
        assert!(add_block(&rc, Path::new("/x")).is_err());
        remove_block(&rc).unwrap();
        assert_eq!(std::fs::read_to_string(&rc).unwrap(), text);
    }

    #[test]
    fn picks_the_file_each_shell_really_reads() {
        let home = tempfile::tempdir().unwrap();
        let pick = |shell, mac| {
            let file = profile_for(shell, home.path(), None, mac);
            file.strip_prefix(home.path()).unwrap().display().to_string()
        };
        assert_eq!(pick("/bin/zsh", true), ".zshrc");
        assert_eq!(pick("/bin/bash", true), ".bash_profile");
        assert_eq!(pick("/bin/bash", false), ".bashrc");
        assert_eq!(pick("/usr/bin/fish", false), ".config/fish/conf.d/pontex-one.fish");
        assert_eq!(pick("/bin/tcsh", true), ".cshrc");
        assert_eq!(pick("/opt/homebrew/bin/pwsh", true), ".config/powershell/profile.ps1");
        assert_eq!(pick("/opt/homebrew/bin/nu", true), "Library/Application Support/nushell/config.nu");
        // 认不出的也不用用户自己动手
        for other in ["/bin/ksh", "/bin/sh", "/opt/homebrew/bin/elvish", ""] {
            assert_eq!(pick(other, true), ".profile");
        }
        let zdot = home.path().join("z");
        assert_eq!(profile_for("/bin/zsh", home.path(), Some(&zdot), true), zdot.join(".zshrc"));
    }

    #[cfg(unix)]
    #[test]
    fn every_syntax_round_trips() {
        let home = tempfile::tempdir().unwrap();
        for name in [".cshrc", "profile.ps1", "config.nu", "pontex-one.fish", ".profile"] {
            let file = home.path().join(name);
            std::fs::write(&file, "# mine\n").unwrap();
            add_block(&file, Path::new("/h/.pontex-one/bin")).unwrap();
            assert!(std::fs::read_to_string(&file).unwrap().contains("/h/.pontex-one/bin"), "{name}");
            remove_block(&file).unwrap();
            assert_eq!(std::fs::read_to_string(&file).unwrap(), "# mine\n", "{name}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_zshrc_falls_back_to_zprofile() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        // 软链到一个不可写的目录，和 nix / dotfiles 工具管着时一样
        let store = home.path().join("store");
        std::fs::create_dir(&store).unwrap();
        std::fs::write(store.join("zshrc"), "# managed\n").unwrap();
        std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o555)).unwrap();
        let rc = home.path().join(".zshrc");
        std::os::unix::fs::symlink(store.join("zshrc"), &rc).unwrap();
        apply(&PathPlan::Profile { file: rc }, Path::new("/x")).unwrap();
        assert!(std::fs::read_to_string(home.path().join(".zprofile")).unwrap().contains("/x"));
        assert_eq!(std::fs::read_to_string(store.join("zshrc")).unwrap(), "# managed\n");
        std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn the_block_really_appends_to_path() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("t.sh");
        std::fs::write(&script, format!("PATH=/first\n{0}{0}printf %s \"$PATH\"", block(Path::new("/my dir/bin")))).unwrap();
        let out = std::process::Command::new("/bin/sh").arg(&script).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "/first:/my dir/bin");
    }
}
