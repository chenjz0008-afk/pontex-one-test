// AI 工具箱：技能和 MCP 放一页，上面一个切换，默认是技能。
// 每一行右边是每个装着的 Agent 的图标：亮着 = 已启用，点一下就启用或停用（先变，后台再写）。
import { useCallback, useEffect, useMemo, useState } from "react";
import { api, type AgentId, type McpHealth, type Snapshot, type Toolbox as Data, type ToolMcp } from "./api";
import { list, reason, type T } from "./i18n";
import { AgentGlyph, Button, Dialog, Notice, useToast } from "./ui";

type Tab = "skills" | "mcp";

export function Toolbox({ snapshot, t, tab, onTab }: { snapshot: Snapshot; t: T; tab: Tab; onTab: (tab: Tab) => void }) {
  const [data, setData] = useState<Data | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [health, setHealth] = useState<Record<string, McpHealth | "checking">>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [broken, setBroken] = useState<Record<string, AgentId>>({});
  const [open, setOpen] = useState<string | null>(null);
  const [editing, setEditing] = useState<ToolMcp | "new" | null>(null);
  const toast = useToast();
  const label = useCallback((id: AgentId) => snapshot.agents.find((a) => a.id === id)?.label ?? id, [snapshot.agents]);

  const pull = useCallback(
    () =>
      api
        .toolbox()
        .then((next) => {
          setData(next);
          setFailed(null);
          return next;
        })
        .catch((cause) => {
          setFailed(reason(t, String(cause)));
          return null;
        }),
    [t],
  );
  useEffect(() => void pull(), [pull]);

  // MCP 页打开时自己检查一遍能不能连上，不用点。
  const check = useCallback((ids: string[]) => {
    if (!ids.length) return;
    setHealth((h) => ({ ...h, ...Object.fromEntries(ids.map((id) => [id, "checking" as const])) }));
    void api
      .mcpCheck(ids)
      .then((rows) => setHealth((h) => ({ ...h, ...Object.fromEntries(rows) })))
      .catch(() => setHealth((h) => ({ ...h, ...Object.fromEntries(ids.map((id) => [id, "failed" as const])) })));
  }, []);
  const mcpIds = useMemo(() => data?.mcps.map((m) => m.id).join("\n") ?? "", [data]);
  useEffect(() => {
    if (tab === "mcp" && mcpIds) check(mcpIds.split("\n").filter((id) => !health[id]));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab, mcpIds]);

  async function toggle(kind: Tab, id: string, agent: AgentId, on: boolean) {
    if (!data) return;
    const key = `${kind}:${id}:${agent}`;
    const flip = (enabled: boolean) => (row: { on: AgentId[] }) => ({ ...row, on: enabled ? [...row.on, agent] : row.on.filter((a) => a !== agent) });
    const apply = (enabled: boolean) =>
      setData((d) =>
        d && {
          ...d,
          skills: kind === "skills" ? d.skills.map((s) => (s.dir === id ? { ...s, ...flip(enabled)(s) } : s)) : d.skills,
          mcps: kind === "mcp" ? d.mcps.map((m) => (m.id === id ? { ...m, ...flip(enabled)(m) } : m)) : d.mcps,
        },
      );
    // 先变；写得慢（超过 0.3 秒）才转圈。
    apply(!on);
    setBroken((b) => Object.fromEntries(Object.entries(b).filter(([k]) => k !== `${kind}:${id}`)));
    const slow = setTimeout(() => setBusy(key), 300);
    try {
      await (kind === "skills" ? api.skillToggle(id, agent, !on) : api.mcpToggle(id, agent, !on));
    } catch (cause) {
      apply(on);
      setBroken((b) => ({ ...b, [`${kind}:${id}`]: agent }));
      toast({ bad: true, text: `${t.toggleFailed(label(agent))}：${reason(t, String(cause))}` });
    } finally {
      clearTimeout(slow);
      setBusy(null);
    }
  }

  async function remove(mcp: ToolMcp) {
    try {
      await api.mcpDelete(mcp.id);
      setOpen(null);
      await pull();
      toast({
        text: t.mcpDeleted(mcp.name),
        sub: t.mcpDeletedSub,
        undo: () => api.mcpSave(mcp.id, mcp.name, mcp.server, mcp.on).then(pull),
      });
    } catch (cause) {
      toast({ bad: true, text: reason(t, String(cause)) });
    }
  }

  const toggles = (kind: Tab, id: string, on: AgentId[], agents: AgentId[]) => (
    <span className="toggles">
      {agents.map((agent) => {
        const lit = on.includes(agent);
        const fail = broken[`${kind}:${id}`] === agent;
        return (
          <button
            key={agent}
            type="button"
            role="switch"
            aria-checked={lit}
            aria-label={`${label(agent)}`}
            className={`tg${lit ? " on" : ""}${busy === `${kind}:${id}:${agent}` ? " busy" : ""}${fail ? " fail" : ""}`}
            data-tip={fail ? t.toggleFailed(label(agent)) : t.toggleTip(label(agent), lit)}
            onClick={() => void toggle(kind, id, agent, lit)}
          >
            <AgentGlyph id={agent} />
          </button>
        );
      })}
    </span>
  );

  const skills = tab === "skills";
  return (
    <section>
      <h1 className="sr-only">{t.tabToolbox}</h1>
      <div className="page-head">
        <div className="segment ext-seg" role="tablist">
          <button role="tab" type="button" aria-selected={skills} className={skills ? "on" : undefined} onClick={() => onTab("skills")}>
            {t.skillsTab}
            <span className="count">{data?.skills.length ?? "–"}</span>
          </button>
          <button role="tab" type="button" aria-selected={!skills} className={skills ? undefined : "on"} onClick={() => onTab("mcp")}>
            {t.mcpTab}
            <span className="count">{data?.mcps.length ?? "–"}</span>
          </button>
        </div>
        <span className="agent-actions">
          {skills ? (
            <Button kind="plain" onClick={() => void api.open("skills")}>{t.browseSkills} ↗</Button>
          ) : (
            <Button kind="primary" disabled={!data} onClick={() => setEditing("new")}>{t.addMcp}</Button>
          )}
        </span>
      </div>
      {failed && <Notice tone="bad" action={<Button kind="quiet" onClick={() => void pull()}>{t.retry}</Button>}>{failed}</Notice>}
      {!skills && data && data.imported > 0 && !snapshot.prefs.toolbox_notice_seen && (
        <Notice action={<Button kind="quiet" onClick={() => void api.setPrefs({ ...snapshot.prefs, toolbox_notice_seen: true })}>{t.gotIt}</Button>}>
          {t.importedNotice(data.imported)}
        </Notice>
      )}
      {!data && !failed && <p className="note">{t.checking}</p>}
      {data && skills && (
        <>
          {data.skill_agents.length === 0 ? (
            <p className="note">{t.noAgentsYet}</p>
          ) : data.skills.length === 0 ? (
            <p className="note">{t.noSkills}</p>
          ) : (
            <>
              <div className="cols-head">{t.enabledIn}</div>
              <ul className="list">
                {data.skills.map((skill) => (
                  <li key={skill.dir}>
                    <div className="agent-row">
                      <div className="agent-main">
                        <span className="mini" aria-hidden>✦</span>
                        <span className="agent-text">
                          <strong>{skill.name}</strong>
                          {skill.description && <small>{skill.description}</small>}
                        </span>
                      </div>
                      <div className="agent-actions">{toggles("skills", skill.dir, skill.on, data.skill_agents)}</div>
                    </div>
                  </li>
                ))}
              </ul>
            </>
          )}
          <p className="note">{t.skillsNote}</p>
        </>
      )}
      {data && !skills && (
        <>
          {data.mcps.length === 0 ? (
            <p className="note">{t.noMcps}</p>
          ) : (
            <>
              <div className="cols-head">{t.enabledIn}</div>
              <ul className="list">
                {data.mcps.map((mcp) => {
                  const state = health[mcp.id] ?? "checking";
                  const tone = state === "ok" ? "ok" : state === "login" ? "warn" : state === "failed" ? "bad" : "pulse";
                  return (
                    <li key={mcp.id}>
                      <div className="agent-row">
                        <button type="button" className="agent-main" aria-expanded={open === mcp.id} onClick={() => setOpen(open === mcp.id ? null : mcp.id)}>
                          <span className="mini" aria-hidden>{isUrl(mcp) ? "⌁" : "›_"}</span>
                          <span className="agent-text">
                            <strong>
                              <span className={`dot ${tone}`} title={t.health[state]} aria-hidden />
                              {mcp.name}
                            </strong>
                            <small className="path">{target(mcp)}</small>
                          </span>
                        </button>
                        <div className="agent-actions">{toggles("mcp", mcp.id, mcp.on, data.mcp_agents)}</div>
                      </div>
                      {open === mcp.id && (
                        <div className="detail">
                          <dl>
                            <dt>{t.mcpTarget}</dt>
                            <dd className="path">{target(mcp)}</dd>
                            <dt>{t.enabledIn}</dt>
                            <dd>{list(t, mcp.on.map(label)) || "—"}</dd>
                            <dt>{t.mcpStatus}</dt>
                            <dd>{t.health[state]}</dd>
                          </dl>
                          <div className="row">
                            <Button kind="plain" onClick={() => setEditing(mcp)}>{t.edit}</Button>
                            <Button kind="danger" onClick={() => void remove(mcp)}>{t.remove}</Button>
                          </div>
                        </div>
                      )}
                    </li>
                  );
                })}
              </ul>
            </>
          )}
          <p className="note">{t.mcpNote}</p>
        </>
      )}
      {editing && data && (
        <McpDialog
          t={t}
          mcp={editing === "new" ? null : editing}
          agents={data.mcp_agents}
          label={label}
          onClose={() => setEditing(null)}
          onSaved={async (id, name, on) => {
            setEditing(null);
            await pull();
            check([id]);
            toast(
              editing === "new"
                ? { text: t.mcpAdded(name), sub: t.mcpAddedSub(list(t, on.map(label))) }
                : { text: t.mcpSaved(name) },
            );
          }}
        />
      )}
    </section>
  );
}

const isUrl = (mcp: ToolMcp) => typeof mcp.server.url === "string";

function target(mcp: ToolMcp): string {
  const server = mcp.server as { url?: string; command?: string; args?: unknown[] };
  if (server.url) return server.url;
  return [server.command, ...(Array.isArray(server.args) ? server.args : [])].filter(Boolean).join(" ");
}

const PRESETS: { id: string; cmd: string }[] = [
  { id: "fetch", cmd: "uvx mcp-server-fetch" },
  { id: "context7", cmd: "npx -y @upstash/context7-mcp" },
  { id: "playwright", cmd: "npx @playwright/mcp@latest" },
  { id: "memory", cmd: "npx -y @modelcontextprotocol/server-memory" },
  { id: "time", cmd: "uvx mcp-server-time" },
];

/** 按空格切命令，引号里的不切。 */
function words(text: string): string[] {
  const out: string[] = [];
  const re = /"([^"]*)"|'([^']*)'|(\S+)/g;
  for (let m = re.exec(text); m; m = re.exec(text)) out.push(m[1] ?? m[2] ?? m[3]);
  return out;
}

interface Parsed {
  name: string;
  server: Record<string, unknown> | null;
  pairs: string[];
  hint: string;
}

/** 一个输入框认三种东西：网址、命令、README 里的 JSON。名字自动起。 */
export function parseTarget(t: T, text: string): Parsed {
  const s = text.trim();
  if (!s) return { name: "", server: null, pairs: [], hint: "" };
  if (s.startsWith("{")) {
    try {
      const json = JSON.parse(s) as Record<string, unknown>;
      const servers = (json.mcpServers ?? json.servers ?? json) as Record<string, unknown>;
      const direct = "command" in servers || "url" in servers;
      const [name, value] = direct ? ["", servers] : Object.entries(servers)[0] ?? ["", {}];
      const server = value as Record<string, unknown>;
      const headers = typeof server.url === "string";
      const pairs = Object.keys(((headers ? server.headers : server.env) as Record<string, string>) ?? {});
      const normalized = headers ? { type: server.type ?? "http", ...server } : { type: server.type ?? "stdio", ...server };
      return { name, server: normalized, pairs, hint: t.mcpRecognized(name || target({ id: "", name: "", on: [], server }), pairs.length, headers) };
    } catch {
      return { name: "", server: null, pairs: [], hint: t.mcpJsonIncomplete };
    }
  }
  if (/^https?:\/\//i.test(s)) {
    const url = s.split(/\s+/)[0];
    let name = "";
    try {
      const host = new URL(url).hostname.split(".");
      name = host.length > 1 ? host[host.length - 2] : host[0];
    } catch {
      name = "";
    }
    return { name, server: { type: "http", url }, pairs: [], hint: "" };
  }
  const parts = words(s);
  const [command, ...args] = parts;
  const last = [...args].reverse().find((w) => !w.startsWith("-")) ?? command;
  const name = last
    .replace(/^@[^/]+\//, "")
    .replace(/@[^@]*$/, "")
    .replace(/^mcp-server-|-mcp$|^server-|^mcp-/g, "");
  return { name, server: { type: "stdio", command, args }, pairs: [], hint: "" };
}

function McpDialog(props: {
  t: T;
  mcp: ToolMcp | null;
  agents: AgentId[];
  label: (id: AgentId) => string;
  onClose: () => void;
  onSaved: (id: string, name: string, on: AgentId[]) => void;
}) {
  const { t, mcp, agents, label } = props;
  const [text, setText] = useState(mcp ? target(mcp) : "");
  const [name, setName] = useState(mcp?.name ?? "");
  const [named, setNamed] = useState(Boolean(mcp));
  const [preset, setPreset] = useState<string | null>(null);
  const existingPairs = useMemo(() => {
    const server = (mcp?.server ?? {}) as { env?: Record<string, string>; headers?: Record<string, string> };
    return Object.entries(server.headers ?? server.env ?? {});
  }, [mcp]);
  const [pairs, setPairs] = useState<[string, string][]>(existingPairs);
  const [on, setOn] = useState<AgentId[]>(mcp?.on ?? agents);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const parsed = useMemo(() => parseTarget(t, text), [t, text]);
  const url = typeof parsed.server?.url === "string";

  function change(next: string) {
    setText(next);
    setPreset(null);
    setError(null);
    const p = parseTarget(t, next);
    if (!named) setName(p.name);
    if (p.pairs.length) setPairs(p.pairs.map((k) => [k, ""]));
  }

  async function save() {
    if (!parsed.server) return setError(t.mcpNeedTarget);
    if (!name.trim()) return setError(reason(t, "MCP_NAME_EMPTY"));
    const filled = pairs.filter(([k]) => k.trim());
    // 编辑时值没改就留原来的（界面上不显示原值）。
    const keep = Object.fromEntries(existingPairs);
    const values = Object.fromEntries(filled.map(([k, v]) => [k.trim(), v || keep[k] || ""]));
    const server = { ...parsed.server };
    if (filled.length) server[url ? "headers" : "env"] = { ...((server[url ? "headers" : "env"] as object) ?? {}), ...values };
    setBusy(true);
    try {
      const id = await api.mcpSave(mcp?.id ?? null, name.trim(), server, on);
      props.onSaved(id, name.trim(), on);
    } catch (cause) {
      setError(reason(t, String(cause)));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog title={mcp ? t.mcpEditTitle(mcp.name) : t.addMcp} onClose={props.onClose} locked={busy}>
      {!mcp && (
        <div className="presets">
          {PRESETS.map((p) => (
            <button
              key={p.id}
              type="button"
              className={preset === p.id ? "preset on" : "preset"}
              onClick={() => {
                setText(p.cmd);
                setName(p.id);
                setNamed(false);
                setPreset(p.id);
                setError(null);
              }}
            >
              {p.id}
              <small>{t.presets[p.id]}</small>
            </button>
          ))}
        </div>
      )}
      <div className="field">
        <label htmlFor="px-mcp-target">{t.mcpTarget}</label>
        <textarea id="px-mcp-target" rows={2} autoFocus={!mcp} spellCheck={false} placeholder={t.mcpTargetHint} value={text} onChange={(e) => change(e.target.value)} />
        {parsed.hint && <p className="hint">{parsed.hint}</p>}
      </div>
      {(name || mcp) && (
        <div className="field">
          <label htmlFor="px-mcp-name">{t.mcpName}</label>
          <input
            id="px-mcp-name"
            type="text"
            spellCheck={false}
            value={name}
            onChange={(e) => {
              setName(e.target.value);
              setNamed(true);
            }}
          />
        </div>
      )}
      {parsed.server && (
        <div className="field">
          <span className="label">{url ? t.mcpHeaders : t.mcpEnv}</span>
          {pairs.map(([k, v], i) => (
            <div className="kv" key={i}>
              <input aria-label={t.mcpName} placeholder="NAME" value={k} onChange={(e) => setPairs(pairs.map((p, j) => (j === i ? [e.target.value, p[1]] : p)))} />
              <input type="password" aria-label="value" placeholder={existingPairs.some(([ek]) => ek === k) ? "••••••••" : "value"} value={v} onChange={(e) => setPairs(pairs.map((p, j) => (j === i ? [p[0], e.target.value] : p)))} />
              <button type="button" className="px" style={{ position: "static", opacity: 1 }} aria-label={t.remove} onClick={() => setPairs(pairs.filter((_, j) => j !== i))}>×</button>
            </div>
          ))}
          <button type="button" className="linkish" style={{ marginTop: 8 }} onClick={() => setPairs([...pairs, ["", ""]])}>＋ {url ? t.mcpHeaders : t.mcpEnv}</button>
        </div>
      )}
      <div className="field">
        <span className="label">{t.enabledIn}</span>
        <span className="toggles">
          {agents.map((agent) => {
            const lit = on.includes(agent);
            return (
              <button
                key={agent}
                type="button"
                role="switch"
                aria-checked={lit}
                aria-label={label(agent)}
                className={lit ? "tg on" : "tg"}
                data-tip={t.toggleTip(label(agent), lit)}
                onClick={() => setOn(lit ? on.filter((a) => a !== agent) : [...on, agent])}
              >
                <AgentGlyph id={agent} />
              </button>
            );
          })}
        </span>
      </div>
      {error && <Notice tone="bad">{error}</Notice>}
      <div className="row end">
        <Button kind="quiet" disabled={busy} onClick={props.onClose}>{t.cancel}</Button>
        <Button kind="primary" busy={busy} disabled={!parsed.server} onClick={() => void save()}>{mcp ? t.save : t.add}</Button>
      </div>
    </Dialog>
  );
}
