//! 五个 Agent 的身份表与安装检测。id 只在这里对应，别处不写字面量。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::upstream;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentId {
    ClaudeCode,
    Codex,
    GeminiCli,
    GrokBuild,
    Workbuddy,
}

use AgentId::*;

impl AgentId {
    pub const ALL: [AgentId; 5] = [ClaudeCode, Codex, GeminiCli, GrokBuild, Workbuddy];

    pub fn key(self) -> &'static str {
        match self {
            ClaudeCode => "claude-code",
            Codex => "codex",
            GeminiCli => "gemini-cli",
            GrokBuild => "grok-build",
            Workbuddy => "workbuddy",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|id| id.key() == value)
    }

    pub fn label(self) -> &'static str {
        match self {
            ClaudeCode => "Claude Code",
            Codex => "Codex",
            GeminiCli => "Gemini CLI",
            GrokBuild => "Grok Build",
            Workbuddy => "WorkBuddy",
        }
    }

    pub fn switchable(self) -> bool {
        upstream::official_row(self).is_some()
    }

    pub fn install_url(self) -> &'static str {
        match self {
            ClaudeCode => "https://code.claude.com/docs/en/setup",
            Codex => "https://developers.openai.com/codex/cli",
            GeminiCli => "https://github.com/google-gemini/gemini-cli#installation",
            GrokBuild => "https://docs.x.ai/build/overview",
            Workbuddy => "https://www.workbuddy.ai",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Install {
    Checking,
    Missing,
    /// 找到了可执行文件，但运行 `--version` 失败。
    Broken { path: String },
    Installed { version: Option<String> },
}

impl Install {
    pub fn usable(&self) -> bool {
        matches!(self, Install::Installed { .. })
    }
}

/// 会启动子进程，只在后台线程调用。
pub fn detect(id: AgentId) -> Install {
    match upstream::tool_name(id) {
        Some(tool) => match upstream::tool_version(id) {
            Some(version) => Install::Installed {
                version: Some(version),
            },
            // 我们装的：新终端要等 PATH 生效，这里按记录认。
            None if managed(id).is_some() => Install::Installed { version: managed(id) },
            None => match find_executable(tool) {
                Some(path) => Install::Broken {
                    path: path.display().to_string(),
                },
                // Codex：没有命令行，但装着桌面版，也算装好了——两者读同一份配置。
                None => match (id == Codex).then(app_version).flatten() {
                    Some(version) => Install::Installed { version: Some(version) },
                    None => Install::Missing,
                },
            },
        },
        None => detect_workbuddy(),
    }
}

/// 装着的 Codex 桌面版版本。
pub fn app_version() -> Option<String> {
    super::installer::desktop::installed()
}

fn managed(id: AgentId) -> Option<String> {
    super::installer::layout::Layout::new(upstream::device_dir()).managed(id)
}

pub(crate) fn find_executable(name: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect())
        .unwrap_or_default();
    let home = upstream::home();
    for rel in [".local/bin", ".npm-global/bin", ".bun/bin", ".grok/bin", ".volta/bin"] {
        dirs.push(home.join(rel));
    }
    #[cfg(not(windows))]
    for abs in ["/opt/homebrew/bin", "/usr/local/bin"] {
        dirs.push(PathBuf::from(abs));
    }
    let names: &[String] = &if cfg!(windows) {
        vec![format!("{name}.exe"), format!("{name}.cmd")]
    } else {
        vec![name.to_string()]
    };
    dirs.iter()
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|path| path.is_file())
}

fn detect_workbuddy() -> Install {
    let found = workbuddy_candidates().into_iter().find(|path| path.exists());
    match found {
        Some(path) => Install::Installed {
            version: workbuddy_version(&path),
        },
        None if workbuddy_registered() => Install::Installed { version: None },
        None => Install::Missing,
    }
}

fn workbuddy_candidates() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        vec![
            PathBuf::from("/Applications/WorkBuddy.app"),
            upstream::home().join("Applications/WorkBuddy.app"),
        ]
    }
    #[cfg(target_os = "windows")]
    {
        let mut out = Vec::new();
        for (var, rel) in [
            ("LOCALAPPDATA", r"Programs\WorkBuddy\WorkBuddy.exe"),
            ("ProgramFiles", r"WorkBuddy\WorkBuddy.exe"),
            ("ProgramFiles(x86)", r"WorkBuddy\WorkBuddy.exe"),
        ] {
            if let Some(base) = std::env::var_os(var) {
                out.push(PathBuf::from(base).join(rel));
            }
        }
        out
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
fn workbuddy_version(app: &Path) -> Option<String> {
    let text = std::fs::read_to_string(app.join("Contents/Info.plist")).ok()?;
    let (_, tail) = text.split_once("<key>CFBundleShortVersionString</key>")?;
    let (_, tail) = tail.split_once("<string>")?;
    let (version, _) = tail.split_once("</string>")?;
    Some(version.trim().to_string())
}

#[cfg(not(target_os = "macos"))]
fn workbuddy_version(_app: &Path) -> Option<String> {
    None
}

/// Windows：安装到自定义目录时，从卸载注册表认。
#[cfg(target_os = "windows")]
fn workbuddy_registered() -> bool {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
    [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
        .into_iter()
        .filter_map(|hive| RegKey::predef(hive).open_subkey(UNINSTALL).ok())
        .any(|root| {
            root.enum_keys().flatten().any(|name| {
                root.open_subkey(name)
                    .and_then(|key| key.get_value::<String, _>("DisplayName"))
                    .is_ok_and(|display| display.trim().starts_with("WorkBuddy"))
            })
        })
}

#[cfg(not(target_os = "windows"))]
fn workbuddy_registered() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for id in AgentId::ALL {
            assert_eq!(AgentId::parse(id.key()), Some(id));
            assert_eq!(serde_json::to_value(id).unwrap(), id.key());
        }
        assert_eq!(upstream::tool_name(GrokBuild), Some("grok"));
        assert_eq!(AgentId::parse("grokbuild"), None);
    }
}
