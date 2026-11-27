//! Windows：装 Codex 桌面版的 MSIX。
//!
//! - 包里带一个以 SYSTEM 运行的沙箱服务，Windows 规定装服务要管理员权限，所以**一定会有一次**
//!   系统的管理员确认。这一次里把要做的事一起做完，不会弹第二次。
//! - 装之前（不需要权限）：核对 MSIX 的包身份（包名、发布者、架构）；系统缺 VC++ 运行库、而包里
//!   的程序又要用它，就先把微软的运行库安装包下好、验好。
//! - 管理员那一步：脚本经 `-EncodedCommand` 直接传给系统（不落盘，没有被替换的机会），
//!   动手前再算一遍 SHA-256、验一遍微软签名。确认的是当前账户就直接装给当前账户；
//!   确认的是另一个管理员账户，就先把包登记到这台电脑，回来再由当前账户自己注册。
//! - 装完（不需要权限）：按包族名找到它，签名类型必须是商店签名；再把包里每个程序导入的
//!   DLL 对一遍，确认这台电脑上都找得到——不会在第一次打开时弹「找不到 xxx.dll」。

#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::path::{Path, PathBuf};

use super::desktop::{FAMILY, PACKAGE_NAME, PUBLISHER};
use super::download::{self, Expect};
use super::{Installer, Notify, Progress, ARCHIVE, UNTRUSTED};
use crate::pontex::agents::AgentId;

/// 包里的程序要用、系统里又没有的 DLL。
pub const DEPENDENCY: &str = "INSTALL_DEPENDENCY";

/// 微软的 VC++ 运行库安装包。版本和哈希编进软件：从哪个源下都一样可信，执行前再验一遍微软签名。
pub struct Redist {
    pub file: &'static str,
    pub sha256: &'static str,
    pub urls: &'static [&'static str],
}

pub const VCREDIST_X64: Redist = Redist {
    file: "VC_redist.x64.exe",
    sha256: "cc0ff0eb1dc3f5188ae6300faef32bf5beeba4bdd6e8e445a9184072096b713b",
    urls: &["https://download.visualstudio.microsoft.com/download/pr/e9604e21-4b79-4096-a4b3-1b63d4b55b4d/CC0FF0EB1DC3F5188AE6300FAEF32BF5BEEBA4BDD6E8E445A9184072096B713B/VC_redist.x64.exe"],
};

pub const VCREDIST_ARM64: Redist = Redist {
    file: "VC_redist.arm64.exe",
    sha256: "5139e1440c3a20b92153a4db561c069a0175aaf76c276c3e5b6f56099edcf4b0",
    urls: &["https://download.visualstudio.microsoft.com/download/pr/d7450eb5-03e1-436d-9e7e-deb5fe4759b3/5139E1440C3A20B92153A4DB561C069A0175AAF76C276C3E5B6F56099EDCF4B0/VC_redist.arm64.exe"],
};

/// MSIX 清单里的包身份。
#[derive(Debug, PartialEq, Eq)]
pub struct Identity {
    pub name: String,
    pub publisher: String,
    pub arch: String,
    pub version: String,
}

/// 从 AppxManifest.xml 里取 `<Identity .../>`。
pub fn identity(manifest: &str) -> Option<Identity> {
    let start = manifest.find("<Identity ")?;
    let tag = &manifest[start..start + manifest[start..].find('>')?];
    let attr = |name: &str| {
        let key = format!(" {name}=\"");
        let from = tag.find(&key)? + key.len();
        Some(tag[from..from + tag[from..].find('"')?].to_string())
    };
    Some(Identity {
        name: attr("Name")?,
        publisher: attr("Publisher")?,
        arch: attr("ProcessorArchitecture")?,
        version: attr("Version")?,
    })
}

/// 装之前：包身份对不对；系统缺不缺包里程序要用的运行库。缺就返回要装的那个运行库安装包。
pub fn preflight(msix: &Path, arch: &str) -> Result<Option<&'static Redist>, String> {
    let file = std::fs::File::open(msix).map_err(|_| ARCHIVE)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| ARCHIVE)?;
    let manifest = {
        use std::io::Read;
        let mut entry = zip.by_name("AppxManifest.xml").map_err(|_| ARCHIVE)?;
        let mut text = String::new();
        entry.read_to_string(&mut text).map_err(|_| ARCHIVE)?;
        text
    };
    let id = identity(&manifest).ok_or(ARCHIVE)?;
    if id.name != PACKAGE_NAME || id.publisher != PUBLISHER || id.arch != arch {
        log::warn!("[PONTEX] MSIX 包身份不对: {id:?}");
        return Err(UNTRUSTED.into());
    }
    if runtime_present() {
        return Ok(None);
    }
    // 系统里没有运行库：看包里的程序到底用不用它。只看 .exe，一个个解到临时文件里读导入表。
    let names: Vec<String> = zip.file_names().map(str::to_string).collect();
    let lower: std::collections::HashSet<String> = names.iter().map(|n| n.to_ascii_lowercase()).collect();
    for name in names.iter().filter(|n| n.to_ascii_lowercase().ends_with(".exe")) {
        let mut temp = tempfile::tempfile().map_err(|_| super::DISK)?;
        std::io::copy(&mut zip.by_name(name).map_err(|_| ARCHIVE)?, &mut temp).map_err(|_| super::DISK)?;
        let imports = super::pe::imports(temp).unwrap_or_default();
        let dir = name.rsplit_once('/').map(|(d, _)| d.to_ascii_lowercase()).unwrap_or_default();
        // 和程序放在一起的运行库（应用自带）不算缺
        let needs = imports
            .iter()
            .any(|dll| super::pe::redistributable(dll) && !lower.contains(&format!("{dir}/{}", dll.to_ascii_lowercase())));
        if needs {
            return Ok(Some(if arch == "arm64" { &VCREDIST_ARM64 } else { &VCREDIST_X64 }));
        }
    }
    Ok(None)
}

fn system32() -> PathBuf {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    root.join("System32")
}

/// 系统里有 VC++ 运行库（2015–2022 那一套）。
pub fn runtime_present() -> bool {
    let dir = system32();
    ["vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll"].iter().all(|dll| dir.join(dll).is_file())
}

impl Installer {
    /// 下运行库安装包。微软官方的地址 + 自己的 R2（配好之后）。
    pub async fn fetch_vcredist(&self, redist: &Redist, notify: &Notify<'_>) -> Result<PathBuf, String> {
        let mut urls: Vec<String> = redist.urls.iter().map(|u| u.to_string()).collect();
        if let Some(b) = std::env::var("PONTEX_CODEX_APP_MIRROR").ok().or_else(|| {
            (!super::desktop::MIRROR_B.is_empty()).then(|| super::desktop::MIRROR_B.to_string())
        }) {
            urls.push(format!("{}/vcredist/{}/{}", b.trim_end_matches('/'), redist.sha256, redist.file));
        }
        let file = self.layout.cache().join(redist.file);
        let id = AgentId::Codex;
        download::fetch(
            &self.client,
            &urls,
            &file,
            None,
            Expect::Sha256(redist.sha256),
            &std::sync::atomic::AtomicBool::new(false),
            &|status| notify(Progress { desktop: true, ..super::downloading(id, "", true, status) }),
        )
        .await?;
        Ok(file)
    }
}

impl Installer {
    /// 命令行工具试跑之前：它导入的 DLL 这台电脑上都得找得到。缺的只是 VC++ 运行库，就弹一次
    /// 管理员确认补上；缺别的就明确报错——总之不让 Windows 在试跑时弹「找不到 xxx.dll」。
    pub async fn ensure_dlls(&self, exe: &Path, id: AgentId, notify: &Notify<'_>) -> Result<(), String> {
        let (probe, system) = (exe.to_path_buf(), system32());
        let check = move || super::pe::unresolved(&probe, &system, &[]).unwrap_or_default();
        let missing = super::blocking(check.clone()).await?;
        if missing.is_empty() {
            return Ok(());
        }
        log::warn!("[PONTEX] {} 缺 DLL: {}", id.key(), missing.join(", "));
        if !missing.iter().all(|dll| super::pe::redistributable(dll)) {
            return Err(DEPENDENCY.into());
        }
        let redist = if self.platform.arch == "arm64" { &VCREDIST_ARM64 } else { &VCREDIST_X64 };
        let file = self.fetch_vcredist(redist, notify).await?;
        notify(Progress::new(id, super::Phase::Authorizing));
        let runtime = file.clone();
        super::blocking(move || elevate(Some(&runtime), None)).await??;
        let _ = std::fs::remove_file(&file);
        if super::blocking(check).await?.is_empty() {
            Ok(())
        } else {
            Err(DEPENDENCY.into())
        }
    }
}

/// PowerShell 单引号字符串。
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// 要装的 MSIX：文件、SHA-256、当前账户的 SID。
pub struct Package<'a> {
    pub file: &'a Path,
    pub sha256: &'a str,
    pub user: &'a str,
}

/// 以管理员身份执行的那段脚本：运行库（要的话）+ MSIX（要的话），一次做完。
pub fn elevated_script(out: &Path, redist: Option<(&Path, &str)>, package: Option<&Package<'_>>) -> String {
    let redist_part = match redist {
        Some((file, sha)) => format!(
            "$r = {file}\n\
             if ((Get-FileHash -Algorithm SHA256 -LiteralPath $r).Hash -ne {sha}) {{ throw 'INSTALL_CHECKSUM' }}\n\
             $s = Get-AuthenticodeSignature -LiteralPath $r\n\
             if ($s.Status -ne 'Valid' -or $s.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') {{ throw 'INSTALL_UNTRUSTED' }}\n\
             $p = Start-Process -FilePath $r -ArgumentList '/install','/quiet','/norestart' -Wait -PassThru\n\
             if (@(0, 1638, 3010) -notcontains $p.ExitCode) {{ throw ('VCREDIST ' + $p.ExitCode) }}\n",
            file = quote(&file.display().to_string()),
            sha = quote(sha),
        ),
        None => String::new(),
    };
    let package_part = match package {
        Some(p) => format!(
            "$m = {msix}\n\
             if ((Get-FileHash -Algorithm SHA256 -LiteralPath $m).Hash -ne {sha}) {{ throw 'INSTALL_CHECKSUM' }}\n\
             $me = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value\n\
             if ($me -eq {user}) {{ Add-AppxPackage -Path $m -ForceApplicationShutdown; $result.mode = 'user' }}\n\
             else {{ Add-AppxProvisionedPackage -Online -PackagePath $m -SkipLicense | Out-Null; $result.mode = 'machine' }}\n",
            msix = quote(&p.file.display().to_string()),
            sha = quote(p.sha256),
            user = quote(p.user),
        ),
        None => String::new(),
    };
    format!(
        "$ErrorActionPreference = 'Stop'\n\
         $ProgressPreference = 'SilentlyContinue'\n\
         $result = @{{ ok = $false }}\n\
         try {{\n\
         {redist_part}\
         {package_part}\
         $result.ok = $true\n\
         }} catch {{ $result.error = $_.Exception.Message }}\n\
         $result | ConvertTo-Json -Compress | Set-Content -LiteralPath {out} -Encoding UTF8\n",
        out = quote(&out.display().to_string()),
    )
}

/// `-EncodedCommand` 要的格式：UTF-16LE 再 base64。
pub fn encode(script: &str) -> String {
    use base64::Engine;
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(target_os = "windows")]
fn powershell(command: &str) -> std::io::Result<std::process::Output> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", command])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
}

#[cfg(target_os = "windows")]
fn current_sid() -> Option<String> {
    let out = powershell("[Security.Principal.WindowsIdentity]::GetCurrent().User.Value").ok()?;
    let sid = String::from_utf8_lossy(&out.stdout).trim().to_string();
    sid.starts_with("S-1-").then_some(sid)
}

/// 弹一次管理员确认，把运行库（需要的话）和 MSIX（需要的话）一起装好。
#[cfg(target_os = "windows")]
pub fn elevate(redist: Option<&Path>, msix: Option<(&Path, &str)>) -> Result<(), String> {
    let user = current_sid().ok_or(super::RUN)?;
    let base = msix.map(|(m, _)| m).or(redist).ok_or(super::RUN)?;
    let out = base.with_extension("result.json");
    let _ = std::fs::remove_file(&out);
    let redist = redist.map(|file| {
        let sha = if file.ends_with(VCREDIST_ARM64.file) { VCREDIST_ARM64.sha256 } else { VCREDIST_X64.sha256 };
        (file, sha)
    });
    let package = msix.map(|(file, sha256)| Package { file, sha256, user: &user });
    let script = elevated_script(&out, redist, package.as_ref());
    let command = format!(
        "Start-Process -FilePath powershell.exe -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList \
         '-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-EncodedCommand','{}'",
        encode(&script)
    );
    let ran = powershell(&command).map_err(|e| e.to_string())?;
    let Ok(text) = std::fs::read_to_string(&out) else {
        // 没有结果文件：管理员确认被取消了（或者根本没弹出来）
        log::warn!("[PONTEX] 管理员确认没有通过: {}", String::from_utf8_lossy(&ran.stderr).trim());
        return Err(super::desktop::DENIED.into());
    };
    let _ = std::fs::remove_file(&out);
    let result: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())?;
    if result["ok"].as_bool() != Some(true) {
        let error = result["error"].as_str().unwrap_or_default().to_string();
        log::warn!("[PONTEX] 管理员那一步失败: {error}");
        return Err(match error.as_str() {
            "INSTALL_CHECKSUM" | "INSTALL_UNTRUSTED" => error,
            _ => super::RUN.into(),
        });
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn elevate(_redist: Option<&Path>, _msix: Option<(&Path, &str)>) -> Result<(), String> {
    Err(super::UNSUPPORTED.into())
}

/// 装完核对：当前账户能找到它、签名是商店签名、包里程序要的 DLL 都在。
#[cfg(target_os = "windows")]
pub fn confirm() -> Result<(), String> {
    let query = format!(
        "$p = Get-AppxPackage -Name {name} | Sort-Object Version -Descending | Select-Object -First 1\n\
         if (-not $p) {{ Add-AppxPackage -RegisterByFamilyName -MainPackage {family} -ErrorAction SilentlyContinue; \
         $p = Get-AppxPackage -Name {name} | Sort-Object Version -Descending | Select-Object -First 1 }}\n\
         if ($p) {{ '{{0}}|{{1}}|{{2}}' -f $p.PackageFamilyName, $p.SignatureKind, $p.InstallLocation }}",
        name = quote(PACKAGE_NAME),
        family = quote(FAMILY),
    );
    let out = powershell(&query).map_err(|e| e.to_string())?;
    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let mut parts = line.splitn(3, '|');
    let (family, kind, location) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    if family != FAMILY {
        log::warn!("[PONTEX] 装完找不到 Codex 桌面版: {line}");
        return Err(super::RUN.into());
    }
    if kind != "Store" {
        log::warn!("[PONTEX] Codex 桌面版的签名类型不对: {kind}");
        return Err(UNTRUSTED.into());
    }
    let missing = missing_dlls(Path::new(location));
    if !missing.is_empty() {
        log::warn!("[PONTEX] Codex 桌面版缺 DLL: {}", missing.join(", "));
        return Err(DEPENDENCY.into());
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn confirm() -> Result<(), String> {
    Err(super::UNSUPPORTED.into())
}

/// 包里各个程序导入、这台电脑上又找不到的 DLL。
pub fn missing_dlls(location: &Path) -> Vec<String> {
    let app = location.join("app");
    let system = system32();
    let mut missing = Vec::new();
    for dir in [app.clone(), app.join("resources")] {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for exe in entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe"))) {
            missing.extend(super::pe::unresolved(&exe, &system, &[&app]).unwrap_or_default());
        }
    }
    missing.sort_by_key(|d| d.to_ascii_lowercase());
    missing.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    missing
}

/// 当前账户装着的版本：读注册表里的包仓库，不起进程。
#[cfg(target_os = "windows")]
pub fn installed() -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    const REPOSITORY: &str =
        r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages";
    let root = winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey(REPOSITORY).ok()?;
    root.enum_keys()
        .flatten()
        .filter_map(|name| package_version(&name))
        .max_by(|a, b| super::desktop::numbers(a).cmp(&super::desktop::numbers(b)))
}

/// `OpenAI.Codex_26.1120.6357.0_x64__2p2nqsd0c76g0` → `26.1120.6357.0`
pub fn package_version(full_name: &str) -> Option<String> {
    let rest = full_name.strip_prefix(&format!("{PACKAGE_NAME}_"))?;
    if !full_name.ends_with("__2p2nqsd0c76g0") {
        return None;
    }
    Some(rest.split('_').next()?.to_string())
}

#[cfg(target_os = "windows")]
pub fn os_version() -> Option<String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    let key = winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion").ok()?;
    let build: String = key.get_value("CurrentBuildNumber").ok()?;
    Some(format!("10.0.{build}"))
}

#[cfg(target_os = "windows")]
pub fn open() -> Result<(), String> {
    std::process::Command::new("explorer.exe")
        .arg(format!(r"shell:AppsFolder\{FAMILY}!App"))
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_identity_of_the_real_package() {
        let manifest = r#"<?xml version="1.0"?><Package xmlns="x"><Identity Name="OpenAI.Codex" ProcessorArchitecture="x64" Version="26.1120.6357.0" Publisher="CN=50BDFD77-8903-4850-9FFE-6E8522F64D5B" /><Properties/></Package>"#;
        let id = identity(manifest).unwrap();
        assert_eq!(id, Identity {
            name: PACKAGE_NAME.into(),
            publisher: PUBLISHER.into(),
            arch: "x64".into(),
            version: "26.1120.6357.0".into(),
        });
        assert_eq!(identity("<Package/>"), None);
    }

    #[test]
    fn version_comes_from_the_package_full_name() {
        assert_eq!(package_version("OpenAI.Codex_26.1120.6357.0_x64__2p2nqsd0c76g0").as_deref(), Some("26.1120.6357.0"));
        assert_eq!(package_version("OpenAI.Codex_26.1120.6357.0_x64__evilpublisher0"), None);
        assert_eq!(package_version("OpenAI.ChatGPT-Desktop_1.0.0.0_x64__2p2nqsd0c76g0"), None);
    }

    #[test]
    fn elevated_script_rechecks_everything_and_quotes_paths() {
        let package = Package { file: Path::new(r"C:\Users\O'Neil\cache\a.Msix"), sha256: "abc", user: "S-1-5-21-1" };
        let script = elevated_script(Path::new(r"C:\out.json"), Some((Path::new(r"C:\VC_redist.x64.exe"), "def")), Some(&package));
        assert!(script.contains(r"$m = 'C:\Users\O''Neil\cache\a.Msix'"), "单引号要转义：{script}");
        // 运行库：先验哈希和微软签名才执行
        let verify = script.find("Get-AuthenticodeSignature").unwrap();
        assert!(script.find("Get-FileHash -Algorithm SHA256 -LiteralPath $r").unwrap() < verify);
        assert!(verify < script.find("Start-Process -FilePath $r").unwrap());
        // MSIX：先验哈希才装；当前账户直接装，别的管理员账户先登记到电脑
        assert!(script.find("-LiteralPath $m).Hash").unwrap() < script.find("Add-AppxPackage").unwrap());
        assert!(script.contains("Add-AppxProvisionedPackage -Online"));
        // 只补运行库、只装包，各自都成立
        let only_runtime = elevated_script(Path::new("o"), Some((Path::new("VC_redist.x64.exe"), "d")), None);
        assert!(only_runtime.contains("VC_redist") && !only_runtime.contains("Add-Appx"));
        let only_package = elevated_script(Path::new("o"), None, Some(&package));
        assert!(only_package.contains("Add-AppxPackage") && !only_package.contains("Authenticode"));
    }

    #[test]
    fn encoded_command_is_utf16_base64() {
        use base64::Engine;
        let raw = base64::engine::general_purpose::STANDARD.decode(encode("Write-Output '好'")).unwrap();
        let back: Vec<u16> = raw.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(String::from_utf16(&back).unwrap(), "Write-Output '好'");
    }
}
