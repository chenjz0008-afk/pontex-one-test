//! 把「装没装、走哪条线路、验证过没有」合成唯一一句话。纯函数，主窗口和托盘共用。

use serde::Serialize;

use super::agents::{AgentId, Install};
use super::live::Route;
use super::overrides::Override;
use super::store::{AgentState, Target};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Code {
    Checking,
    NotInstalled,
    Waiting,
    Broken,
    Unreadable,
    Failed,
    Overridden,
    Drifted,
    OtherKey,
    Other,
    Verified,
    Written,
    VerifyFailed,
    Native,
    Added,
    NotAdded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Install,
    Verify,
    Apply,
    OpenApp,
    ShowDetail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    pub code: Code,
    /// 文案里要填的内容：网关名、变量名、文件、原因码。
    pub detail: Option<String>,
    pub action: Option<Action>,
}

pub struct Facts<'a> {
    pub id: AgentId,
    pub install: &'a Install,
    pub route: &'a Route,
    pub state: &'a AgentState,
    pub overrides: &'a [Override],
    /// 已连接的 Key 的前缀。
    pub key_hint: Option<&'a str>,
}

/// 现在走的线路对应哪一格。
pub fn current(route: &Route) -> Option<Target> {
    match route {
        Route::Pontex { .. } | Route::Added { .. } => Some(Target::Pontex),
        Route::Official | Route::OfficialKey { .. } => Some(Target::Native),
        Route::Other { .. } => Some(Target::Previous),
        Route::NotAdded | Route::Unreadable { .. } => None,
    }
}

pub fn derive(facts: &Facts<'_>) -> Status {
    let state = facts.state;
    let say = |code, detail: Option<String>, action| Status { code, detail, action };
    match facts.install {
        Install::Checking => return say(Code::Checking, None, None),
        Install::Missing if state.waiting => {
            return say(Code::Waiting, None, Some(Action::Install))
        }
        Install::Missing => return say(Code::NotInstalled, None, Some(Action::Install)),
        Install::Broken { path } => {
            return say(Code::Broken, Some(path.clone()), Some(Action::ShowDetail))
        }
        Install::Installed { .. } => {}
    }
    if let Route::Unreadable { file } = facts.route {
        return say(Code::Unreadable, Some(file.clone()), Some(Action::ShowDetail));
    }
    if let Some(error) = &state.error {
        return say(Code::Failed, Some(error.clone()), Some(Action::Apply));
    }
    let hint = match facts.route {
        Route::Pontex { key_hint, .. } | Route::Added { key_hint, .. } => key_hint.as_deref(),
        _ => None,
    };
    let other_key = facts.route.is_pontex() && facts.key_hint.is_some() && hint != facts.key_hint;
    match facts.route {
        Route::Pontex { .. } => {
            if let Some(found) = facts.overrides.iter().find(|o| o.blocking) {
                return say(Code::Overridden, Some(found.name.clone()), Some(Action::ShowDetail));
            }
            if other_key {
                return say(Code::OtherKey, hint.map(str::to_string), Some(Action::Apply));
            }
            verified(facts)
        }
        Route::Added { .. } if other_key => {
            say(Code::OtherKey, hint.map(str::to_string), Some(Action::Apply))
        }
        // 模型在列表里不等于正在用：WorkBuddy 的当前模型要用户自己选。
        Route::Added { .. } => say(Code::Added, None, Some(Action::OpenApp)),
        Route::NotAdded => say(
            Code::NotAdded,
            None,
            facts.key_hint.map(|_| Action::Apply),
        ),
        Route::Other { name } => say(Code::Other, Some(name.clone()), None),
        Route::Official | Route::OfficialKey { .. } => {
            if state.waiting {
                // 装好了，马上就会接入。
                say(Code::Checking, None, None)
            } else if state.target == Some(Target::Pontex) {
                say(Code::Drifted, None, Some(Action::Apply))
            } else {
                let via = match facts.route {
                    Route::OfficialKey { via } => Some(via.clone()),
                    _ => None,
                };
                say(Code::Native, via, None)
            }
        }
        Route::Unreadable { .. } => unreachable!("handled above"),
    }
}

/// 验证结果只在 Key、模型都和现在一致时才算数。
fn verified(facts: &Facts<'_>) -> Status {
    let Route::Pontex { key_hint, model } = facts.route else {
        unreachable!("only for PonteX routes")
    };
    let matching = facts.state.verify.as_ref().filter(|v| {
        Some(v.key_hint.as_str()) == key_hint.as_deref() && Some(v.model.as_str()) == model.as_deref()
    });
    match matching {
        Some(v) if v.ok => Status { code: Code::Verified, detail: None, action: None },
        Some(v) => Status {
            code: Code::VerifyFailed,
            detail: v.reason.clone(),
            action: Some(Action::Verify),
        },
        None => Status { code: Code::Written, detail: None, action: Some(Action::Verify) },
    }
}

#[cfg(test)]
mod tests {
    use super::super::store::Verify;
    use super::*;

    fn installed() -> Install {
        Install::Installed { version: Some("1".into()) }
    }
    fn pontex() -> Route {
        Route::Pontex { key_hint: Some("px-AAAAAAAA".into()), model: Some("m".into()) }
    }
    fn ask(install: Install, route: Route, state: AgentState, overrides: Vec<Override>) -> Code {
        derive(&Facts {
            id: AgentId::GeminiCli,
            install: &install,
            route: &route,
            state: &state,
            overrides: &overrides,
            key_hint: Some("px-AAAAAAAA"),
        })
        .code
    }
    fn verify(ok: bool, model: &str) -> AgentState {
        AgentState {
            verify: Some(Verify {
                at: 0,
                ok,
                reason: (!ok).then(|| "MODEL_NOT_AVAILABLE".into()),
                request_id: None,
                model: model.into(),
                key_hint: "px-AAAAAAAA".into(),
            }),
            ..AgentState::default()
        }
    }

    #[test]
    fn written_is_never_shown_as_verified() {
        assert_eq!(ask(installed(), pontex(), AgentState::default(), vec![]), Code::Written);
        assert_eq!(ask(installed(), pontex(), verify(true, "m"), vec![]), Code::Verified);
        assert_eq!(ask(installed(), pontex(), verify(false, "m"), vec![]), Code::VerifyFailed);
        // 换了模型，旧的验证不算数
        assert_eq!(ask(installed(), pontex(), verify(true, "old"), vec![]), Code::Written);
    }

    #[test]
    fn install_state_comes_first() {
        let waiting = AgentState { waiting: true, ..AgentState::default() };
        assert_eq!(ask(Install::Missing, Route::Official, waiting.clone(), vec![]), Code::Waiting);
        assert_eq!(ask(Install::Missing, pontex(), verify(true, "m"), vec![]), Code::NotInstalled);
        assert_eq!(ask(installed(), Route::Official, waiting, vec![]), Code::Checking);
    }

    #[test]
    fn outside_changes_are_reported_not_reverted() {
        let ours = AgentState { target: Some(Target::Pontex), ..AgentState::default() };
        assert_eq!(ask(installed(), Route::Official, ours.clone(), vec![]), Code::Drifted);
        assert_eq!(
            ask(installed(), Route::Other { name: "x".into() }, ours, vec![]),
            Code::Other
        );
        let route = Route::Pontex { key_hint: Some("px-BBBBBBBB".into()), model: None };
        assert_eq!(ask(installed(), route, AgentState::default(), vec![]), Code::OtherKey);
    }

    #[test]
    fn only_blocking_overrides_change_the_status() {
        let ov = |blocking| vec![Override { name: "GEMINI_API_KEY".into(), source: "~/.zshrc".into(), blocking }];
        assert_eq!(ask(installed(), pontex(), verify(true, "m"), ov(true)), Code::Overridden);
        assert_eq!(ask(installed(), pontex(), verify(true, "m"), ov(false)), Code::Verified);
        assert_eq!(ask(installed(), Route::Official, AgentState::default(), ov(true)), Code::Native);
    }

    #[test]
    fn workbuddy_added_is_not_in_use() {
        let route = Route::Added { models: 2, key_hint: Some("px-AAAAAAAA".into()) };
        assert_eq!(ask(installed(), route, AgentState::default(), vec![]), Code::Added);
    }
}
