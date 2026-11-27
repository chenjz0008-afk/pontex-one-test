//! 在临时 HOME 里对真实引擎做端到端测试：不联网，不碰这台电脑的任何配置。

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::agents::{AgentId, Install};
use super::core::{Choice, Core};
use super::key::{self, KeyReport, ModelInfo};
use super::live::{self, Route};
use super::status::Code;
use super::store::Target;

struct Box_ {
    dir: tempfile::TempDir,
    _env: crate::test_support::EnvVars,
}

impl Box_ {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let env = crate::test_support::EnvVars::new()
            .set("CC_SWITCH_TEST_HOME", dir.path())
            .set("HOME", dir.path());
        let _ = crate::settings::reload_settings();
        Self { dir, _env: env }
    }
    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }
    fn put(&self, rel: &str, text: &str) {
        let path = self.path(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path(rel)).unwrap_or_else(|_| "<absent>".into())
    }
    fn files(&self) -> Vec<String> {
        fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
            for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, root, out);
                } else {
                    out.push(path.strip_prefix(root).unwrap().display().to_string());
                }
            }
        }
        let mut out = Vec::new();
        walk(self.dir.path(), self.dir.path(), &mut out);
        out.sort();
        out
    }
    /// Agent 自己目录里的文件（不含我们的数据目录）。
    fn agent_files(&self) -> Vec<String> {
        self.files()
            .into_iter()
            .filter(|f| !f.starts_with(crate::config::APP_DIR_NAME))
            .collect()
    }
    fn core(&self, installed: &[AgentId]) -> Core {
        let core = Core::new(super::upstream::Engine::open().unwrap());
        for id in AgentId::ALL {
            core.set_install(
                id,
                if installed.contains(&id) {
                    Install::Installed { version: None }
                } else {
                    Install::Missing
                },
            );
        }
        core
    }
}

const K1: &str = "px-AAAAAAAAone1one1";
const K2: &str = "px-BBBBBBBBtwo2two2";

fn report() -> KeyReport {
    let models = [
        "anthropic/claude-opus-5.5",
        "anthropic/claude-sonnet-5",
        "anthropic/claude-haiku-4.5",
        "openai/gpt-6-luna",
        "google/gemini-3.5-flash",
        "x-ai/grok-4.7",
    ]
    .map(|id| ModelInfo {
        id: id.into(),
        aliases: vec![],
        modality: Some("text".into()),
        context_length: Some(200_000),
    })
    .to_vec();
    KeyReport {
        usable: true,
        listed: true,
        reason: None,
        request_id: None,
        rotated_until: None,
        hint: String::new(),
        models,
        gemini: Some(vec!["google/gemini-3.5-flash".into()]),
    }
}

fn all() -> Vec<Choice> {
    AgentId::ALL.map(|agent| Choice { agent, apply: true }).to_vec()
}

fn connect(core: &Core, key: &str) -> Vec<(AgentId, &'static str, Option<String>)> {
    core.connect(&key::parse(key).unwrap(), &report(), &all())
        .into_iter()
        .map(|o| (o.agent, o.result, o.error))
        .collect()
}

fn code(core: &Core, id: AgentId) -> Code {
    core.snapshot().agents.into_iter().find(|a| a.id == id).unwrap().status.code
}

fn ok(outcome: super::core::Outcome) {
    assert_eq!(outcome.error, None, "{outcome:?}");
}

const CLAUDE: &str = ".claude/settings.json";
const CODEX: &str = ".codex/config.toml";
const AUTH: &str = ".codex/auth.json";
const GENV: &str = ".gemini/.env";
const GSET: &str = ".gemini/settings.json";
const GROK: &str = ".grok/config.toml";
const WB: &str = ".workbuddy/models.json";

const CODEX_LOGIN: &str = r#"{"auth_mode":"chatgpt","tokens":{"id_token":"a.b.c","access_token":"a.b.c","refresh_token":"r","account_id":"acc"}}"#;

fn user_with_official_keys(b: &Box_) {
    b.put(CLAUDE, "{\n  \"model\": \"opus\",\n  \"env\": {\"ANTHROPIC_API_KEY\": \"sk-ant-mine\", \"FOO\": \"1\"},\n  \"hooks\": {}\n}\n");
    b.put(AUTH, CODEX_LOGIN);
    b.put(CODEX, "# mine\nmodel = \"gpt-5.5\"\napproval_policy = \"on-request\"\n\n[mcp_servers.x]\ncommand = \"x\"\n");
    b.put(GENV, "# my key\nGEMINI_API_KEY=AIzaMine\nGEMINI_SANDBOX=true\n");
    b.put(GSET, "{\n  \"security\": {\"auth\": {\"selectedType\": \"gemini-api-key\"}},\n  \"ui\": {\"theme\": \"x\"}\n}\n");
    b.put(GROK, "theme = \"dark\"\n\n[models]\ndefault = \"grok-4\"\n");
    b.put(WB, "{\"models\":[{\"id\":\"mine\",\"url\":\"https://x.test/v1/chat/completions\"}],\"keep\":1}");
}

fn json_of(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[test]
#[serial_test::serial]
fn opening_the_app_touches_no_agent_file() {
    let b = Box_::new();
    user_with_official_keys(&b);
    let before: Vec<String> = b.agent_files().iter().map(|f| b.read(f)).collect();
    let core = b.core(&AgentId::ALL);
    let _ = core.snapshot();
    let after: Vec<String> = b.agent_files().iter().map(|f| b.read(f)).collect();
    assert_eq!(before, after);
    assert!(!core.snapshot().connected);
}

#[test]
#[serial_test::serial]
fn uninstalled_agents_are_prepared_without_creating_files() {
    let b = Box_::new();
    let core = b.core(&[]);
    let results = connect(&core, K1);
    assert!(results.iter().all(|(_, r, _)| *r == "waiting"), "{results:?}");
    assert_eq!(b.agent_files(), Vec::<String>::new());
    for id in AgentId::ALL {
        assert_eq!(code(&core, id), Code::Waiting);
    }
    // 装好了、什么都没配过：直接应用
    core.set_install(AgentId::Codex, Install::Installed { version: None });
    let done = core.apply_waiting();
    assert_eq!(done.len(), 1);
    assert_eq!(code(&core, AgentId::Codex), Code::Written);
    // 装好了、已经有自己的登录：一样接入，原来的留底，随时切得回去
    b.put(".gemini/oauth_creds.json", "{}");
    b.put(GENV, "GEMINI_API_KEY=AIzaMine\n");
    core.set_install(AgentId::GeminiCli, Install::Installed { version: None });
    assert_eq!(core.apply_waiting().len(), 1);
    assert_eq!(code(&core, AgentId::GeminiCli), Code::Written);
    ok(core.switch(AgentId::GeminiCli, Target::Native));
    assert!(b.read(GENV).contains("AIzaMine"));
    // 用户自己切走了，之后不再替他切回来
    assert!(core.apply_waiting().is_empty());
    assert_eq!(code(&core, AgentId::GeminiCli), Code::Native);
}

#[test]
#[serial_test::serial]
fn clicking_install_means_connect_once_installed() {
    let b = Box_::new();
    let core = b.core(&[AgentId::ClaudeCode]);
    // 没连接 Key：装好也不接入
    core.expect(AgentId::Codex);
    assert_eq!(code(&core, AgentId::Codex), Code::NotInstalled);
    let only: Vec<Choice> = vec![Choice { agent: AgentId::ClaudeCode, apply: true }];
    core.connect(&key::parse(K1).unwrap(), &report(), &only);
    assert_eq!(code(&core, AgentId::Codex), Code::NotInstalled, "没勾就不等");
    core.expect(AgentId::Codex);
    assert_eq!(code(&core, AgentId::Codex), Code::Waiting);
    // 换 Key 不带勾选，等着的继续等，而且用新 Key
    core.connect(&key::parse(K2).unwrap(), &report(), &[]);
    assert_eq!(code(&core, AgentId::Codex), Code::Waiting);
    core.set_install(AgentId::Codex, Install::Installed { version: None });
    assert_eq!(core.apply_waiting().len(), 1);
    assert!(b.read(CODEX).contains(K2));
}

#[test]
#[serial_test::serial]
fn a_failed_auto_connect_is_not_retried_behind_the_users_back() {
    let b = Box_::new();
    let core = b.core(&[]);
    connect(&core, K1);
    b.put(CODEX, "this is = = not toml");
    core.set_install(AgentId::Codex, Install::Installed { version: None });
    assert!(core.apply_waiting()[0].error.is_some());
    assert!(core.apply_waiting().is_empty());
    assert_eq!(b.read(CODEX), "this is = = not toml");
}

#[test]
#[serial_test::serial]
fn a_restored_agent_can_be_connected_again_without_the_key() {
    for last in [AgentId::ClaudeCode, AgentId::GrokBuild] {
        let b = Box_::new();
        user_with_official_keys(&b);
        let core = b.core(&AgentId::ALL);
        connect(&core, K1);
        // 只剩 `last` 还留着 Key，其余逐个还原
        for id in AgentId::ALL.into_iter().filter(|id| *id != last) {
            assert!(core.disconnect(Some(id)).iter().all(|o| o.error.is_none()));
            assert!(!live::read(id).is_pontex(), "{id:?}");
        }
        assert!(core.snapshot().connected);
        ok(core.switch(AgentId::GeminiCli, Target::Pontex));
        assert!(b.read(GENV).contains(K1));
        // 最后一个也还原：等于全部断开，Key 不留
        core.disconnect(Some(AgentId::GeminiCli));
        core.disconnect(Some(last));
        assert!(!core.snapshot().connected);
        assert!(core.stored_key().is_none());
    }
}

#[test]
#[serial_test::serial]
fn connect_writes_five_agents_and_keeps_everything_else() {
    let b = Box_::new();
    user_with_official_keys(&b);
    let core = b.core(&AgentId::ALL);
    let results = connect(&core, K1);
    assert!(results.iter().all(|(_, r, e)| *r == "applied" && e.is_none()), "{results:?}");

    let claude = json_of(&b.read(CLAUDE));
    assert_eq!(claude["env"]["ANTHROPIC_AUTH_TOKEN"], K1);
    assert_eq!(claude["env"]["ANTHROPIC_DEFAULT_HAIKU_MODEL"], "anthropic/claude-haiku-4.5");
    assert_eq!(claude["env"]["FOO"], "1");
    assert_eq!(claude["hooks"], json!({}));
    let codex = b.read(CODEX);
    assert!(codex.contains("[mcp_servers.x]") && codex.contains("approval_policy"));
    assert!(codex.contains("https://api.ponte-x.com/v1") && codex.contains(K1));
    assert_eq!(b.read(AUTH), CODEX_LOGIN, "官方登录原样保留");
    assert!(b.read(GENV).contains("GEMINI_SANDBOX=true"));
    assert_eq!(json_of(&b.read(GSET))["ui"]["theme"], "x");
    assert!(b.read(GROK).contains("theme = \"dark\""));
    let wb = json_of(&b.read(WB));
    assert_eq!(wb["models"][0], json!({"id":"mine","url":"https://x.test/v1/chat/completions"}));
    assert_eq!(wb["keep"], 1);

    for id in AgentId::ALL {
        assert!(live::read(id).is_pontex(), "{id:?} {:?}", live::read(id));
    }
    assert_eq!(code(&core, AgentId::Workbuddy), Code::Added);
    assert!(!b.path(".cc-switch/cc-switch.db").exists() || cfg!(test));
}

#[test]
#[serial_test::serial]
fn native_brings_back_the_users_own_key_every_time() {
    let b = Box_::new();
    user_with_official_keys(&b);
    let core = b.core(&AgentId::ALL);
    connect(&core, K1);
    for round in 0..3 {
        for id in AgentId::ALL.into_iter().filter(|id| id.switchable()) {
            ok(core.switch(id, Target::Native));
            assert_eq!(code(&core, id), Code::Native, "{id:?} round {round}");
        }
        let _ = crate::services::provider::claude_direct::settle_claude_tombstones();
        let claude = json_of(&b.read(CLAUDE));
        assert_eq!(claude["env"], json!({"FOO": "1", "ANTHROPIC_API_KEY": "sk-ant-mine"}), "round {round}");
        assert_eq!(claude["model"], "opus");
        let codex: toml_edit::DocumentMut = b.read(CODEX).parse().unwrap();
        assert_eq!(codex["model"].as_str(), Some("gpt-5.5"));
        assert!(codex.get("model_provider").is_none());
        assert_eq!(b.read(AUTH), CODEX_LOGIN);
        assert!(b.read(GENV).contains("GEMINI_API_KEY=AIzaMine"));
        assert!(!b.read(GENV).contains("ponte-x"));
        assert_eq!(json_of(&b.read(GSET))["security"]["auth"]["selectedType"], "gemini-api-key");
        assert!(b.read(GROK).contains("default = \"grok-4\""), "{}", b.read(GROK));
        for f in [CLAUDE, CODEX, GENV, GROK] {
            assert!(!b.read(f).contains(K1), "{f} 里不该还有 PonteX Key");
        }
        for id in AgentId::ALL.into_iter().filter(|id| id.switchable()) {
            ok(core.switch(id, Target::Pontex));
            assert!(live::read(id).is_pontex());
        }
    }
}

#[test]
#[serial_test::serial]
fn third_party_users_keep_their_gateway_as_a_third_choice() {
    let b = Box_::new();
    b.put(CLAUDE, r#"{"env":{"ANTHROPIC_BASE_URL":"https://relay.test","ANTHROPIC_AUTH_TOKEN":"sk-relay"}}"#);
    b.put(CODEX, "model = \"m\"\nmodel_provider = \"relay\"\n\n[model_providers.relay]\nname = \"R\"\nbase_url = \"https://relay.test/v1\"\nwire_api = \"responses\"\nexperimental_bearer_token = \"sk-relay\"\n");
    b.put(GENV, "GOOGLE_GEMINI_BASE_URL=https://relay.test\nGEMINI_API_KEY=sk-relay\n");
    b.put(GROK, "[models]\ndefault = \"r\"\n\n[model.r]\nmodel = \"r\"\nbase_url = \"https://relay.test/v1\"\nname = \"R\"\napi_key = \"sk-relay\"\napi_backend = \"responses\"\ncontext_window = 1000\n");
    let core = b.core(&AgentId::ALL);
    for id in AgentId::ALL.into_iter().filter(|id| id.switchable()) {
        assert_eq!(code(&core, id), Code::Other, "{id:?}");
    }
    connect(&core, K1);
    for id in AgentId::ALL.into_iter().filter(|id| id.switchable()) {
        assert!(live::read(id).is_pontex(), "{id:?}");
        ok(core.switch(id, Target::Native));
        assert_eq!(live::read(id), Route::Official, "{id:?}");
        ok(core.switch(id, Target::Pontex));
        ok(core.switch(id, Target::Previous));
        assert_eq!(live::read(id), Route::Other { name: "relay.test".into() }, "{id:?}");
        let view = core.snapshot().agents.into_iter().find(|a| a.id == id).unwrap();
        assert_eq!(view.previous.as_deref(), Some("relay.test"));
    }
    for f in [CLAUDE, CODEX, GENV, GROK] {
        assert!(b.read(f).contains("sk-relay"), "{f}: {}", b.read(f));
        assert!(!b.read(f).contains(K1), "{f}");
    }
    let codex: toml_edit::DocumentMut = b.read(CODEX).parse().unwrap();
    assert_eq!(codex["model_provider"].as_str(), Some("relay"));
    assert!(codex["model_providers"].get("custom").is_none());
    // 断开回到第一次连接之前：别家网关
    for id in AgentId::ALL.into_iter().filter(|id| id.switchable()) {
        ok(core.switch(id, Target::Pontex));
    }
    for outcome in core.disconnect(None) {
        ok(outcome);
    }
    for id in AgentId::ALL.into_iter().filter(|id| id.switchable()) {
        assert_eq!(live::read(id), Route::Other { name: "relay.test".into() }, "{id:?}");
    }
}

#[test]
#[serial_test::serial]
fn a_new_native_login_never_replaces_the_saved_gateway() {
    let b = Box_::new();
    b.put(CLAUDE, r#"{"env":{"ANTHROPIC_BASE_URL":"https://relay.test","ANTHROPIC_AUTH_TOKEN":"sk-relay"}}"#);
    let core = b.core(&[AgentId::ClaudeCode]);
    connect(&core, K1);
    ok(core.switch(AgentId::ClaudeCode, Target::Native));
    // 用户在原生期间自己配了官方 Key
    b.put(CLAUDE, r#"{"env":{"ANTHROPIC_API_KEY":"sk-ant-new"}}"#);
    ok(core.switch(AgentId::ClaudeCode, Target::Pontex));
    ok(core.switch(AgentId::ClaudeCode, Target::Native));
    assert!(b.read(CLAUDE).contains("sk-ant-new"));
    ok(core.switch(AgentId::ClaudeCode, Target::Previous));
    assert!(b.read(CLAUDE).contains("sk-relay"));
}

#[test]
#[serial_test::serial]
fn rotating_updates_only_what_uses_pontex() {
    let b = Box_::new();
    user_with_official_keys(&b);
    let core = b.core(&AgentId::ALL);
    connect(&core, K1);
    ok(core.switch(AgentId::GeminiCli, Target::Native));
    // 另一个软件把 Claude 切去了别处：换 Key 不能抢回来
    b.put(CLAUDE, r#"{"env":{"ANTHROPIC_BASE_URL":"https://relay.test","ANTHROPIC_AUTH_TOKEN":"sk-relay"}}"#);
    let none: Vec<Choice> = vec![];
    let results = core.connect(&key::parse(K2).unwrap(), &report(), &none);
    assert!(results.iter().all(|o| o.error.is_none()), "{results:?}");
    assert!(b.read(CLAUDE).contains("sk-relay") && !b.read(CLAUDE).contains(K2));
    assert!(b.read(GENV).contains("AIzaMine") && !b.read(GENV).contains(K2));
    for f in [CODEX, GROK, WB] {
        assert!(b.read(f).contains(K2) && !b.read(f).contains(K1), "{f}");
    }
    ok(core.switch(AgentId::GeminiCli, Target::Pontex));
    assert!(b.read(GENV).contains(K2));
    assert_eq!(core.stored_key().unwrap().expose(), K2);
}

#[test]
#[serial_test::serial]
fn disconnect_restores_bytes_and_removes_the_key() {
    let b = Box_::new();
    user_with_official_keys(&b);
    let originals: Vec<(&str, String)> = [CODEX, AUTH, GENV, GSET, GROK].map(|f| (f, b.read(f))).to_vec();
    let claude = json_of(&b.read(CLAUDE));
    let core = b.core(&AgentId::ALL);
    connect(&core, K1);
    for outcome in core.disconnect(None) {
        ok(outcome);
    }
    let _ = crate::services::provider::claude_direct::settle_claude_tombstones();
    for (file, text) in originals {
        assert_eq!(b.read(file), text, "{file} 应当逐字节还原");
    }
    assert_eq!(json_of(&b.read(CLAUDE)), claude);
    assert_eq!(json_of(&b.read(WB))["models"].as_array().unwrap().len(), 1);
    assert!(core.stored_key().is_none());
    assert!(!core.snapshot().connected);
    for file in b.agent_files() {
        assert!(!b.read(&file).contains(K1), "{file}");
    }
}

#[test]
#[serial_test::serial]
fn disconnect_keeps_what_the_user_changed_meanwhile() {
    let b = Box_::new();
    user_with_official_keys(&b);
    let core = b.core(&AgentId::ALL);
    connect(&core, K1);
    let edited = format!("{}\n[projects.\"/w\"]\ntrust_level = \"trusted\"\n", b.read(CODEX));
    b.put(CODEX, &edited);
    for outcome in core.disconnect(None) {
        ok(outcome);
    }
    let doc: toml_edit::DocumentMut = b.read(CODEX).parse().unwrap();
    assert_eq!(doc["projects"]["/w"]["trust_level"].as_str(), Some("trusted"));
    assert_eq!(doc["model"].as_str(), Some("gpt-5.5"));
    assert!(doc.get("model_providers").is_none(), "{doc}");
    assert!(b.read(CODEX).contains("# mine"));
}

#[test]
#[serial_test::serial]
fn fresh_machine_round_trip_leaves_no_pontex_trace() {
    let b = Box_::new();
    let core = b.core(&AgentId::ALL);
    connect(&core, K1);
    for id in AgentId::ALL.into_iter().filter(|id| id.switchable()) {
        ok(core.switch(id, Target::Native));
        assert_eq!(live::read(id), Route::Official, "{id:?}");
    }
    let _ = crate::services::provider::claude_direct::settle_claude_tombstones();
    for outcome in core.disconnect(None) {
        ok(outcome);
    }
    for file in b.agent_files() {
        let text = b.read(&file);
        assert!(!text.contains("ponte-x.com") && !text.contains(K1), "{file}: {text}");
    }
}

#[test]
#[serial_test::serial]
fn broken_config_is_left_alone_and_others_still_work() {
    let b = Box_::new();
    b.put(CLAUDE, "{ not json");
    b.put(CODEX, "model = \n");
    let core = b.core(&AgentId::ALL);
    let results = connect(&core, K1);
    let of = |id| results.iter().find(|(a, ..)| *a == id).unwrap().clone();
    assert_eq!(of(AgentId::ClaudeCode).1, "failed");
    assert_eq!(of(AgentId::Codex).1, "failed");
    assert_eq!(of(AgentId::GeminiCli).1, "applied");
    assert_eq!(of(AgentId::GrokBuild).1, "applied");
    assert_eq!(b.read(CLAUDE), "{ not json");
    assert_eq!(b.read(CODEX), "model = \n");
    assert_eq!(code(&core, AgentId::ClaudeCode), Code::Unreadable);
}

#[test]
#[serial_test::serial]
fn workbuddy_never_takes_over_a_model_someone_else_added() {
    let b = Box_::new();
    b.put(WB, r#"{"models":[{"id":"openai/gpt-6-luna","url":"https://mine.test/chat/completions","apiKey":"mine"}]}"#);
    let core = b.core(&[AgentId::Workbuddy]);
    connect(&core, K1);
    let wb = json_of(&b.read(WB));
    let same: Vec<&Value> = wb["models"].as_array().unwrap().iter().filter(|m| m["id"] == "openai/gpt-6-luna").collect();
    assert_eq!(same.len(), 1);
    assert_eq!(same[0]["apiKey"], "mine");
    assert!(wb["models"].as_array().unwrap().len() > 1);
}

#[test]
#[serial_test::serial]
fn a_key_without_the_model_writes_nothing_for_that_agent() {
    let b = Box_::new();
    let core = b.core(&AgentId::ALL);
    let mut limited = report();
    limited.models.retain(|m| m.id.starts_with("anthropic/"));
    limited.gemini = Some(vec![]);
    let results = core.connect(&key::parse(K1).unwrap(), &limited, &all());
    let of = |id| results.iter().find(|o| o.agent == id).unwrap().result;
    assert_eq!(of(AgentId::ClaudeCode), "applied");
    assert_eq!(of(AgentId::Codex), "no-model");
    assert_eq!(of(AgentId::GeminiCli), "no-model");
    assert!(!b.path(CODEX).exists() && !b.path(GENV).exists());
}

#[test]
#[serial_test::serial]
fn state_survives_a_restart_and_a_damaged_state_file() {
    let b = Box_::new();
    user_with_official_keys(&b);
    let core = b.core(&AgentId::ALL);
    connect(&core, K1);
    drop(core);
    let core = b.core(&AgentId::ALL);
    assert!(core.snapshot().connected);
    ok(core.switch(AgentId::ClaudeCode, Target::Native));
    assert!(b.read(CLAUDE).contains("sk-ant-mine"));
    drop(core);
    fs::write(super::store::path(), "garbage").unwrap();
    let core = b.core(&AgentId::ALL);
    assert!(!core.snapshot().connected);
    assert!(b.files().iter().any(|f| f.contains("pontex-state.json.bad-") || f.contains(".bad-")));
}

#[test]
#[serial_test::serial]
fn changing_the_model_rewrites_only_when_in_use() {
    let b = Box_::new();
    let core = b.core(&AgentId::ALL);
    connect(&core, K1);
    ok(core.set_model(AgentId::ClaudeCode, "anthropic/claude-sonnet-5"));
    assert_eq!(json_of(&b.read(CLAUDE))["env"]["ANTHROPIC_MODEL"], "anthropic/claude-sonnet-5");
    ok(core.switch(AgentId::Codex, Target::Native));
    let before = b.read(CODEX);
    ok(core.set_model(AgentId::Codex, "anthropic/claude-sonnet-5"));
    assert_eq!(b.read(CODEX), before);
    assert!(core.set_model(AgentId::Codex, "nope/none").error.is_some());
}

#[test]
#[serial_test::serial]
fn nothing_we_persist_outside_the_engine_contains_the_key() {
    let b = Box_::new();
    user_with_official_keys(&b);
    let core = b.core(&AgentId::ALL);
    connect(&core, K1);
    assert!(!fs::read_to_string(super::store::path()).unwrap().contains(K1));
    assert!(!serde_json::to_string(&core.snapshot()).unwrap().contains(K1));
}
