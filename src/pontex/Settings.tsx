import { useEffect, useRef, useState, type ReactNode } from "react";
import { api, type Folders, type KeyReport, type Outcome, type Prefs, type Snapshot } from "./api";
import { list, outcomeText, reason, type T } from "./i18n";
import { isWindows } from "./Launch";
import { UpdateDialog } from "./Update";
import { Button, Dialog, Notice, useToast } from "./ui";

export function Settings({ snapshot, t, onConnect }: { snapshot: Snapshot; t: T; onConnect: () => void }) {
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [message, setMessage] = useState<{ bad: boolean; text: string } | null>(null);
  useEffect(() => {
    void api.autostart(null).then(setAutostart).catch(() => setAutostart(null));
  }, []);
  const [folders, setFolders] = useState<Folders | null>(null);
  useEffect(() => {
    void api.folders().then(setFolders).catch(() => setFolders(null));
  }, [snapshot.prefs.folders_dir]);
  const save = (change: Partial<Prefs>) => void api.setPrefs({ ...snapshot.prefs, ...change });

  return (
    <section>
      <h1 className="sr-only">{t.settings}</h1>
      {message && <Notice tone={message.bad ? "bad" : "info"}>{message.text}</Notice>}
      <Group title="PonteX">
        {snapshot.connected ? (
          <KeyRow snapshot={snapshot} t={t} />
        ) : (
          <Item label={t.notConnectedTitle}>
            <Button kind="primary" onClick={onConnect}>{t.connect}</Button>
          </Item>
        )}
      </Group>
      <Group title={t.foldersSection}>
        <Item label={t.foldersDefault} hint={t.foldersDefaultHint(isWindows ? "OneDrive" : "iCloud")} path={folders?.default_dir}>
          <Button
            onClick={() =>
              void api.pickFolder(folders?.default_dir ?? null).then((dir) => dir && save({ folders_dir: dir }))
            }
          >
            {t.change}
          </Button>
        </Item>
        <Item label={t.openFolderToo} hint={t.openFolderTooHint(t.finder(isWindows))} htmlFor="px-open-folder">
          <input id="px-open-folder" type="checkbox" role="switch" checked={snapshot.prefs.open_folder} onChange={(e) => save({ open_folder: e.target.checked })} />
        </Item>
        {folders && folders.terminals.length > 1 && (
          <Item label={t.terminal} hint={t.terminalHint} htmlFor="px-terminal">
            <select
              id="px-terminal"
              value={snapshot.prefs.terminal && folders.terminals.includes(snapshot.prefs.terminal) ? snapshot.prefs.terminal : folders.terminals[0]}
              onChange={(e) => save({ terminal: e.target.value === folders.terminals[0] ? null : e.target.value })}
            >
              {folders.terminals.map((id) => <option key={id} value={id}>{t.terminals[id] ?? id}</option>)}
            </select>
          </Item>
        )}
      </Group>
      <Updates snapshot={snapshot} t={t} save={save} />
      <Group title={t.generalSection}>
        <Item label={t.language} htmlFor="px-language">
          <select id="px-language" value={snapshot.prefs.language ?? ""} onChange={(e) => save({ language: e.target.value || null })}>
            <option value="">{t.languageAuto}</option>
            <option value="zh">简体中文</option>
            <option value="en">English</option>
          </select>
        </Item>
        <Item label={t.autostart} htmlFor="px-autostart">
          <input
            id="px-autostart"
            type="checkbox"
            role="switch"
            checked={autostart ?? false}
            disabled={autostart === null}
            onChange={(e) =>
              void api
                .autostart(e.target.checked)
                .then(setAutostart)
                .catch((cause) => setMessage({ bad: true, text: String(cause) }))
            }
          />
        </Item>
        <Item label={t.closeToTray} htmlFor="px-tray">
          <input id="px-tray" type="checkbox" role="switch" checked={snapshot.prefs.close_to_tray} onChange={(e) => save({ close_to_tray: e.target.checked })} />
        </Item>
      </Group>
      <Group title={t.helpSection}>
        <Item label={t.docs}><Button onClick={() => void api.open("docs")}>docs.ponte-x.com</Button></Item>
        <Item label={t.diagnose} hint={t.diagnoseHint}>
          <Button onClick={() => void api.diagnose().then((p) => setMessage({ bad: false, text: t.diagnosed(p) })).catch((c) => setMessage({ bad: true, text: String(c) }))}>{t.diagnose}</Button>
        </Item>
        <Item label={t.openData}><Button onClick={() => void api.open("data")}>{t.openData}</Button></Item>
      </Group>
      {snapshot.connected && (
        <Group title={t.disconnectSection}>
          <Item label={t.disconnect} hint={t.disconnectHint}>
            <Button kind="danger" onClick={() => setConfirming(true)}>{t.disconnect}</Button>
          </Item>
        </Group>
      )}
      <Group title={t.aboutSection}>
        <Item label={t.about(snapshot.version, snapshot.engine)} hint={t.license}>
          <Button onClick={() => void api.open("upstream")}>CC Switch</Button>
        </Item>
      </Group>
      {confirming && <Disconnect snapshot={snapshot} t={t} onClose={() => setConfirming(false)} />}
    </section>
  );
}

function Group({ title, id, children }: { title: string; id?: string; children: ReactNode }) {
  return (
    <fieldset id={id}>
      <legend>{title}</legend>
      {children}
    </fieldset>
  );
}

function Item({ label, hint, path, htmlFor, children }: { label: string; hint?: string; path?: string; htmlFor?: string; children: ReactNode }) {
  return (
    <div className="item">
      <div>
        {htmlFor ? <label htmlFor={htmlFor}>{label}</label> : <span>{label}</span>}
        {path && <small className="path">{path}</small>}
        {hint && <small>{hint}</small>}
      </div>
      {children}
    </div>
  );
}

/** Key：平时一行；点「更换」就地展开。粘贴后自动检查，有效才能保存；只更新正在用 PonteX 的 Agent。 */
function KeyRow({ snapshot, t }: { snapshot: Snapshot; t: T }) {
  const [editing, setEditing] = useState(false);
  const [key, setKey] = useState("");
  const [check, setCheck] = useState<{ state: "checking" | "ok" | "bad"; why?: string } | null>(null);
  const [saving, setSaving] = useState(false);
  const toast = useToast();
  const seq = useRef(0);
  const using = snapshot.agents.filter((a) => a.current === "pontex").map((a) => a.label);

  useEffect(() => {
    const value = key.trim();
    if (!value) return setCheck(null);
    const mine = ++seq.current;
    setCheck({ state: "checking" });
    const timer = setTimeout(() => {
      void api
        .checkKey(value)
        .then((report: KeyReport) => {
          if (mine !== seq.current) return;
          setCheck(report.usable ? { state: "ok" } : { state: "bad", why: reason(t, report.reason ?? "INVALID_API_KEY") });
        })
        .catch((cause) => mine === seq.current && setCheck({ state: "bad", why: reason(t, String(cause)) }));
    }, 450);
    return () => clearTimeout(timer);
  }, [key, t]);

  async function save() {
    if (check?.state !== "ok" || saving) return;
    setSaving(true);
    try {
      const { outcomes } = await api.connect(key.trim(), snapshot.agents.map((a) => ({ agent: a.id, apply: false })));
      const failed = outcomes.filter((o: Outcome) => o.error);
      if (failed.length) {
        toast({ bad: true, text: failed.map((o) => `${snapshot.agents.find((a) => a.id === o.agent)?.label}：${outcomeText(t, o)}`).join("\n") });
        return;
      }
      const changed = outcomes.filter((o) => o.result === "applied").length;
      setEditing(false);
      setKey("");
      toast({ text: t.keySaved(changed), sub: t.restartAll });
    } catch (cause) {
      setCheck({ state: "bad", why: reason(t, String(cause)) });
    } finally {
      setSaving(false);
    }
  }

  if (!editing)
    return (
      <Item label="API Key" path={`${snapshot.key_hint}…`}>
        <span className="key-side">
          <span className="kstat"><span className="dot ok" aria-hidden />{t.keyEnabled}</span>
          <Button onClick={() => setEditing(true)}>{t.keyChange}</Button>
        </span>
      </Item>
    );

  const line =
    check?.state === "checking" ? (
      <><span className="spinner" aria-hidden />{t.checking}</>
    ) : check?.state === "ok" ? (
      <><span className="kok">{t.keyValid}</span>{"　"}{t.keyWillApply(list(t, using))}</>
    ) : check?.state === "bad" ? (
      <span className="kbad">{check.why}</span>
    ) : (
      <>{t.keyPaste}{"　"}<button type="button" className="linkish" onClick={() => void api.open("key")}>{t.keyGet} ↗</button></>
    );
  return (
    <div className="item key-edit">
      <label htmlFor="px-key">{t.keyChangeTitle}</label>
      <input
        id="px-key"
        className={check?.state === "bad" ? "kin bad" : check?.state === "ok" ? "kin good" : "kin"}
        type="password"
        autoFocus
        autoComplete="off"
        spellCheck={false}
        placeholder="px-…"
        value={key}
        aria-invalid={check?.state === "bad"}
        onChange={(e) => setKey(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.nativeEvent.isComposing) {
            e.preventDefault();
            void save();
          }
          if (e.key === "Escape") {
            setEditing(false);
            setKey("");
          }
        }}
      />
      <p className="kline" role="status">{line}</p>
      <div className="kbtns">
        <Button kind="quiet" disabled={saving} onClick={() => { setEditing(false); setKey(""); }}>{t.cancel}</Button>
        <Button kind="primary" busy={saving} disabled={check?.state !== "ok"} onClick={() => void save()}>{t.save}</Button>
      </div>
    </div>
  );
}

/** 更新：现在的版本、检查更新 / 重启更新、自动更新开关。 */
function Updates({ snapshot, t, save }: { snapshot: Snapshot; t: T; save: (change: Partial<Prefs>) => void }) {
  const [checking, setChecking] = useState(false);
  const [open, setOpen] = useState(false);
  const toast = useToast();
  const update = snapshot.update;
  const ready = update.state === "ready" ? update : null;
  const hint = ready ? t.updateReady(ready.version) : update.state === "downloading" ? t.updateStatus.downloading : t.updateStatus[update.state] ?? "";
  return (
    <Group title={t.updateSection}>
      {update.required && <Notice tone="warn">{t.updateRequired(update.required)}</Notice>}
      <Item label={t.updateCurrent(snapshot.version)} hint={hint || undefined}>
        {ready ? (
          <span className="key-side">
            <button type="button" className="linkish" onClick={() => setOpen(true)}>{t.updateView}</button>
            <Button kind="primary" onClick={() => setOpen(true)}>{t.updateRestart}</Button>
          </span>
        ) : (
          <Button
            busy={checking || update.state === "checking" || update.state === "downloading"}
            disabled={update.state === "unsupported"}
            onClick={() => {
              setChecking(true);
              void api
                .updateCheck()
                .then((next) => next.state === "ready" && setOpen(true))
                .catch((cause) => toast({ bad: true, text: reason(t, String(cause)) }))
                .finally(() => setChecking(false));
            }}
          >
            {t.updateCheck}
          </Button>
        )}
      </Item>
      <Item label={t.autoUpdate} hint={t.autoUpdateHint} htmlFor="px-auto-update">
        <input id="px-auto-update" type="checkbox" role="switch" checked={snapshot.prefs.auto_update} onChange={(e) => save({ auto_update: e.target.checked })} />
      </Item>
      {open && ready && <UpdateDialog update={ready} t={t} onClose={() => setOpen(false)} />}
    </Group>
  );
}

function Disconnect({ snapshot, t, onClose }: { snapshot: Snapshot; t: T; onClose: () => void }) {
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState<Outcome[]>([]);
  async function go() {
    setBusy(true);
    try {
      const outcomes = await api.disconnect(null);
      const bad = outcomes.filter((o) => o.error);
      if (bad.length) setFailed(bad);
      else onClose();
    } catch (cause) {
      setFailed([{ agent: "codex", result: "failed", error: String(cause), locked_sessions: null }]);
    } finally {
      setBusy(false);
    }
  }
  return (
    <Dialog title={t.disconnect} onClose={onClose} locked={busy}>
      <p>{t.disconnectConfirm}</p>
      {failed.map((o) => (
        <Notice key={o.agent} tone="bad">{snapshot.agents.find((a) => a.id === o.agent)?.label}：{reason(t, o.error)}</Notice>
      ))}
      <div className="row end">
        <Button kind="quiet" disabled={busy} autoFocus onClick={onClose}>{t.cancel}</Button>
        <Button kind="danger" busy={busy} onClick={() => void go()}>{busy ? t.disconnecting : failed.length ? t.retry : t.disconnect}</Button>
      </div>
    </Dialog>
  );
}
