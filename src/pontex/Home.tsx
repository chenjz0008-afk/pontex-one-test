import { useEffect, useState } from "react";
import { api, type Agent, type AgentId, type Outcome, type Progress, type Snapshot, type Target } from "./api";
import { actionText, reason, statusText, timeText, tone, type T } from "./i18n";
import { active, Bar, cancellable, MirrorCommand, phaseText, useInstaller } from "./Install";
import { OpenButton, opensIn } from "./Launch";
import { AgentIcon, Button, Dialog, Notice, useToast } from "./ui";

export function Home({ snapshot, t }: { snapshot: Snapshot; t: T }) {
  const [open, setOpen] = useState<AgentId | null>(null);
  const [busy, setBusy] = useState<AgentId | null>(null);
  const [message, setMessage] = useState<{ bad: boolean; text: string } | null>(null);
  const [asking, setAsking] = useState<{ agent: Agent; what: "remove" | "restore" } | null>(null);
  const { progress, missing, install, dialog, notice } = useInstaller(snapshot, t);
  const toast = useToast();
  const installed = snapshot.agents.filter((a) => a.install.state === "installed");
  const using = installed.filter((a) => a.current === "pontex");
  // 说明类的提示看过就走，出错的留着。
  useEffect(() => {
    if (!message || message.bad) return;
    const timer = setTimeout(() => setMessage(null), 8000);
    return () => clearTimeout(timer);
  }, [message]);

  async function act(agent: AgentId, work: () => Promise<Outcome | Outcome[] | unknown>): Promise<boolean> {
    setBusy(agent);
    setMessage(null);
    try {
      const result = await work();
      const outcome = (Array.isArray(result) ? result[0] : result) as Partial<Outcome> | null;
      if (outcome?.error) {
        setMessage({ bad: true, text: reason(t, outcome.error) });
        return false;
      }
      if (outcome?.locked_sessions) setMessage({ bad: false, text: t.lockedSessions(outcome.locked_sessions) });
      return true;
    } catch (cause) {
      setMessage({ bad: true, text: reason(t, String(cause)) });
      return false;
    } finally {
      setBusy(null);
    }
  }

  /** 切 PonteX / 原生：说清楚要彻底关闭再打开才生效，能撤销。 */
  async function pick(agent: Agent, target: Target) {
    const before = agent.current;
    const ok = await act(agent.id, () => api.switch(agent.id, target));
    if (!ok) return;
    toast({
      text: t.switchedTo(agent.label, target),
      sub: t.restartOne(agent.label),
      undo: before && before !== target ? () => act(agent.id, () => api.switch(agent.id, before)) : undefined,
    });
  }

  return (
    <section>
      <h1 className="sr-only">{t.homeTitle}</h1>
      <div className="page-head">
        <span className="summary">{t.summary(installed.length, using.length)}</span>
        <span className="agent-actions">
          {missing.length > 1 && <Button kind="plain" onClick={() => install(missing)}>{t.installAll(missing.length)}</Button>}
        </span>
      </div>
      {snapshot.cc_switch_present && !snapshot.prefs.coexist_notice_seen && (
        <Notice
          action={
            <Button
              kind="quiet"
              onClick={() => void api.setPrefs({ ...snapshot.prefs, coexist_notice_seen: true })}
            >
              {t.gotIt}
            </Button>
          }
        >
          {t.coexist}
        </Notice>
      )}
      {notice}
      {message && (
        <Notice
          tone={message.bad ? "bad" : "info"}
          action={<Button kind="quiet" onClick={() => setMessage(null)}>{t.dismiss}</Button>}
        >
          {message.text}
        </Notice>
      )}
      <ul className="list">
        {snapshot.agents.map((agent) => (
          <Row
            key={agent.id}
            agent={agent}
            t={t}
            busy={busy === agent.id}
            locked={busy !== null}
            open={open === agent.id}
            toggle={() => setOpen(open === agent.id ? null : agent.id)}
            act={(work) => void act(agent.id, work)}
            pick={(target) => void pick(agent, target)}
            firstTime={!snapshot.prefs.launched_once}
            progress={progress[agent.id]}
            connected={snapshot.connected}
            onInstall={() => install([agent])}
            ask={(what) => setAsking({ agent, what })}
          />
        ))}
      </ul>
      <p className="note">{t.restartNote}</p>
      {dialog}
      {asking && (
        <Dialog
          title={asking.what === "remove" ? t.uninstallTitle(asking.agent.label) : t.restoreTitle(asking.agent.label)}
          onClose={() => setAsking(null)}
        >
          <p>{asking.what === "remove" ? t.uninstallConfirm(asking.agent.label) : t.restoreConfirm(asking.agent.label)}</p>
          <div className="row end">
            <Button kind="quiet" autoFocus onClick={() => setAsking(null)}>{t.cancel}</Button>
            <Button
              kind="danger"
              onClick={() => {
                const { agent, what } = asking;
                void act(agent.id, () => (what === "remove" ? api.uninstall(agent.id) : api.disconnect(agent.id)));
                setAsking(null);
              }}
            >
              {asking.what === "remove" ? t.uninstall : t.restoreOne}
            </Button>
          </div>
        </Dialog>
      )}
    </section>
  );
}

function Row(props: {
  agent: Agent;
  t: T;
  busy: boolean;
  locked: boolean;
  open: boolean;
  toggle: () => void;
  act: (work: () => Promise<unknown>) => void;
  progress?: Progress;
  connected: boolean;
  onInstall: () => void;
  ask: (what: "remove" | "restore") => void;
  pick: (target: Target) => void;
  firstTime: boolean;
}) {
  const { agent, t, act, progress } = props;
  const busy = active(progress);
  const failed = progress?.phase === "failed" && (agent.installable || Boolean(agent.update));
  const cancelled = failed && progress?.error === "INSTALL_CANCELLED";
  const installed = agent.install.state === "installed";
  const run = () => {
    switch (agent.status.action) {
      case "install":
        if (agent.installable) return props.onInstall();
        return agent.mirror ? props.toggle() : void api.open("install", agent.id);
      case "verify":
        return act(() => api.verify(agent.id));
      case "apply":
        return act(() => api.switch(agent.id, "pontex"));
      case "open-app":
        return act(() => api.open("workbuddy"));
      case "show-detail":
        return props.toggle();
      default:
        return undefined;
    }
  };
  // 最右边永远是一个：装了的是「打开」，能装的是「安装」。别的动作（验证、接入、查看）放在它左边。
  const installButton = !busy && agent.installable && (agent.status.action === "install" || failed);
  const opens = opensIn(agent);
  const action = agent.status.action === "open-app" && opens ? null : agent.status.action;
  const label = installButton ? "" : action === "install" && agent.installable ? "" : action ? actionText(t, agent) : "";
  return (
    <li className={props.open ? "open" : undefined}>
      <div className="agent-row">
        <button
          type="button"
          className="agent-main"
          aria-expanded={props.open}
          aria-controls={`detail-${agent.id}`}
          onClick={props.toggle}
        >
          <AgentIcon id={agent.id} />
          <span className="agent-text">
            <strong>{agent.label}</strong>
            <small>
              <span className={`dot ${failed && !cancelled ? "bad" : tone(agent)}`} aria-hidden />
              {progress && (busy || failed) ? phaseText(t, progress) : statusText(t, agent)}
            </small>
            {progress && busy && progress.phase !== "queued" && <Bar progress={progress} />}
          </span>
        </button>
        <div className="agent-actions">
          {busy && cancellable(progress!) && <Button kind="quiet" onClick={() => void api.installCancel(agent.id)}>{t.cancel}</Button>}
          {!busy && agent.update && (
            <Button kind="plain" onClick={() => void api.install([agent.id])}>{t.upgrade(agent.update)}</Button>
          )}
          {!busy && label && (
            <Button kind="quiet" busy={props.busy} disabled={props.locked} onClick={run}>{label}</Button>
          )}
          {installed && agent.switchable && (
            <Segment agent={agent} t={t} connected={props.connected} disabled={props.locked} onPick={props.pick} />
          )}
          {installButton && (
            <Button kind="plain" busy={props.busy} disabled={props.locked} onClick={run}>{failed && !cancelled ? t.retry : t.install}</Button>
          )}
          {!busy && opens && <OpenButton agent={agent} t={t} firstTime={props.firstTime} disabled={props.locked} />}
        </div>
      </div>
      {props.open && <Detail agent={agent} t={t} locked={props.locked || busy} act={act} ask={props.ask} />}
    </li>
  );
}

function Segment({ agent, t, connected, disabled, onPick }: { agent: Agent; t: T; connected: boolean; disabled: boolean; onPick: (target: Target) => void }) {
  const options: { target: Target; label: string; off?: boolean; hint?: string }[] = [
    { target: "pontex", label: "PonteX", off: !connected },
    { target: "native", label: t.native },
  ];
  // 域名放在悬停里：按钮上写太长，也没人记得住。
  if (agent.previous) options.push({ target: "previous", label: t.other, hint: agent.previous });
  return (
    <div className="segment" role="radiogroup" aria-label={agent.label}>
      {options.map((option) => {
        const on = agent.current === option.target;
        return (
          <button
            key={option.target}
            type="button"
            role="radio"
            aria-checked={on}
            className={on ? "on" : undefined}
            disabled={disabled || option.off}
            title={option.hint}
            onClick={() => !on && onPick(option.target)}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}

function Detail({ agent, t, locked, act, ask }: { agent: Agent; t: T; locked: boolean; act: (work: () => Promise<unknown>) => void; ask: (what: "remove" | "restore") => void }) {
  const unusable = agent.install.state === "missing" || agent.install.state === "broken";
  const version = agent.install.state === "installed" ? agent.install.version : null;
  return (
    <div className="detail" id={`detail-${agent.id}`}>
      <dl>
        {version && version !== agent.app && (<><dt>{t.version}</dt><dd>{version}{agent.managed && ` · ${t.managed}`}</dd></>)}
        {agent.app && (<><dt>{t.appOption}</dt><dd>{agent.app}</dd></>)}
        {agent.route.file && (<><dt>{t.location}</dt><dd className="path">{agent.route.file}</dd></>)}
        {agent.options.length > 0 && agent.connected && agent.id !== "workbuddy" && (
          <>
            <dt><label htmlFor={`model-${agent.id}`}>{t.model}</label></dt>
            <dd>
              <select
                id={`model-${agent.id}`}
                value={agent.model ?? ""}
                disabled={locked}
                onChange={(event) => act(() => api.setModel(agent.id, event.target.value))}
              >
                {(!agent.model || !agent.options.includes(agent.model)) && (
                  <option value={agent.model ?? ""}>{agent.model ?? "—"}</option>
                )}
                {agent.options.map((id) => <option key={id}>{id}</option>)}
              </select>
            </dd>
          </>
        )}
        {agent.verify && agent.current === "pontex" && (
          <>
            <dt>{t.lastVerify}</dt>
            <dd>
              {timeText(t, agent.verify.at)} · {agent.verify.ok ? t.verifyOk : reason(t, agent.verify.reason)}
              {!agent.verify.ok && agent.verify.request_id && <small className="path">{t.requestId} {agent.verify.request_id}</small>}
            </dd>
          </>
        )}
        {agent.overrides.length > 0 && (
          <>
            <dt>{t.envNotes}</dt>
            <dd>
              {agent.overrides.map((o) => (
                <p key={o.name + o.source}>
                  <code>{o.name}</code> <span className="path">{o.source}</span>
                  <small>{o.blocking ? t.envBlocking : t.envHarmless}</small>
                </p>
              ))}
            </dd>
          </>
        )}
      </dl>
      {agent.install.state === "broken" && <p>{t.brokenHint(agent.install.path)}</p>}
      {unusable && agent.mirror && (
        <>
          {agent.install.state === "missing" && <p className="muted">{t.mirrorCommand}</p>}
          <MirrorCommand agent={agent} t={t} />
        </>
      )}
      {agent.id === "gemini-cli" && <p className="note">{t.officialOffline}</p>}
      <div className="row">
        {agent.route.kind === "pontex" && (
          <Button kind="plain" disabled={locked} onClick={() => act(() => api.verify(agent.id))}>{t.verifyNow}</Button>
        )}
        {agent.connected && (agent.current === "pontex") && (
          <Button kind="plain" disabled={locked} onClick={() => ask("restore")}>{t.restoreOne}</Button>
        )}
        {agent.app && <Button kind="plain" onClick={() => act(() => api.open("codex-app"))}>{t.openApp}</Button>}
        {agent.app_installable && !unusable && (
          <Button kind="plain" disabled={locked} onClick={() => act(() => api.install([agent.id], true))}>{t.installApp}</Button>
        )}
        {agent.managed && <Button kind="danger" disabled={locked} onClick={() => ask("remove")}>{t.uninstall}</Button>}
      </div>
    </div>
  );
}
