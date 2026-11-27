import { useState, type FormEvent } from "react";
import { api, type AgentId, type KeyReport, type Outcome, type Snapshot } from "./api";
import { outcomeText, reason, timeText, type T } from "./i18n";
import { active, Bar, cancellable, phaseText, useInstaller } from "./Install";
import { AgentIcon, Button, Notice } from "./ui";

type Step =
  | { at: "key" }
  | { at: "plan"; report: KeyReport }
  | { at: "result"; outcomes: Outcome[] };

/** 两步：先验证 Key，再列出会改什么，用户点头才写。 */
export function Connect({ snapshot, t, onDone }: { snapshot: Snapshot; t: T; onDone: () => void }) {
  const [step, setStep] = useState<Step>({ at: "key" });
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const { progress, missing, install, dialog, notice } = useInstaller(snapshot, t);
  // 默认全部接入；原来的配置会留底，随时切得回去。读不懂的不动。
  const [picked, setPicked] = useState<Set<AgentId>>(
    () =>
      new Set(
        snapshot.agents
          .filter((a) => a.route.kind !== "unreadable")
          .map((a) => a.id),
      ),
  );

  async function check(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      const report = await api.checkKey(key);
      if (report.usable) setStep({ at: "plan", report });
      else setError(reason(t, report.reason));
    } catch (cause) {
      setError(reason(t, String(cause)));
    } finally {
      setBusy(false);
    }
  }

  async function apply() {
    setBusy(true);
    setError("");
    try {
      const choices = snapshot.agents.map((a) => ({ agent: a.id, apply: picked.has(a.id) }));
      const { outcomes } = await api.connect(key, choices);
      setKey("");
      setStep({ at: "result", outcomes });
    } catch (cause) {
      setError(reason(t, String(cause)));
    } finally {
      setBusy(false);
    }
  }

  if (step.at === "key")
    return (
      <section className="narrow hero">
        <h1>{t.connectTitle}</h1>
        <p className="lead">{t.connectLead}</p>
        <form onSubmit={(event) => void check(event)}>
          <label htmlFor="px-key" className="sr-only">{t.keyLabel}</label>
          <div className="key-row">
            <input
              id="px-key"
              type="password"
              value={key}
              placeholder="px-"
              autoComplete="off"
              autoCorrect="off"
              spellCheck={false}
              autoFocus
              aria-invalid={Boolean(error)}
              aria-describedby="px-key-note"
              onChange={(event) => {
                setKey(event.target.value);
                setError("");
              }}
            />
            <Button kind="primary" type="submit" busy={busy} disabled={!key.trim()}>
              {busy ? t.verifying : t.next}
            </Button>
          </div>
        </form>
        {error && <Notice tone="bad">{error}</Notice>}
        <button type="button" className="link" onClick={() => void api.open("key")}>
          {t.keyHelp} →
        </button>
        <p id="px-key-note" className="note">{t.keyStored}</p>
        <ul className="supported" aria-label={t.planTitle}>
          {snapshot.agents.map((agent) => (
            <li key={agent.id}><AgentIcon id={agent.id} />{agent.label}</li>
          ))}
        </ul>
      </section>
    );

  if (step.at === "plan")
    return (
      <section>
        <h1>{t.planTitle}</h1>
        <p className="lead">{t.planLead}</p>
        {!step.report.listed && <Notice tone="warn">{t.quotaNotice}</Notice>}
        {step.report.rotated_until && (
          <Notice tone="warn">{t.rotatedNotice(timeText(t, step.report.rotated_until))}</Notice>
        )}
        <ul className="list">
          {snapshot.agents.map((agent) => {
            const broken = agent.route.kind === "unreadable";
            return (
              <li key={agent.id}>
                <label className="pick">
                  <input
                    type="checkbox"
                    checked={picked.has(agent.id) && !broken}
                    disabled={broken || busy}
                    onChange={() =>
                      setPicked((now) => {
                        const next = new Set(now);
                        if (!next.delete(agent.id)) next.add(agent.id);
                        return next;
                      })
                    }
                  />
                  <AgentIcon id={agent.id} />
                  <span className="agent-text">
                    <strong>{agent.label}</strong>
                    <small>{nowText(t, agent, picked.has(agent.id))}</small>
                  </span>
                </label>
              </li>
            );
          })}
        </ul>
        {error && <Notice tone="bad">{error}</Notice>}
        <div className="row end">
          <Button kind="quiet" disabled={busy} onClick={() => setStep({ at: "key" })}>{t.changeKey}</Button>
          <Button kind="primary" busy={busy} disabled={picked.size === 0} onClick={() => void apply()}>
            {busy ? t.applying : t.apply}
          </Button>
        </div>
        <p className="note">{t.planVerifyNote}</p>
      </section>
    );

  const locked = step.outcomes.find((o) => o.locked_sessions)?.locked_sessions;
  return (
    <section>
      <div className="title-row">
        <h1>{t.resultTitle}</h1>
        {missing.length > 1 && <Button kind="plain" onClick={() => install(missing)}>{t.installAll(missing.length)}</Button>}
      </div>
      {notice}
      <ul className="list">
        {step.outcomes.map((outcome) => {
          const agent = snapshot.agents.find((a) => a.id === outcome.agent)!;
          const now = progress[agent.id];
          const busy = active(now);
          const failed = now?.phase === "failed" && agent.installable;
          // 在这一页装好、接好的，直接改成结果。
          const late = outcome.result === "waiting" && agent.install.state === "installed";
          const text =
            now && (busy || failed) ? phaseText(t, now)
            : late && agent.current === "pontex" ? outcomeText(t, { ...outcome, result: "applied" })
            : late && agent.status.code === "failed" ? reason(t, agent.status.detail)
            : outcomeText(t, outcome);
          const bad = Boolean(outcome.error) || (failed && now?.error !== "INSTALL_CANCELLED") || (late && agent.status.code === "failed");
          return (
            <li key={outcome.agent}>
              <AgentIcon id={outcome.agent} />
              <span className="agent-text">
                <strong>{agent.label}</strong>
                <small className={bad ? "bad" : undefined}>{text}</small>
                {now && busy && now.phase !== "queued" && <Bar progress={now} />}
              </span>
              <span className="agent-actions">
                {now && busy && cancellable(now) && <Button kind="quiet" onClick={() => void api.installCancel(agent.id)}>{t.cancel}</Button>}
                {!busy && agent.installable && <Button kind="plain" onClick={() => install([agent])}>{failed ? t.retry : t.install}</Button>}
                {agent.id === "workbuddy" && agent.install.state === "missing" && (
                  <Button kind="quiet" onClick={() => void api.open("install", agent.id)}>{t.officialSite}</Button>
                )}
              </span>
            </li>
          );
        })}
      </ul>
      {dialog}
      {locked ? <Notice>{t.lockedSessions(locked)}</Notice> : null}
      <p className="note">{t.newSession}</p>
      <div className="row end">
        <Button kind="primary" autoFocus onClick={onDone}>{t.done}</Button>
      </div>
    </section>
  );
}

function nowText(t: T, agent: Snapshot["agents"][number], picked: boolean): string {
  if (agent.route.kind === "unreadable") return t.planNowBroken;
  if (agent.install.state === "missing") return t.planNowMissing;
  if (agent.id === "workbuddy") return t.planNowWb;
  switch (agent.route.kind) {
    case "pontex":
      return t.planNow + t.planNowPontex;
    case "other":
      return t.planNow + (picked ? t.planNowOther : t.planNowOtherKept)(agent.route.name ?? "");
    case "official-key":
      return `${t.planNow}${t.planNowNative} · ${agent.route.via ?? ""}`;
    default:
      return t.planNow + t.planNowNative;
  }
}
