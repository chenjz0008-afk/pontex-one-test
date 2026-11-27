//! AI 工具箱里的技能：每个 Agent 的技能目录里有什么，在哪些 Agent 里启用。
//!
//! 停用不删：把技能文件夹挪到 `~/.pontex-one/skills-off/<agent>/`，启用时挪回来；
//! 这个 Agent 从没有过这个技能，就从别的 Agent 那里复制一份。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::agents::AgentId;
use super::upstream;

/// 有技能目录的 Agent。WorkBuddy 没有。
pub const AGENTS: [AgentId; 4] = [
    AgentId::ClaudeCode,
    AgentId::Codex,
    AgentId::GeminiCli,
    AgentId::GrokBuild,
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Skill {
    /// 文件夹名，也是 id。
    pub dir: String,
    pub name: String,
    pub description: String,
    /// 已启用的 Agent。
    pub on: Vec<AgentId>,
}

fn live_root(agent: AgentId) -> Option<PathBuf> {
    upstream::skills_dir(agent).ok()
}

fn off_root(agent: AgentId) -> PathBuf {
    upstream::device_dir().join("skills-off").join(agent.key())
}

/// 文件夹名只能是一层、不能是隐藏的。
fn check_dir(dir: &str) -> Result<&str, String> {
    let ok = !dir.is_empty()
        && !dir.starts_with('.')
        && !dir.contains(['/', '\\'])
        && dir != ".."
        && !dir.chars().any(char::is_control);
    if ok {
        Ok(dir)
    } else {
        Err("SKILL_NAME".into())
    }
}

/// 读 SKILL.md 开头的 `name:` 和 `description:`。
fn read_meta(dir: &Path) -> Option<(String, String)> {
    let text = std::fs::read_to_string(dir.join("SKILL.md")).ok()?;
    let mut lines = text.lines();
    let (mut name, mut description) = (String::new(), String::new());
    if lines.next().map(str::trim) == Some("---") {
        for line in lines {
            if line.trim() == "---" {
                break;
            }
            let unquote = |v: &str| v.trim().trim_matches(|c| c == '"' || c == '\'').to_string();
            if let Some(v) = line.strip_prefix("name:") {
                name = unquote(v);
            } else if let Some(v) = line.strip_prefix("description:") {
                description = unquote(v);
            }
        }
    }
    Some((name, description))
}

fn skill_dirs(root: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            // 跟着符号链接走：上游的统一技能目录用链接挂进来。
            (check_dir(&name).is_ok() && path.join("SKILL.md").is_file()).then_some((name, path))
        })
        .collect()
}

/// 全部技能，按名字排。只看 `agents` 里的 Agent（装着的）。
pub fn list(agents: &[AgentId]) -> Vec<Skill> {
    let mut found: BTreeMap<String, Skill> = BTreeMap::new();
    for &agent in AGENTS.iter().filter(|a| agents.contains(a)) {
        let live = live_root(agent).map(|r| skill_dirs(&r)).unwrap_or_default();
        let off = skill_dirs(&off_root(agent));
        for (enabled, (dir, path)) in live
            .into_iter()
            .map(|d| (true, d))
            .chain(off.into_iter().map(|d| (false, d)))
        {
            let entry = found.entry(dir.clone()).or_insert_with(|| {
                let (name, description) = read_meta(&path).unwrap_or_default();
                Skill {
                    name: if name.is_empty() { dir.clone() } else { name },
                    description,
                    dir,
                    on: Vec::new(),
                }
            });
            if enabled && !entry.on.contains(&agent) {
                entry.on.push(agent);
            }
        }
    }
    let mut out: Vec<Skill> = found.into_values().collect();
    out.sort_by_key(|s| s.name.to_lowercase());
    out
}

/// 在一个 Agent 里启用或停用。
pub fn toggle(dir: &str, agent: AgentId, enabled: bool) -> Result<(), String> {
    let dir = check_dir(dir)?;
    if !AGENTS.contains(&agent) {
        return Err("SKILL_AGENT".into());
    }
    let live = live_root(agent).ok_or("SKILL_AGENT")?.join(dir);
    let off = off_root(agent).join(dir);
    if enabled {
        if live.join("SKILL.md").is_file() {
            return Ok(());
        }
        if off.join("SKILL.md").is_file() {
            return move_dir(&off, &live);
        }
        // 这个 Agent 从没有过：从别的 Agent 复制一份（先找启用着的，再找停用的）。
        let source = AGENTS
            .iter()
            .filter(|&&a| a != agent)
            .flat_map(|&a| {
                [
                    live_root(a).map(|r| r.join(dir)),
                    Some(off_root(a).join(dir)),
                ]
            })
            .flatten()
            .find(|p| p.join("SKILL.md").is_file())
            .ok_or("SKILL_MISSING")?;
        copy_dir(&source, &live)
    } else {
        if !live.exists() && std::fs::symlink_metadata(&live).is_err() {
            return Ok(());
        }
        if std::fs::symlink_metadata(&off).is_ok() {
            remove(&off)?;
        }
        move_dir(&live, &off)
    }
}

fn remove(path: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
    .map_err(|e| e.to_string())
}

/// 挪过去；跨磁盘挪不了就复制再删。
fn move_dir(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    copy_dir(from, to)?;
    remove(from)
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(from)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let target = to.join(entry.file_name());
        // 跟着链接复制内容。
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(root: &Path, dir: &str, desc: &str) {
        std::fs::create_dir_all(root.join(dir).join("scripts")).unwrap();
        std::fs::write(
            root.join(dir).join("SKILL.md"),
            format!("---\nname: {dir}\ndescription: \"{desc}\"\n---\n\nbody"),
        )
        .unwrap();
        std::fs::write(root.join(dir).join("scripts/run.py"), "print(1)").unwrap();
    }

    #[test]
    fn meta_is_read_from_the_front_matter() {
        let tmp = tempfile::tempdir().unwrap();
        skill(tmp.path(), "pdf", "读、拆、合并 PDF");
        assert_eq!(
            read_meta(&tmp.path().join("pdf")),
            Some(("pdf".into(), "读、拆、合并 PDF".into()))
        );
    }

    #[test]
    fn names_cannot_escape_the_skills_folder() {
        for bad in ["", "..", ".hidden", "a/b", "a\\b"] {
            assert!(check_dir(bad).is_err(), "{bad}");
        }
        assert!(check_dir("web-research").is_ok());
    }

    #[test]
    fn moving_and_copying_keep_every_file() {
        let tmp = tempfile::tempdir().unwrap();
        skill(tmp.path(), "pdf", "x");
        let to = tmp.path().join("off/claude/pdf");
        move_dir(&tmp.path().join("pdf"), &to).unwrap();
        assert!(!tmp.path().join("pdf").exists());
        assert!(to.join("scripts/run.py").is_file());
        copy_dir(&to, &tmp.path().join("codex/pdf")).unwrap();
        assert!(tmp.path().join("codex/pdf/SKILL.md").is_file());
        assert!(to.join("SKILL.md").is_file());
    }

    #[test]
    fn listing_merges_agents_and_reads_hidden_folders_as_not_skills() {
        let tmp = tempfile::tempdir().unwrap();
        skill(tmp.path(), "pdf", "x");
        std::fs::create_dir_all(tmp.path().join(".system/x")).unwrap();
        std::fs::create_dir_all(tmp.path().join("notes")).unwrap();
        let dirs: Vec<String> = skill_dirs(tmp.path()).into_iter().map(|d| d.0).collect();
        assert_eq!(dirs, vec!["pdf".to_string()]);
    }
}
