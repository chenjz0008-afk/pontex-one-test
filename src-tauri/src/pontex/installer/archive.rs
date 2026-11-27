//! 解包。只认普通文件和目录：链接、设备、越界路径一律整包拒绝。

use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::Path;

use crate::pontex::safe_path::safe_relative;

const MAX_TOTAL: u64 = 2 << 30;
const MAX_FILES: usize = 20_000;

#[derive(Debug, PartialEq, Eq)]
pub enum ArchiveError {
    Bad(&'static str),
    Io(String),
}

impl From<std::io::Error> for ArchiveError {
    fn from(error: std::io::Error) -> Self {
        ArchiveError::Io(error.to_string())
    }
}

/// 要落盘的内容：把归档里的路径映射成目标相对路径，`None` 跳过。
pub type Select<'a> = dyn Fn(&str) -> Option<String> + 'a;

pub fn tar_gz(source: &Path, into: &Path, select: &Select<'_>) -> Result<usize, ArchiveError> {
    let file = BufReader::with_capacity(1 << 20, File::open(source)?);
    tar(&mut flate2::read::GzDecoder::new(file), into, select)
}

fn octal(field: &[u8]) -> Option<u64> {
    // 大文件用 base-256。
    if field.first().is_some_and(|b| b & 0x80 != 0) {
        return field[1..].iter().try_fold(u64::from(field[0] & 0x7f), |acc, b| {
            acc.checked_mul(256)?.checked_add(u64::from(*b))
        });
    }
    let text = std::str::from_utf8(field).ok()?;
    let text = text.trim_matches(['\0', ' ']);
    if text.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(text, 8).ok()
}

fn text(field: &[u8]) -> Result<String, ArchiveError> {
    let end = field.iter().position(|b| *b == 0).unwrap_or(field.len());
    String::from_utf8(field[..end].to_vec()).map_err(|_| ArchiveError::Bad("name"))
}

/// pax 扩展头里的 `path`。记录格式：`<长度> <键>=<值>\n`。
fn pax_path(data: &[u8]) -> Option<String> {
    let mut rest = data;
    let mut found = None;
    while !rest.is_empty() {
        let space = rest.iter().position(|b| *b == b' ')?;
        let length: usize = std::str::from_utf8(&rest[..space]).ok()?.parse().ok()?;
        let record = rest.get(space + 1..length.checked_sub(1)?)?;
        let equals = record.iter().position(|b| *b == b'=')?;
        if &record[..equals] == b"path" {
            found = Some(String::from_utf8(record[equals + 1..].to_vec()).ok()?);
        }
        rest = rest.get(length..)?;
    }
    found
}

fn tar(reader: &mut dyn Read, into: &Path, select: &Select<'_>) -> Result<usize, ArchiveError> {
    let mut header = [0u8; 512];
    let mut long: Option<String> = None;
    let mut written = 0;
    let mut seen = 0;
    let mut left = MAX_TOTAL;
    loop {
        reader.read_exact(&mut header)?;
        if header.iter().all(|b| *b == 0) {
            return Ok(written);
        }
        let sum: u64 = header
            .iter()
            .enumerate()
            .map(|(i, b)| if (148..156).contains(&i) { 32 } else { u64::from(*b) })
            .sum();
        if octal(&header[148..156]) != Some(sum) {
            return Err(ArchiveError::Bad("checksum"));
        }
        let size = octal(&header[124..136]).ok_or(ArchiveError::Bad("size"))?;
        let mode = octal(&header[100..108]).unwrap_or(0);
        let padded = size.div_ceil(512) * 512;
        left = left.checked_sub(size).ok_or(ArchiveError::Bad("too large"))?;
        match header[156] {
            kind @ (b'x' | b'L') => {
                if size > 1 << 20 {
                    return Err(ArchiveError::Bad("header"));
                }
                let mut data = vec![0u8; padded as usize];
                reader.read_exact(&mut data)?;
                data.truncate(size as usize);
                long = if kind == b'L' {
                    Some(text(&data)?)
                } else {
                    pax_path(&data).or(long)
                };
            }
            b'g' => skip(reader, padded)?,
            b'0' | 0 | b'7' => {
                seen += 1;
                if seen > MAX_FILES {
                    return Err(ArchiveError::Bad("too many files"));
                }
                let name = match long.take() {
                    Some(name) => name,
                    None => {
                        let name = text(&header[..100])?;
                        let prefix = text(&header[345..500])?;
                        // 只有 ustar 才有 prefix 字段
                        if &header[257..262] == b"ustar" && !prefix.is_empty() {
                            format!("{prefix}/{name}")
                        } else {
                            name
                        }
                    }
                };
                if !safe_relative(&name) {
                    return Err(ArchiveError::Bad("path"));
                }
                match select(&name) {
                    Some(target) => {
                        if !safe_relative(&target) {
                            return Err(ArchiveError::Bad("path"));
                        }
                        write_file(reader, size, &into.join(target), mode & 0o111 != 0)?;
                        skip(reader, padded - size)?;
                        written += 1;
                    }
                    None => skip(reader, padded)?,
                }
            }
            b'5' => {
                long = None;
                skip(reader, padded)?;
            }
            _ => return Err(ArchiveError::Bad("link")),
        }
    }
}

fn skip(reader: &mut dyn Read, bytes: u64) -> Result<(), ArchiveError> {
    let copied = std::io::copy(&mut reader.take(bytes), &mut std::io::sink())?;
    if copied == bytes {
        Ok(())
    } else {
        Err(ArchiveError::Bad("truncated"))
    }
}

fn write_file(
    reader: &mut dyn Read,
    size: u64,
    target: &Path,
    executable: bool,
) -> Result<(), ArchiveError> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(target)?;
    let copied = std::io::copy(&mut reader.take(size), &mut file)?;
    if copied != size {
        return Err(ArchiveError::Bad("truncated"));
    }
    file.flush()?;
    if executable {
        make_executable(target)?;
    }
    Ok(())
}

pub fn make_executable(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Windows 的 Node 是 zip，只取一个文件。
pub fn zip_one(source: &Path, entry: &str, target: &Path) -> Result<(), ArchiveError> {
    let mut archive = zip::ZipArchive::new(BufReader::new(File::open(source)?))
        .map_err(|_| ArchiveError::Bad("zip"))?;
    let mut file = archive.by_name(entry).map_err(|_| ArchiveError::Bad("entry"))?;
    if file.is_symlink() || file.size() > MAX_TOTAL {
        return Err(ArchiveError::Bad("entry"));
    }
    let size = file.size();
    write_file(&mut file, size, target, true)
}

pub fn brotli(source: &Path, target: &Path) -> Result<(), ArchiveError> {
    let mut input = brotli::Decompressor::new(BufReader::new(File::open(source)?), 1 << 16);
    let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(target)?;
    let copied = std::io::copy(&mut (&mut input).take(MAX_TOTAL), &mut output)?;
    if copied == 0 || copied == MAX_TOTAL {
        return Err(ArchiveError::Bad("brotli"));
    }
    output.flush()?;
    make_executable(target)?;
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// 造一个 npm 风格的 tgz：（类型，名字，内容，可执行）。
    pub fn tgz(entries: &[(u8, &str, &[u8], bool)]) -> Vec<u8> {
        let mut raw = Vec::new();
        for (kind, name, data, exec) in entries {
            let mut h = [0u8; 512];
            h[..name.len()].copy_from_slice(name.as_bytes());
            h[100..107].copy_from_slice(if *exec { b"0000755" } else { b"0000644" });
            h[124..135].copy_from_slice(format!("{:011o}", data.len()).as_bytes());
            h[156] = *kind;
            h[257..263].copy_from_slice(b"ustar\0");
            h[148..156].fill(b' ');
            let sum: u32 = h.iter().map(|b| u32::from(*b)).sum();
            h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
            raw.extend_from_slice(&h);
            raw.extend_from_slice(data);
            raw.resize(raw.len().div_ceil(512) * 512, 0);
        }
        raw.resize(raw.len() + 1024, 0);
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&raw).unwrap();
        gz.finish().unwrap()
    }

    fn run(bytes: &[u8]) -> (tempfile::TempDir, Result<usize, ArchiveError>) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.tgz");
        std::fs::write(&file, bytes).unwrap();
        let out = dir.path().join("out");
        let result = tar_gz(&file, &out, &|name| name.strip_prefix("package/").map(str::to_string));
        (dir, result)
    }

    #[test]
    fn unpacks_files_with_their_executable_bit() {
        let (dir, result) = run(&tgz(&[
            (b'5', "package/", b"", false),
            (b'0', "package/claude", b"bin", true),
            (b'0', "package/docs/a.md", b"doc", false),
        ]));
        assert_eq!(result, Ok(2));
        assert_eq!(std::fs::read(dir.path().join("out/docs/a.md")).unwrap(), b"doc");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &str| std::fs::metadata(dir.path().join("out").join(p)).unwrap().permissions().mode();
            assert_ne!(mode("claude") & 0o111, 0);
            assert_eq!(mode("docs/a.md") & 0o111, 0);
        }
    }

    #[test]
    fn long_names_from_pax_and_gnu_headers() {
        let long = format!("package/{}/x.js", "d".repeat(120));
        let record = format!("path={long}\n");
        let mut length = record.len() + 4;
        length = format!("{length} {record}").len();
        let pax = format!("{length} {record}");
        for (kind, data) in [(b'x', pax.as_bytes().to_vec()), (b'L', format!("{long}\0").into_bytes())] {
            let (dir, result) = run(&tgz(&[(kind, "hdr", &data, false), (b'0', "short", b"ok", false)]));
            assert_eq!(result, Ok(1));
            assert!(dir.path().join("out").join("d".repeat(120)).join("x.js").exists());
        }
    }

    #[test]
    fn refuses_links_traversal_and_truncation() {
        for (kind, name) in [(b'2', "package/link"), (b'1', "package/hard"), (b'3', "package/dev")] {
            assert_eq!(run(&tgz(&[(kind, name, b"", false)])).1, Err(ArchiveError::Bad("link")));
        }
        for name in ["package/../../evil", "/etc/evil", "package//x", "package/a\\b"] {
            assert_eq!(run(&tgz(&[(b'0', name, b"x", false)])).1, Err(ArchiveError::Bad("path")), "{name}");
        }
        let good = tgz(&[(b'0', "package/a", &[1u8; 4096], false)]);
        assert!(run(&good[..good.len() / 2]).1.is_err());
        // 同一个文件出现两次：不允许后一个盖掉前一个
        assert!(run(&tgz(&[(b'0', "package/a", b"1", false), (b'0', "package/a", b"2", false)])).1.is_err());
    }

    #[test]
    fn brotli_entry_is_restored() {
        let dir = tempfile::tempdir().unwrap();
        let mut packed = Vec::new();
        brotli::CompressorWriter::new(&mut packed, 4096, 5, 20).write_all(b"grok binary").unwrap();
        let (src, out) = (dir.path().join("grok.br"), dir.path().join("grok"));
        std::fs::write(&src, packed).unwrap();
        brotli(&src, &out).unwrap();
        assert_eq!(std::fs::read(out).unwrap(), b"grok binary");
        std::fs::write(&src, b"not brotli").unwrap();
        assert!(brotli(&src, &dir.path().join("other")).is_err());
    }
}
