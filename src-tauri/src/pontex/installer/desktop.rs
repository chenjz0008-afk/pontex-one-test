//! Codex 桌面版（ChatGPT 桌面应用里的 Codex）：选源、下载、三层校验、装进系统。
//!
//! 官方只发两种包：macOS 的 DMG、Windows 的 MSIX（商店签名）。我们不碰它们的内容，原样装。
//!
//! 源（都不必可信，内容由哈希和系统签名把关）：
//! - 官方 CDN：只有 macOS 的 DMG；
//! - A：codexapp.agentsmirror.com；
//! - B：自己的 R2，由公开仓库 pontex-codex-mirror 的工作流验过签名才同步上去；
//! - C：GitHub Releases，经 gh-proxy 或直连。
//!
//! 清单：A、B、C 各取一份 `release-manifest.json`，取其中最新的版本；同一版本的几份清单，
//! 长度和 SHA-256 必须完全一致，否则这一轮作废（有人改了其中一份）。B 同步慢于 A 是常态，不算冲突。
//!
//! 校验三层：① 长度 ② SHA-256 ③ 系统签名——macOS 认苹果签名里的 OpenAI Team ID、应用 ID
//! 和公证；Windows 认 MSIX 的包身份，安装时由 Windows 验商店签名，装完再核对一次。
//! 前两层镜像能一起改，第三层改不了，所以第三层不可省。

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use serde::Serialize;
use serde_json::Value;

use super::download::{self, Expect};
use super::spec::Platform;
use super::{Installer, Notify, Phase, Progress, DISK, OFFLINE, UNSUPPORTED};
use crate::pontex::agents::AgentId;

pub const MIRROR_A: &str = "https://codexapp.agentsmirror.com";
pub const GITHUB: &str = "https://github.com/Wangnov/codex-app-mirror";
pub const GH_PROXY: &str = "https://gh-proxy.org/";
/// 自己的 R2（CI 验过签名才同步上去）。`PONTEX_CODEX_APP_MIRROR` 可以临时换成别的。
pub const MIRROR_B: &str = "https://mirror.ponte-x.com";

/// macOS：苹果签名里 OpenAI 的 Team ID 和应用 ID。
pub const TEAM_ID: &str = "2DC432GLL2";
pub const BUNDLE_ID: &str = "com.openai.codex";
/// Windows：MSIX 的包名、发布者，以及由发布者算出来的包族名。
pub const PACKAGE_NAME: &str = "OpenAI.Codex";
pub const PUBLISHER: &str = "CN=50BDFD77-8903-4850-9FFE-6E8522F64D5B";
pub const FAMILY: &str = "OpenAI.Codex_2p2nqsd0c76g0";
/// 清单里同一版本的几份对不上。
pub const CONFLICT: &str = "INSTALL_CONFLICT";
/// 系统版本太旧。
pub const OLD_OS: &str = "INSTALL_OLD_OS";
/// 「应用程序」里已经有一个同名、但不是 Codex 的 ChatGPT。
pub const TAKEN: &str = "INSTALL_APP_TAKEN";
/// 用户在系统的管理员确认里点了取消。
pub const DENIED: &str = "INSTALL_DENIED";

/// 这台电脑对应清单里的哪一份包。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    MacArm64,
    MacX64,
    WinX64,
    WinArm64,
}

impl Kind {
    pub fn of(platform: Platform) -> Option<Self> {
        Some(match (platform.os, platform.arch) {
            ("darwin", "arm64") => Kind::MacArm64,
            ("darwin", "x64") => Kind::MacX64,
            ("win32", "x64") => Kind::WinX64,
            ("win32", "arm64") => Kind::WinArm64,
            _ => return None,
        })
    }
    /// A 镜像 `/latest/<这个>` 指向的就是这份包。
    fn latest_key(self) -> &'static str {
        match self {
            Kind::MacArm64 => "mac-arm64",
            Kind::MacX64 => "mac-intel",
            Kind::WinX64 => "win-x64",
            Kind::WinArm64 => "win-arm64",
        }
    }
    pub fn windows(self) -> bool {
        matches!(self, Kind::WinX64 | Kind::WinArm64)
    }
}

/// 一份包：文件名、长度、哈希、能从哪下。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub version: String,
    pub file: String,
    pub size: u64,
    pub sha256: String,
    /// 官方 CDN 上的同一个文件（只有 macOS 有）。
    pub official: Option<String>,
    pub min_os: Option<String>,
}

/// 装之前给用户看的。
#[derive(Debug, Clone, Serialize)]
pub struct AppPlan {
    pub version: String,
    /// 要下载多少。
    pub size: u64,
    /// 装到哪里；Windows 是系统的应用列表，没有路径可说。
    pub dir: Option<String>,
    /// Windows 要弹一次管理员确认（包里带一个系统服务）。
    pub admin: bool,
}

/* ───────────────────────── 清单 ───────────────────────── */

/// 读一份 `release-manifest.json`（schema 5），取出这台电脑要的那份包。格式不对、身份不对一律 None。
pub fn parse(manifest: &Value, kind: Kind) -> Option<Asset> {
    let version = manifest["codexVersion"].as_str()?.to_string();
    let sums = &manifest["derived"]["latestChecksums"];
    let sum = |file: &str| sums[file].as_str().map(str::to_ascii_lowercase);
    match kind {
        Kind::MacArm64 | Kind::MacX64 => {
            let arch = if kind == Kind::MacArm64 { "arm64" } else { "x64" };
            let s = &manifest["sources"]["macos"][arch];
            // 清单自己说的身份也得是 OpenAI 的 Codex：不是就不理这份清单。
            if s["teamIdentifier"].as_str()? != TEAM_ID || s["bundleIdentifier"].as_str()? != BUNDLE_ID {
                return None;
            }
            let file = format!("Codex-mac-{arch}.dmg");
            let sha256 = s["sha256"].as_str()?.to_ascii_lowercase();
            if sum(&file).is_some_and(|other| other != sha256) {
                return None;
            }
            let official = s["url"].as_str().filter(|u| u.starts_with("https://persistent.oaistatic.com/"));
            Some(Asset {
                version,
                file,
                size: s["contentLength"].as_u64()?,
                sha256: hex64(&sha256)?,
                official: official.map(str::to_string),
                min_os: s["minimumSystemVersion"].as_str().map(str::to_string),
            })
        }
        Kind::WinX64 | Kind::WinArm64 => {
            let arch = if kind == Kind::WinX64 { "x64" } else { "arm64" };
            let w = &manifest["sources"]["windows"]["architectures"][arch];
            let moniker = w["packageMoniker"].as_str()?;
            if !moniker.starts_with(&format!("{PACKAGE_NAME}_")) || !moniker.ends_with(&format!("_{arch}__2p2nqsd0c76g0")) {
                return None;
            }
            let file = format!("{moniker}.Msix");
            let sha256 = sum(&file)?;
            // 微软商店目录里登记的哈希（base64）：和镜像算的必须是同一个。
            if let Some(catalog) = w["catalog"]["hash"].as_str() {
                use base64::Engine;
                let raw = base64::engine::general_purpose::STANDARD.decode(catalog).ok()?;
                if hex(&raw) != sha256 {
                    return None;
                }
            }
            Some(Asset {
                version,
                file,
                size: w["contentLength"].as_u64()?,
                sha256: hex64(&sha256)?,
                official: None,
                min_os: Some("10.0.19041".into()),
            })
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex64(text: &str) -> Option<String> {
    (text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())).then(|| text.to_string())
}

/// `26.1120.50957` 这样的版本号，逐段按数字比。
pub fn numbers(version: &str) -> Vec<u64> {
    version.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// 几份清单合在一起：取最新版本；同一版本的几份必须完全一致。
pub fn pick(found: &[(Source, Asset)]) -> Result<Asset, String> {
    let newest = found.iter().map(|(_, a)| a).max_by(|a, b| numbers(&a.version).cmp(&numbers(&b.version)));
    let Some(newest) = newest else { return Err(OFFLINE.into()) };
    let same: Vec<&Asset> = found.iter().map(|(_, a)| a).filter(|a| a.version == newest.version).collect();
    if same.iter().any(|a| a.sha256 != newest.sha256 || a.size != newest.size || a.file != newest.file) {
        log::warn!("[PONTEX] Codex 桌面版 {} 的清单互相对不上", newest.version);
        return Err(CONFLICT.into());
    }
    // 官方地址只要有一份清单给了就用
    let official = same.iter().find_map(|a| a.official.clone());
    Ok(Asset { official, ..newest.clone() })
}

/// 清单从哪来。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    A,
    B,
    GitHub,
}

fn mirror_b() -> Option<String> {
    let from_env = std::env::var("PONTEX_CODEX_APP_MIRROR").ok();
    let base = from_env.as_deref().unwrap_or(MIRROR_B).trim().trim_end_matches('/');
    base.starts_with("https://").then(|| base.to_string())
}

fn manifest_urls() -> Vec<(Source, String)> {
    let mut out = vec![(Source::A, format!("{MIRROR_A}/latest/manifest"))];
    if let Some(b) = mirror_b() {
        out.push((Source::B, format!("{b}/codex-app/latest/release-manifest.json")));
    }
    let github = format!("{GITHUB}/releases/latest/download/release-manifest.json");
    out.push((Source::GitHub, format!("{GH_PROXY}{github}")));
    out.push((Source::GitHub, github));
    out
}

/// 这份包能从哪些地址下。同一个文件，下载器会全部探一遍、挑快的并行下。
pub fn urls(asset: &Asset, kind: Kind, have: &[Source]) -> Vec<String> {
    let mut out = Vec::new();
    out.extend(asset.official.clone());
    if let Some(b) = mirror_b() {
        out.push(format!("{b}/codex-app/releases/{}/{}", asset.version, asset.file));
    }
    // A 只有 /latest：它的清单不是这个版本，就别去它那下（长度也对不上，白探一次）。
    if have.contains(&Source::A) {
        out.push(format!("{MIRROR_A}/latest/{}", kind.latest_key()));
    }
    let github = format!("{GITHUB}/releases/download/codex-app-{}/{}", asset.version, asset.file);
    out.push(format!("{GH_PROXY}{github}"));
    out.push(github);
    out
}

/// 各源的清单一起取，取到几份算几份。
async fn resolve(client: &reqwest::Client, kind: Kind) -> Result<(Asset, Vec<String>), String> {
    let fetch = |source: Source, url: String| async move {
        let response = tokio::time::timeout(std::time::Duration::from_secs(20), client.get(&url).send()).await.ok()?.ok()?;
        if !response.status().is_success() {
            return None;
        }
        let body = tokio::time::timeout(std::time::Duration::from_secs(20), response.bytes()).await.ok()?.ok()?;
        let manifest: Value = serde_json::from_slice(&body).ok()?;
        Some((source, parse(&manifest, kind)?))
    };
    let mut found: Vec<(Source, Asset)> = futures::future::join_all(manifest_urls().into_iter().map(|(s, u)| fetch(s, u)))
        .await
        .into_iter()
        .flatten()
        .collect();
    // 同一个源（GitHub 两条路）只留一份
    found.sort_by_key(|a| a.0);
    found.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    let asset = pick(&found)?;
    let have: Vec<Source> = found.iter().filter(|(_, a)| a.version == asset.version).map(|(s, _)| *s).collect();
    let urls = urls(&asset, kind, &have);
    Ok((asset, urls))
}

/* ───────────────────────── 检测 ───────────────────────── */

/// 装着的 Codex 桌面版版本。没装返回 None。只读文件和注册表，不起进程。
pub fn installed() -> Option<String> {
    #[cfg(target_os = "macos")]
    return super::desktop_mac::installed();
    #[cfg(target_os = "windows")]
    return super::desktop_win::installed();
    #[allow(unreachable_code)]
    None
}

/// 这台电脑能不能装桌面版。
pub fn supported(platform: Platform) -> bool {
    Kind::of(platform).is_some()
}

/* ───────────────────────── 计划与安装 ───────────────────────── */

impl Installer {
    pub async fn app_plan(&self) -> Result<AppPlan, String> {
        let kind = Kind::of(self.platform).ok_or(UNSUPPORTED)?;
        let (asset, _) = resolve(&self.client, kind).await?;
        os_new_enough(asset.min_os.as_deref())?;
        Ok(AppPlan {
            version: asset.version,
            size: asset.size,
            dir: (!kind.windows()).then(|| applications_dir().display().to_string()),
            admin: kind.windows(),
        })
    }

    /// 下载、校验、装进系统。返回装上的版本。
    pub async fn install_app(&self, cancel: &AtomicBool, notify: &Notify<'_>) -> Result<String, String> {
        let id = AgentId::Codex;
        let kind = Kind::of(self.platform).ok_or(UNSUPPORTED)?;
        let say = |phase, version: &str| {
            notify(Progress { version: Some(version.into()), desktop: true, ..Progress::new(id, phase) })
        };
        // 已经装着（用户自己装的、或者之前装过）：什么都不动。之后由它自己更新。
        if let Some(version) = installed() {
            say(Phase::Done, &version);
            return Ok(version);
        }
        notify(Progress { desktop: true, ..Progress::new(id, Phase::Resolving) });
        let (asset, urls) = resolve(&self.client, kind).await?;
        os_new_enough(asset.min_os.as_deref())?;
        // 包本身 + 装好之后（DMG 里的应用、MSIX 解开）大约两倍半，再留 1 GB。
        let need = asset.size.saturating_mul(7) / 2 + (1 << 30);
        if super::layout::free_space(&self.layout.root).is_some_and(|free| free < need) {
            return Err(DISK.into());
        }
        let cache = self.layout.cache();
        std::fs::create_dir_all(&cache).map_err(|_| DISK)?;
        let file = cache.join(&asset.file);
        say(Phase::Downloading, &asset.version);
        download::fetch(
            &self.client,
            &urls,
            &file,
            Some(asset.size),
            Expect::Sha256(&asset.sha256),
            cancel,
            &|status| notify(Progress { desktop: true, ..super::downloading(id, &asset.version, false, status) }),
        )
        .await?;
        let result = self.install_file(kind, &asset, &file, notify).await;
        // 装好了就删包；没装好也删：包已经验过，下次几分钟就能重下，不值得占着几百 MB。
        let _ = std::fs::remove_file(&file);
        result?;
        say(Phase::Done, &asset.version);
        Ok(asset.version)
    }

    async fn install_file(&self, kind: Kind, asset: &Asset, file: &std::path::Path, notify: &Notify<'_>) -> Result<(), String> {
        let say = |phase| {
            notify(Progress { version: Some(asset.version.clone()), desktop: true, ..Progress::new(AgentId::Codex, phase) })
        };
        #[cfg(target_os = "macos")]
        {
            let _ = kind;
            say(Phase::Verifying);
            let (dmg, dest) = (file.to_path_buf(), applications_dir());
            super::blocking(move || super::desktop_mac::install(&dmg, &dest)).await??;
            say(Phase::Activating);
            return Ok(());
        }
        #[cfg(target_os = "windows")]
        {
            say(Phase::Verifying);
            let (msix, arch) = (file.to_path_buf(), if kind == Kind::WinX64 { "x64" } else { "arm64" });
            let vcredist = super::blocking(move || super::desktop_win::preflight(&msix, arch)).await??;
            let runtime = match vcredist {
                Some(redist) => Some(self.fetch_vcredist(redist, notify).await?),
                None => None,
            };
            say(Phase::Authorizing);
            let (msix, sha) = (file.to_path_buf(), asset.sha256.clone());
            super::blocking(move || super::desktop_win::elevate(runtime.as_deref(), Some((msix.as_path(), sha.as_str())))).await??;
            say(Phase::Activating);
            super::blocking(super::desktop_win::confirm).await??;
            return Ok(());
        }
        #[allow(unreachable_code)]
        {
            let _ = (kind, asset, file, say);
            Err(UNSUPPORTED.into())
        }
    }
}

/// 装到哪个「应用程序」：系统的那个写得进去就用它，否则用自己的 ~/Applications。
pub fn applications_dir() -> PathBuf {
    let system = PathBuf::from("/Applications");
    let probe = system.join(format!(".pontex-write-test-{}", std::process::id()));
    if std::fs::write(&probe, b"").is_ok() {
        let _ = std::fs::remove_file(&probe);
        return system;
    }
    super::super::upstream::home().join("Applications")
}

/// 系统版本够不够。读不到版本就不拦：装的时候系统自己会拒绝。
fn os_new_enough(min: Option<&str>) -> Result<(), String> {
    let (Some(min), Some(have)) = (min, os_version()) else { return Ok(()) };
    if numbers(&have) < numbers(min) {
        return Err(OLD_OS.into());
    }
    Ok(())
}

fn os_version() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/bin/sw_vers").arg("-productVersion").output().ok()?;
        return Some(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }
    #[cfg(target_os = "windows")]
    return super::desktop_win::os_version();
    #[allow(unreachable_code)]
    None
}

/// 打开装好的桌面版。
pub fn open() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let ok = std::process::Command::new("/usr/bin/open").args(["-b", BUNDLE_ID]).status().is_ok_and(|s| s.success());
        return if ok { Ok(()) } else { Err(super::MISSING.into()) };
    }
    #[cfg(target_os = "windows")]
    return super::desktop_win::open();
    #[allow(unreachable_code)]
    Err(UNSUPPORTED.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(version: &str, mac_sha: &str, win_sha: &str) -> Value {
        use base64::Engine;
        let raw: Vec<u8> = (0..32).map(|i| u8::from_str_radix(&win_sha[i * 2..i * 2 + 2], 16).unwrap()).collect();
        let catalog = base64::engine::general_purpose::STANDARD.encode(raw);
        let moniker = "OpenAI.Codex_26.1120.6357.0_x64__2p2nqsd0c76g0";
        serde_json::json!({
            "codexVersion": version,
            "sources": {
                "macos": { "arm64": {
                    "url": format!("https://persistent.oaistatic.com/codex-app-prod/ChatGPT-{version}-arm64.dmg"),
                    "contentLength": 531053276u64, "sha256": mac_sha,
                    "teamIdentifier": TEAM_ID, "bundleIdentifier": BUNDLE_ID, "minimumSystemVersion": "13.0"
                }},
                "windows": { "architectures": { "x64": {
                    "packageMoniker": moniker, "contentLength": 770050306u64, "catalog": { "hash": catalog }
                }}}
            },
            "derived": { "latestChecksums": {
                "Codex-mac-arm64.dmg": mac_sha,
                format!("{moniker}.Msix"): win_sha
            }}
        })
    }

    const MAC: &str = "00db5b0c41e91d6d8c257a7ddfff15bfe2b1e56f1119b54079821b144e86a5a3";
    const WIN: &str = "70a09c02a0dab1411fff42a6d9ec8ae8a6ef1c03686c0c8fdcfdbf0fc90dbbdc";

    #[test]
    fn reads_the_real_manifest_shape() {
        let m = manifest("26.1120.50957", MAC, WIN);
        let mac = parse(&m, Kind::MacArm64).unwrap();
        assert_eq!(mac.file, "Codex-mac-arm64.dmg");
        assert_eq!(mac.size, 531053276);
        assert_eq!(mac.min_os.as_deref(), Some("13.0"));
        assert!(mac.official.unwrap().starts_with("https://persistent.oaistatic.com/"));
        let win = parse(&m, Kind::WinX64).unwrap();
        assert_eq!(win.file, "OpenAI.Codex_26.1120.6357.0_x64__2p2nqsd0c76g0.Msix");
        assert_eq!(win.sha256, WIN);
        assert_eq!(parse(&m, Kind::WinArm64), None, "清单里没有的架构不猜");
    }

    #[test]
    fn refuses_a_manifest_that_lies_about_identity() {
        let mut m = manifest("26.1120.50957", MAC, WIN);
        m["sources"]["macos"]["arm64"]["teamIdentifier"] = "XXXXXXXXXX".into();
        assert_eq!(parse(&m, Kind::MacArm64), None);
        let mut m = manifest("26.1120.50957", MAC, WIN);
        m["sources"]["windows"]["architectures"]["x64"]["packageMoniker"] = "Evil.App_1.0.0.0_x64__2p2nqsd0c76g0".into();
        assert_eq!(parse(&m, Kind::WinX64), None);
        // 商店目录的哈希和镜像算的对不上
        let mut m = manifest("26.1120.50957", MAC, WIN);
        m["sources"]["windows"]["architectures"]["x64"]["catalog"]["hash"] = "AAAA".into();
        assert_eq!(parse(&m, Kind::WinX64), None);
        // 官方地址只认 OpenAI 的 CDN
        let mut m = manifest("26.1120.50957", MAC, WIN);
        m["sources"]["macos"]["arm64"]["url"] = "https://evil.example/x.dmg".into();
        assert_eq!(parse(&m, Kind::MacArm64).unwrap().official, None);
    }

    #[test]
    fn newest_wins_and_same_version_must_agree() {
        let at = |v: &str, sha: &str| parse(&manifest(v, sha, WIN), Kind::MacArm64).unwrap();
        let old = at("26.1113.40000", &"1".repeat(64));
        let new = at("26.1120.50957", MAC);
        // B 落后一版：不算冲突，用新的
        assert_eq!(pick(&[(Source::B, old.clone()), (Source::A, new.clone())]).unwrap().version, "26.1120.50957");
        // 同一版本、哈希不同：有人改了其中一份
        let forged = at("26.1120.50957", &"f".repeat(64));
        assert_eq!(pick(&[(Source::A, new.clone()), (Source::GitHub, forged)]), Err(CONFLICT.into()));
        assert_eq!(pick(&[]), Err(OFFLINE.into()));
        assert!(numbers("26.1120.50957") > numbers("26.901.1"));
    }

    #[test]
    fn download_sources_follow_what_each_mirror_has() {
        let asset = parse(&manifest("26.1120.50957", MAC, WIN), Kind::MacArm64).unwrap();
        let all = urls(&asset, Kind::MacArm64, &[Source::A, Source::GitHub]);
        assert!(all[0].starts_with("https://persistent.oaistatic.com/"), "官方排第一");
        assert!(all.contains(&format!("{MIRROR_A}/latest/mac-arm64")));
        assert!(all.iter().any(|u| u.ends_with("/releases/download/codex-app-26.1120.50957/Codex-mac-arm64.dmg")));
        // A 还停在旧版：不去它那下
        let without_a = urls(&asset, Kind::MacArm64, &[Source::GitHub]);
        assert!(!without_a.iter().any(|u| u.starts_with(MIRROR_A)));
    }
}
