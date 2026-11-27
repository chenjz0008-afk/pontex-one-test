import { useEffect, useState, type FormEvent, type ReactNode } from "react";
import { api, type Outcome, type Prefs, type SkillOwned, type Snapshot } from "./api";
import { list, outcomeText, reason, type T } from "./i18n";
import { Button, Dialog, Notice } from "./ui";

export function Settings({ snapshot, t, focus, onConnect }: { snapshot: Snapshot; t: T; focus?: "skills"; onConnect: () => void }) {
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [message, setMessage] = useState<{ bad: boolean; text: string } | null>(null);
  useEffect(() => {
    void api.autostart(null).then(setAutostart).catch(() => setAutostart(null));
  }, []);
  useEffect(() => {
    if (focus) document.getElementById(`px-${focus}`)?.scrollIntoView({ block: "center" });
  }, [focus]);
  const save = (change: Partial<Prefs>) => void api.setPrefs({ ...snapshot.prefs, ...change });

  return (
    <section>
      <h1>{t.settings}</h1>
      {message && <Notice tone={message.bad ? "bad" : "info"}>{message.text}</Notice>}
      <Group title={t.keySection}>
        {snapshot.connected ? (
          <Rotate snapshot={snapshot} t={t} onMessage={setMessage} />
        ) : (
          <Item label={t.notConnectedTitle}>
            <Button kind="primary" onClick={onConnect}>{t.connect}</Button>
          </Item>
        )}
      </Group>
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
      <Skills t={t} onMessage={setMessage} />
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

function Item({ label, hint, htmlFor, children }: { label: string; hint?: string; htmlFor?: string; children: ReactNode }) {
  return (
    <div className="item">
      <div>
        {htmlFor ? <label htmlFor={htmlFor}>{label}</label> : <span>{label}</span>}
        {hint && <small>{hint}</small>}
      </div>
      {children}
    </div>
  );
}

/** 换 Key：只更新正在用 PonteX 的，其余不变，所以不勾选任何 Agent。 */
function Rotate({ snapshot, t, onMessage }: { snapshot: Snapshot; t: T; onMessage: (m: { bad: boolean; text: string } | null) => void }) {
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    onMessage(null);
    try {
      const { outcomes } = await api.connect(key, snapshot.agents.map((a) => ({ agent: a.id, apply: false })));
      const failed = outcomes.filter((o: Outcome) => o.error);
      const changed = outcomes
        .filter((o) => o.result === "applied")
        .map((o) => snapshot.agents.find((a) => a.id === o.agent)?.label ?? o.agent);
      setKey("");
      onMessage(
        failed.length
          ? { bad: true, text: failed.map((o) => `${snapshot.agents.find((a) => a.id === o.agent)?.label}: ${outcomeText(t, o)}`).join("\n") }
          : { bad: false, text: changed.length ? t.rotatedBody(list(t, changed)) : t.rotated },
      );
    } catch (cause) {
      onMessage({ bad: true, text: reason(t, String(cause)) });
    } finally {
      setBusy(false);
    }
  }
  return (
    <form className="item" onSubmit={(event) => void submit(event)}>
      <div>
        <label htmlFor="px-rotate">{snapshot.key_hint}…</label>
        <small>{t.rotateHint}</small>
      </div>
      <div className="key-row small">
        <input id="px-rotate" type="password" autoComplete="off" spellCheck={false} placeholder="px-" value={key} onChange={(e) => setKey(e.target.value)} />
        <Button type="submit" busy={busy} disabled={!key.trim()}>{busy ? t.verifying : t.update}</Button>
      </div>
    </form>
  );
}

/** 从网站装进来的技能：装在哪、什么版本，能移除。 */
function Skills({ t, onMessage }: { t: T; onMessage: (m: { bad: boolean; text: string } | null) => void }) {
  const [owned, setOwned] = useState<SkillOwned[]>([]);
  const [removing, setRemoving] = useState<SkillOwned | null>(null);
  const [busy, setBusy] = useState(false);
  const pull = () => api.skills().then(setOwned).catch(() => setOwned([]));
  useEffect(() => void pull(), []);
  async function remove(skill: SkillOwned) {
    setBusy(true);
    try {
      for (const place of skill.places) await api.skillRemove(skill.slug, place.agent);
      onMessage({ bad: false, text: t.skillRemoved(skill.title) });
    } catch (cause) {
      onMessage({ bad: true, text: reason(t, String(cause)) });
    } finally {
      await pull();
      setBusy(false);
      setRemoving(null);
    }
  }
  return (
    <Group title={t.skillSection} id="px-skills">
      {owned.map((skill) => (
        <Item
          key={skill.slug}
          label={skill.title}
          hint={list(t, skill.places.map((p) => t.skillPlace(p.label, p.version) + (p.modified ? ` · ${t.skillEdited}` : "")))}
        >
          <Button onClick={() => setRemoving(skill)}>{t.skillRemove}</Button>
        </Item>
      ))}
      <Item label={owned.length ? t.skillBrowse : t.skillEmpty} hint={owned.length ? undefined : t.skillEmptyHint}>
        <Button onClick={() => void api.open("skills")}>skills.ponte-x.com</Button>
      </Item>
      {removing && (
        <Dialog title={t.skillRemoveTitle(removing.title)} onClose={() => setRemoving(null)} locked={busy}>
          <p>{t.skillRemoveConfirm(list(t, removing.places.map((p) => p.label)))}</p>
          <div className="row end">
            <Button kind="quiet" disabled={busy} autoFocus onClick={() => setRemoving(null)}>{t.cancel}</Button>
            <Button kind="danger" busy={busy} onClick={() => void remove(removing)}>{t.skillRemove}</Button>
          </div>
        </Dialog>
      )}
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
