//! 对着本机假镜像走完整流程：断流、慢、404、坏内容、空间、取消、失败不留痕迹。
//! 装到临时目录，不碰这台电脑上的任何工具。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha512};

use super::archive::tests::tgz;
use super::*;

#[derive(Clone, Copy)]
enum Mode {
    Good,
    Missing,
    Dead,
    /// 每次只给这么多字节就断。
    Cut(usize),
    /// 内容被改过一个字节。
    Corrupt,
    /// 不支持续传。
    NoRange,
}

async fn mirror(bytes: Vec<u8>, mode: Mode) -> (String, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let count = hits.clone();
    let app = axum::Router::new().fallback(move |_: Uri, headers: HeaderMap| {
        count.fetch_add(1, Ordering::SeqCst);
        let mut bytes = bytes.clone();
        async move { serve(&mut bytes, mode, &headers) }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, hits)
}

fn serve(bytes: &mut Vec<u8>, mode: Mode, headers: &HeaderMap) -> Response {
    if matches!(mode, Mode::Missing) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if matches!(mode, Mode::Dead) {
        return StatusCode::BAD_GATEWAY.into_response();
    }
    if matches!(mode, Mode::Corrupt) {
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0xff;
    }
    let total = bytes.len();
    // `bytes=10-` 或 `bytes=10-19`
    let range = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split_once('-'))
        .and_then(|(a, b)| Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok())))
        .filter(|_| !matches!(mode, Mode::NoRange));
    let (start, end) = match range {
        Some((a, b)) => (a, b.map_or(total, |b| (b + 1).min(total))),
        None => (0, total),
    };
    if start >= total {
        return StatusCode::RANGE_NOT_SATISFIABLE.into_response();
    }
    let rest = bytes[start..end].to_vec();
    let length = rest.len();
    let body = match mode {
        // 声明全长，只送一段就断开
        Mode::Cut(n) => {
            let part = rest[..n.min(rest.len())].to_vec();
            let cut = part.len() < rest.len();
            Body::from_stream(async_stream::stream! {
                yield Ok::<_, std::io::Error>(bytes::Bytes::from(part));
                if cut {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    yield Err(std::io::Error::other("cut"));
                }
            })
        }
        _ => Body::from(rest),
    };
    let mut response = Response::builder().header("content-length", length);
    if range.is_some() {
        response = response
            .status(206)
            .header("content-range", format!("bytes {start}-{}/{total}", end - 1));
    }
    response.body(body).unwrap()
}

/// 假的 Codex 包：入口是一个能跑的脚本。
fn package(entry: &str, script: &str) -> Vec<u8> {
    // 不可压缩，下载量才真的有这么大
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    let noise: Vec<u8> = (0..300_000)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect();
    tgz(&[
        (b'5', "package/", b"", false),
        (b'0', &format!("package/{entry}"), script.as_bytes(), true),
        (b'0', "package/data.bin", &noise, false),
    ])
}

struct Setup {
    installer: Installer,
    artifact: Artifact,
    trusted: Trusted,
    spec: &'static Spec,
    dir: tempfile::TempDir,
}

const OK: &str = "#!/bin/sh\necho codex 9.9.9\n";

fn setup(mirrors: Vec<String>, bytes: &[u8]) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let spec = spec::spec(AgentId::Codex).unwrap();
    let platform = Platform::current().unwrap();
    let artifact = spec.artifact(platform, "9.9.9").unwrap();
    let sha512 = Sha512::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
    Setup {
        trusted: Trusted {
            package: artifact.package.clone(),
            version: artifact.version.clone(),
            sha512,
            unpacked: 1 << 20,
        },
        artifact,
        spec,
        installer: Installer {
            layout: Layout::new(dir.path().join("root")),
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
            mirrors,
            node_sources: vec![],
            platform,
            home: dir.path().join("home"),
        },
        dir,
    }
}

impl Setup {
    async fn run(&self, cancel: &AtomicBool) -> (Result<(), String>, Vec<Progress>) {
        let seen = Mutex::new(Vec::new());
        let result = self
            .installer
            .install_resolved(self.spec, "9.9.9", &self.artifact, &self.trusted, cancel, &|p| {
                seen.lock().unwrap().push(p);
            })
            .await;
        (result, seen.into_inner().unwrap())
    }
    fn leftovers(&self) -> Vec<String> {
        let tools = self.installer.layout.root.join("tools");
        std::fs::read_dir(tools)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect()
    }
}

fn entry() -> String {
    spec::spec(AgentId::Codex).unwrap().artifact(Platform::current().unwrap(), "9.9.9").unwrap().entry
}

#[cfg(unix)]
#[tokio::test]
async fn installs_and_the_command_really_works() {
    let bytes = package(&entry(), OK);
    let (url, _) = mirror(bytes.clone(), Mode::Good).await;
    let s = setup(vec![url], &bytes);
    let (result, seen) = s.run(&AtomicBool::new(false)).await;
    result.unwrap();
    let shim = s.installer.layout.shim(s.spec, false);
    let out = std::process::Command::new(&shim).arg("--version").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "codex 9.9.9\n");
    assert_eq!(s.installer.layout.managed(AgentId::Codex).as_deref(), Some("9.9.9"));
    assert_eq!(seen.last().unwrap().phase, Phase::Done);
    assert_eq!(s.leftovers(), vec!["codex"]);
    assert_eq!(std::fs::read_dir(s.installer.layout.cache()).unwrap().count(), 0, "下载的包用完就删");
    // 只写自己的目录
    assert!(!s.dir.path().join("home").exists());
    s.installer.uninstall(AgentId::Codex).unwrap();
    assert!(!shim.exists() && s.leftovers().is_empty());
    assert!(s.installer.uninstall(AgentId::Codex).is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn survives_a_network_that_keeps_dropping() {
    let bytes = package(&entry(), OK);
    let (a, hits_a) = mirror(bytes.clone(), Mode::Cut(40_000)).await;
    let (b, hits_b) = mirror(bytes.clone(), Mode::Cut(30_000)).await;
    let s = setup(vec![a, b], &bytes);
    let (result, seen) = s.run(&AtomicBool::new(false)).await;
    result.unwrap();
    // 每次只给几万字节就断：必须是接着断点往下要，而不是每次从头来
    let requests = hits_a.load(Ordering::SeqCst) + hits_b.load(Ordering::SeqCst);
    assert!(requests >= bytes.len() / 40_000, "断点续传：{requests} 次请求");
    assert!(requests <= bytes.len() / 30_000 + 8, "没有从头重下：{requests} 次请求");
    // 进度只增不减：没有从头来过
    let got: Vec<u64> = seen.iter().filter(|p| p.received > 0).map(|p| p.received).collect();
    assert!(got.windows(2).all(|w| w[0] <= w[1]), "{got:?}");
}

#[cfg(unix)]
#[tokio::test]
async fn skips_sources_that_are_missing_dead_or_cannot_resume() {
    let bytes = package(&entry(), OK);
    let (missing, hits) = mirror(bytes.clone(), Mode::Missing).await;
    let (dead, _) = mirror(bytes.clone(), Mode::Dead).await;
    let (cut, _) = mirror(bytes.clone(), Mode::Cut(50_000)).await;
    let (plain, _) = mirror(bytes.clone(), Mode::NoRange).await;
    let s = setup(vec![missing, dead, cut, plain], &bytes);
    s.run(&AtomicBool::new(false)).await.0.unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), 1, "404 的源不反复试");
}

#[cfg(unix)]
#[tokio::test]
async fn a_tampering_mirror_is_caught_and_replaced() {
    let bytes = package(&entry(), OK);
    let (bad, _) = mirror(bytes.clone(), Mode::Corrupt).await;
    let (good, _) = mirror(bytes.clone(), Mode::Good).await;
    let s = setup(vec![bad.clone(), good], &bytes);
    s.run(&AtomicBool::new(false)).await.0.unwrap();
    // 只有坏源：拒绝，什么都不留
    let s = setup(vec![bad], &bytes);
    assert_eq!(s.run(&AtomicBool::new(false)).await.0, Err(CHECKSUM.into()));
    assert!(s.leftovers().is_empty());
    assert_eq!(s.installer.layout.managed(AgentId::Codex), None);
}

#[cfg(unix)]
#[tokio::test]
async fn a_binary_that_does_not_run_is_never_activated() {
    for script in ["#!/bin/sh\nexit 3\n", "not a program"] {
        let bytes = package(&entry(), script);
        let (url, _) = mirror(bytes.clone(), Mode::Good).await;
        let s = setup(vec![url], &bytes);
        assert_eq!(s.run(&AtomicBool::new(false)).await.0, Err(RUN.into()));
        assert!(s.leftovers().is_empty());
        assert!(!s.installer.layout.shim(s.spec, false).exists());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_failed_upgrade_keeps_the_working_version() {
    let good = package(&entry(), OK);
    let (url, _) = mirror(good.clone(), Mode::Good).await;
    let mut s = setup(vec![url], &good);
    s.run(&AtomicBool::new(false)).await.0.unwrap();
    let bad = package(&entry(), "#!/bin/sh\nexit 1\n");
    let (url, _) = mirror(bad.clone(), Mode::Good).await;
    s.installer.mirrors = vec![url];
    s.trusted.sha512 = Sha512::digest(&bad).iter().map(|b| format!("{b:02x}")).collect();
    let next = s.spec.artifact(s.installer.platform, "10.0.0").unwrap();
    let result = s
        .installer
        .install_resolved(s.spec, "10.0.0", &next, &s.trusted, &AtomicBool::new(false), &|_| {})
        .await;
    assert_eq!(result, Err(RUN.into()));
    let out = std::process::Command::new(s.installer.layout.shim(s.spec, false)).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "codex 9.9.9\n");
}

#[tokio::test]
async fn wrong_layout_is_refused() {
    let bytes = tgz(&[(b'0', "package/other", b"x", true)]);
    let (url, _) = mirror(bytes.clone(), Mode::Good).await;
    let s = setup(vec![url], &bytes);
    assert_eq!(s.run(&AtomicBool::new(false)).await.0, Err(ARCHIVE.into()));
    assert!(s.leftovers().is_empty());
}

#[tokio::test]
async fn cancelling_and_low_disk_stop_cleanly() {
    let bytes = package(&entry(), OK);
    let (url, hits) = mirror(bytes.clone(), Mode::Good).await;
    let mut s = setup(vec![url], &bytes);
    assert_eq!(s.run(&AtomicBool::new(true)).await.0, Err(CANCELLED.into()));
    s.trusted.unpacked = u64::MAX / 32;
    let before = hits.load(Ordering::SeqCst);
    assert_eq!(s.run(&AtomicBool::new(false)).await.0, Err(DISK.into()));
    assert_eq!(hits.load(Ordering::SeqCst), before, "空间不够就不下载");
    assert!(s.leftovers().is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn gemini_runs_on_the_bundled_node_only() {
    let script = "#!/bin/sh\nexit 0\n";
    let bytes = tgz(&[(b'0', "package/bundle/gemini.js", b"// js", false)]);
    let (url, _) = mirror(bytes.clone(), Mode::Good).await;
    let mut s = setup(vec![url], &bytes);
    s.spec = spec::spec(AgentId::GeminiCli).unwrap();
    s.artifact = s.spec.artifact(s.installer.platform, "9.9.9").unwrap();
    // 自带的 Node 已经在位
    let node = s.installer.node_path();
    std::fs::create_dir_all(node.parent().unwrap()).unwrap();
    std::fs::write(&node, script).unwrap();
    archive::make_executable(&node).unwrap();
    s.run(&AtomicBool::new(false)).await.0.unwrap();
    let text = std::fs::read_to_string(s.installer.layout.shim(s.spec, false)).unwrap();
    assert!(text.contains(&node.display().to_string()) && text.contains("bundle/gemini.js"));
    s.installer.uninstall(AgentId::GeminiCli).unwrap();
    assert!(!node.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn an_entry_that_moved_is_found_again() {
    for moved in ["native/codex", "vendor/new-layout/bin/codex"] {
        let bytes = package(moved, OK);
        let (url, _) = mirror(bytes.clone(), Mode::Good).await;
        let s = setup(vec![url], &bytes);
        s.run(&AtomicBool::new(false)).await.0.unwrap();
        assert!(s.installer.layout.lock().tools[&AgentId::Codex].entry.ends_with(moved));
    }
    // 两个同名文件：有歧义，不猜
    let bytes = tgz(&[(b'0', "package/a/codex", b"1", true), (b'0', "package/b/codex", b"2", true)]);
    let (url, _) = mirror(bytes.clone(), Mode::Good).await;
    let s = setup(vec![url], &bytes);
    assert_eq!(s.run(&AtomicBool::new(false)).await.0, Err(ARCHIVE.into()));
    // 脚本型的看 package.json，但不能指到包外面
    let dir = tempfile::tempdir().unwrap();
    let gemini = spec::spec(AgentId::GeminiCli).unwrap().artifact(Platform::current().unwrap(), "9.9.9").unwrap();
    std::fs::create_dir_all(dir.path().join("dist")).unwrap();
    std::fs::write(dir.path().join("dist/cli.js"), "").unwrap();
    for (bin, want) in [("./dist/cli.js", true), ("../dist/cli.js", false), ("/etc/hosts", false)] {
        std::fs::write(dir.path().join("package.json"), format!(r#"{{"bin":{{"gemini":"{bin}"}}}}"#)).unwrap();
        assert_eq!(discover(dir.path(), &gemini, "gemini").is_some(), want, "{bin}");
    }
}

#[test]
fn enterprise_mirrors_must_be_https() {
    assert_eq!(mirrors(Some("https://npm.corp.test/ http://evil.test")), vec!["https://npm.corp.test"]);
    assert_eq!(mirrors(Some("ftp://x")).len(), 4);
    assert_eq!(mirrors(None).len(), 4);
}

#[test]
fn crashed_installs_are_swept() {
    let dir = tempfile::tempdir().unwrap();
    let layout = Layout::new(dir.path().to_path_buf());
    let stale = dir.path().join("tools/.staging-abc");
    let keep = dir.path().join("tools/codex/1.0.0");
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::create_dir_all(&keep).unwrap();
    layout.sweep();
    assert!(!stale.exists() && keep.exists());
}

/// 对着真实镜像：查版本、逐源验签、下载一个小的并解包。**不运行、不安装**，只在临时目录。
/// `cargo test --lib live_mirrors -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn live_mirrors() {
    let client = reqwest::Client::builder().https_only(true).build().unwrap();
    let all = mirrors(None);
    let platforms = [("darwin", "arm64"), ("darwin", "x64"), ("win32", "x64"), ("win32", "arm64")];
    let (mut good, mut bad) = (0, 0);
    for spec in &spec::SPECS {
        let version = match registry::latest(&client, &all, spec).await {
            Ok(version) => version,
            Err(error) => {
                println!("{} latest: {error:?}", spec.meta);
                continue;
            }
        };
        println!("{} {version}", spec.meta);
        for (os, arch) in platforms {
            let artifact = spec.artifact(Platform { os, arch }, &version).unwrap();
            for mirror in &all {
                match registry::resolve(&client, std::slice::from_ref(mirror), &artifact).await {
                    Ok(t) => {
                        good += 1;
                        println!("  ok {os}-{arch} {} {}MB", download::host(mirror), t.unpacked >> 20);
                    }
                    Err(e) => {
                        bad += 1;
                        println!("  -- {os}-{arch} {} {e:?}", download::host(mirror));
                    }
                }
            }
            if spec.id == AgentId::GeminiCli {
                break;
            }
        }
        {
            // 留着给静态检查看（`codesign --verify`），不运行
            let dir = match std::env::var_os("PONTEX_LIVE_DIR") {
                Some(root) => tempfile::Builder::new().prefix(spec.command).tempdir_in(root).unwrap(),
                None => tempfile::tempdir().unwrap(),
            };
            let artifact = spec.artifact(Platform::current().unwrap(), &version).unwrap();
            let trusted = registry::resolve(&client, &all, &artifact).await.unwrap();
            let urls: Vec<String> = all.iter().map(|m| format!("{m}/{}", artifact.tarball())).collect();
            let file = dir.path().join("p.tgz");
            let began = Instant::now();
            let got = download::fetch(&client, &urls, &file, None, Expect::Sha512(&trusted.sha512), &AtomicBool::new(false), &|_| {}).await;
            println!("  download {got:?} {}s", began.elapsed().as_secs());
            got.unwrap();
            let found = unpack(&file, &dir.path().join("out"), &artifact, spec.command).unwrap();
            let entry = dir.path().join("out").join(found);
            println!("  unpacked {} {}MB", artifact.entry, std::fs::metadata(entry).unwrap().len() >> 20);
            if std::env::var_os("PONTEX_LIVE_DIR").is_some() {
                let _ = dir.keep();
            }
        }
    }
    println!("signatures ok={good} failed={bad}");
}
