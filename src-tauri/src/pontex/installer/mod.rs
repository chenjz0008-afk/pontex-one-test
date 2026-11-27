//! 安装没装的 Agent。
//!
//! - 只在**没有检测到**时才装；已经装着的（不管谁装的）一概不碰。
//! - 不用 npm、不要 Node、不要管理员权限：直接取官方平台二进制，验 npm 官方签名和 SHA-512。
//! - 装到自己的目录，先跑通 `--version` 再启用；任何一步失败都不留半成品。

pub mod archive;
pub mod desktop;
mod desktop_mac;
mod desktop_win;
pub mod download;
pub mod layout;
pub mod path;
pub mod pe;
pub mod registry;
pub mod spec;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use serde::Serialize;

use self::download::{DownloadError, Expect};
use self::layout::{Installed, Layout};
use self::path::PathPlan;
use self::registry::{RegistryError, Trusted};
use self::spec::{stable, Artifact, Platform, Spec};
use super::agents::AgentId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Queued,
    Resolving,
    Downloading,
    Verifying,
    /// 等用户在系统的管理员确认里点「是」。
    Authorizing,
    Unpacking,
    Testing,
    Activating,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub agent: AgentId,
    pub phase: Phase,
    pub version: Option<String>,
    pub received: u64,
    pub total: Option<u64>,
    /// 正在用的源，只给主机名。
    pub source: Option<String>,
    /// 下载的是自带的 Node，而不是 Agent 本身。
    pub runtime: bool,
    /// 这一轮没下完，几秒后自动重试。
    pub wait: Option<u64>,
    /// 装的是 Codex 桌面版，不是命令行。
    pub desktop: bool,
    pub error: Option<String>,
}

impl Progress {
    pub fn new(agent: AgentId, phase: Phase) -> Self {
        Self {
            agent,
            phase,
            version: None,
            received: 0,
            total: None,
            source: None,
            runtime: false,
            wait: None,
            desktop: false,
            error: None,
        }
    }
}

/// 一个 Agent 的安装计划，或查不到的原因。各查各的，一个查不到不影响其他。
#[derive(Debug, Clone, Serialize)]
pub struct Planned {
    pub agent: AgentId,
    pub plan: Option<Plan>,
    pub error: Option<String>,
    /// Codex：桌面版的计划（这台电脑能装、还没装时才有）。
    pub app: Option<desktop::AppPlan>,
    pub app_error: Option<String>,
}

/// 装之前给用户看的。
#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub agent: AgentId,
    pub version: String,
    /// 装好之后占多少。
    pub unpacked: u64,
    pub dir: String,
    pub path: PathPlan,
    pub needs_node: bool,
}

pub const OFFLINE: &str = "INSTALL_OFFLINE";
pub const MISSING: &str = "INSTALL_MISSING";
pub const UNTRUSTED: &str = "INSTALL_UNTRUSTED";
pub const CHECKSUM: &str = "INSTALL_CHECKSUM";
pub const DISK: &str = "INSTALL_DISK";
pub const ARCHIVE: &str = "INSTALL_ARCHIVE";
pub const RUN: &str = "INSTALL_RUN";
pub const CANCELLED: &str = "INSTALL_CANCELLED";
pub const UNSUPPORTED: &str = "INSTALL_UNSUPPORTED";
pub const EXISTS: &str = "INSTALL_EXISTS";

impl From<DownloadError> for String {
    fn from(error: DownloadError) -> Self {
        match error {
            DownloadError::Offline => OFFLINE,
            DownloadError::Cancelled => CANCELLED,
            DownloadError::Disk => DISK,
            DownloadError::TooLarge => ARCHIVE,
            DownloadError::Checksum => CHECKSUM,
        }
        .into()
    }
}

impl From<RegistryError> for String {
    fn from(error: RegistryError) -> Self {
        match error {
            RegistryError::Offline => OFFLINE,
            RegistryError::Missing => MISSING,
            RegistryError::Untrusted => UNTRUSTED,
        }
        .into()
    }
}

impl From<archive::ArchiveError> for String {
    fn from(error: archive::ArchiveError) -> Self {
        log::warn!("[PONTEX] 解包失败: {error:?}");
        match error {
            archive::ArchiveError::Bad(_) => ARCHIVE.into(),
            archive::ArchiveError::Io(text) if text.contains("No space") || text.contains("os error 112") => DISK.into(),
            archive::ArchiveError::Io(_) => ARCHIVE.into(),
        }
    }
}

/// 这台电脑上能不能装它。
pub fn supported(id: AgentId) -> bool {
    Platform::current()
        .zip(spec::spec(id))
        .is_some_and(|(p, s)| s.artifact(p, "0.0.0").is_some_and(|a| !a.node || spec::node::file(p).is_some()))
}

/// 这台电脑能不能装 Codex 桌面版。
pub fn app_supported() -> bool {
    Platform::current().is_some_and(desktop::supported)
}

pub struct Installer {
    pub layout: Layout,
    pub client: reqwest::Client,
    pub mirrors: Vec<String>,
    pub node_sources: Vec<String>,
    pub platform: Platform,
    pub home: PathBuf,
}

pub type Notify<'a> = dyn Fn(Progress) + Send + Sync + 'a;

impl Installer {
    /// 正式环境用的那一个。
    pub fn system() -> Result<Self, String> {
        Ok(Self {
            layout: Layout::new(super::upstream::device_dir()),
            client: super::upstream::https()?,
            mirrors: mirrors(std::env::var("PONTEX_NPM_MIRRORS").ok().as_deref()),
            node_sources: spec::node::SOURCES.map(str::to_string).to_vec(),
            platform: Platform::current().ok_or(UNSUPPORTED)?,
            home: super::upstream::home(),
        })
    }

    fn artifact(&self, spec: &Spec, version: &str) -> Result<Artifact, String> {
        spec.artifact(self.platform, version).ok_or_else(|| UNSUPPORTED.into())
    }

    async fn resolve(&self, spec: &Spec) -> Result<(String, Artifact, Trusted), String> {
        let version = registry::latest(&self.client, &self.mirrors, spec).await?;
        let artifact = self.artifact(spec, &version)?;
        let trusted = registry::resolve(&self.client, &self.mirrors, &artifact).await?;
        Ok((version, artifact, trusted))
    }

    pub async fn path_plan(&self) -> Result<PathPlan, String> {
        let (bin, home) = (self.layout.bin(), self.home.clone());
        blocking(move || path::plan(&bin, &home)).await
    }

    pub async fn plan(&self, id: AgentId, path: PathPlan) -> Result<Plan, String> {
        let spec = spec::spec(id).ok_or(UNSUPPORTED)?;
        let (version, artifact, trusted) = self.resolve(spec).await?;
        Ok(Plan {
            agent: id,
            version,
            unpacked: trusted.unpacked,
            dir: self.layout.root.display().to_string(),
            path,
            needs_node: artifact.node && !self.node_path().is_file(),
        })
    }

    pub async fn install(
        &self,
        id: AgentId,
        cancel: &AtomicBool,
        notify: &Notify<'_>,
    ) -> Result<String, String> {
        let spec = spec::spec(id).ok_or(UNSUPPORTED)?;
        notify(Progress::new(id, Phase::Resolving));
        let newest = registry::latest(&self.client, &self.mirrors, spec).await?;
        match self.install_version(spec, &newest, cancel, notify).await {
            Ok(()) => return Ok(newest),
            // 内容是真的，但结构变了或跑不起来：退到确认过的版本，总比装不上强。升级时不退，
            // 手里那一版还好好的。
            Err(error) if [MISSING, ARCHIVE, RUN].contains(&error.as_str())
                && stable(&newest) > stable(spec.tested)
                && self.layout.managed(id).is_none() =>
            {
                log::warn!("[PONTEX] {} {newest} 装不起来（{error}），改装 {}", id.key(), spec.tested);
            }
            Err(error) => return Err(error),
        }
        self.install_version(spec, spec.tested, cancel, notify).await?;
        Ok(spec.tested.into())
    }

    async fn install_version(
        &self,
        spec: &Spec,
        version: &str,
        cancel: &AtomicBool,
        notify: &Notify<'_>,
    ) -> Result<(), String> {
        let artifact = self.artifact(spec, version)?;
        let trusted = registry::resolve(&self.client, &self.mirrors, &artifact).await?;
        self.install_resolved(spec, version, &artifact, &trusted, cancel, notify).await
    }

    /// 我们装的这个有没有新版。查不到就当没有。
    pub async fn newer(&self, id: AgentId) -> Option<String> {
        let current = stable(&self.layout.managed(id)?)?;
        let newest = registry::latest(&self.client, &self.mirrors, spec::spec(id)?).await.ok()?;
        (stable(&newest)? > current).then_some(newest)
    }

    /// 已经验过真的包：下载、校验、解包、试跑、启用。
    pub async fn install_resolved(
        &self,
        spec: &Spec,
        version: &str,
        artifact: &Artifact,
        trusted: &Trusted,
        cancel: &AtomicBool,
        notify: &Notify<'_>,
    ) -> Result<(), String> {
        let id = spec.id;
        let say = |phase| {
            notify(Progress { version: Some(version.to_string()), ..Progress::new(id, phase) });
        };
        // 下载的包 + 解开的文件同时在盘上，再留一点余量。
        let need = trusted.unpacked.saturating_mul(17) / 10 + (256 << 20);
        if layout::free_space(&self.layout.root).is_some_and(|free| free < need) {
            return Err(DISK.into());
        }
        let cache = self.layout.cache();
        std::fs::create_dir_all(&cache).map_err(|_| DISK)?;

        if artifact.node {
            self.ensure_node(id, version, cancel, notify).await?;
        }

        let file = cache.join(format!(
            "{}-{}.tgz",
            artifact.package.replace(['/', '@'], "_"),
            artifact.version
        ));
        let urls: Vec<String> =
            self.mirrors.iter().map(|m| format!("{m}/{}", artifact.tarball())).collect();
        say(Phase::Downloading);
        download::fetch(
            &self.client,
            &urls,
            &file,
            None,
            Expect::Sha512(&trusted.sha512),
            cancel,
            &|status| notify(downloading(id, version, false, status)),
        )
        .await?;

        say(Phase::Unpacking);
        let staging = self.layout.staging().map_err(|_| DISK)?;
        let (from, into, art) = (file.clone(), staging.path().to_path_buf(), artifact.clone());
        let command = spec.command;
        let found = blocking(move || unpack(&from, &into, &art, command)).await??;

        say(Phase::Testing);
        let entry = staging.path().join(&found);
        let node = artifact.node.then(|| self.node_path());
        // Windows：先确认它要的 DLL 都在，再试跑。缺 DLL 时系统会弹窗，那一下不能让用户看到。
        if self.platform.windows() {
            self.ensure_dlls(node.as_deref().unwrap_or(&entry), id, notify).await?;
        }
        let (probe_node, probe_entry) = (node.clone(), entry.clone());
        blocking(move || runs(probe_node.as_deref(), &probe_entry)).await??;

        say(Phase::Activating);
        let target = self.layout.tool(id, version);
        let activate = || -> Result<(), String> {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            if target.exists() {
                std::fs::remove_dir_all(&target).map_err(|e| e.to_string())?;
            }
            let staged = staging.keep();
            if let Err(error) = std::fs::rename(&staged, &target) {
                let _ = std::fs::remove_dir_all(&staged);
                return Err(error.to_string());
            }
            let entry = target.join(&found);
            self.write_shim(spec, node.as_deref(), &entry)?;
            let mut lock = self.layout.lock();
            let previous = lock.tools.get(&id).map(|t| t.version.clone()).filter(|v| v != version);
            lock.version = 1;
            lock.tools.insert(
                id,
                Installed {
                    version: version.into(),
                    entry,
                    previous: previous.clone(),
                    at: chrono::Utc::now().timestamp(),
                },
            );
            self.layout.save(&lock)?;
            let mut keep = vec![version];
            keep.extend(previous.as_deref());
            self.layout.prune(id, &keep);
            Ok(())
        };
        activate()?;
        let _ = std::fs::remove_file(&file);
        say(Phase::Done);
        Ok(())
    }

    fn write_shim(&self, spec: &Spec, node: Option<&Path>, entry: &Path) -> Result<(), String> {
        let windows = self.platform.windows();
        let bin = self.layout.bin();
        let shim = self.layout.shim(spec, windows);
        let text = layout::shim_text(windows, spec, &bin, node, entry);
        super::upstream::write_private(&shim, text.as_bytes())?;
        archive::make_executable(&shim).map_err(|e| e.to_string())
    }

    fn node_path(&self) -> PathBuf {
        self.layout.node(spec::node::VERSION, self.platform.windows())
    }

    async fn ensure_node(
        &self,
        id: AgentId,
        version: &str,
        cancel: &AtomicBool,
        notify: &Notify<'_>,
    ) -> Result<(), String> {
        let target = self.node_path();
        if target.is_file() {
            return Ok(());
        }
        let (name, sha) = spec::node::file(self.platform).ok_or(UNSUPPORTED)?;
        let urls: Vec<String> = self
            .node_sources
            .iter()
            .map(|s| format!("{s}/v{}/{name}", spec::node::VERSION))
            .collect();
        let file = self.layout.cache().join(&name);
        download::fetch(
            &self.client,
            &urls,
            &file,
            None,
            Expect::Sha256(sha),
            cancel,
            &|status| notify(downloading(id, version, true, status)),
        )
        .await?;
        let staging = self.layout.staging().map_err(|_| DISK)?;
        let out = staging.path().join(target.file_name().unwrap_or_default());
        let entry = spec::node::entry(self.platform, &name);
        let (from, to) = (file.clone(), out.clone());
        blocking(move || extract_node(&from, &entry, &to)).await??;
        let probe = out.clone();
        blocking(move || runs(None, &probe)).await??;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&out, &target).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(file);
        Ok(())
    }

    /// 只移除我们装的。
    pub fn uninstall(&self, id: AgentId) -> Result<(), String> {
        let spec = spec::spec(id).ok_or(UNSUPPORTED)?;
        let mut lock = self.layout.lock();
        if lock.tools.remove(&id).is_none() {
            return Err(EXISTS.into());
        }
        let _ = std::fs::remove_file(self.layout.shim(spec, self.platform.windows()));
        self.layout.save(&lock)?;
        self.layout.remove(id);
        if !lock.tools.contains_key(&AgentId::GeminiCli) {
            self.layout.remove_runtime();
        }
        if lock.tools.is_empty() {
            path::remove(&self.layout.bin(), &self.home);
        }
        Ok(())
    }
}

/// 下载中的进度。
pub fn downloading(id: AgentId, version: &str, runtime: bool, status: download::Status<'_>) -> Progress {
    Progress {
        version: Some(version.to_string()),
        received: status.received,
        total: status.total,
        source: (!status.source.is_empty()).then(|| status.source.to_string()),
        runtime,
        wait: status.wait,
        ..Progress::new(id, Phase::Downloading)
    }
}

/// 企业内网可以换成自建镜像（只认 https）。内容照样验签，所以不需要信任它。
fn mirrors(custom: Option<&str>) -> Vec<String> {
    let custom: Vec<String> = custom
        .unwrap_or_default()
        .split([' ', ',', ';'])
        .map(|s| s.trim().trim_end_matches('/'))
        .filter(|s| s.starts_with("https://"))
        .map(str::to_string)
        .collect();
    if custom.is_empty() {
        registry::MIRRORS.map(str::to_string).to_vec()
    } else {
        custom
    }
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| e.to_string())
}

/// 解包并找到入口，返回相对路径。
fn unpack(file: &Path, into: &Path, artifact: &Artifact, command: &str) -> Result<PathBuf, String> {
    // npm 包的根目录通常叫 `package`，但不保证：去掉第一层就是。
    archive::tar_gz(file, into, &|name| name.split_once('/').map(|(_, rest)| rest.to_string()))?;
    let entry = match &artifact.brotli {
        Some(packed) if into.join(packed).is_file() => PathBuf::from(packed),
        _ if into.join(&artifact.entry).is_file() => PathBuf::from(&artifact.entry),
        _ => discover(into, artifact, command).ok_or(ARCHIVE)?,
    };
    let entry = if entry.extension().is_some_and(|e| e == "br") {
        let plain = entry.with_extension("");
        // `grok.exe.br` → `grok.exe`
        if !into.join(&plain).is_file() {
            archive::brotli(&into.join(&entry), &into.join(&plain))?;
        }
        let _ = std::fs::remove_file(into.join(&entry));
        plain
    } else {
        entry
    };
    if !artifact.node {
        archive::make_executable(&into.join(&entry)).map_err(|e| e.to_string())?;
    }
    Ok(entry)
}

/// 官方把入口挪了位置时自己找。包的内容已经过签名校验，里面的 `package.json` 可信。
fn discover(root: &Path, artifact: &Artifact, command: &str) -> Option<PathBuf> {
    if artifact.node {
        let package: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("package.json")).ok()?).ok()?;
        let bin = &package["bin"];
        let path = bin.as_str().or_else(|| bin[command].as_str())?;
        let path = path.trim_start_matches("./");
        return (super::safe_path::safe_relative(path) && root.join(path).is_file())
            .then(|| PathBuf::from(path));
    }
    let exe = if artifact.entry.ends_with(".exe") { format!("{command}.exe") } else { command.into() };
    let packed = format!("{exe}.br");
    let mut found = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if *entry.file_name() == *exe || *entry.file_name() == *packed {
                found.push(path);
            }
        }
    }
    // 有歧义就不猜。
    match found.as_slice() {
        [one] => one.strip_prefix(root).ok().map(Path::to_path_buf),
        _ => None,
    }
}

fn extract_node(file: &Path, entry: &str, target: &Path) -> Result<(), String> {
    if file.extension().is_some_and(|e| e == "zip") {
        archive::zip_one(file, entry, target)?;
        return Ok(());
    }
    let dir = target.parent().unwrap_or(Path::new("."));
    let name = target.file_name().unwrap_or_default().to_string_lossy().to_string();
    let count = archive::tar_gz(file, dir, &|path| (path == entry).then(|| name.clone()))?;
    if count == 1 {
        Ok(())
    } else {
        Err(ARCHIVE.into())
    }
}

/// 真的跑一下。HOME 指到临时目录，不让它在用户目录里留下任何东西。
fn runs(node: Option<&Path>, entry: &Path) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let home = tempfile::tempdir().map_err(|_| DISK)?;
    let mut command = match node {
        Some(node) => {
            let mut c = Command::new(node);
            c.arg(entry);
            c
        }
        None => Command::new(entry),
    };
    command
        .arg("--version")
        .current_dir(home.path())
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("DISABLE_AUTOUPDATER", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(|e| {
        log::warn!("[PONTEX] 试跑失败: {e}");
        RUN.to_string()
    })?;
    // 几百 MB 的二进制第一次跑要等系统扫描。
    let until = Instant::now() + Duration::from_secs(90);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                log::warn!("[PONTEX] 试跑退出: {status}");
                return Err(RUN.into());
            }
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(60)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RUN.into());
            }
        }
    }
}

#[cfg(test)]
mod e2e;
#[cfg(test)]
mod tests;
