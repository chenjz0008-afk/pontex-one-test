//! 从镜像取版本信息，用 npm 官方签名验真。
//!
//! npm 给每个发布的包签了名：`<包名>@<版本>:<integrity>`，ECDSA P-256。公钥编进软件，镜像只是
//! 搬运工 —— 它能拒绝服务，但改不了内容。integrity 本身和包同源，单靠它防不住镜像作恶。

use std::time::Duration;

use base64::Engine as _;
use serde::Deserialize;

use super::spec::{stable, Artifact, Spec};

/// registry 镜像。顺序是默认优先级，实际按谁先答上来。
pub const MIRRORS: [&str; 4] = [
    "https://registry.npmmirror.com",
    "https://mirrors.cloud.tencent.com/npm",
    "https://repo.huaweicloud.com/repository/npm",
    "https://registry.npmjs.org",
];

/// registry.npmjs.org/-/npm/v1/keys 里当前有效的那一把（2025-01 起）。值是 SPKI 里的公钥点。
const KEY_ID: &str = "SHA256:DhQ8wR5APBvFHLF/+Tc+AYvPOdTpcIDqOhxsBHRwC7U";
const KEY_SPKI: &str = "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEY6Ya7W++7aUPzvMTrezH6Ycx3c+HOKYCcNGybJZSCJq/fd7Qa8uuAKtdIkUQtQiEKERhAmE5lMMJhP8OkDOa2g==";
/// P-256 的 SPKI 前缀长度，后面 65 字节是未压缩点。
const SPKI_PREFIX: usize = 26;
const KEYS_URL: &str = "https://registry.npmjs.org/-/npm/v1/keys";
/// 「最新」以它为准；连不上才看镜像。
const AUTHORITY: &str = "https://registry.npmjs.org";
/// 镜像先答了，再等官方源这么久。
const GRACE: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub dist: Dist,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Dist {
    #[serde(default)]
    pub integrity: String,
    #[serde(default, rename = "unpackedSize")]
    pub unpacked_size: Option<u64>,
    #[serde(default)]
    pub signatures: Vec<Signature>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Signature {
    pub keyid: String,
    pub sig: String,
}

/// 验真通过的包。只能由 [`verified`] 造出来。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trusted {
    pub package: String,
    pub version: String,
    /// sha512，十六进制。
    pub sha512: String,
    pub unpacked: u64,
}

fn b64(text: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(text.trim()).ok()
}

/// 认哪些签名公钥：编进来的一把；npm 换密钥之后，再加上从 npm 官方站点现取的。
///
/// 现取只走 registry.npmjs.org 的 TLS（npm 自己也是这么取的），镜像给的一概不认。
#[derive(Debug, Clone)]
pub struct Keys(Vec<(String, Vec<u8>)>);

impl Default for Keys {
    fn default() -> Self {
        Self(b64(KEY_SPKI).map(|spki| (KEY_ID.to_string(), spki)).into_iter().collect())
    }
}

#[derive(Deserialize)]
struct KeyList {
    keys: Vec<KeyEntry>,
}

#[derive(Deserialize)]
struct KeyEntry {
    expires: Option<String>,
    keyid: String,
    keytype: String,
    key: String,
}

impl Keys {
    fn knows(&self, manifest: &Manifest) -> bool {
        manifest.dist.signatures.iter().any(|s| self.0.iter().any(|(id, _)| *id == s.keyid))
    }

    fn add(&mut self, list: KeyList) {
        for entry in list.keys {
            // 有失效日期的是已经退役的。
            if entry.expires.is_some() || entry.keytype != "ecdsa-sha2-nistp256" {
                continue;
            }
            if let Some(spki) = b64(&entry.key).filter(|k| k.len() == SPKI_PREFIX + 65) {
                self.0.push((entry.keyid, spki));
            }
        }
    }

    /// 有一份清单的签名用的是不认识的密钥，才去取。
    async fn refresh(&mut self, client: &reqwest::Client) -> bool {
        let sent = client.get(KEYS_URL).timeout(Duration::from_secs(12)).send().await;
        match sent {
            Ok(response) if response.status().is_success() => match response.json().await {
                Ok(list) => {
                    self.add(list);
                    true
                }
                Err(_) => false,
            },
            _ => false,
        }
    }

    fn valid(&self, keyid: &str, message: &str, signature: &[u8]) -> bool {
        let provider = rustls::crypto::ring::default_provider();
        self.0.iter().filter(|(id, _)| id == keyid).any(|(_, spki)| {
            // SPKI 里的曲线 + SHA-256：provider 里可能有同曲线不同摘要的几个，任一通过即可。
            provider.signature_verification_algorithms.all.iter().any(|algorithm| {
                algorithm
                    .verify_signature(&spki[SPKI_PREFIX..], message.as_bytes(), signature)
                    .is_ok()
            })
        })
    }
}

/// 名字、版本是我们要的，且带着有效的官方签名。
pub fn verified(
    keys: &Keys,
    manifest: &Manifest,
    package: &str,
    version: Option<&str>,
) -> Option<Trusted> {
    if manifest.name != package || version.is_some_and(|v| v != manifest.version) {
        return None;
    }
    let digest = b64(manifest.dist.integrity.strip_prefix("sha512-")?)?;
    if digest.len() != 64 {
        return None;
    }
    let message = format!("{}@{}:{}", manifest.name, manifest.version, manifest.dist.integrity);
    let signed = manifest
        .dist
        .signatures
        .iter()
        .filter_map(|s| b64(&s.sig).map(|sig| (&s.keyid, sig)))
        .any(|(keyid, sig)| keys.valid(keyid, &message, &sig));
    signed.then(|| Trusted {
        package: package.into(),
        version: manifest.version.clone(),
        sha512: digest.iter().map(|b| format!("{b:02x}")).collect(),
        unpacked: manifest.dist.unpacked_size.unwrap_or(0),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryError {
    /// 一个源都没答上来。
    Offline,
    /// 有源答了，都说没有这个包。
    Missing,
    /// 有源答了，但没有一份过得了验真或版本线。
    Untrusted,
}

enum Answer {
    Found(Manifest),
    Missing,
    Dead,
}

async fn fetch(client: reqwest::Client, mirror: String, path: String) -> (String, Answer) {
    for attempt in 0..2 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(600)).await;
        }
        let sent = client
            .get(format!("{mirror}/{path}"))
            .header("accept", "application/json")
            // 有的源前面有 CDN，别拿到昨天的「最新」。
            .header("cache-control", "no-cache")
            .timeout(Duration::from_secs(12))
            .send()
            .await;
        match sent {
            Ok(response) if response.status().is_success() => {
                if let Ok(manifest) = response.json::<Manifest>().await {
                    return (mirror, Answer::Found(manifest));
                }
            }
            // 没有这个版本，重试同一个源没有意义。
            Ok(response) if response.status().as_u16() == 404 => return (mirror, Answer::Missing),
            _ => {}
        }
    }
    (mirror, Answer::Dead)
}

/// 官方源答了就听官方的（它撤回坏版本时，滞后的镜像会给出更高的版本号）；否则取验真通过的
/// 最高正式版，滞后的镜像不会把版本拉低。
fn choose(keys: &Keys, answers: &[(String, Manifest)], spec: &Spec) -> Option<String> {
    let mut good: Vec<_> = answers
        .iter()
        .filter_map(|(mirror, manifest)| {
            let trusted = verified(keys, manifest, spec.meta, None)?;
            let version = stable(&trusted.version).filter(|v| *v >= spec.floor)?;
            Some((mirror == AUTHORITY, version, trusted.version))
        })
        .collect();
    good.sort();
    good.pop().map(|(_, _, version)| version)
}

pub async fn latest(
    client: &reqwest::Client,
    mirrors: &[String],
    spec: &Spec,
) -> Result<String, RegistryError> {
    use futures::stream::{FuturesUnordered, StreamExt};
    let mut asks: FuturesUnordered<_> = mirrors
        .iter()
        .map(|mirror| fetch(client.clone(), mirror.clone(), format!("{}/latest", spec.meta)))
        .collect();
    let mut keys = Keys::default();
    let mut answers: Vec<(String, Manifest)> = Vec::new();
    let mut refreshed = false;
    let mut deadline: Option<tokio::time::Instant> = None;
    loop {
        let next = match deadline {
            None => asks.next().await,
            Some(at) => match tokio::time::timeout_at(at, asks.next()).await {
                Ok(next) => next,
                Err(_) => break,
            },
        };
        let Some((mirror, answer)) = next else { break };
        let Answer::Found(manifest) = answer else { continue };
        if !refreshed && !keys.knows(&manifest) {
            refreshed = true;
            keys.refresh(client).await;
        }
        let official = mirror == AUTHORITY;
        answers.push((mirror, manifest));
        if choose(&keys, &answers, spec).is_some() {
            if official {
                break;
            }
            deadline.get_or_insert_with(|| tokio::time::Instant::now() + GRACE);
        }
    }
    if answers.is_empty() {
        return Err(RegistryError::Offline);
    }
    choose(&keys, &answers, spec).ok_or(RegistryError::Untrusted)
}

/// 平台包：谁先给出验真通过的就用谁。
pub async fn resolve(
    client: &reqwest::Client,
    mirrors: &[String],
    artifact: &Artifact,
) -> Result<Trusted, RegistryError> {
    use futures::stream::{FuturesUnordered, StreamExt};
    let mut asks: FuturesUnordered<_> = mirrors
        .iter()
        .map(|mirror| {
            fetch(
                client.clone(),
                mirror.clone(),
                format!("{}/{}", artifact.package, artifact.version),
            )
        })
        .collect();
    let mut keys = Keys::default();
    let (mut answered, mut missing, mut refreshed) = (false, false, false);
    while let Some((_, answer)) = asks.next().await {
        match answer {
            Answer::Found(manifest) => {
                answered = true;
                if !refreshed && !keys.knows(&manifest) {
                    refreshed = true;
                    keys.refresh(client).await;
                }
                if let Some(trusted) =
                    verified(&keys, &manifest, &artifact.package, Some(&artifact.version))
                {
                    return Ok(trusted);
                }
            }
            Answer::Missing => missing = true,
            Answer::Dead => {}
        }
    }
    Err(match (answered, missing) {
        (true, _) => RegistryError::Untrusted,
        (false, true) => RegistryError::Missing,
        (false, false) => RegistryError::Offline,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// registry 上的真实记录。
    fn real() -> Manifest {
        serde_json::from_str(include_str!("testdata/grok-darwin-arm64-1.0.92.json")).unwrap()
    }

    #[test]
    fn accepts_a_real_npm_signature() {
        let m = real();
        let t = verified(&Keys::default(), &m, "@xai-official/grok-darwin-arm64", Some("1.0.92")).expect("real signature");
        assert_eq!(t.sha512.len(), 128);
        assert!(t.unpacked > 50_000_000);
    }

    #[test]
    fn a_mirror_cannot_change_anything() {
        let name = "@xai-official/grok-darwin-arm64";
        // 换内容、连 integrity 一起换
        let mut m = real();
        m.dist.integrity = format!("sha512-{}", base64::engine::general_purpose::STANDARD.encode([7u8; 64]));
        assert_eq!(verified(&Keys::default(), &m, name, None), None);
        // 把别的版本冒充成新版本
        let mut m = real();
        m.version = "9.9.9".into();
        assert_eq!(verified(&Keys::default(), &m, name, None), None);
        // 把别的包冒充成这个包
        let mut m = real();
        m.name = "@xai-official/grok-win32-x64".into();
        assert_eq!(verified(&Keys::default(), &m, "@xai-official/grok-win32-x64", None), None);
        // 去掉签名、换一个不认识的 key
        let mut m = real();
        m.dist.signatures.clear();
        assert_eq!(verified(&Keys::default(), &m, name, None), None);
        let mut m = real();
        for s in &mut m.dist.signatures {
            s.keyid = "SHA256:evil".into();
        }
        assert_eq!(verified(&Keys::default(), &m, name, None), None);
        // 签名正确，但不是要的版本
        assert_eq!(verified(&Keys::default(), &real(), name, Some("1.0.91")), None);
    }

    fn grok(version: &str) -> Manifest {
        let text = match version {
            "1.0.90" => include_str!("testdata/grok-1.0.90.json"),
            "1.0.91" => include_str!("testdata/grok-1.0.91.json"),
            _ => include_str!("testdata/grok-1.0.92.json"),
        };
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn newest_means_what_npm_says_not_what_a_mirror_says() {
        let spec = super::super::spec::spec(crate::pontex::agents::AgentId::GrokBuild).unwrap();
        let pick = |list: &[(&str, Manifest)]| {
            let list: Vec<_> = list.iter().map(|(m, v)| (m.to_string(), v.clone())).collect();
            choose(&Keys::default(), &list, spec)
        };
        let mirror = MIRRORS[0];
        // 镜像滞后：官方更新
        assert_eq!(pick(&[(mirror, grok("1.0.90")), (AUTHORITY, grok("1.0.92"))]).as_deref(), Some("1.0.92"));
        // 官方撤回了 1.0.92，镜像还没跟上：听官方的
        assert_eq!(pick(&[(mirror, grok("1.0.92")), (AUTHORITY, grok("1.0.91"))]).as_deref(), Some("1.0.91"));
        // 官方连不上：镜像里最高的
        assert_eq!(pick(&[(mirror, grok("1.0.90")), (MIRRORS[1], grok("1.0.92"))]).as_deref(), Some("1.0.92"));
        // 镜像把版本号改高：签名对不上，不认
        let mut fake = grok("1.0.90");
        fake.version = "8.0.0".into();
        assert_eq!(pick(&[(mirror, fake), (MIRRORS[1], grok("1.0.91"))]).as_deref(), Some("1.0.91"));
        assert!(MIRRORS.contains(&AUTHORITY));
    }

    #[test]
    fn a_rotated_key_works_only_when_it_comes_from_npm() {
        // 同一把密钥换个名字，相当于 npm 换了密钥
        let mut m = grok("1.0.92");
        for s in &mut m.dist.signatures {
            s.keyid = "SHA256:next".into();
        }
        let mut keys = Keys::default();
        assert!(!keys.knows(&m));
        assert_eq!(verified(&keys, &m, "@xai-official/grok", None), None);
        let list = |expires: &str| {
            serde_json::from_str::<KeyList>(&format!(
                r#"{{"keys":[{{"expires":{expires},"keyid":"SHA256:next","keytype":"ecdsa-sha2-nistp256","key":"{KEY_SPKI}"}}]}}"#
            ))
            .unwrap()
        };
        keys.add(list("\"2025-01-29T00:00:00.000Z\""));
        assert!(!keys.knows(&m), "退役的不要");
        keys.add(list("null"));
        assert!(verified(&keys, &m, "@xai-official/grok", None).is_some());
        assert!(KEYS_URL.starts_with(AUTHORITY));
    }
}
