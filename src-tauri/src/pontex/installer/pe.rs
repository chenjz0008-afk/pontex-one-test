//! Windows 程序（PE）导入了哪些 DLL，这台电脑上找不找得到。
//!
//! 「The code execution cannot proceed because VCRUNTIME140.dll was not found」这一类弹窗，
//! 都是程序导入的 DLL 在加载器的搜索路径上找不到。装之前把导入表读一遍、逐个对照，
//! 缺什么当场就知道，不让用户在启动时才撞上系统弹窗。

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// 只按需读：头 + 用到的那几个节。几百 MB 的 exe 也只读几 MB。
struct Image<R> {
    file: R,
    sections: Vec<(u32, u32, u32, u32)>,
    cache: HashMap<usize, Vec<u8>>,
}

impl<R: Read + Seek> Image<R> {
    fn bytes(&mut self, rva: u32, len: usize) -> Option<&[u8]> {
        let index = self.sections.iter().position(|(va, size, _, _)| rva >= *va && rva < va.saturating_add(*size))?;
        if !self.cache.contains_key(&index) {
            let (_, _, raw, raw_size) = self.sections[index];
            // 节的原始数据最多读 64 MB：导入表不会在比这还大的节里
            let mut buf = vec![0u8; (raw_size as usize).min(64 << 20)];
            self.file.seek(SeekFrom::Start(u64::from(raw))).ok()?;
            let n = read_full(&mut self.file, &mut buf);
            buf.truncate(n);
            self.cache.insert(index, buf);
        }
        let start = (rva - self.sections[index].0) as usize;
        let data = self.cache.get(&index)?;
        data.get(start..(start + len).min(data.len()))
    }

    fn u32_at(&mut self, rva: u32) -> Option<u32> {
        Some(u32::from_le_bytes(self.bytes(rva, 4)?.get(..4)?.try_into().ok()?))
    }

    fn name(&mut self, rva: u32) -> Option<String> {
        let data = self.bytes(rva, 512)?;
        let end = data.iter().position(|&b| b == 0)?;
        Some(String::from_utf8_lossy(&data[..end]).into_owned())
    }
}

fn read_full(file: &mut impl Read, buf: &mut [u8]) -> usize {
    let mut done = 0;
    while done < buf.len() {
        match file.read(&mut buf[done..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => done += n,
        }
    }
    done
}

/// 导入的 DLL（含延迟加载），原样大小写。读不了（不是 PE、截断）返回 None。
pub fn imports(file: impl Read + Seek) -> Option<Vec<String>> {
    let mut file = file;
    let mut head = vec![0u8; 4096];
    file.seek(SeekFrom::Start(0)).ok()?;
    let got = read_full(&mut file, &mut head);
    head.truncate(got);
    let u16_at = |o: usize| Some(u16::from_le_bytes(head.get(o..o + 2)?.try_into().ok()?));
    let u32_at = |o: usize| Some(u32::from_le_bytes(head.get(o..o + 4)?.try_into().ok()?));
    let pe = u32_at(0x3c)? as usize;
    if head.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let count = u16_at(pe + 6)? as usize;
    let optional = pe + 24;
    let optional_size = u16_at(pe + 20)? as usize;
    let dirs = optional + if u16_at(optional)? == 0x20b { 112 } else { 96 };
    let mut sections = Vec::with_capacity(count);
    for i in 0..count {
        let o = optional + optional_size + 40 * i;
        let size = u32_at(o + 8)?.max(u32_at(o + 16)?);
        sections.push((u32_at(o + 12)?, size, u32_at(o + 20)?, u32_at(o + 16)?));
    }
    let tables = [(u32_at(dirs + 8)?, 20u32, 12u32), (u32_at(dirs + 8 * 13)?, 32, 4)];
    let mut image = Image { file, sections, cache: HashMap::new() };
    let mut out = Vec::new();
    // 普通导入表：每项 20 字节，名字在 +12；延迟导入表：每项 32 字节，名字在 +4。
    for (start, stride, field) in tables {
        if start == 0 {
            continue;
        }
        let mut at = start;
        loop {
            let name = image.u32_at(at + field)?;
            if name == 0 {
                break;
            }
            out.push(image.name(name)?);
            at += stride;
            if out.len() > 4096 {
                return None;
            }
        }
    }
    Some(out)
}

pub fn imports_of(path: &Path) -> Option<Vec<String>> {
    imports(std::fs::File::open(path).ok()?)
}

/// 需要另外安装才有的运行库（VC++ 运行库一家）。
pub fn redistributable(dll: &str) -> bool {
    let dll = dll.to_ascii_lowercase();
    ["vcruntime1", "msvcp1", "concrt1", "vccorlib1", "vcomp1"].iter().any(|p| dll.starts_with(p))
}

/// API 集合（`api-ms-win-*` / `ext-ms-*`）由系统加载器解析，不是磁盘上的文件。
fn api_set(dll: &str) -> bool {
    let dll = dll.to_ascii_lowercase();
    dll.starts_with("api-ms-") || dll.starts_with("ext-ms-")
}

/// 这个程序导入、但在「程序所在目录」和「系统目录」里都找不到的 DLL。
/// `extra` 是同一个包里其他可能放 DLL 的目录（比如 Electron 应用的 app 目录）。
pub fn unresolved(exe: &Path, system: &Path, extra: &[&Path]) -> Option<Vec<String>> {
    let dir = exe.parent()?;
    let mut missing: Vec<String> = imports_of(exe)?
        .into_iter()
        .filter(|dll| !api_set(dll))
        .filter(|dll| {
            let here = |base: &Path| exists_ignoring_case(base, dll);
            !(here(dir) || here(system) || extra.iter().any(|d| here(d)))
        })
        .collect();
    missing.sort_by_key(|d| d.to_ascii_lowercase());
    missing.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    Some(missing)
}

fn exists_ignoring_case(dir: &Path, name: &str) -> bool {
    if dir.join(name).is_file() {
        return true;
    }
    // Windows 本身不分大小写；在别的系统上跑测试时也按 Windows 的规矩来。
    std::fs::read_dir(dir).ok().is_some_and(|entries| {
        entries.flatten().any(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// 手工拼一个最小的 64 位 PE：一个节，导入表里列出给定的 DLL。
    pub fn fake_pe(dlls: &[&str], delayed: &[&str]) -> Vec<u8> {
        let mut b = vec![0u8; 0x400];
        b[0..2].copy_from_slice(b"MZ");
        b[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        let pe = 0x80;
        b[pe..pe + 4].copy_from_slice(b"PE\0\0");
        b[pe + 6..pe + 8].copy_from_slice(&1u16.to_le_bytes());
        let optional_size = 240u16;
        b[pe + 20..pe + 22].copy_from_slice(&optional_size.to_le_bytes());
        let opt = pe + 24;
        b[opt..opt + 2].copy_from_slice(&0x20bu16.to_le_bytes());
        // 节：VA 0x1000，文件偏移 0x200，大小 0x200
        let sec = opt + optional_size as usize;
        b[sec + 8..sec + 12].copy_from_slice(&0x200u32.to_le_bytes());
        b[sec + 12..sec + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        b[sec + 16..sec + 20].copy_from_slice(&0x200u32.to_le_bytes());
        b[sec + 20..sec + 24].copy_from_slice(&0x200u32.to_le_bytes());
        let rva = |file: usize| (file - 0x200 + 0x1000) as u32;
        let dirs = opt + 112;
        let mut cursor = 0x300;
        let mut write_names = |names: &[&str], b: &mut Vec<u8>| -> Vec<u32> {
            names
                .iter()
                .map(|n| {
                    let at = cursor;
                    b[at..at + n.len()].copy_from_slice(n.as_bytes());
                    cursor += n.len() + 1;
                    rva(at)
                })
                .collect()
        };
        let normal = write_names(dlls, &mut b);
        let delay = write_names(delayed, &mut b);
        let mut table = 0x200;
        b[dirs + 8..dirs + 12].copy_from_slice(&rva(table).to_le_bytes());
        for n in &normal {
            b[table + 12..table + 16].copy_from_slice(&n.to_le_bytes());
            table += 20;
        }
        table += 20;
        b[dirs + 8 * 13..dirs + 8 * 13 + 4].copy_from_slice(&rva(table).to_le_bytes());
        for n in &delay {
            b[table + 4..table + 8].copy_from_slice(&n.to_le_bytes());
            table += 32;
        }
        b
    }

    #[test]
    fn reads_normal_and_delayed_imports() {
        let pe = fake_pe(&["KERNEL32.dll", "VCRUNTIME140.dll"], &["wintrust.dll"]);
        let read = |b: &[u8]| imports(std::io::Cursor::new(b.to_vec()));
        assert_eq!(read(&pe).unwrap(), ["KERNEL32.dll", "VCRUNTIME140.dll", "wintrust.dll"]);
        assert_eq!(read(b"not a program"), None);
        assert_eq!(read(&pe[..0x90]), None, "截断的文件不猜");
    }

    #[test]
    fn knows_which_dlls_need_the_vc_runtime() {
        for dll in ["VCRUNTIME140.dll", "vcruntime140_1.dll", "MSVCP140.dll", "concrt140.dll"] {
            assert!(redistributable(dll), "{dll}");
        }
        for dll in ["kernel32.dll", "ucrtbase.dll", "api-ms-win-crt-runtime-l1-1-0.dll", "msvcrt.dll"] {
            assert!(!redistributable(dll), "{dll}");
        }
    }

    #[test]
    fn finds_what_a_clean_windows_would_be_missing() {
        let root = tempfile::tempdir().unwrap();
        let (system, app) = (root.path().join("System32"), root.path().join("app"));
        std::fs::create_dir_all(&system).unwrap();
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(system.join("kernel32.dll"), b"").unwrap();
        std::fs::write(app.join("libglib.dll"), b"").unwrap();
        let exe = app.join("tool.exe");
        let pe = fake_pe(&["KERNEL32.dll", "VCRUNTIME140.dll", "api-ms-win-crt-heap-l1-1-0.dll", "libglib.dll"], &["msvcp140.dll"]);
        std::fs::write(&exe, pe).unwrap();
        assert_eq!(unresolved(&exe, &system, &[]).unwrap(), ["msvcp140.dll", "VCRUNTIME140.dll"]);
        // 装好运行库之后
        std::fs::write(system.join("vcruntime140.dll"), b"").unwrap();
        std::fs::write(system.join("MSVCP140.DLL"), b"").unwrap();
        assert!(unresolved(&exe, &system, &[]).unwrap().is_empty());
    }
}
