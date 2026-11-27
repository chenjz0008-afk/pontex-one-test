//! 下载：多源、分片、并行、可续传。最后整体校验哈希，所以片段来自哪个源都无所谓。
//!
//! 照 aria2 的多源分片来：
//! - 每轮先用一小段 Range 同时探一遍所有源：活不活、多快、总长对不对、支不支持续传；
//! - 文件切成 4 MB 的片，几条连接各自领片，谁快谁多领；拖后腿的那片放回去给别人；
//! - 下完的片、以及没下完的片已经收到多少，都记在 `<文件>.part.json`：断网、退出、重启都从断点接着下，
//!   一条连接中途被掐断，这一片收到的部分也不丢；
//! - 一轮下不完，隔 30 秒 / 2 分钟 / 5 分钟再来一轮，每轮重新探源，不认死一个源；
//! - 哈希不对：不知道是哪个源坏的，就一个源一个源单独下，坏的那个排除掉。
//!
//! 内容由调用方给的哈希（或者装之后的系统签名）把关，所以源本身不需要可信。

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};

const CHUNK: u64 = 4 << 20;
const WORKERS: usize = 6;
/// 一个源最多同时占几条连接：别把一个源打到限流。
const PER_SOURCE: usize = 4;
const PROBE: u64 = 256 << 10;
/// 这么久没有新字节就算这条连接断了。
const STALL: Duration = Duration::from_secs(15);
/// 每轮开始前等多久。第一轮立刻开始。
const ROUNDS: [u64; 4] = [0, 30, 120, 300];
const MAX_BYTES: u64 = 2 << 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadError {
    Offline,
    Cancelled,
    Disk,
    TooLarge,
    Checksum,
}

#[derive(Debug, Clone, Copy)]
pub enum Expect<'a> {
    Sha512(&'a str),
    Sha256(&'a str),
}

/// 进度。`wait` 有值时表示这一轮没下完，几秒后自动重试。
#[derive(Debug, Clone, Copy)]
pub struct Status<'a> {
    pub received: u64,
    pub total: Option<u64>,
    pub source: &'a str,
    pub wait: Option<u64>,
}

pub type Report<'a> = dyn Fn(Status<'_>) + Send + Sync + 'a;

pub fn host(url: &str) -> &str {
    url.split("://").nth(1).and_then(|rest| rest.split('/').next()).unwrap_or(url)
}

/// 下载到 `dest`。`size` 已知时（清单里有）先拿它筛源：长度不对的源连试都不试。
pub async fn fetch(
    client: &reqwest::Client,
    urls: &[String],
    dest: &Path,
    size: Option<u64>,
    expect: Expect<'_>,
    cancel: &AtomicBool,
    report: &Report<'_>,
) -> Result<(), DownloadError> {
    if size.is_some_and(|s| s > MAX_BYTES) {
        return Err(DownloadError::TooLarge);
    }
    let part = with_suffix(dest, ".part");
    let shown = AtomicU64::new(0);
    // 哈希对不上之后：只用一个源，且不用已经证明坏了的源。
    let mut banned: BTreeSet<String> = BTreeSet::new();
    let mut isolate = false;
    let mut round = 0;
    while round < ROUNDS.len() {
        if ROUNDS[round] > 0 {
            report(Status { received: shown.load(Ordering::Relaxed), total: size, source: "", wait: Some(ROUNDS[round]) });
            pause(Duration::from_secs(ROUNDS[round]), cancel).await?;
        }
        let allowed: Vec<String> = urls.iter().filter(|u| !banned.contains(*u)).cloned().collect();
        if allowed.is_empty() {
            break;
        }
        let mut sources = probe_all(client, &allowed, size, cancel).await?;
        if isolate {
            sources.truncate(1);
        }
        let Some(total) = sources.first().map(|s| s.total) else {
            round += 1;
            continue;
        };
        if total > MAX_BYTES {
            return Err(DownloadError::TooLarge);
        }
        let used: Vec<String> = sources.iter().map(|s| s.origin.clone()).collect();
        let done = if sources.iter().any(|s| s.ranged) {
            sources.retain(|s| s.ranged);
            Swarm::new(client, &part, total, sources, &shown, cancel, report)?.run().await?
        } else {
            single(client, &sources[0], &part, total, &shown, cancel, report).await?
        };
        if !done {
            round += 1;
            continue;
        }
        if matches(&part, &expect) {
            let _ = std::fs::remove_file(with_suffix(dest, ".part.json"));
            std::fs::rename(&part, dest).map_err(|e| disk_error(&e))?;
            return Ok(());
        }
        log::warn!("[PONTEX] 下载校验不过（{}），改为逐个源单独下", used.join(", "));
        discard(dest);
        shown.store(0, Ordering::Relaxed);
        if isolate {
            banned.extend(used);
        }
        isolate = true;
        // 换源重下不算一轮：还有没试过的源就立刻接着试。
    }
    if isolate {
        discard(dest);
        return Err(DownloadError::Checksum);
    }
    Err(DownloadError::Offline)
}

/// 删掉没下完的东西。
pub fn discard(dest: &Path) {
    let _ = std::fs::remove_file(with_suffix(dest, ".part"));
    let _ = std::fs::remove_file(with_suffix(dest, ".part.json"));
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

async fn pause(total: Duration, cancel: &AtomicBool) -> Result<(), DownloadError> {
    let until = Instant::now() + total;
    while Instant::now() < until {
        if cancel.load(Ordering::Relaxed) {
            return Err(DownloadError::Cancelled);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(())
}

fn disk_error(error: &std::io::Error) -> DownloadError {
    log::warn!("[PONTEX] 写下载文件失败: {error}");
    DownloadError::Disk
}

/* ───────────────────────── 探源 ───────────────────────── */

#[derive(Debug, Clone)]
struct Source {
    origin: String,
    host: String,
    total: u64,
    ranged: bool,
    /// 强 ETag。之后每一片都带 If-Range：源上的文件中途换了版本，服务器会回整份而不是这一段。
    tag: Option<String>,
    /// 字节/秒，取最近几片的滑动平均。
    rate: f64,
    busy: usize,
    fails: u32,
    resting: Option<Instant>,
    dead: bool,
}

async fn probe_all(
    client: &reqwest::Client,
    urls: &[String],
    size: Option<u64>,
    cancel: &AtomicBool,
) -> Result<Vec<Source>, DownloadError> {
    let found = futures::future::join_all(urls.iter().map(|url| probe(client, url))).await;
    if cancel.load(Ordering::Relaxed) {
        return Err(DownloadError::Cancelled);
    }
    let mut alive: Vec<Source> = found.into_iter().flatten().collect();
    // 长度：清单给了就按清单；没给就按多数（同票取排在前面的源）。
    let want = size.or_else(|| {
        let mut votes: Vec<(u64, usize, usize)> = Vec::new();
        for (order, s) in alive.iter().enumerate() {
            match votes.iter_mut().find(|v| v.0 == s.total) {
                Some(v) => v.1 += 1,
                None => votes.push((s.total, 1, order)),
            }
        }
        votes.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(b.2.cmp(&a.2))).map(|v| v.0)
    });
    alive.retain(|s| Some(s.total) == want && s.total > 0);
    // 快的排前面；只剩一个源时就用它。
    alive.sort_by(|a, b| b.rate.total_cmp(&a.rate));
    Ok(alive)
}

async fn probe(client: &reqwest::Client, url: &str) -> Option<Source> {
    let started = Instant::now();
    let request = client
        .get(url)
        .header("accept-encoding", "identity")
        .header("range", format!("bytes=0-{}", PROBE - 1));
    let mut response = tokio::time::timeout(STALL, request.send()).await.ok()?.ok()?;
    let header = |name: &str| response.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    let tag = header("etag").filter(|t| !t.starts_with("W/"));
    let (ranged, total) = match response.status().as_u16() {
        206 => (true, header("content-range").and_then(|r| r.rsplit_once('/')?.1.parse::<u64>().ok())?),
        200 => (false, header("content-length").and_then(|v| v.parse::<u64>().ok())?),
        _ => return None,
    };
    let mut got = 0u64;
    let until = Instant::now() + Duration::from_secs(8);
    while got < PROBE.min(total) && Instant::now() < until {
        match tokio::time::timeout(STALL, response.chunk()).await {
            Ok(Ok(Some(chunk))) => got += chunk.len() as u64,
            _ => break,
        }
    }
    if got == 0 && total > 0 {
        return None;
    }
    let rate = got as f64 / started.elapsed().as_secs_f64().max(0.001);
    Some(Source {
        origin: url.to_string(),
        host: host(url).to_string(),
        total,
        ranged,
        tag,
        rate,
        busy: 0,
        fails: 0,
        resting: None,
        dead: false,
    })
}

/* ───────────────────────── 分片并行 ───────────────────────── */

/// 哪些片下完了、没下完的片已经写进去多少。每次写盘后落一次。
#[derive(Debug, Default, Serialize, Deserialize)]
struct Ledger {
    total: u64,
    chunk: u64,
    done: BTreeSet<u64>,
    #[serde(default)]
    partial: BTreeMap<u64, u64>,
}

struct Swarm<'a> {
    client: &'a reqwest::Client,
    part: &'a Path,
    total: u64,
    count: u64,
    sources: Mutex<Vec<Source>>,
    queue: Mutex<VecDeque<u64>>,
    ledger: Mutex<Ledger>,
    /// 已完成片的字节 + 正在下的片里已收到的字节。
    flowing: AtomicU64,
    shown: &'a AtomicU64,
    cancel: &'a AtomicBool,
    /// 写盘失败：不是源的错，所有连接一起停。
    halt: AtomicBool,
    report: &'a Report<'a>,
}

enum Miss {
    /// 连不上、断了、太慢，一个字节都没拿到：歇一会再用这个源。
    Flaky,
    /// 拿到了一部分就断了：已经记下，接着下剩下的，这个源照常用。
    Partial,
    /// 源上的文件换了：这一轮不再用它。
    Changed,
    Cancelled,
    Disk,
}

impl<'a> Swarm<'a> {
    fn new(
        client: &'a reqwest::Client,
        part: &'a Path,
        total: u64,
        sources: Vec<Source>,
        shown: &'a AtomicU64,
        cancel: &'a AtomicBool,
        report: &'a Report<'a>,
    ) -> Result<Self, DownloadError> {
        let count = total.div_ceil(CHUNK).max(1);
        let ledger_path = with_suffix(part, ".json");
        let mut ledger: Ledger = std::fs::read(&ledger_path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .filter(|l: &Ledger| l.total == total && l.chunk == CHUNK && part.is_file())
            .unwrap_or_default();
        if ledger.total != total {
            ledger = Ledger { total, chunk: CHUNK, ..Ledger::default() };
        }
        // 预先占好整个文件的大小：各片按偏移直接写进去。
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(part)
            .map_err(|e| disk_error(&e))?;
        file.set_len(total).map_err(|e| disk_error(&e))?;
        let queue: VecDeque<u64> = (0..count).filter(|i| !ledger.done.contains(i)).collect();
        let finished: u64 = ledger.done.iter().map(|&i| span(i, total).1 - span(i, total).0 + 1).sum::<u64>()
            + ledger.partial.values().sum::<u64>();
        Ok(Self {
            client,
            part,
            total,
            count,
            sources: Mutex::new(sources),
            queue: Mutex::new(queue),
            ledger: Mutex::new(ledger),
            flowing: AtomicU64::new(finished),
            shown,
            cancel,
            halt: AtomicBool::new(false),
            report,
        })
    }

    /// 这一轮下完了吗。
    async fn run(&self) -> Result<bool, DownloadError> {
        let workers = (0..WORKERS).map(|_| self.worker());
        let ticker = self.ticker();
        let (results, ()) = futures::future::join(futures::future::join_all(workers), ticker).await;
        for result in results {
            result?;
        }
        self.tell();
        Ok(lock(&self.ledger).done.len() as u64 == self.count)
    }

    async fn ticker(&self) {
        while !self.idle() {
            self.tell();
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// 没有排队的片，也没有在下的片。
    fn idle(&self) -> bool {
        if self.cancel.load(Ordering::Relaxed) || self.halt.load(Ordering::Relaxed) {
            return true;
        }
        let queued = !lock(&self.queue).is_empty();
        let sources = lock(&self.sources);
        sources.iter().all(|s| s.dead) || (!queued && sources.iter().all(|s| s.busy == 0))
    }

    fn tell(&self) {
        let now = self.flowing.load(Ordering::Relaxed).min(self.total);
        // 一片中途断了会把它那部分退回去：显示上只增不减。
        let received = self.shown.fetch_max(now, Ordering::Relaxed).max(now);
        let source = {
            let sources = lock(&self.sources);
            sources
                .iter()
                .filter(|s| s.busy > 0)
                .max_by(|a, b| a.rate.total_cmp(&b.rate))
                .map(|s| s.host.clone())
                .unwrap_or_default()
        };
        (self.report)(Status { received, total: Some(self.total), source: &source, wait: None });
    }

    async fn worker(&self) -> Result<(), DownloadError> {
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(DownloadError::Cancelled);
            }
            if self.halt.load(Ordering::Relaxed) {
                return Ok(());
            }
            let next = lock(&self.queue).pop_front();
            let Some(index) = next else { return Ok(()) };
            let Some(pick) = self.pick().await else {
                lock(&self.queue).push_front(index);
                return Ok(());
            };
            let result = self.chunk(pick, index).await;
            {
                let mut sources = lock(&self.sources);
                let source = &mut sources[pick];
                source.busy -= 1;
                match &result {
                    Ok(rate) => {
                        source.rate = if source.rate > 0.0 { source.rate * 0.6 + rate * 0.4 } else { *rate };
                        source.fails = 0;
                    }
                    Err(Miss::Changed) => source.dead = true,
                    Err(Miss::Flaky) => {
                        source.fails += 1;
                        source.dead = source.fails >= 4;
                        source.resting = Some(Instant::now() + Duration::from_secs(5 * u64::from(source.fails)));
                    }
                    Err(Miss::Partial | Miss::Cancelled | Miss::Disk) => {}
                }
            }
            match result {
                Ok(_) => {}
                Err(miss) => {
                    // 断在半路的片排到最前面：马上接着下，免得最后剩一堆半截的片。
                    if matches!(miss, Miss::Partial) {
                        lock(&self.queue).push_front(index);
                    } else {
                        lock(&self.queue).push_back(index);
                    }
                    match miss {
                        Miss::Cancelled => return Err(DownloadError::Cancelled),
                        Miss::Disk => {
                            self.halt.store(true, Ordering::Relaxed);
                            return Err(DownloadError::Disk);
                        }
                        Miss::Changed | Miss::Flaky | Miss::Partial => {}
                    }
                }
            }
        }
    }

    /// 挑最快的、还有空位的源。都在歇着就等一等；全死了返回 None。
    async fn pick(&self) -> Option<usize> {
        loop {
            {
                let mut sources = lock(&self.sources);
                let now = Instant::now();
                let usable = sources
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| !s.dead && s.busy < PER_SOURCE && s.resting.is_none_or(|t| t <= now))
                    .max_by(|a, b| a.1.rate.total_cmp(&b.1.rate))
                    .map(|(i, _)| i);
                if let Some(i) = usable {
                    sources[i].busy += 1;
                    return Some(i);
                }
                if sources.iter().all(|s| s.dead) {
                    return None;
                }
            }
            if self.cancel.load(Ordering::Relaxed) || self.halt.load(Ordering::Relaxed) {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    /// 下一片（或它没下完的剩余部分），写进文件，返回速度。中途断了，收到的部分照样写进去记下来。
    async fn chunk(&self, pick: usize, index: u64) -> Result<f64, Miss> {
        let (from, to) = span(index, self.total);
        let have = lock(&self.ledger).partial.get(&index).copied().unwrap_or(0);
        let start = from + have;
        let (url, tag) = {
            let sources = lock(&self.sources);
            (sources[pick].origin.clone(), sources[pick].tag.clone())
        };
        let started = Instant::now();
        let mut request = self
            .client
            .get(&url)
            .header("accept-encoding", "identity")
            .header("range", format!("bytes={start}-{to}"));
        if let Some(tag) = &tag {
            request = request.header("if-range", tag);
        }
        let mut response = match tokio::time::timeout(STALL, request.send()).await {
            Ok(Ok(r)) => r,
            _ => return Err(Miss::Flaky),
        };
        match response.status().as_u16() {
            206 => {}
            // If-Range 不成立，服务器回了整份：文件换过了。
            200 => return Err(Miss::Changed),
            _ => return Err(Miss::Flaky),
        }
        let starts_right = response
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("bytes ")?.split_once('-')?.0.parse::<u64>().ok())
            == Some(start);
        if !starts_right {
            return Err(Miss::Changed);
        }
        let want = (to - start + 1) as usize;
        let mut body = Vec::with_capacity(want);
        let result = loop {
            if self.cancel.load(Ordering::Relaxed) {
                break Err(Miss::Cancelled);
            }
            match tokio::time::timeout(STALL, response.chunk()).await {
                Ok(Ok(Some(bytes))) => {
                    let take = bytes.len().min(want - body.len());
                    body.extend_from_slice(&bytes[..take]);
                    self.flowing.fetch_add(take as u64, Ordering::Relaxed);
                    if body.len() == want {
                        break Ok(());
                    }
                    if self.lagging(pick, body.len() as u64, started) {
                        break Err(Miss::Flaky);
                    }
                }
                Ok(Ok(None)) if body.len() == want => break Ok(()),
                _ => break Err(Miss::Flaky),
            }
        };
        let rate = body.len() as f64 / started.elapsed().as_secs_f64().max(0.001);
        if !body.is_empty() {
            let got = body.len() as u64;
            if let Err(error) = self.write(index, start, body, have + got, result.is_ok()).await {
                self.flowing.fetch_sub(got, Ordering::Relaxed);
                log::warn!("[PONTEX] 写下载文件失败: {error}");
                return Err(Miss::Disk);
            }
        }
        match result {
            Ok(()) => Ok(rate),
            Err(Miss::Flaky) if rate > 0.0 => Err(Miss::Partial),
            Err(miss) => Err(miss),
        }
    }

    /// 这条连接比最快的那个源慢八倍以上，而且已经下了好几秒：放手，让快的源来。
    fn lagging(&self, pick: usize, got: u64, started: Instant) -> bool {
        let elapsed = started.elapsed().as_secs_f64();
        if elapsed < 4.0 {
            return false;
        }
        let sources = lock(&self.sources);
        let best = sources
            .iter()
            .enumerate()
            .filter(|(i, s)| *i != pick && !s.dead)
            .map(|(_, s)| s.rate)
            .fold(0.0, f64::max);
        best > 256.0 * 1024.0 && (got as f64 / elapsed) * 8.0 < best
    }

    /// 写进文件，再记进账本：先写数据后记账，掉电也只会少记、不会多记。
    async fn write(&self, index: u64, offset: u64, body: Vec<u8>, have: u64, complete: bool) -> std::io::Result<()> {
        let part = self.part.to_path_buf();
        tokio::task::spawn_blocking(move || write_at(&part, offset, &body))
            .await
            .map_err(std::io::Error::other)??;
        let mut ledger = lock(&self.ledger);
        if complete {
            ledger.partial.remove(&index);
            ledger.done.insert(index);
        } else {
            ledger.partial.insert(index, have);
        }
        let text = serde_json::to_vec(&*ledger).map_err(std::io::Error::other)?;
        std::fs::write(with_suffix(self.part, ".json"), text)
    }
}

fn span(index: u64, total: u64) -> (u64, u64) {
    let from = index * CHUNK;
    (from, (from + CHUNK).min(total) - 1)
}

fn write_at(path: &Path, offset: u64, bytes: &[u8]) -> std::io::Result<()> {
    let file = std::fs::OpenOptions::new().write(true).open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.write_all_at(bytes, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut done = 0;
        while done < bytes.len() {
            done += file.seek_write(&bytes[done..], offset + done as u64)?;
        }
        Ok(())
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/* ───────────────────────── 不支持续传的源 ───────────────────────── */

/// 所有源都不支持 Range 时：从头整份下。下不完就算这一轮没成。
async fn single(
    client: &reqwest::Client,
    source: &Source,
    part: &Path,
    total: u64,
    shown: &AtomicU64,
    cancel: &AtomicBool,
    report: &Report<'_>,
) -> Result<bool, DownloadError> {
    use tokio::io::AsyncWriteExt;
    let request = client.get(&source.origin).header("accept-encoding", "identity");
    let Ok(Ok(mut response)) = tokio::time::timeout(STALL, request.send()).await else { return Ok(false) };
    if response.status().as_u16() != 200 {
        return Ok(false);
    }
    let _ = std::fs::remove_file(with_suffix(part, ".json"));
    let mut file = tokio::fs::File::create(part).await.map_err(|e| disk_error(&e))?;
    let mut received = 0u64;
    let mut told = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(DownloadError::Cancelled);
        }
        match tokio::time::timeout(STALL, response.chunk()).await {
            Ok(Ok(Some(chunk))) => {
                received += chunk.len() as u64;
                if received > total {
                    return Ok(false);
                }
                file.write_all(&chunk).await.map_err(|e| disk_error(&e))?;
                if told.elapsed() >= Duration::from_millis(200) {
                    told = Instant::now();
                    let shown_now = shown.fetch_max(received, Ordering::Relaxed).max(received);
                    report(Status { received: shown_now, total: Some(total), source: &source.host, wait: None });
                }
            }
            Ok(Ok(None)) => break,
            _ => return Ok(false),
        }
    }
    file.flush().await.map_err(|e| disk_error(&e))?;
    Ok(received == total)
}

/* ───────────────────────── 校验 ───────────────────────── */

pub fn sha256_file(path: &Path) -> Option<String> {
    let mut reader = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(path).ok()?);
    let mut hash = Sha256::new();
    std::io::copy(&mut reader, &mut hash).ok()?;
    Some(hex(&hash.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn matches(path: &Path, expect: &Expect<'_>) -> bool {
    match expect {
        Expect::Sha256(want) => sha256_file(path).is_some_and(|got| got.eq_ignore_ascii_case(want)),
        Expect::Sha512(want) => {
            let Ok(file) = std::fs::File::open(path) else { return false };
            let mut reader = std::io::BufReader::with_capacity(1 << 20, file);
            let mut hash = Sha512::new();
            std::io::copy(&mut reader, &mut hash).is_ok() && hex(&hash.finalize()) == **want
        }
    }
}
