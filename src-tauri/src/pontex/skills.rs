//! 从 PonteX Skills 安装技能。
//!
//! 下载 → 校验 → 用户确认 → 逐 Agent 原子换入。归属规则和网站的 `install.md` 相同，两种安装
//! 方式互相认得；不属于我们的目录从不覆盖。用复制不用软链：卸载软件后技能还在。

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::agents::AgentId;
use super::consts::skills_origin;
use super::links::valid_slug;
use super::safe_path::safe_relative;

const MAX_MANIFEST: usize = 1 << 20;
const MAX_ZIP: usize = 16 << 20;
const MAX_UNPACKED: u64 = 32 << 20;
const MAX_FILES: usize = 1000;
/// 打包时附带、不在清单 `files[]` 里的两个文件。
const EXTRAS: [&str; 2] = ["pontex.json", "INSTALL.txt"];
const MARKER: &str = "pontex.json";
const SCRIPT_EXT: &[&str] = &["sh", "ps1", "py", "js", "mjs", "ts", "applescript"];

#[derive(Debug, Clone, Deserialize)]
struct Manifest {
    id: String,
    slug: String,
    dir: String,
    title: String,
    #[serde(default)]
    description: String,
    version: String,
    #[serde(default, rename = "worksWith")]
    works_with: Vec<String>,
    #[serde(default, rename = "safetyNote")]
    safety_note: Option<String>,
    files: Vec<ManifestFile>,
    zip: ManifestZip,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestFile {
    path: String,
    sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestZip {
    sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Marker {
    id: String,
    #[serde(default)]
    version: String,
}

/// 校验过、等用户确认的技能。
#[derive(Debug, Clone)]
pub struct Ready {
    manifest: Manifest,
    files: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub slug: String,
    pub title: String,
    pub description: String,
    pub version: String,
    pub file_count: usize,
    pub bytes: usize,
    pub scripts: Vec<String>,
    pub safety_note: Option<String>,
    pub targets: Vec<TargetView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetView {
    pub agent: AgentId,
    pub label: &'static str,
    pub installed: bool,
    /// 网站标注过在这个 Agent 上验证过。
    pub verified: bool,
    pub state: Place,
    pub dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Place {
    New,
    /// 同名目录是用户自己的，改装到另一个名字。
    Renamed,
    UpToDate,
    Update { from: String },
    /// 内容被用户改过，会先备份。
    Modified { from: String },
    /// 由别的工具用软链管理，不动。
    Linked,
}

#[derive(Debug, Clone, Serialize)]
pub struct Installed {
    pub agent: AgentId,
    pub ok: bool,
    pub dir: Option<String>,
    pub backup: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Lock {
    version: u32,
    /// `slug` → 目录 → 装下去时的内容。
    skills: BTreeMap<String, BTreeMap<String, LockEntry>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LockEntry {
    version: String,
    agent: AgentId,
    tree: String,
    #[serde(default)]
    title: String,
}

/// 下载技能包之前就能拿到的信息，让用户先看到正确的名字。
#[derive(Debug, Clone, Serialize)]
pub struct Info {
    pub title: String,
    pub description: String,
    pub version: String,
}

/// 已经装着的一个技能。
#[derive(Debug, Clone, Serialize)]
pub struct Owned {
    pub slug: String,
    pub title: String,
    pub places: Vec<OwnedPlace>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OwnedPlace {
    pub agent: AgentId,
    pub label: &'static str,
    pub version: String,
    pub dir: String,
    /// 装好之后被用户改过；移除时会先备份。
    pub modified: bool,
}

fn lock_path() -> PathBuf {
    super::upstream::device_dir().join("skills-lock.json")
}

fn load_lock() -> Lock {
    std::fs::read(lock_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn site_id(id: AgentId) -> &'static str {
    id.key()
}

fn skills_dir(id: AgentId) -> Result<PathBuf, String> {
    super::upstream::skills_dir(id)
}

async fn fetch(url: &str, limit: usize, seconds: u64) -> Result<Vec<u8>, &'static str> {
    let client = super::key::client_for(url).map_err(|_| "NETWORK")?;
    let mut response = client
        .get(url)
        .timeout(Duration::from_secs(seconds))
        .send()
        .await
        .map_err(|_| "NETWORK")?;
    match response.status().as_u16() {
        200 => {}
        404 => return Err("SKILL_NOT_FOUND"),
        _ => return Err("SKILL_SITE_ERROR"),
    }
    let mut out = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "NETWORK")? {
        if out.len() + chunk.len() > limit {
            return Err("SKILL_TOO_LARGE");
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// 地址全部自己拼，清单里的 `urls` 不采信。
async fn manifest(slug: &str) -> Result<(String, Manifest), String> {
    if !valid_slug(slug) {
        return Err("SKILL_BAD_LINK".into());
    }
    let base = format!("{}/skills/{slug}", skills_origin());
    let manifest = fetch(&format!("{base}.json"), MAX_MANIFEST, 15).await?;
    let manifest: Manifest = serde_json::from_slice(&manifest).map_err(|_| "SKILL_BAD_MANIFEST")?;
    check_manifest(&manifest, slug)?;
    Ok((base, manifest))
}

pub async fn info(slug: &str) -> Result<Info, String> {
    let (_, manifest) = manifest(slug).await?;
    Ok(Info {
        title: manifest.title,
        description: manifest.description,
        version: manifest.version,
    })
}

/// 记录里有、目录也还在的。
pub fn owned() -> Vec<Owned> {
    load_lock()
        .skills
        .into_iter()
        .filter_map(|(slug, entries)| {
            let mut title = String::new();
            let mut places: Vec<OwnedPlace> = entries
                .into_iter()
                .filter_map(|(dir, entry)| {
                    let path = Path::new(&dir);
                    if !std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
                        return None;
                    }
                    if !entry.title.is_empty() {
                        title.clone_from(&entry.title);
                    }
                    Some(OwnedPlace {
                        agent: entry.agent,
                        label: entry.agent.label(),
                        version: entry.version,
                        modified: tree_hash(path).as_ref() != Some(&entry.tree),
                        dir,
                    })
                })
                .collect();
            places.sort_by_key(|p| p.agent);
            let title = if title.is_empty() { slug.clone() } else { title };
            (!places.is_empty()).then_some(Owned { slug, title, places })
        })
        .collect()
}

pub async fn prepare(slug: &str) -> Result<Ready, String> {
    let (base, manifest) = manifest(slug).await?;
    let zip = fetch(&format!("{base}.zip"), MAX_ZIP, 60).await?;
    let files = unpack(&manifest, &zip)?;
    Ok(Ready { manifest, files })
}

fn check_manifest(manifest: &Manifest, slug: &str) -> Result<(), &'static str> {
    if manifest.slug != slug
        || manifest.id != format!("@pontex/{slug}")
        || !valid_slug(&manifest.dir)
        || manifest.files.is_empty()
        || manifest.files.len() > MAX_FILES
        || !manifest.files.iter().any(|f| f.path == "SKILL.md")
        || !manifest.files.iter().all(|f| safe_relative(&f.path))
    {
        return Err("SKILL_BAD_MANIFEST");
    }
    Ok(())
}

fn unpack(manifest: &Manifest, bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, &'static str> {
    if hex(bytes) != manifest.zip.sha256.to_ascii_lowercase() {
        return Err("SKILL_CHECKSUM");
    }
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| "SKILL_BAD_ZIP")?;
    if archive.len() > MAX_FILES * 2 {
        return Err("SKILL_TOO_LARGE");
    }
    let prefix = format!("{}/", manifest.dir);
    let mut files = BTreeMap::new();
    let mut left = MAX_UNPACKED;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|_| "SKILL_BAD_ZIP")?;
        let name = entry.name().to_string();
        if entry.is_dir() {
            if name.starts_with(&prefix) {
                continue;
            }
            return Err("SKILL_BAD_ZIP");
        }
        if entry.is_symlink() {
            return Err("SKILL_BAD_ZIP");
        }
        let path = name.strip_prefix(&prefix).ok_or("SKILL_BAD_ZIP")?.to_string();
        if !safe_relative(&path) {
            return Err("SKILL_BAD_ZIP");
        }
        let mut data = Vec::new();
        entry
            .take(left + 1)
            .read_to_end(&mut data)
            .map_err(|_| "SKILL_BAD_ZIP")?;
        left = left
            .checked_sub(data.len() as u64)
            .ok_or("SKILL_TOO_LARGE")?;
        // 大小写不同的同名文件在 macOS / Windows 上会互相覆盖。
        if files.keys().any(|k: &String| k.eq_ignore_ascii_case(&path)) {
            return Err("SKILL_BAD_ZIP");
        }
        files.insert(path, data);
    }
    for file in &manifest.files {
        let data = files.get(&file.path).ok_or("SKILL_CHECKSUM")?;
        if hex(data) != file.sha256.to_ascii_lowercase() {
            return Err("SKILL_CHECKSUM");
        }
    }
    let unlisted = files
        .keys()
        .filter(|path| !manifest.files.iter().any(|f| &f.path == *path))
        .any(|path| !EXTRAS.contains(&path.as_str()));
    if unlisted {
        return Err("SKILL_CHECKSUM");
    }
    let marker: Marker = files
        .get(MARKER)
        .and_then(|data| serde_json::from_slice(data).ok())
        .ok_or("SKILL_BAD_MANIFEST")?;
    if marker.id != manifest.id || marker.version != manifest.version {
        return Err("SKILL_BAD_MANIFEST");
    }
    Ok(files)
}

/// 目录内容的指纹；系统自己放的文件不算。
fn tree_hash(dir: &Path) -> Option<String> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, String>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name();
            if name == ".DS_Store" || name == "__pycache__" {
                continue;
            }
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                walk(&path, root, out)?;
            } else {
                let rel = path.strip_prefix(root).unwrap_or(&path);
                let rel = rel.to_string_lossy().replace('\\', "/");
                out.insert(rel, hex(&std::fs::read(&path)?));
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out).ok()?;
    Some(hash_list(out.iter()))
}

fn hash_list<'a>(items: impl Iterator<Item = (&'a String, &'a String)>) -> String {
    let text: String = items.map(|(path, sum)| format!("{path}\n{sum}\n")).collect();
    hex(text.as_bytes())
}

impl Ready {
    pub fn slug(&self) -> &str {
        &self.manifest.slug
    }

    fn tree(&self) -> String {
        let sums: BTreeMap<String, String> =
            self.files.iter().map(|(k, v)| (k.clone(), hex(v))).collect();
        hash_list(sums.iter())
    }

    /// 装到哪个目录、那里现在是什么。
    fn place(&self, root: &Path, lock: &Lock) -> (PathBuf, Place) {
        let first = root.join(&self.manifest.dir);
        match self.inspect(&first, lock) {
            Some(place) => (first, place),
            // 用户自己的同名技能：和网站安装说明一样改用 `<dir>-pontex`。
            None => {
                let second = root.join(format!("{}-pontex", self.manifest.dir));
                match self.inspect(&second, lock) {
                    Some(Place::New) | None => (second, Place::Renamed),
                    Some(place) => (second, place),
                }
            }
        }
    }

    /// `None`：是别人的目录。
    fn inspect(&self, dir: &Path, lock: &Lock) -> Option<Place> {
        match std::fs::symlink_metadata(dir) {
            Err(_) => return Some(Place::New),
            Ok(meta) if meta.file_type().is_symlink() => return Some(Place::Linked),
            Ok(meta) if !meta.is_dir() => return None,
            Ok(_) => {}
        }
        let marker: Marker =
            serde_json::from_slice(&std::fs::read(dir.join(MARKER)).ok()?).ok()?;
        if marker.id != self.manifest.id {
            return None;
        }
        let now = tree_hash(dir)?;
        if now == self.tree() {
            return Some(Place::UpToDate);
        }
        let recorded = lock
            .skills
            .get(&self.manifest.slug)
            .and_then(|places| places.get(&dir.display().to_string()));
        let from = marker.version;
        Some(match recorded {
            Some(entry) if entry.tree == now => Place::Update { from },
            // 没有记录（网页「复制一句话」装的）或和记录不一致：按改过处理，先备份。
            _ => Place::Modified { from },
        })
    }

    pub fn preview(&self, installed: impl Fn(AgentId) -> bool) -> Preview {
        let lock = load_lock();
        let targets = AgentId::ALL
            .into_iter()
            .filter_map(|agent| {
                let (dir, state) = self.place(&skills_dir(agent).ok()?, &lock);
                Some(TargetView {
                    agent,
                    label: agent.label(),
                    installed: installed(agent),
                    verified: self.manifest.works_with.iter().any(|w| w == site_id(agent)),
                    state,
                    dir: dir.display().to_string(),
                })
            })
            .collect();
        Preview {
            slug: self.manifest.slug.clone(),
            title: self.manifest.title.clone(),
            description: self.manifest.description.clone(),
            version: self.manifest.version.clone(),
            file_count: self.manifest.files.len(),
            bytes: self.files.values().map(Vec::len).sum(),
            scripts: self
                .files
                .keys()
                .filter(|path| {
                    Path::new(path)
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| SCRIPT_EXT.contains(&e.to_ascii_lowercase().as_str()))
                })
                .cloned()
                .collect(),
            safety_note: self.manifest.safety_note.clone(),
            targets,
        }
    }

    pub fn install(&self, agents: &[AgentId]) -> Vec<Installed> {
        agents
            .iter()
            .map(|agent| match self.install_one(*agent) {
                Ok((dir, backup)) => Installed {
                    agent: *agent,
                    ok: true,
                    dir: Some(dir.display().to_string()),
                    backup: backup.map(|b| b.display().to_string()),
                    error: None,
                },
                Err(error) => Installed {
                    agent: *agent,
                    ok: false,
                    dir: None,
                    backup: None,
                    error: Some(error),
                },
            })
            .collect()
    }

    fn install_one(&self, agent: AgentId) -> Result<(PathBuf, Option<PathBuf>), String> {
        let root = skills_dir(agent)?;
        let mut lock = load_lock();
        let (dir, place) = self.place(&root, &lock);
        let mut backup = None;
        match place {
            Place::Linked => return Err("SKILL_LINKED".into()),
            Place::UpToDate => {}
            Place::New | Place::Renamed | Place::Update { .. } | Place::Modified { .. } => {
                std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
                let staged = tempfile::Builder::new()
                    .prefix(".pontex-one-")
                    .tempdir_in(&root)
                    .map_err(|e| e.to_string())?;
                for (path, data) in &self.files {
                    let target = staged.path().join(path);
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    std::fs::write(&target, data).map_err(|e| e.to_string())?;
                }
                if tree_hash(staged.path()) != Some(self.tree()) {
                    return Err("SKILL_WRITE_MISMATCH".into());
                }
                let mut moved = None;
                if dir.exists() {
                    let aside = backup_dir(agent, &dir)?;
                    move_dir(&dir, &aside)?;
                    moved = Some(aside);
                }
                let staged = staged.keep();
                if let Err(error) = std::fs::rename(&staged, &dir) {
                    let _ = std::fs::remove_dir_all(&staged);
                    if let Some(aside) = &moved {
                        let _ = move_dir(aside, &dir);
                    }
                    return Err(error.to_string());
                }
                if matches!(place, Place::Modified { .. }) {
                    backup = moved;
                } else if let Some(aside) = moved {
                    let _ = std::fs::remove_dir_all(aside);
                }
            }
        }
        lock.version = 1;
        lock.skills.entry(self.manifest.slug.clone()).or_default().insert(
            dir.display().to_string(),
            LockEntry {
                version: self.manifest.version.clone(),
                agent,
                tree: self.tree(),
                title: self.manifest.title.clone(),
            },
        );
        save_lock(&lock)?;
        Ok((dir, backup))
    }
}

/// 移除：只移除我们装的、没被改过的；改过的先挪到备份。
pub fn remove(slug: &str, agent: AgentId) -> Result<(), String> {
    let mut lock = load_lock();
    let Some(places) = lock.skills.get_mut(slug) else {
        return Err("SKILL_NOT_OURS".into());
    };
    let dirs: Vec<String> = places
        .iter()
        .filter(|(_, entry)| entry.agent == agent)
        .map(|(dir, _)| dir.clone())
        .collect();
    if dirs.is_empty() {
        return Err("SKILL_NOT_OURS".into());
    }
    for dir in dirs {
        let path = PathBuf::from(&dir);
        let is_dir = std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir());
        if is_dir {
            let aside = backup_dir(agent, &path)?;
            let untouched = places.get(&dir).map(|e| e.tree.clone()) == tree_hash(&path);
            move_dir(&path, &aside)?;
            if untouched {
                let _ = std::fs::remove_dir_all(aside);
            }
        }
        places.remove(&dir);
    }
    save_lock(&lock)
}

fn save_lock(lock: &Lock) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(lock).map_err(|e| e.to_string())?;
    super::upstream::write_private(&lock_path(), &bytes)
}

/// 备份放在自己的数据目录，不放在技能目录里：放那里会被 Agent 当成另一个技能加载。
fn backup_dir(agent: AgentId, dir: &Path) -> Result<PathBuf, String> {
    let root = super::upstream::device_dir().join("skill-backups").join(agent.key());
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let name = dir.file_name().unwrap_or_default().to_string_lossy().to_string();
    let stamp = chrono::Utc::now().format("%Y%m%d%H%M%S%3f");
    Ok(root.join(format!("{name}-{stamp}")))
}

fn move_dir(from: &Path, to: &Path) -> Result<(), String> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    // 跨盘：复制完整、核对过再删原来的。
    copy_dir(from, to).map_err(|e| e.to_string())?;
    if tree_hash(from) != tree_hash(to) {
        let _ = std::fs::remove_dir_all(to);
        return Err("SKILL_BACKUP_FAILED".into());
    }
    std::fs::remove_dir_all(from).map_err(|e| e.to_string())
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
