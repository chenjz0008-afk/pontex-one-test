//! 落盘位置。全部在自己的目录里，不碰任何已有的安装：
//!
//! ```text
//! ~/.pontex-one/bin/<命令>              启动脚本，PATH 里排最后
//! ~/.pontex-one/tools/<agent>/<版本>/   官方二进制
//! ~/.pontex-one/runtime/node-<版本>/    只给 Gemini CLI 用
//! ~/.pontex-one/cache/                  没下完的包，下次接着下
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::spec::Spec;
use crate::pontex::agents::AgentId;

#[derive(Debug, Clone)]
pub struct Layout {
    pub root: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Lock {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub tools: BTreeMap<AgentId, Installed>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installed {
    pub version: String,
    pub entry: PathBuf,
    pub previous: Option<String>,
    pub at: i64,
}

impl Layout {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }
    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }
    fn tools(&self) -> PathBuf {
        self.root.join("tools")
    }
    pub fn tool(&self, id: AgentId, version: &str) -> PathBuf {
        self.tools().join(id.key()).join(version)
    }
    pub fn node(&self, version: &str, windows: bool) -> PathBuf {
        self.root
            .join("runtime")
            .join(format!("node-{version}"))
            .join(if windows { "node.exe" } else { "node" })
    }
    pub fn shim(&self, spec: &Spec, windows: bool) -> PathBuf {
        self.bin().join(if windows {
            format!("{}.cmd", spec.command)
        } else {
            spec.command.to_string()
        })
    }

    /// 和正式位置同盘的临时目录，丢弃时自动清掉。
    pub fn staging(&self) -> std::io::Result<tempfile::TempDir> {
        std::fs::create_dir_all(self.tools())?;
        tempfile::Builder::new().prefix(".staging-").tempdir_in(self.tools())
    }

    /// 上次崩溃留下的临时目录、放了一周还没下完的包。
    pub fn sweep(&self) {
        let each = |dir: PathBuf| std::fs::read_dir(dir).into_iter().flatten().flatten();
        for entry in each(self.tools()) {
            if entry.file_name().to_string_lossy().starts_with(".staging-") {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
        for entry in each(self.cache()) {
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age.as_secs() > 7 * 86_400);
            if old {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    fn lock_path(&self) -> PathBuf {
        self.root.join("tools.json")
    }

    pub fn lock(&self) -> Lock {
        std::fs::read(self.lock_path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, lock: &Lock) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(lock).map_err(|e| e.to_string())?;
        crate::pontex::upstream::write_private(&self.lock_path(), &bytes)
    }

    /// 由我们装着、文件也还在的版本。
    pub fn managed(&self, id: AgentId) -> Option<String> {
        self.lock()
            .tools
            .get(&id)
            .filter(|t| t.entry.is_file())
            .map(|t| t.version.clone())
    }

    /// 只留现在这版和上一版。
    pub fn prune(&self, id: AgentId, keep: &[&str]) {
        for entry in std::fs::read_dir(self.tools().join(id.key())).into_iter().flatten().flatten() {
            if !keep.contains(&entry.file_name().to_string_lossy().as_ref()) {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }

    pub fn remove(&self, id: AgentId) {
        let _ = std::fs::remove_dir_all(self.tools().join(id.key()));
    }

    pub fn remove_runtime(&self) {
        let _ = std::fs::remove_dir_all(self.root.join("runtime"));
    }
}

/// 启动脚本。不用符号链接：要带环境变量，Windows 上链接还要管理员权限。
pub fn shim_text(
    windows: bool,
    spec: &Spec,
    bin: &Path,
    node: Option<&Path>,
    entry: &Path,
) -> String {
    if windows {
        // 路径写成相对 bin 的：用户名可能有非 ASCII 字符，批处理按系统代码页读会乱码。
        let rel = |path: &Path| {
            let root = bin.parent().unwrap_or(bin);
            let rel = path.strip_prefix(root).unwrap_or(path);
            format!("%~dp0..\\{}", rel.display().to_string().replace('/', "\\"))
        };
        let env: String = spec.env.iter().map(|(k, v)| format!("set \"{k}={v}\"\r\n")).collect();
        let node = node.map(|n| format!("\"{}\" ", rel(n))).unwrap_or_default();
        format!(
            "@echo off\r\nrem Managed by PonteX One\r\nsetlocal\r\n{env}{node}\"{}\" %*\r\nexit /b %ERRORLEVEL%\r\n",
            rel(entry)
        )
    } else {
        let quote = |path: &Path| format!("'{}'", path.display().to_string().replace('\'', r"'\''"));
        let env: String = spec
            .env
            .iter()
            .map(|(k, v)| format!("{k}=\"${{{k}:-{v}}}\"; export {k}\n"))
            .collect();
        let node = node.map(|n| format!("{} ", quote(n))).unwrap_or_default();
        format!("#!/bin/sh\n# Managed by PonteX One\n{env}exec {node}{} \"$@\"\n", quote(entry))
    }
}

/// 剩余空间；拿不到就当够用，写失败时再报。
pub fn free_space(path: &Path) -> Option<u64> {
    let mut probe = path.to_path_buf();
    while !probe.exists() {
        probe = probe.parent()?.to_path_buf();
    }
    free_space_at(&probe)
}

#[cfg(unix)]
fn free_space_at(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: path 以 NUL 结尾，stat 指向足够大的内存，成功返回后才读取。
    let stat = unsafe {
        if libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) != 0 {
            return None;
        }
        stat.assume_init()
    };
    #[allow(clippy::unnecessary_cast)]
    Some(stat.f_bavail as u64 * stat.f_frsize as u64)
}

#[cfg(windows)]
fn free_space_at(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0u64;
    // SAFETY: 路径以 NUL 结尾，输出指针有效，另外两个输出不需要。
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free)
}

#[cfg(test)]
mod tests {
    use super::super::spec;
    use super::*;

    #[test]
    fn shims_quote_paths_and_pass_arguments_through() {
        let claude = spec::spec(AgentId::ClaudeCode).unwrap();
        let gemini = spec::spec(AgentId::GeminiCli).unwrap();
        let text = shim_text(false, claude, Path::new("/h/o'neil/bin"), None, Path::new("/h/o'neil/tools/claude"));
        assert!(text.starts_with("#!/bin/sh\n"));
        assert!(text.contains(r"exec '/h/o'\''neil/tools/claude' "));
        assert!(text.contains("\"$@\"") && text.contains("DISABLE_AUTOUPDATER"));
        let text = shim_text(false, gemini, Path::new("/r/bin"), Some(Path::new("/r/node")), Path::new("/r/g.js"));
        assert!(text.contains("exec '/r/node' '/r/g.js' \"$@\""));
        let text = shim_text(
            true,
            claude,
            Path::new("/中文/.pontex-one/bin"),
            None,
            Path::new("/中文/.pontex-one/tools/claude-code/1.0.0/claude.exe"),
        );
        assert!(text.contains("\"%~dp0..\\tools\\claude-code\\1.0.0\\claude.exe\" %*"), "{text}");
        assert!(text.is_ascii());
    }

    #[test]
    fn free_space_is_known_for_paths_that_do_not_exist_yet() {
        let dir = tempfile::tempdir().unwrap();
        assert!(free_space(&dir.path().join("a/b/c")).unwrap() > 0);
    }
}
