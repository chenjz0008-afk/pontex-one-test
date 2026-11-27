//! macOS：把 DMG 里的 Codex 桌面版装进「应用程序」。
//!
//! 1. 只读挂载 DMG（不弹 Finder 窗口），找到包 ID 是 `com.openai.codex` 的那个 .app；
//! 2. 用 ditto 原样复制到目标目录旁边的临时名字下（保留签名、扩展属性、符号链接）；
//! 3. 对复制出来的那份验签：苹果签名链、OpenAI 的 Team ID、包 ID，再过一遍公证检查；
//! 4. 通过了才原子地改名成正式的名字。任何一步失败都不留半成品，DMG 一定卸载。
//!
//! 已经有一个同名、但不是 Codex 的 .app（比如老的 ChatGPT 应用）：不覆盖，交给用户决定。

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::process::Command;

use super::desktop::{BUNDLE_ID, TAKEN, TEAM_ID};
use super::{ARCHIVE, UNTRUSTED};

/// codesign 的要求：苹果签发的证书链 + OpenAI 的 Team ID + Codex 的包 ID。
pub fn requirement() -> String {
    format!("=anchor apple generic and identifier \"{BUNDLE_ID}\" and certificate leaf[subject.OU] = \"{TEAM_ID}\"")
}

/// 装着的版本。看「应用程序」和自己的 ~/Applications 两处。
pub fn installed() -> Option<String> {
    let home = super::super::upstream::home();
    ["/Applications".into(), home.join("Applications")]
        .iter()
        .flat_map(|dir: &PathBuf| ["ChatGPT.app", "Codex.app"].map(|name| dir.join(name)))
        .find(|app| info(app, "CFBundleIdentifier").as_deref() == Some(BUNDLE_ID))
        .map(|app| info(&app, "CFBundleShortVersionString").unwrap_or_default())
}

/// 读 Info.plist 里的一个字符串。文本格式直接读，二进制格式交给系统的 plutil。
pub fn info(app: &Path, key: &str) -> Option<String> {
    let plist = app.join("Contents/Info.plist");
    let bytes = std::fs::read(&plist).ok()?;
    if bytes.starts_with(b"bplist") {
        let out = Command::new("/usr/bin/plutil").args(["-extract", key, "raw", "-o", "-"]).arg(&plist).output().ok()?;
        return out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string());
    }
    let text = String::from_utf8_lossy(&bytes);
    let (_, tail) = text.split_once(&format!("<key>{key}</key>"))?;
    let (_, tail) = tail.split_once("<string>")?;
    let (value, _) = tail.split_once("</string>")?;
    Some(value.trim().to_string())
}

/// 挂载点：离开作用域就卸载，失败的路径上也一样。
struct Mounted(PathBuf);

impl Drop for Mounted {
    fn drop(&mut self) {
        let detach = |force: bool| {
            let mut c = Command::new("/usr/bin/hdiutil");
            c.arg("detach").arg(&self.0).arg("-quiet");
            if force {
                c.arg("-force");
            }
            c.status().is_ok_and(|s| s.success())
        };
        if !detach(false) {
            let _ = detach(true);
        }
    }
}

fn attach(dmg: &Path) -> Result<Mounted, String> {
    let root = tempfile::Builder::new().prefix("pontex-codex-").tempdir().map_err(|e| e.to_string())?.keep();
    // 已经校验过 SHA-256，-noverify 省掉 hdiutil 自己再读一遍几百 MB。
    let out = Command::new("/usr/bin/hdiutil")
        .args(["attach", "-nobrowse", "-readonly", "-noautoopen", "-noverify", "-plist", "-mountroot"])
        .arg(&root)
        .arg(dmg)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        log::warn!("[PONTEX] 挂载 DMG 失败: {}", String::from_utf8_lossy(&out.stderr).trim());
        return Err(ARCHIVE.into());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let point = text
        .split("<key>mount-point</key>")
        .skip(1)
        .find_map(|tail| tail.split_once("<string>")?.1.split_once("</string>").map(|(p, _)| p.trim().to_string()))
        .ok_or(ARCHIVE)?;
    Ok(Mounted(PathBuf::from(point)))
}

/// 对一个 .app 验签。两关都要过。
pub fn verify(app: &Path) -> Result<(), String> {
    let codesign = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict", "-R"])
        .arg(requirement())
        .arg(app)
        .output()
        .map_err(|e| e.to_string())?;
    if !codesign.status.success() {
        log::warn!("[PONTEX] Codex 桌面版签名不对: {}", String::from_utf8_lossy(&codesign.stderr).trim());
        return Err(UNTRUSTED.into());
    }
    // 公证：Gatekeeper 的评估。我们下载的文件不带隔离标记，打开时系统不会再查，所以这里替它查。
    let gatekeeper = Command::new("/usr/sbin/spctl")
        .args(["--assess", "--type", "execute", "-vv"])
        .arg(app)
        .output()
        .map_err(|e| e.to_string())?;
    if !gatekeeper.status.success() {
        log::warn!("[PONTEX] Codex 桌面版没通过公证检查: {}", String::from_utf8_lossy(&gatekeeper.stderr).trim());
        return Err(UNTRUSTED.into());
    }
    Ok(())
}

/// 装进 `dest`（「应用程序」或 ~/Applications）。已经装着同一个应用就什么都不做。
pub fn install(dmg: &Path, dest: &Path) -> Result<PathBuf, String> {
    let mounted = attach(dmg)?;
    let source = std::fs::read_dir(&mounted.0)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "app") && info(p, "CFBundleIdentifier").as_deref() == Some(BUNDLE_ID))
        .ok_or(ARCHIVE)?;
    let name = source.file_name().ok_or(ARCHIVE)?.to_owned();
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let target = dest.join(&name);
    if target.exists() {
        return match info(&target, "CFBundleIdentifier").as_deref() {
            Some(BUNDLE_ID) => Ok(target),
            _ => Err(TAKEN.into()),
        };
    }
    let staging = dest.join(format!(".{}.pontex-{}", name.to_string_lossy(), std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    let result = (|| {
        let copied = Command::new("/usr/bin/ditto").arg(&source).arg(&staging).status().map_err(|e| e.to_string())?;
        if !copied.success() {
            return Err(super::DISK.to_string());
        }
        // 验的是要留下来的那一份，不是 DMG 里的
        verify(&staging)?;
        std::fs::rename(&staging, &target).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    drop(mounted);
    result.map(|()| target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requirement_pins_team_and_bundle() {
        let r = requirement();
        assert!(r.starts_with("=anchor apple generic"));
        assert!(r.contains("certificate leaf[subject.OU] = \"2DC432GLL2\""));
        assert!(r.contains("identifier \"com.openai.codex\""));
    }

    #[test]
    fn reads_text_info_plists() {
        let app = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(app.path().join("Contents")).unwrap();
        std::fs::write(
            app.path().join("Contents/Info.plist"),
            "<plist><dict><key>CFBundleIdentifier</key>\n\t<string>com.openai.codex</string>\
             <key>CFBundleShortVersionString</key><string>26.1120.50957</string></dict></plist>",
        )
        .unwrap();
        assert_eq!(info(app.path(), "CFBundleIdentifier").as_deref(), Some(BUNDLE_ID));
        assert_eq!(info(app.path(), "CFBundleShortVersionString").as_deref(), Some("26.1120.50957"));
        assert_eq!(info(app.path(), "Missing"), None);
    }
}
