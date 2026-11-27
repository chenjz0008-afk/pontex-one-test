//! 用正式构建的目录名验证隔离：单元测试里上游目录名保持原样，测不到这一点。

use std::path::Path;

fn files(dir: &Path, root: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, root, out);
        } else {
            out.push(path.strip_prefix(root).unwrap().display().to_string());
        }
    }
}

#[test]
fn everything_lives_in_our_own_directory() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("CC_SWITCH_TEST_HOME", home.path());
    std::env::set_var("HOME", home.path());
    // 这台电脑上装着原版：它的数据一个字节都不能动
    let theirs = home.path().join(".cc-switch");
    std::fs::create_dir_all(&theirs).unwrap();
    for name in ["settings.json", "cc-switch.db", "live-state.json"] {
        std::fs::write(theirs.join(name), b"theirs").unwrap();
    }
    let snapshot = cc_switch_lib::pontex::open_for_test().unwrap();
    assert!(snapshot.contains("\"cc_switch_present\":true"));
    let mut found = Vec::new();
    files(home.path(), home.path(), &mut found);
    for file in &found {
        assert!(
            file.starts_with(".pontex-one") || file.starts_with(".cc-switch"),
            "写到了不该写的地方：{file}"
        );
    }
    for name in ["settings.json", "cc-switch.db", "live-state.json"] {
        assert_eq!(std::fs::read(theirs.join(name)).unwrap(), b"theirs", "{name}");
    }
    assert_eq!(found.iter().filter(|f| f.starts_with(".cc-switch")).count(), 3, "{found:?}");
    assert!(found.iter().any(|f| f == ".pontex-one/cc-switch.db" || f == ".pontex-one\\cc-switch.db"));
}
