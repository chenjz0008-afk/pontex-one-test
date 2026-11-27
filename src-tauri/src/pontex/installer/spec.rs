//! 四个命令行 Agent 的官方发行包在 npm 上的位置。直接取平台二进制，不经过 npm 和 Node。

use crate::pontex::agents::AgentId;

/// 这台电脑。命名跟 npm 的平台包一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Platform {
    pub os: &'static str,
    pub arch: &'static str,
}

impl Platform {
    pub fn current() -> Option<Self> {
        let os = match std::env::consts::OS {
            "macos" => "darwin",
            "windows" => "win32",
            "linux" => "linux",
            _ => return None,
        };
        let arch = match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" => "x64",
            _ => return None,
        };
        Some(Self { os, arch })
    }

    fn triple(self) -> Option<&'static str> {
        Some(match (self.os, self.arch) {
            ("darwin", "arm64") => "aarch64-apple-darwin",
            ("darwin", "x64") => "x86_64-apple-darwin",
            ("win32", "x64") => "x86_64-pc-windows-msvc",
            ("win32", "arm64") => "aarch64-pc-windows-msvc",
            ("linux", "x64") => "x86_64-unknown-linux-musl",
            ("linux", "arm64") => "aarch64-unknown-linux-musl",
            _ => return None,
        })
    }

    pub fn windows(self) -> bool {
        self.os == "win32"
    }
}

/// 要下载的那个包。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub package: String,
    pub version: String,
    /// 归档根下的入口（已去掉 `package/`）。
    pub entry: String,
    /// 入口是 Brotli 压缩的，先解出来。
    pub brotli: Option<String>,
    /// 入口是脚本，要用自带的 Node 跑。
    pub node: bool,
}

impl Artifact {
    /// registry 上的压缩包路径，各镜像一致。
    pub fn tarball(&self) -> String {
        let base = self.package.rsplit('/').next().unwrap_or(&self.package);
        format!("{}/-/{base}-{}.tgz", self.package, self.version)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Spec {
    pub id: AgentId,
    pub command: &'static str,
    /// 查最新版本的包。
    pub meta: &'static str,
    /// 低于它一律不认：镜像的 `latest` 会滞后（grok 在 npmmirror 上长期停在 0.1.4），也防止
    /// 有人拿一个签名正确的旧版本来替换。
    pub floor: (u64, u64, u64),
    /// 发布 PonteX One 时确认过包结构的版本。最新版装不起来（官方改了包结构、或它自己有问题）时退到这里。
    pub tested: &'static str,
    /// 启动时附带的环境变量：版本由 PonteX One 管，不让它自己更新到别处。
    pub env: &'static [(&'static str, &'static str)],
}

pub const SPECS: [Spec; 4] = [
    Spec {
        id: AgentId::ClaudeCode,
        command: "claude",
        meta: "@anthropic-ai/claude-code",
        floor: (2, 1, 291),
        tested: "2.1.332",
        env: &[("DISABLE_AUTOUPDATER", "1")],
    },
    Spec {
        id: AgentId::Codex,
        command: "codex",
        meta: "@openai/codex",
        floor: (0, 162, 0),
        tested: "0.172.0",
        env: &[],
    },
    Spec {
        id: AgentId::GeminiCli,
        command: "gemini",
        meta: "@google/gemini-cli",
        floor: (0, 63, 0),
        tested: "0.71.1",
        env: &[],
    },
    Spec {
        id: AgentId::GrokBuild,
        command: "grok",
        meta: "@xai-official/grok",
        floor: (1, 0, 80),
        tested: "1.0.92",
        env: &[],
    },
];

pub fn spec(id: AgentId) -> Option<&'static Spec> {
    SPECS.iter().find(|s| s.id == id)
}

impl Spec {
    /// 自己动手时用的一行命令。范围写法：镜像的 `latest` 滞后时 npm 会取范围里最高的正式版。
    pub fn mirror_command(&self) -> String {
        format!(
            "npm install -g \"{}@>={}\" --registry={}",
            self.meta,
            self.tested,
            super::registry::MIRRORS[0]
        )
    }

    pub fn artifact(&self, platform: Platform, version: &str) -> Option<Artifact> {
        let Platform { os, arch } = platform;
        let exe = if platform.windows() { ".exe" } else { "" };
        let native = |package: String, version: String, entry: String| Artifact {
            package,
            version,
            entry,
            brotli: None,
            node: false,
        };
        Some(match self.id {
            AgentId::ClaudeCode => native(
                format!("{}-{os}-{arch}", self.meta),
                version.into(),
                format!("claude{exe}"),
            ),
            AgentId::Codex => native(
                self.meta.into(),
                format!("{version}-{os}-{arch}"),
                format!("vendor/{}/bin/codex{exe}", platform.triple()?),
            ),
            AgentId::GrokBuild => Artifact {
                brotli: Some(format!("bin/grok{exe}.br")),
                ..native(
                    format!("{}-{os}-{arch}", self.meta),
                    version.into(),
                    format!("bin/grok{exe}"),
                )
            },
            AgentId::GeminiCli => Artifact {
                node: true,
                ..native(self.meta.into(), version.into(), "bundle/gemini.js".into())
            },
            AgentId::Workbuddy => return None,
        })
    }
}

/// `1.2.3` 这样的正式版；带后缀的预发布不要。
pub fn stable(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let mut next = || parts.next()?.parse::<u64>().ok();
    let parsed = (next()?, next()?, next()?);
    parts.next().is_none().then_some(parsed)
}

/// Gemini CLI 用的 Node。版本和哈希编进软件，从哪个镜像下都一样可信。
pub mod node {
    use super::Platform;

    pub const VERSION: &str = "24.22.3";

    pub fn file(platform: Platform) -> Option<(String, &'static str)> {
        let sha = match (platform.os, platform.arch) {
            ("darwin", "arm64") => "ff6e17d4874b76d272212b908cc020d6e815fba5a6c7eaaa06add725840307fd",
            ("darwin", "x64") => "977614af0504c81045e7fd85596dbca0e5f951b96681516dadafedcca8c447f7",
            ("win32", "arm64") => "b3cc74a295b39d4f273271e96f58c06dd2b35757ca12abec64c7d0598b434f14",
            ("win32", "x64") => "e612af99f626ab415ea225af6bce5c971feb448cb5cae250bb1c3fa50c5f7d9f",
            _ => return None,
        };
        let name = if platform.windows() {
            format!("node-v{VERSION}-win-{}.zip", platform.arch)
        } else {
            format!("node-v{VERSION}-{}-{}.tar.gz", platform.os, platform.arch)
        };
        Some((name, sha))
    }

    /// 归档里唯一要用的文件。
    pub fn entry(platform: Platform, file: &str) -> String {
        let root = file.trim_end_matches(".tar.gz").trim_end_matches(".zip");
        if platform.windows() {
            format!("{root}/node.exe")
        } else {
            format!("{root}/bin/node")
        }
    }

    pub const SOURCES: [&str; 6] = [
        "https://registry.npmmirror.com/-/binary/node",
        "https://mirrors.cloud.tencent.com/nodejs-release",
        "https://mirrors.huaweicloud.com/nodejs",
        "https://mirrors.aliyun.com/nodejs-release",
        "https://mirrors.tuna.tsinghua.edu.cn/nodejs-release",
        "https://nodejs.org/dist",
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifacts_match_what_is_published() {
        let mac = Platform { os: "darwin", arch: "arm64" };
        let win = Platform { os: "win32", arch: "x64" };
        let at = |id, p, v| spec(id).unwrap().artifact(p, v).unwrap();
        assert_eq!(
            at(AgentId::ClaudeCode, mac, "2.1.332").tarball(),
            "@anthropic-ai/claude-code-darwin-arm64/-/claude-code-darwin-arm64-2.1.332.tgz"
        );
        let codex = at(AgentId::Codex, mac, "0.172.0");
        assert_eq!(codex.tarball(), "@openai/codex/-/codex-0.172.0-darwin-arm64.tgz");
        assert_eq!(codex.entry, "vendor/aarch64-apple-darwin/bin/codex");
        assert_eq!(at(AgentId::Codex, win, "0.172.0").entry, "vendor/x86_64-pc-windows-msvc/bin/codex.exe");
        assert_eq!(at(AgentId::GrokBuild, mac, "1.0.92").brotli.as_deref(), Some("bin/grok.br"));
        let gemini = at(AgentId::GeminiCli, win, "0.71.1");
        assert_eq!(gemini.tarball(), "@google/gemini-cli/-/gemini-cli-0.71.1.tgz");
        assert!(gemini.node);
        assert!(spec(AgentId::Workbuddy).is_none());
    }

    #[test]
    fn only_plain_releases_count_as_stable() {
        assert_eq!(stable("1.0.92"), Some((1, 0, 92)));
        for bad in ["1.0", "1.0.92-alpha.1", "1.0.92.1", "v1.0.0", "", "1..2"] {
            assert_eq!(stable(bad), None, "{bad}");
        }
        // npmmirror 的 grok latest
        assert!(stable("0.1.4").unwrap() < spec(AgentId::GrokBuild).unwrap().floor);
    }

    #[test]
    fn node_entries() {
        let mac = Platform { os: "darwin", arch: "arm64" };
        let (file, _) = node::file(mac).unwrap();
        assert_eq!(node::entry(mac, &file), "node-v24.22.3-darwin-arm64/bin/node");
        let win = Platform { os: "win32", arch: "x64" };
        let (file, _) = node::file(win).unwrap();
        assert_eq!(node::entry(win, &file), "node-v24.22.3-win-x64/node.exe");
    }
}
