//! 用户自己的配置留底。两个槽位：原生（官方登录、官方 Key）和原配置（别家网关），互不覆盖。
//! 每次写之前只要现场不是我们上次写下的，就重新留底，所以切回去总是「离开时的样子」。
//!
//! 两层保险：关键字段级还原（Agent 自己也会改配置文件，不能整份盖回去），以及断开时如果
//! 文件在我们写完之后没人动过，就按字节还原。登录文件不复制：里面的令牌会轮换，复制件没有用。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use toml_edit::{DocumentMut, Item, Table};

use super::agents::AgentId;
use super::consts::{API_HOST, NATIVE_ID, PREVIOUS_ID};
use super::live::{self, Route};
use super::store::Target;
use super::templates::row;
use super::upstream::{self, key_fields, Edit, Engine, Row};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    pub at: i64,
    pub route: Route,
    pub files: Vec<FileCopy>,
    /// 存成了上游的一行，切回去时用它；没有就用官方卡。
    pub has_row: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCopy {
    pub path: PathBuf,
    /// 副本；`None` 表示当时没有这个文件。
    pub copy: Option<PathBuf>,
}

/// 这条线路归哪个槽位；PonteX 自己的和读不懂的不留底。
pub fn slot_of(route: &Route) -> Option<Target> {
    match route {
        Route::Official | Route::OfficialKey { .. } | Route::NotAdded => Some(Target::Native),
        Route::Other { .. } => Some(Target::Previous),
        Route::Pontex { .. } | Route::Added { .. } | Route::Unreadable { .. } => None,
    }
}

fn row_id(slot: Target) -> &'static str {
    match slot {
        Target::Previous => PREVIOUS_ID,
        _ => NATIVE_ID,
    }
}

fn dir(id: AgentId, slot: Target) -> PathBuf {
    upstream::device_dir()
        .join("baseline")
        .join(id.key())
        .join(if slot == Target::Previous { "previous" } else { "native" })
}

/// 上游切回 Codex 官方时留下的休眠表（指向本地路由、没有被选中）。它是我们这边产生的，
/// 不算用户的内容，留底时去掉。
fn without_dormant_table(id: AgentId, bytes: Vec<u8>) -> Vec<u8> {
    if id != AgentId::Codex {
        return bytes;
    }
    let Some(mut doc) = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|text| text.parse::<DocumentMut>().ok())
    else {
        return bytes;
    };
    let dormant = doc.get("model_provider").and_then(Item::as_str) != Some("custom")
        && doc
            .get("model_providers")
            .and_then(|p| p.get("custom"))
            .and_then(|t| t.get("experimental_bearer_token"))
            .and_then(Item::as_str)
            == Some("PROXY_MANAGED");
    if !dormant {
        return bytes;
    }
    restore_table(&DocumentMut::new(), &mut doc, "model_providers", "custom");
    doc.to_string().into_bytes()
}

pub fn capture(
    engine: &Engine,
    id: AgentId,
    slot: Target,
    route: Route,
) -> Result<Baseline, String> {
    let folder = dir(id, slot);
    let mut files = Vec::new();
    for (index, path) in live::managed_files(id).into_iter().enumerate() {
        let copy = match std::fs::read(&path) {
            Ok(bytes) => {
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                let target = folder.join(format!("{index}-{name}"));
                let bytes = without_dormant_table(id, bytes);
                upstream::write_private(&target, &bytes)?;
                Some(target)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        files.push(FileCopy { path, copy });
    }
    let has_row = match baseline_row(id, &route, slot) {
        Some(row) => {
            engine.put_row(id, row)?;
            true
        }
        None => false,
    };
    Ok(Baseline {
        at: chrono::Utc::now().timestamp(),
        route,
        files,
        has_row,
    })
}

/// 只取关键字段存成上游的一行，别的内容不进数据库。
fn baseline_row(id: AgentId, route: &Route, slot: Target) -> Option<Row> {
    let custom = matches!(route, Route::OfficialKey { .. } | Route::Other { .. });
    let (category, settings) = match id {
        AgentId::ClaudeCode => (
            if custom { "custom" } else { "official" },
            claude_key_fields()?,
        ),
        AgentId::Codex => (
            if matches!(route, Route::Other { .. }) { "custom" } else { "official" },
            codex_key_fields(matches!(route, Route::Other { .. }))?,
        ),
        AgentId::GeminiCli if custom => {
            let env: Map<String, Value> = live::gemini_env()
                .ok()?
                .into_iter()
                .filter(|(k, v)| key_fields::gemini_env(k) && !v.trim().is_empty())
                .map(|(k, v)| (k, Value::String(v)))
                .collect();
            if env.is_empty() {
                return None;
            }
            ("custom", json!({ "env": env }))
        }
        AgentId::GrokBuild if custom => ("custom", grok_key_fields()?),
        _ => return None,
    };
    Some(row(row_id(slot), "我的原有配置", category, settings))
}

fn claude_key_fields() -> Option<Value> {
    let bytes = std::fs::read(upstream::claude_settings()).ok()?;
    let root: Value = serde_json::from_slice(&bytes).ok()?;
    let mut out = Map::new();
    for (key, value) in root.as_object()? {
        if key_fields::claude_top(key) {
            out.insert(key.clone(), value.clone());
        }
    }
    let env: Map<String, Value> = root
        .get("env")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(k, v)| {
            key_fields::claude_env(k)
                && v.as_str().is_none_or(|s| !s.is_empty())
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if out.is_empty() && env.is_empty() {
        return None;
    }
    out.insert("env".into(), Value::Object(env));
    Some(Value::Object(out))
}

fn codex_key_fields(third_party: bool) -> Option<Value> {
    let text = std::fs::read_to_string(upstream::codex_config()).ok()?;
    let doc: DocumentMut = text.parse().ok()?;
    let mut out = DocumentMut::new();
    for key in key_fields::CODEX_TOP {
        if let Some(item) = doc.get(key).filter(|item| item.is_value()) {
            out.insert(key, item.clone());
        }
    }
    if third_party {
        let provider = doc.get("model_provider")?.as_str()?;
        let table = doc.get("model_providers")?.get(provider)?.clone();
        let mut providers = Table::new();
        providers.set_implicit(true);
        providers.insert(provider, table);
        out.insert("model_providers", Item::Table(providers));
    }
    out.iter().next()?;
    let auth = third_party
        .then(|| std::fs::read(upstream::codex_auth()).ok())
        .flatten()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|auth| auth.get("OPENAI_API_KEY").cloned())
        .filter(|key| key.as_str().is_some_and(|k| !k.is_empty()));
    Some(json!({
        "auth": auth.map_or_else(|| json!({}), |key| json!({ "OPENAI_API_KEY": key })),
        "config": out.to_string(),
    }))
}

fn grok_key_fields() -> Option<Value> {
    let text = std::fs::read_to_string(upstream::grok_config()).ok()?;
    let doc: DocumentMut = text.parse().ok()?;
    let profile = doc.get("models")?.get("default")?.as_str()?;
    let table = doc.get("model")?.get(profile)?.clone();
    let mut out = DocumentMut::new();
    let mut models = Table::new();
    models["default"] = toml_edit::value(profile);
    out["models"] = Item::Table(models);
    let mut profiles = Table::new();
    profiles.set_implicit(true);
    profiles.insert(profile, table);
    out["model"] = Item::Table(profiles);
    Some(json!({ "config": out.to_string() }))
}

fn copy_of<'a>(baseline: &'a Baseline, path: &Path) -> Option<&'a Path> {
    baseline
        .files
        .iter()
        .find(|f| f.path == path)
        .and_then(|f| f.copy.as_deref())
}

/// 引擎切完之后，把它没有照顾到的关键字段补回基线的值（Codex 的 `model`、Gemini 的
/// `selectedType` 等）。`purge`：连 PonteX 时期留下的休眠表一起清掉（断开时用；平时切到原生
/// 要留着，PonteX 时期的 Codex 会话还要靠它打开）。
pub fn restore_key_fields(
    engine: &Engine,
    id: AgentId,
    baseline: &Baseline,
    purge: bool,
) -> Result<(), String> {
    match id {
        AgentId::Codex | AgentId::GrokBuild => {
            let path = live::managed_files(id).remove(0);
            if !path.exists() {
                return Ok(());
            }
            let before = copy_of(baseline, &path)
                .map(|copy| std::fs::read_to_string(copy).map_err(|e| e.to_string()))
                .transpose()?
                .unwrap_or_default()
                .parse::<DocumentMut>()
                .map_err(|e| e.to_string())?;
            let change = move |doc: &mut DocumentMut| match id {
                AgentId::Codex => restore_codex(&before, doc, purge),
                _ => restore_grok(&before, doc),
            };
            engine.write(id, &[Edit::Toml(path, Box::new(change))])
        }
        AgentId::GeminiCli => {
            let path = upstream::gemini_settings();
            if !path.exists() {
                return Ok(());
            }
            let before: Value = copy_of(baseline, &path)
                .and_then(|copy| std::fs::read_to_string(copy).ok())
                .and_then(|text| json5::from_str(&text).ok())
                .unwrap_or(Value::Null);
            let values = key_fields::GEMINI_SETTINGS
                .iter()
                .map(|keys| {
                    let old = before.pointer(&format!("/{}", keys.join("/"))).cloned();
                    (keys.iter().map(|k| k.to_string()).collect(), old)
                })
                .collect();
            engine.write(id, &[Edit::Json(path, values)])
        }
        // Claude 由引擎按基线行还原（开着的会话靠它先写空值再删），WorkBuddy 没有关键字段。
        AgentId::ClaudeCode | AgentId::Workbuddy => Ok(()),
    }
}

/// 按字节还原这些文件。调用方保证它们在我们写完之后没被改过。
pub fn restore_bytes(
    engine: &Engine,
    id: AgentId,
    baseline: &Baseline,
    unchanged: &[PathBuf],
) -> Result<(), String> {
    let mut edits = Vec::new();
    for file in baseline.files.iter().filter(|f| unchanged.contains(&f.path)) {
        edits.push(match &file.copy {
            Some(copy) => Edit::Write(
                file.path.clone(),
                std::fs::read(copy).map_err(|e| e.to_string())?,
            ),
            None => Edit::Delete(file.path.clone()),
        });
    }
    engine.write(id, &edits)
}

fn restore_codex(before: &DocumentMut, doc: &mut DocumentMut, purge: bool) {
    for key in key_fields::CODEX_TOP {
        copy_key(before, doc, key);
    }
    // 用户自己选的 provider：表要和 `model_provider` 一起回来，引擎只会写 `custom` 这个名字。
    let own = before.get("model_provider").and_then(Item::as_str);
    if let Some(name) = own {
        restore_table(before, doc, "model_providers", name);
    }
    if purge || own.is_some_and(|name| name != "custom") {
        restore_table(before, doc, "model_providers", "custom");
    }
}

fn restore_grok(before: &DocumentMut, doc: &mut DocumentMut) {
    let old = before.get("models");
    match doc.get_mut("models").and_then(Item::as_table_like_mut) {
        Some(models) => {
            for key in ["default", "session_summary"] {
                match old.and_then(|m| m.get(key)) {
                    Some(item) => {
                        models.insert(key, item.clone());
                    }
                    None => {
                        models.remove(key);
                    }
                }
            }
            if models.is_empty() && old.is_none() {
                doc.remove("models");
            }
        }
        None => {
            if let Some(old) = old {
                doc.insert("models", old.clone());
            }
        }
    }
    let ours: Vec<String> = doc
        .get("model")
        .and_then(Item::as_table_like)
        .into_iter()
        .flat_map(|t| t.iter())
        .filter(|(_, item)| {
            item.get("base_url")
                .and_then(Item::as_str)
                .is_some_and(|url| url.contains(API_HOST))
        })
        .map(|(key, _)| key.to_string())
        .collect();
    for key in ours {
        restore_table(before, doc, "model", &key);
    }
}

/// 顶层键照基线恢复（连同它前面的注释），基线没有就删。
fn copy_key(before: &DocumentMut, doc: &mut DocumentMut, name: &str) {
    match before.get_key_value(name) {
        Some((key, item)) if item.is_value() => {
            doc.remove(name);
            doc.as_table_mut().insert_formatted(key, item.clone());
        }
        _ => {
            if doc.get(name).is_some_and(Item::is_value) {
                doc.remove(name);
            }
        }
    }
}

fn restore_table(before: &DocumentMut, doc: &mut DocumentMut, parent: &str, name: &str) {
    let old = before.get(parent).and_then(|p| p.get(name)).cloned();
    if old.is_some() && doc.get(parent).is_none() {
        let mut created = Table::new();
        created.set_implicit(true);
        doc.insert(parent, Item::Table(created));
    }
    let Some(table) = doc.get_mut(parent).and_then(Item::as_table_like_mut) else {
        return;
    };
    match old {
        Some(item) => {
            table.insert(name, item);
        }
        None => {
            table.remove(name);
        }
    }
    if table.is_empty() && before.get(parent).is_none() {
        doc.remove(parent);
    }
}
