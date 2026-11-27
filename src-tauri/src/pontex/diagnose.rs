//! 脱敏诊断。写到数据目录，由用户自己决定发给谁。

use std::path::PathBuf;

use regex::Regex;
use serde_json::json;

use super::upstream;

const LOG_TAIL: usize = 64 * 1024;

/// 去掉一切像凭据的东西。宁可多去。
pub fn redact(text: &str) -> String {
    static PATTERNS: std::sync::OnceLock<Vec<Regex>> = std::sync::OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        [
            r"px[p]?-[A-Za-z0-9_-]{4,}",
            r"sk-[A-Za-z0-9_-]{6,}",
            r"AIza[A-Za-z0-9_-]{6,}",
            r"xai-[A-Za-z0-9_-]{6,}",
            r"eyJ[A-Za-z0-9_-]{4,}(?:\.[A-Za-z0-9_-]+)*",
            r"(?i)bearer\s+[A-Za-z0-9._~+/=-]{6,}",
            r#"(?i)"?(?:api[_-]?key|token|secret|password)"?\s*[:=]\s*"?[^\s",}]{4,}"#,
        ]
        .iter()
        .map(|p| Regex::new(p).expect("static pattern"))
        .collect()
    });
    let mut out = text.to_string();
    for pattern in patterns {
        out = pattern.replace_all(&out, "[已隐藏]").into_owned();
    }
    match upstream::home().to_str() {
        Some(home) if home.len() > 1 => out.replace(home, "~"),
        _ => out,
    }
}

pub fn logs_dir() -> PathBuf {
    upstream::device_dir().join("logs")
}

fn log_tail() -> String {
    let path = logs_dir().join("pontex-one.log");
    let bytes = std::fs::read(path).unwrap_or_default();
    let start = bytes.len().saturating_sub(LOG_TAIL);
    redact(&String::from_utf8_lossy(&bytes[start..]))
}

/// `view` 是界面现在看到的内容；启动失败时是失败原因。
pub fn write(version: &str, view: &impl serde::Serialize) -> Result<PathBuf, String> {
    let body = json!({
        "product": version,
        "engine": upstream::VERSION,
        "os": format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        "at": chrono::Utc::now().to_rfc3339(),
        "view": view,
    });
    let text = serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?;
    let text = format!("{}\n\n===== log =====\n{}", redact(&text), log_tail());
    let name = format!("diagnostics-{}.txt", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    let path = upstream::device_dir().join(name);
    upstream::write_private(&path, text.as_bytes())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hides_every_kind_of_credential() {
        for secret in [
            "px-AbCdEfGhIjKl",
            "pxp-AbCdEfGh",
            "sk-ant-api03-abcdef",
            "AIzaSyAbcdefgh",
            "xai-abcdefgh",
            "Bearer abc.def.ghi",
            "eyJhbGciOi.eyJzdWIi.sig",
            "\"api_key\": \"whatever1\"",
        ] {
            let out = redact(&format!("before {secret} after"));
            assert!(!out.contains(secret), "{out}");
            assert!(out.starts_with("before ") && out.ends_with(" after"), "{out}");
        }
    }
}
