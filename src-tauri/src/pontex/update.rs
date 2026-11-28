//! PonteX One 自己的自动更新。
//!
//! 后台每 6 小时查一次 `downloads/one/update/stable.json`（Tauri 更新清单 + 我们的三项：
//! `rollout` 推送比例、`min_version` 最低可用版本、`blocked` 撤回的版本）。有新版就在后台下好、
//! 核对签名（公钥编进程序，私钥只在发布者自己的电脑上），然后安静地提示「重启更新」：
//!   · 点「重启更新」：macOS 换掉 .app 再重开；Windows 交给 MSI 安静安装，装完自动重开。
//!   · macOS 上不点也行：退出时自动装好，下次打开就是新版（Sparkle 的做法）。
//!     Windows 不在退出时装：MSI 装完会把软件重新打开，用户刚退出又弹出来很怪。
//! 只换 PonteX One 自己，不碰任何 Agent 的配置。正在装 Agent、正在切换时不装。

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use tauri_plugin_updater::{Update, UpdaterExt};

/// 正式的更新清单。
pub const MANIFEST: &str = "https://docs.ponte-x.com/downloads/one/update/stable.json";
/// 启动后多久第一次查。
pub const FIRST_CHECK: Duration = Duration::from_secs(30);

pub fn first_check() -> Duration {
    if e2e().is_some() {
        Duration::from_secs(3)
    } else {
        FIRST_CHECK
    }
}
/// 之后多久查一次。
pub const INTERVAL: Duration = Duration::from_secs(6 * 3600);

/// 给界面的样子。
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum View {
    #[default]
    Idle,
    Checking,
    /// 已是最新。
    Latest { at: i64 },
    Downloading { version: String, received: u64, total: Option<u64> },
    /// 下好、签名对了，等重启。`notes` 中文、`notes_en` 英文的更新说明（发布时写的几条）。
    Ready { version: String, current: String, notes: Option<String>, notes_en: Option<String> },
    Installing { version: String },
    Failed { reason: String },
    /// 这个平台 / 这种安装方式不能自动更新（开发版、Linux）。
    Unsupported,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Status {
    #[serde(flatten)]
    pub view: View,
    /// 低于清单里的 `min_version`：要尽快更新。
    pub required: Option<String>,
}

struct Pending {
    update: Update,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct State {
    status: Status,
    pending: Option<Pending>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(State::default))
}

pub fn status() -> Status {
    with(|s| s.status.clone())
}

fn set(view: View) {
    with(|s| s.status.view = view);
}

/// 能不能自动更新：只有正式打包的 macOS 和 Windows 版。
pub fn supported() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows")) && !cfg!(debug_assertions) || e2e().is_some()
}

/// 端到端测试：编译时打开 `PONTEX_UPDATE_E2E`，运行时才能把清单指到本机。正式版没有这个入口。
pub fn e2e() -> Option<String> {
    option_env!("PONTEX_UPDATE_E2E")?;
    std::env::var("PONTEX_UPDATE_ENDPOINT").ok().filter(|u| u.starts_with("http://127.0.0.1:"))
}

/// 比较 `1.2.3` 这样的版本号（预发布后缀忽略）。
pub fn newer(a: &str, b: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v')
            .split(['-', '+'])
            .next()
            .unwrap_or_default()
            .split('.')
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parse(a), parse(b));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

/// 这台电脑在这个版本的推送里排第几（0–99）。每个版本重新抽，不会总是同一批人先拿到。
pub fn bucket(install_id: &str, version: &str) -> u8 {
    let hash = Sha256::digest(format!("{install_id}:{version}").as_bytes());
    (u16::from_be_bytes([hash[0], hash[1]]) % 100) as u8
}

/// 清单里我们自己的三项。
#[derive(Debug, Default, PartialEq)]
pub struct Policy {
    pub rollout: u8,
    pub min_version: Option<String>,
    pub blocked: Vec<String>,
}

pub fn policy(raw: &serde_json::Value) -> Policy {
    Policy {
        rollout: raw.get("rollout").and_then(|v| v.as_u64()).map(|v| v.min(100) as u8).unwrap_or(100),
        min_version: raw.get("min_version").and_then(|v| v.as_str()).map(str::to_string),
        blocked: raw
            .get("blocked")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
    }
}

/// 要不要给这台电脑这个版本。低于最低可用版本的不看推送比例。
pub fn eligible(policy: &Policy, current: &str, offered: &str, install_id: &str) -> bool {
    if policy.blocked.iter().any(|v| v == offered) || !newer(offered, current) {
        return false;
    }
    let required = policy.min_version.as_deref().is_some_and(|min| newer(min, current));
    required || bucket(install_id, offered) < policy.rollout
}

/// 查一次；有新版就下载、核对签名，准备好等重启。`manual`：用户点的「检查更新」，失败要说出来。
pub async fn check(app: &AppHandle, install_id: &str, manual: bool) -> Status {
    if !supported() {
        set(View::Unsupported);
        return status();
    }
    if matches!(status().view, View::Downloading { .. } | View::Installing { .. } | View::Ready { .. }) {
        return status();
    }
    set(View::Checking);
    let result = find(app, install_id).await;
    match result {
        Ok(None) => set(View::Latest { at: chrono::Utc::now().timestamp() }),
        Ok(Some(update)) => download(update).await,
        Err(reason) => {
            log::warn!("[PONTEX] 检查 PonteX One 更新失败: {reason}");
            // 自动检查失败不打扰：回到空闲，下次再查。
            set(if manual { View::Failed { reason } } else { View::Idle });
        }
    }
    status()
}

async fn find(app: &AppHandle, install_id: &str) -> Result<Option<Update>, String> {
    let endpoint = e2e().unwrap_or_else(|| MANIFEST.to_string());
    let url = endpoint.parse().map_err(|e: url::ParseError| e.to_string())?;
    let updater = app
        .updater_builder()
        .endpoints(vec![url])
        .map_err(|e| e.to_string())?
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        with(|s| s.status.required = None);
        return Ok(None);
    };
    let policy = policy(&update.raw_json);
    let current = update.current_version.clone();
    let required = policy.min_version.clone().filter(|min| newer(min, &current));
    with(|s| s.status.required = required);
    Ok(eligible(&policy, &current, &update.version, install_id).then_some(update))
}

async fn download(update: Update) {
    let version = update.version.clone();
    set(View::Downloading { version: version.clone(), received: 0, total: None });
    let mut received = 0u64;
    let result = update
        .download(
            |chunk, total| {
                received += chunk as u64;
                set(View::Downloading { version: version.clone(), received, total });
            },
            || {},
        )
        .await;
    match result {
        // download() 里已经核对过签名：对不上会报错，不会走到这里。
        Ok(bytes) => {
            let notes = update.body.clone().filter(|n| !n.trim().is_empty());
            let notes_en = update.raw_json.get("notes_en").and_then(|v| v.as_str()).map(str::to_string).filter(|n| !n.trim().is_empty());
            let current = update.current_version.clone();
            with(|s| {
                s.pending = Some(Pending { update, bytes });
                s.status.view = View::Ready { version, current, notes, notes_en };
            });
        }
        Err(error) => {
            log::warn!("[PONTEX] 下载 PonteX One 更新失败: {error}");
            set(View::Failed { reason: error.to_string() });
        }
    }
}

/// 装上下好的新版。macOS 装完要调用方重开；Windows 会交给安装程序并直接退出本进程。
pub fn install() -> Result<String, String> {
    let pending = with(|s| s.pending.take()).ok_or("UPDATE_NOT_READY")?;
    let version = pending.update.version.clone();
    set(View::Installing { version: version.clone() });
    match pending.update.install(&pending.bytes) {
        Ok(()) => Ok(version),
        Err(error) => {
            let reason = error.to_string();
            log::warn!("[PONTEX] 安装 PonteX One 更新失败: {reason}");
            // 没装上：留着包，界面说清楚，用户可以再点一次或去官网下载。
            with(|s| {
                s.pending = Some(pending);
                s.status.view = View::Failed { reason: reason.clone() };
            });
            Err(reason)
        }
    }
}

/// 准备好了要不要弹窗：这个版本没弹过，或者上次点「稍后」已经过了一天。
pub fn should_prompt(version: &str, last: Option<&(String, i64)>, now: i64) -> bool {
    match last {
        Some((v, at)) if v == version => now - at >= 24 * 3600,
        _ => true,
    }
}

/// macOS 退出时：有下好的就装上（不重开）。
pub fn install_on_exit() {
    if !cfg!(target_os = "macos") || !with(|s| s.pending.is_some()) {
        return;
    }
    match install() {
        Ok(version) => log::info!("[PONTEX] 退出时装好了 PonteX One {version}"),
        Err(reason) => log::warn!("[PONTEX] 退出时没装上更新: {reason}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number_not_text() {
        assert!(newer("0.12.0", "0.11.9"));
        assert!(newer("0.11.10", "0.11.9"));
        assert!(newer("1.0.0", "0.99.99"));
        assert!(!newer("0.11.1", "0.11.1"));
        assert!(!newer("0.11.0", "0.11.1"));
        assert!(newer("v0.11.2", "0.11.1-e2e"));
    }

    #[test]
    fn rollout_buckets_are_stable_and_spread() {
        assert_eq!(bucket("abc", "0.12.0"), bucket("abc", "0.12.0"));
        let ids: Vec<String> = (0..2000).map(|i| format!("install-{i}")).collect();
        let under_ten = ids.iter().filter(|id| bucket(id, "0.12.0") < 10).count();
        assert!((120..=280).contains(&under_ten), "10% 推送实际落到 {under_ten}/2000");
        // 换一个版本会重新抽
        let same = ids.iter().filter(|id| (bucket(id, "0.12.0") < 10) == (bucket(id, "0.13.0") < 10)).count();
        assert!(same < 2000);
    }

    #[test]
    fn the_window_pops_once_per_version_then_again_a_day_later() {
        let now = 1_000_000;
        assert!(should_prompt("0.12.0", None, now));
        let shown = ("0.12.0".to_string(), now);
        assert!(!should_prompt("0.12.0", Some(&shown), now + 3600));
        assert!(should_prompt("0.12.0", Some(&shown), now + 24 * 3600));
        assert!(should_prompt("0.12.1", Some(&shown), now + 60));
    }

    #[test]
    fn policy_decides_who_gets_the_update() {
        let raw = serde_json::json!({ "version": "0.12.0", "rollout": 0, "min_version": null, "blocked": ["0.12.1"] });
        let p = policy(&raw);
        assert_eq!(p.rollout, 0);
        // 推送比例 0：谁都不给
        assert!(!eligible(&p, "0.11.1", "0.12.0", "anyone"));
        // 低于最低可用版本：不看推送比例
        let forced = Policy { min_version: Some("0.11.5".into()), ..p };
        assert!(eligible(&forced, "0.11.1", "0.12.0", "anyone"));
        // 撤回的版本不给
        assert!(!eligible(&forced, "0.11.1", "0.12.1", "anyone"));
        // 不往回装
        assert!(!eligible(&Policy { rollout: 100, ..Policy::default() }, "0.12.0", "0.11.9", "x"));
        // 缺省是全量
        assert_eq!(policy(&serde_json::json!({})).rollout, 100);
    }
}
