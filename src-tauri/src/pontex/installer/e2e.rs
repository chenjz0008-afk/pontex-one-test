//! 真机端到端：真的下载、校验、安装 Codex 桌面版，再让它自带的 codex 按 PonteX 写的那种配置
//! 发一次请求，确认请求发到了我们指定的地址、带着 Key。
//!
//! 会往系统里装东西，只在 CI 的测试机上跑（.github/workflows/pontex-codex-e2e.yml）：
//! `cargo test --release --lib pontex::installer::e2e -- --ignored --nocapture`

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::http::HeaderMap;

use super::desktop::{self, Kind};
use super::layout::Layout;
use super::spec::Platform;
use super::{Installer, Phase, Progress};

fn installer(root: &std::path::Path) -> Installer {
    Installer {
        layout: Layout::new(root.to_path_buf()),
        client: crate::pontex::upstream::https().unwrap(),
        mirrors: Vec::new(),
        node_sources: Vec::new(),
        platform: Platform::current().unwrap(),
        home: root.to_path_buf(),
    }
}

/// 桌面版里自带的 codex 命令行。包里的位置各版本可能不同，按文件名找。
/// Windows 的 WindowsApps 目录只许在应用自己的身份下执行，所以复制一份到 `scratch` 再跑；
/// 它是单个静态程序，复制出来照样能用。
fn bundled_codex(scratch: &std::path::Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    let (root, name) = (
        ["/Applications/ChatGPT.app", "/Applications/Codex.app"]
            .into_iter()
            .map(PathBuf::from)
            .find(|p| p.exists())
            .expect("装完找不到应用")
            .join("Contents"),
        "codex",
    );
    #[cfg(target_os = "windows")]
    let (root, name) = {
        let out = super::desktop_win::windows_powershell()
            .args(["-Command", "(Get-AppxPackage -Name OpenAI.Codex).InstallLocation"])
            .output()
            .unwrap();
        (PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()), "codex.exe")
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let (root, name) = (PathBuf::new(), "codex");

    fn find(dir: &std::path::Path, name: &str) -> Option<PathBuf> {
        let mut dirs = Vec::new();
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            let kind = entry.file_type().ok()?;
            if kind.is_file() && entry.file_name() == name {
                return Some(path);
            }
            if kind.is_dir() {
                dirs.push(path);
            }
        }
        dirs.iter().find_map(|d| find(d, name))
    }
    let found = find(&root, name).unwrap_or_else(|| panic!("{} 里找不到 {name}", root.display()));
    if cfg!(windows) {
        let copy = scratch.join(name);
        std::fs::copy(&found, &copy).expect("复制自带的 codex");
        return copy;
    }
    found
}

#[tokio::test]
#[ignore = "真机：会往系统里装 Codex 桌面版，只在 CI 上跑"]
async fn installs_the_desktop_app_and_it_talks_to_pontex() {
    let kind = Kind::of(Platform::current().unwrap()).expect("这台测试机不支持桌面版");
    println!("== 平台 {kind:?}，装之前：{:?}", desktop::installed());
    let root = tempfile::tempdir().unwrap();
    let installer = installer(root.path());

    // 1. 计划：版本、大小、要不要管理员
    let plan = installer.app_plan().await.expect("查不到桌面版");
    println!("== 计划 {plan:?}");
    assert_eq!(plan.admin, kind.windows());

    // 2. 真装：记下每个阶段和用过的源
    let seen: Arc<Mutex<Vec<Progress>>> = Arc::default();
    let log = seen.clone();
    let started = std::time::Instant::now();
    let version = installer
        .install_app(&AtomicBool::new(false), &move |p| {
            if p.phase != Phase::Downloading || p.received == p.total.unwrap_or(0) {
                println!("   {:?} {:?} {}/{:?} {:?} wait={:?}", p.phase, p.version, p.received, p.total, p.source, p.wait);
            }
            log.lock().unwrap().push(p);
        })
        .await
        .expect("安装失败");
    let seen = seen.lock().unwrap().clone();
    let sources: std::collections::BTreeSet<String> = seen.iter().filter_map(|p| p.source.clone()).collect();
    println!("== 装好 {version}，用时 {:?}，用到的源 {sources:?}", started.elapsed());
    assert!(seen.iter().any(|p| p.phase == Phase::Done));
    assert!(seen.iter().all(|p| p.desktop), "进度都标成桌面版");
    assert!(desktop::installed().is_some(), "装完检测不到");
    // 下载的包不留在缓存里
    let leftovers: Vec<_> = std::fs::read_dir(installer.layout.cache()).map(|d| d.flatten().map(|e| e.file_name()).collect()).unwrap_or_default();
    assert!(leftovers.is_empty(), "缓存里还剩 {leftovers:?}");

    // 3. 已经装着时再装：什么都不动，直接成功
    #[cfg(target_os = "macos")]
    {
        let again = installer.install_app(&AtomicBool::new(false), &|_| {}).await;
        assert!(again.is_ok(), "{again:?}");
    }

    // 4. 自带的 codex 按 PonteX 的写法读配置：请求发到我们指定的地址、带着 Key
    let (url, requests) = mock().await;
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            "model_provider = \"custom\"\nmodel = \"pontex-e2e\"\n\n[model_providers.custom]\nname = \"PonteX\"\n\
             base_url = \"{url}/v1\"\nwire_api = \"responses\"\nexperimental_bearer_token = \"px-e2e-key\"\n"
        ),
    )
    .unwrap();
    let bin = tempfile::tempdir().unwrap();
    let codex = bundled_codex(bin.path());
    println!("== 自带的 codex：{}", codex.display());
    let mut child = tokio::process::Command::new(&codex)
        .args(["exec", "--skip-git-repo-check", "say hi"])
        .env("CODEX_HOME", home.path())
        .current_dir(home.path())
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("自带的 codex 起不来");
    let until = std::time::Instant::now() + Duration::from_secs(90);
    while requests.lock().unwrap().is_empty() && std::time::Instant::now() < until {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let _ = child.kill().await;
    let got = requests.lock().unwrap().clone();
    println!("== 假服务器收到：{got:?}");
    assert!(
        got.iter().any(|(path, auth)| path.ends_with("/v1/responses") && auth == "Bearer px-e2e-key"),
        "桌面版自带的 codex 没有按配置把请求发给 PonteX"
    );
}

/// 一个什么都回 500 的假服务器，只记下请求路径和 Authorization。
async fn mock() -> (String, Arc<Mutex<Vec<(String, String)>>>) {
    let requests: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
    let log = requests.clone();
    let app = axum::Router::new().fallback(move |uri: axum::http::Uri, headers: HeaderMap| {
        let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        log.lock().unwrap().push((uri.path().to_string(), auth));
        async { axum::http::StatusCode::INTERNAL_SERVER_ERROR }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, requests)
}

/// Windows：模拟一台没有 VC++ 运行库的新电脑之后，包里程序要的 DLL 都还找得到。
#[cfg(target_os = "windows")]
#[test]
#[ignore = "真机：先在 CI 上卸掉 VC++ 运行库再跑"]
fn the_desktop_app_needs_nothing_that_a_clean_windows_lacks() {
    println!("系统里有 VC++ 运行库：{}", super::desktop_win::runtime_present());
    let out = super::desktop_win::windows_powershell()
        .args(["-Command", "(Get-AppxPackage -Name OpenAI.Codex).InstallLocation"])
        .output()
        .unwrap();
    let location = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    let missing = super::desktop_win::missing_dlls(&location);
    assert!(missing.is_empty(), "缺 {missing:?}");
}

/// Windows：确认管理员权限的是**另一个**账户时，脚本走「先登记到这台电脑」那条路。
/// CI 先建一个普通账户，把它的 SID 放进 PX_OTHER_SID，MSIX 放在 PX_MSIX。
#[cfg(target_os = "windows")]
#[test]
#[ignore = "真机：CI 上以管理员身份跑"]
fn another_admin_account_provisions_for_the_machine() {
    let msix = PathBuf::from(std::env::var("PX_MSIX").unwrap());
    let other = std::env::var("PX_OTHER_SID").unwrap();
    let sha = super::download::sha256_file(&msix).unwrap();
    let out = msix.with_extension("result.json");
    let package = super::desktop_win::Package { file: &msix, sha256: &sha, user: &other };
    let script = super::desktop_win::elevated_script(&out, None, Some(&package));
    // 和 PonteX One 一样经由 windows_powershell() 起，只是这里本来就是管理员，不用再弹确认
    let status = super::desktop_win::windows_powershell()
        .arg("-EncodedCommand")
        .arg(super::desktop_win::encode(&script))
        .status()
        .unwrap();
    assert!(status.success());
    let text = std::fs::read_to_string(&out).unwrap();
    let result: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap();
    println!("== {result}");
    assert_eq!(result["ok"], true);
    assert_eq!(result["mode"], "machine");
}

/// Windows：以那个普通账户的身份，自己注册、核对（PonteX One 装完时做的同一件事）。
#[cfg(target_os = "windows")]
#[test]
#[ignore = "真机：CI 上以普通账户身份跑"]
fn the_standard_account_registers_it_for_itself() {
    println!("== 注册：{:?}", super::desktop_win::register());
    super::desktop_win::confirm().expect("普通账户注册或核对失败");
    assert!(desktop::installed().is_some(), "普通账户下检测不到");
}
