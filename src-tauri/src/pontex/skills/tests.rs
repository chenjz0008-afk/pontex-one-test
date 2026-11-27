use std::io::Write;

use super::*;

fn pack(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, data) in entries {
        writer.start_file(*name, options).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn marker(version: &str) -> Vec<u8> {
    format!(r#"{{"id":"@pontex/demo","version":"{version}"}}"#).into_bytes()
}

fn manifest(version: &str, listed: &[(&str, &[u8])], zip: &[u8]) -> Manifest {
    serde_json::from_value(serde_json::json!({
        "id": "@pontex/demo", "slug": "demo", "dir": "demo", "title": "演示", "version": version,
        "worksWith": ["claude-code", "codex"],
        "files": listed.iter().map(|(p, d)| serde_json::json!({"path": p, "sha256": hex(d)})).collect::<Vec<_>>(),
        "zip": {"sha256": hex(zip)},
    }))
    .unwrap()
}

fn ready(version: &str, body: &[u8]) -> Ready {
    let mark = marker(version);
    let zip = pack(&[
        ("demo/SKILL.md", body),
        ("demo/run.py", b"print(1)"),
        ("demo/pontex.json", &mark),
        ("demo/INSTALL.txt", b"hi"),
    ]);
    let manifest = manifest(version, &[("SKILL.md", body), ("run.py", b"print(1)")], &zip);
    check_manifest(&manifest, "demo").unwrap();
    let files = unpack(&manifest, &zip).unwrap();
    Ready { manifest, files }
}

struct Home {
    dir: tempfile::TempDir,
    _env: crate::test_support::EnvVars,
}

fn home() -> Home {
    let dir = tempfile::tempdir().unwrap();
    let env = crate::test_support::EnvVars::new()
        .set("CC_SWITCH_TEST_HOME", dir.path())
        .set("HOME", dir.path());
    let _ = crate::settings::reload_settings();
    Home { dir, _env: env }
}

#[test]
fn rejects_every_tampered_package() {
    let body: &[u8] = b"---\nname: demo\n---\n";
    let mark = marker("1");
    let good: Vec<(&str, &[u8])> = vec![("demo/SKILL.md", body), ("demo/pontex.json", &mark)];
    let listed: Vec<(&str, &[u8])> = vec![("SKILL.md", body)];
    let zip = pack(&good);
    assert!(unpack(&manifest("1", &listed, &zip), &zip).is_ok());

    // 包的哈希不对
    assert_eq!(unpack(&manifest("1", &listed, b"other"), &zip), Err("SKILL_CHECKSUM"));
    // 内容和清单不符
    let wrong: Vec<(&str, &[u8])> = vec![("SKILL.md", b"x")];
    assert_eq!(unpack(&manifest("1", &wrong, &zip), &zip), Err("SKILL_CHECKSUM"));
    // 缺文件
    let more: Vec<(&str, &[u8])> = vec![("SKILL.md", body), ("gone.md", b"x")];
    assert_eq!(unpack(&manifest("1", &more, &zip), &zip), Err("SKILL_CHECKSUM"));
    for (extra, reason) in [
        ("demo/evil.sh", "SKILL_CHECKSUM"),
        ("demo/../../evil", "SKILL_BAD_ZIP"),
        ("/etc/evil", "SKILL_BAD_ZIP"),
        ("other/SKILL.md", "SKILL_BAD_ZIP"),
        ("demo/skill.md", "SKILL_BAD_ZIP"),
        ("demo/a\\b", "SKILL_BAD_ZIP"),
        ("demo/CON.txt", "SKILL_BAD_ZIP"),
    ] {
        let mut entries = good.clone();
        entries.push((extra, b"x"));
        let zip = pack(&entries);
        assert_eq!(unpack(&manifest("1", &listed, &zip), &zip), Err(reason), "{extra}");
    }
    // 标记和清单对不上
    let other = marker("2");
    let zip = pack(&[("demo/SKILL.md", body), ("demo/pontex.json", &other)]);
    assert_eq!(unpack(&manifest("1", &listed, &zip), &zip), Err("SKILL_BAD_MANIFEST"));
}

#[test]
fn rejects_symlinks_and_bombs() {
    let body: &[u8] = b"x";
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    writer
        .add_symlink("demo/SKILL.md", "/etc/passwd", zip::write::SimpleFileOptions::default())
        .unwrap();
    let zip = writer.finish().unwrap().into_inner();
    assert_eq!(unpack(&manifest("1", &[("SKILL.md", body)], &zip), &zip), Err("SKILL_BAD_ZIP"));

    let huge = vec![0u8; (MAX_UNPACKED + 10) as usize];
    let zip = pack(&[("demo/SKILL.md", &huge)]);
    assert!(zip.len() < MAX_ZIP, "压缩后很小");
    assert_eq!(unpack(&manifest("1", &[("SKILL.md", &huge)], &zip), &zip), Err("SKILL_TOO_LARGE"));
}

#[test]
fn rejects_manifests_for_something_else() {
    let m = manifest("1", &[("SKILL.md", b"x")], b"");
    assert!(check_manifest(&m, "other").is_err());
    for bad in ["../x", "/abs", "a//b", "a/./b", "a/../b", "", "x:y", "NUL", "dir/aux.md", "end.", "a\\b"] {
        assert!(!safe_relative(bad), "{bad}");
    }
    assert!(safe_relative("assets/templates/A.md"));
}

#[test]
#[serial_test::serial]
fn installs_updates_and_removes() {
    let h = home();
    let v1 = ready("1", b"one");
    let preview = v1.preview(|_| true);
    assert_eq!(preview.scripts, vec!["run.py"]);
    assert!(preview.targets.iter().find(|t| t.agent == AgentId::ClaudeCode).unwrap().verified);
    assert!(!preview.targets.iter().find(|t| t.agent == AgentId::Workbuddy).unwrap().verified);

    assert!(v1.install(&[AgentId::ClaudeCode, AgentId::Codex]).iter().all(|r| r.ok));
    let dir = h.dir.path().join(".claude/skills/demo");
    assert_eq!(std::fs::read(dir.join("SKILL.md")).unwrap(), b"one");
    let state = |r: &Ready| r.preview(|_| true).targets.into_iter().find(|t| t.agent == AgentId::ClaudeCode).unwrap().state;
    assert_eq!(state(&v1), Place::UpToDate);

    let v2 = ready("2", b"two");
    assert_eq!(state(&v2), Place::Update { from: "1".into() });
    let done = v2.install(&[AgentId::ClaudeCode]);
    assert!(done[0].ok && done[0].backup.is_none());
    assert_eq!(std::fs::read(dir.join("SKILL.md")).unwrap(), b"two");

    // 没有半成品留在技能目录里
    let names: Vec<_> = std::fs::read_dir(dir.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, vec!["demo"]);

    let list = owned();
    assert_eq!((list.len(), list[0].title.as_str()), (1, "演示"));
    assert!(list[0].places.iter().any(|p| p.agent == AgentId::ClaudeCode && !p.modified));
    remove("demo", AgentId::ClaudeCode).unwrap();
    assert!(!dir.exists());
    assert!(h.dir.path().join(".codex/skills/demo").exists());
    assert!(remove("demo", AgentId::GeminiCli).is_err());
}

#[test]
#[serial_test::serial]
fn never_overwrites_what_it_does_not_own() {
    let h = home();
    let mine = h.dir.path().join(".claude/skills/demo");
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::write(mine.join("SKILL.md"), "my own").unwrap();
    let v1 = ready("1", b"one");
    let done = v1.install(&[AgentId::ClaudeCode]);
    assert!(done[0].ok, "{done:?}");
    assert_eq!(std::fs::read_to_string(mine.join("SKILL.md")).unwrap(), "my own");
    assert!(h.dir.path().join(".claude/skills/demo-pontex/SKILL.md").exists());
    remove("demo", AgentId::ClaudeCode).unwrap();
    assert!(mine.exists());
}

#[test]
#[serial_test::serial]
fn edited_skills_are_backed_up_first() {
    let h = home();
    ready("1", b"one").install(&[AgentId::ClaudeCode]);
    let file = h.dir.path().join(".claude/skills/demo/SKILL.md");
    std::fs::write(&file, "edited by me").unwrap();
    let done = ready("2", b"two").install(&[AgentId::ClaudeCode]);
    let backup = PathBuf::from(done[0].backup.clone().unwrap());
    assert_eq!(std::fs::read_to_string(backup.join("SKILL.md")).unwrap(), "edited by me");
    assert_eq!(std::fs::read(&file).unwrap(), b"two");
}

#[test]
#[serial_test::serial]
fn recognises_skills_installed_from_the_website_sentence() {
    let h = home();
    let v1 = ready("1", b"one");
    let dir = h.dir.path().join(".codex/skills/demo");
    for (path, data) in &v1.files {
        let target = dir.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, data).unwrap();
    }
    let state = |r: &Ready| r.preview(|_| true).targets.into_iter().find(|t| t.agent == AgentId::Codex).unwrap().state;
    assert_eq!(state(&v1), Place::UpToDate);
    assert_eq!(state(&ready("2", b"two")), Place::Modified { from: "1".into() });
}

#[cfg(unix)]
#[test]
#[serial_test::serial]
fn leaves_symlinked_skills_alone() {
    let h = home();
    let elsewhere = h.dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let skills = h.dir.path().join(".claude/skills");
    std::fs::create_dir_all(&skills).unwrap();
    std::os::unix::fs::symlink(&elsewhere, skills.join("demo")).unwrap();
    let done = ready("1", b"one").install(&[AgentId::ClaudeCode]);
    assert_eq!(done[0].error.as_deref(), Some("SKILL_LINKED"));
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
}
