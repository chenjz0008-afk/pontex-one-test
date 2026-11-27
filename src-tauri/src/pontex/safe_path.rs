//! 归档里的相对路径是否可以落盘。技能包和工具包共用。

use std::path::{Component, Path};

/// 只允许普通的相对路径；Windows 的保留字符和保留名也不要。
pub fn safe_relative(path: &str) -> bool {
    if path.is_empty()
        || path.len() > 240
        || path.contains(['\\', ':', '\0', '<', '>', '"', '|', '?', '*'])
    {
        return false;
    }
    Path::new(path).components().all(|part| match part {
        Component::Normal(name) => {
            let name = name.to_string_lossy();
            let stem = name.split('.').next().unwrap_or_default().to_ascii_uppercase();
            let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || (stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem.ends_with(|c: char| c.is_ascii_digit()));
            !reserved && !name.ends_with(['.', ' '])
        }
        _ => false,
    }) && !path.split('/').any(|part| part.is_empty() || part == ".")
}
