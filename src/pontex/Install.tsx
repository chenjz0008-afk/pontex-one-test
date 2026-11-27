import { useEffect, useState } from "react";
import { api, onInstall, type Agent, type AgentId, type Planned, type Progress, type Snapshot } from "./api";
import { list, reason, type T } from "./i18n";
import { AgentIcon, Button, Copyable, Dialog, Notice } from "./ui";

const COMMAND: Partial<Record<AgentId, string>> = {
  "claude-code": "claude",
  codex: "codex",
  "gemini-cli": "gemini",
  "grok-build": "grok",
};

/** 解包、试运行停不下来，也就几秒：这时不给「取消」。 */
export const cancellable = (p: Progress) => ["queued", "resolving", "downloading"].includes(p.phase);
export const active = (p: Progress | undefined) => Boolean(p && p.phase !== "done" && p.phase !== "failed");

/** 安装进度。和状态分开走：下载时一秒好几次，不值得每次重画整页。 */
function useInstalls() {
  const [all, setAll] = useState<Partial<Record<AgentId, Progress>>>({});
  const [finished, setFinished] = useState<AgentId[]>([]);
  // 装好的 Codex 桌面版：单独一句话，带「打开 Codex」。
  const [app, setApp] = useState(false);
  useEffect(() => {
    let alive = true;
    const put = (p: Progress) => alive && setAll((now) => ({ ...now, [p.agent]: p }));
    const off = onInstall((p) => {
      put(p);
      if (!alive || p.phase !== "done") return;
      if (p.desktop) setApp(true);
      else setFinished((now) => [...now.filter((id) => id !== p.agent), p.agent]);
    });
    void api.installState().then((list) => list.forEach(put));
    return () => {
      alive = false;
      void off.then((stop) => stop());
    };
  }, []);
  return {
    all,
    finished,
    app,
    forget: () => {
      setFinished([]);
      setApp(false);
    },
  };
}

/** 首页和接入结果页共用：装、看进度、装好后的一句话。 */
export function useInstaller(snapshot: Snapshot, t: T) {
  const { all, finished, app, forget } = useInstalls();
  const [asking, setAsking] = useState<Agent[] | null>(null);
  // 装过一个就说明看过说明、PATH 也加好了，之后点一下直接装。Codex 还没选过桌面版还是命令行，照样先问。
  const familiar = snapshot.agents.some((a) => a.managed);
  const install = (agents: Agent[]) =>
    familiar && !agents.some((a) => a.app_installable) ? void api.install(agents.map((a) => a.id)) : setAsking(agents);
  const names = finished.filter((id) => COMMAND[id]);
  const label = (id: AgentId) => snapshot.agents.find((a) => a.id === id)?.label ?? id;
  return {
    progress: all,
    missing: snapshot.agents.filter((a) => a.installable && !active(all[a.id])),
    install,
    dialog: asking && <InstallDialog agents={asking} connected={snapshot.connected} t={t} onClose={() => setAsking(null)} />,
    notice: (names.length > 0 || app) && (
      <Notice
        action={
          <>
            {app && <Button kind="primary" onClick={() => void api.open("codex-app")}>{t.openApp}</Button>}
            <Button kind="quiet" onClick={forget}>{t.dismiss}</Button>
          </>
        }
      >
        {[app && t.appDone, names.length > 0 && t.installed(list(t, names.map(label)), list(t, names.map((id) => COMMAND[id]!)))]
          .filter(Boolean)
          .join(t.lang === "zh" ? "" : " ")}
      </Notice>
    ),
  };
}

/** 装不上时自己动手的那一行。复制了就当是要装：装好自动接入。 */
export function MirrorCommand({ agent, t }: { agent: Agent; t: T }) {
  if (!agent.mirror) return null;
  return <Copyable text={agent.mirror} t={t} onCopy={() => void api.expect(agent.id)} />;
}

const size = (bytes: number) =>
  bytes >= 1 << 30 ? `${(bytes / (1 << 30)).toFixed(1)} GB` : `${Math.max(1, Math.round(bytes / (1 << 20)))} MB`;

export function phaseText(t: T, p: Progress): string {
  if (p.phase === "failed") return reason(t, p.error);
  if (p.phase === "done") return t.done;
  if (p.phase === "queued") return t.phases.queued;
  if (p.phase === "downloading" && p.wait) return t.retryIn(p.wait);
  const runtime = p.desktop ? t.downloadingRuntime : t.downloadingNode;
  const label = p.phase === "downloading" && p.runtime ? runtime : t.phases[p.phase];
  if (p.phase !== "downloading" || !p.received) return `${label}…`;
  const amount = p.total ? `${size(p.received)} / ${size(p.total)}` : size(p.received);
  return `${label} ${amount}${p.source ? ` · ${p.source}` : ""}`;
}

/** 只有下载知道百分比；其余阶段是不确定进度，不假装。 */
export function Bar({ progress }: { progress: Progress }) {
  const known = progress.phase === "downloading" && progress.total;
  const value = known ? Math.min(100, (progress.received / progress.total!) * 100) : undefined;
  return (
    <span
      className={known ? "bar" : "bar busy"}
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={value === undefined ? undefined : Math.round(value)}
    >
      <i style={known ? { width: `${value}%` } : undefined} />
    </span>
  );
}

/** 装之前说清楚：装什么版本、多大、放哪、会不会改 shell 配置。Codex 先选桌面版（推荐）还是命令行。 */
function InstallDialog({ agents, connected, t, onClose }: { agents: Agent[]; connected: boolean; t: T; onClose: () => void }) {
  const [found, setFound] = useState<Planned[] | null>(null);
  const [error, setError] = useState("");
  const [round, setRound] = useState(0);
  const [desktop, setDesktop] = useState(true);
  const ids = agents.map((a) => a.id).join(",");
  useEffect(() => {
    let alive = true;
    api
      .installPlan(ids.split(",") as AgentId[])
      .then((plans) => alive && setFound(plans))
      .catch((cause) => alive && setError(String(cause)));
    return () => {
      alive = false;
    };
  }, [ids, round]);

  const title = t.installTitle(list(t, agents.map((a) => a.label)));
  const codex = found?.find((f) => f.agent === "codex" && (f.app || f.app_error));
  // 桌面版查不到时只能装命令行：选择跟着落到命令行上
  const useApp = Boolean(codex?.app) && (desktop || !codex?.plan);
  const cli = found?.flatMap((f) => (f.plan && !(useApp && f.agent === "codex") ? [f.plan] : [])) ?? [];
  const nothing = found && cli.length === 0 && !useApp;
  const why = error || (nothing ? found[0]?.error ?? found[0]?.app_error ?? "" : "");
  if (why)
    return (
      <Dialog title={title} onClose={onClose}>
        <Notice tone="bad">{why === "INSTALL_OFFLINE" ? t.planOffline : reason(t, why)}</Notice>
        {agents.some((a) => a.mirror) && <p className="note">{t.planMirror}</p>}
        {agents.map((agent) => <MirrorCommand key={agent.id} agent={agent} t={t} />)}
        <div className="row end">
          <Button kind="quiet" onClick={onClose}>{t.close}</Button>
          <Button
            kind="primary"
            autoFocus
            onClick={() => {
              setError("");
              setFound(null);
              setRound((n) => n + 1);
            }}
          >
            {t.retry}
          </Button>
        </div>
      </Dialog>
    );
  if (!found)
    return (
      <Dialog title={title} onClose={onClose}>
        <p className="muted busy-line"><span className="spinner" aria-hidden />{t.installPreparing}</p>
        <div className="row end"><Button kind="quiet" onClick={onClose}>{t.cancel}</Button></div>
      </Dialog>
    );

  // 其余的 Agent（以及选了命令行的 Codex 之外）照旧列出来
  const others = found.filter((f) => f !== codex);
  const app = useApp ? codex?.app ?? null : null;
  const path = cli[0]?.path;
  const notes = [
    cli.length > 0 && t.installSafe,
    cli.some((p) => p.needs_node) && t.installNode,
    app && t.appSafe,
    app?.admin && t.appAdmin,
    connected && t.installThen,
  ].filter(Boolean);
  return (
    <Dialog title={title} onClose={onClose}>
      {codex && (
        <>
          <p className="muted">{t.appChoice}</p>
          <ul className="list compact" role="radiogroup" aria-label={t.appChoice}>
            <li>
              <label className="pick">
                <input type="radio" name="codex-kind" checked={useApp} disabled={!codex.app} onChange={() => setDesktop(true)} />
                <AgentIcon id="codex" />
                <span className="agent-text">
                  <strong>{t.appOption} <span className="tag">{t.appRecommended}</span></strong>
                  <small>{codex.app ? t.appDesc(codex.app.version, size(codex.app.size)) : t.appFailed}</small>
                </span>
              </label>
            </li>
            <li>
              <label className="pick">
                <input type="radio" name="codex-kind" checked={!useApp} disabled={!codex.plan} onChange={() => setDesktop(false)} />
                <AgentIcon id="codex" />
                <span className="agent-text">
                  <strong>{t.cliOption}</strong>
                  <small>{codex.plan ? t.cliDesc(codex.plan.version, size(codex.plan.unpacked)) : t.planFailed}</small>
                </span>
              </label>
            </li>
          </ul>
          <p className="note">{t.appShared}</p>
        </>
      )}
      {others.length > 0 && (
        <>
          <p className="muted">{t.installFrom}</p>
          <ul className="list compact">
            {others.map(({ agent, plan }) => (
              <li key={agent}>
                <AgentIcon id={agent} />
                <span className="agent-text">
                  <strong>{agents.find((a) => a.id === agent)?.label} {plan?.version}</strong>
                  <small>{plan ? t.installSize(size(plan.unpacked)) : t.planFailed}</small>
                </span>
              </li>
            ))}
          </ul>
        </>
      )}
      <dl className="facts">
        {app && (
          <>
            <dt>{t.installWhere}</dt>
            <dd className={app.dir ? "path" : undefined}>{app.dir ? t.appWhere(app.dir) : t.appWhereWindows}</dd>
          </>
        )}
        {cli.length > 0 && path && (
          <>
            {!app && <dt>{t.installWhere}</dt>}
            {app && <dt>{t.cliOption}</dt>}
            <dd className="path">{cli[0].dir}</dd>
            <dt>PATH</dt>
            <dd>
              {path.kind === "ready" && t.installPathReady}
              {path.kind === "profile" && t.installPathProfile(path.file)}
              {path.kind === "user-path" && t.installPathUser}
            </dd>
          </>
        )}
      </dl>
      <p className="note">{notes.join(t.lang === "zh" ? "" : " ")}</p>
      <div className="row end">
        <Button kind="quiet" onClick={onClose}>{t.cancel}</Button>
        <Button
          kind="primary"
          autoFocus
          onClick={() => {
            const chosen = [...cli.map((p) => p.agent), ...(useApp ? (["codex"] as AgentId[]) : [])];
            void api.install(chosen, useApp);
            onClose();
          }}
        >
          {t.installStart}
        </Button>
      </div>
    </Dialog>
  );
}
