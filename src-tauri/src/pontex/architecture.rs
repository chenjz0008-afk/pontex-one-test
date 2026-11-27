//! 架构约束，写成测试：破坏了就不能合并。

use std::fs;
use std::path::Path;

fn sources() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/pontex");
    [dir.clone(), dir.join("installer")]
        .into_iter()
        .flat_map(|d| fs::read_dir(d).unwrap().flatten())
        .filter(|e| e.path().extension().is_some_and(|x| x == "rs"))
        .map(|e| {
            (
                e.file_name().to_string_lossy().to_string(),
                fs::read_to_string(e.path()).unwrap(),
            )
        })
        .collect()
}

/// 上游代码只能经过防腐层。
#[test]
fn only_the_anti_corruption_layer_touches_upstream() {
    for (name, text) in sources() {
        if matches!(name.as_str(), "upstream.rs" | "architecture.rs" | "tests.rs" | "network_tests.rs") {
            continue;
        }
        for (index, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            assert!(
                !code.contains("crate::")
                    || code.contains("crate::pontex::")
                    || code.contains("crate::test_support"),
                "{name}:{} 绕过了 upstream.rs：{line}",
                index + 1
            );
        }
    }
}

/// Key 只去一个主机：别的文件里不许出现 PonteX 以外的 API 地址常量。
#[test]
fn the_api_origin_is_defined_once() {
    for (name, text) in sources() {
        if matches!(name.as_str(), "consts.rs" | "tests.rs" | "templates.rs" | "architecture.rs") {
            continue;
        }
        assert!(!text.contains("https://api."), "{name} 里写死了 API 地址");
    }
}

/// 日志里不许出现 Key 的明文出口。
#[test]
fn secrets_are_never_formatted() {
    for (name, text) in sources() {
        for line in text.lines().filter(|l| l.contains("log::") || l.contains("println!")) {
            assert!(!line.contains("expose("), "{name}: {line}");
        }
    }
}
