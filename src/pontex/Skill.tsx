import { useEffect, useState } from "react";
import { api, type AgentId, type SkillInfo, type SkillInstalled, type SkillPreview } from "./api";
import { placeText, reason, type T } from "./i18n";
import { AgentIcon, Button, Dialog, Notice } from "./ui";

/** 从网站来的安装请求。这里只有确认和结果，没有技能目录。 */
export function SkillDialog({ slug, t, onClose, onManage }: { slug: string; t: T; onClose: () => void; onManage: () => void }) {
  const [info, setInfo] = useState<SkillInfo | null>(null);
  const [preview, setPreview] = useState<SkillPreview | null>(null);
  const [picked, setPicked] = useState<Set<AgentId>>(new Set());
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<SkillInstalled[] | null>(null);

  useEffect(() => {
    let alive = true;
    // 先拿名字和简介，再下载技能包：等的时候看到的就是对的名字。
    void api.skillInfo(slug).then((found) => alive && setInfo(found)).catch(() => undefined);
    api
      .skillPrepare(slug)
      .then((found) => {
        if (!alive) return;
        setPreview(found);
        setPicked(
          new Set(
            found.targets
              .filter((x) => x.installed && x.verified && x.state.kind !== "linked" && x.state.kind !== "up-to-date")
              .map((x) => x.agent),
          ),
        );
      })
      .catch((cause) => alive && setError(reason(t, String(cause))));
    return () => {
      alive = false;
    };
    // 换了 slug 会整个重新挂载。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [slug]);

  async function install() {
    setBusy(true);
    setError("");
    try {
      setDone(await api.skillCommit(slug, [...picked]));
    } catch (cause) {
      setError(reason(t, String(cause)));
    } finally {
      setBusy(false);
    }
  }

  if (error && !preview)
    return (
      <Dialog title={info?.title ?? t.skillTitle} onClose={onClose}>
        <Notice tone="bad">{error}</Notice>
        <div className="row end"><Button kind="primary" autoFocus onClick={onClose}>{t.close}</Button></div>
      </Dialog>
    );
  if (!preview)
    return (
      <Dialog title={info?.title ?? t.skillTitle} onClose={onClose}>
        <p className="muted busy-line"><span className="spinner" aria-hidden />{info ? t.skillFetching(info.version) : t.skillLoading}</p>
        {info?.description && <p>{info.description}</p>}
        <div className="row end"><Button kind="quiet" onClick={onClose}>{t.cancel}</Button></div>
      </Dialog>
    );

  const label = (id: AgentId) => preview.targets.find((x) => x.agent === id)?.label ?? id;
  if (done) {
    const ok = done.filter((d) => d.ok);
    const bad = done.filter((d) => !d.ok);
    return (
      <Dialog title={bad.length ? t.skillFailed : t.skillDone} onClose={onClose}>
        {ok.length > 0 && (
          <>
            <p>{t.skillDoneBody(preview.title, preview.version)}</p>
            <ul className="list compact">
              {ok.map((d) => (
                <li key={d.agent}>
                  <AgentIcon id={d.agent} />
                  <span className="agent-text"><strong>{label(d.agent)}</strong><small className="path">{d.dir}</small></span>
                </li>
              ))}
            </ul>
            <p className="note">{t.skillUsage}</p>
          </>
        )}
        {ok.some((d) => d.backup) && <p className="note">{t.skillBackup}</p>}
        {bad.map((d) => <Notice key={d.agent} tone="bad">{label(d.agent)}：{reason(t, d.error)}</Notice>)}
        <div className="row end">
          {ok.length > 0 && <Button kind="quiet" onClick={onManage}>{t.skillManage}</Button>}
          <Button kind="primary" autoFocus onClick={onClose}>{t.done}</Button>
        </div>
      </Dialog>
    );
  }

  return (
    <Dialog title={preview.title} onClose={onClose} locked={busy}>
      <p className="muted">{t.skillFrom(preview.version, preview.file_count)}</p>
      <p>{preview.description}</p>
      {preview.safety_note && <Notice tone="warn">{preview.safety_note}</Notice>}
      {preview.scripts.length > 0 && (
        <details className="scripts">
          <summary>{t.skillScripts(preview.scripts.length)}</summary>
          <ul>{preview.scripts.map((path) => <li key={path}><code>{path}</code></li>)}</ul>
        </details>
      )}
      <ul className="list compact">
        {preview.targets.map((target) => {
          const off = !target.installed || target.state.kind === "linked" || target.state.kind === "up-to-date";
          const hint = !target.installed
            ? t.skillNotInstalled
            : placeText(t, target.state, target.dir) || (target.verified ? "" : t.skillUnverified);
          return (
            <li key={target.agent}>
              <label className="pick">
                <input
                  type="checkbox"
                  disabled={off || busy}
                  checked={picked.has(target.agent)}
                  onChange={() =>
                    setPicked((now) => {
                      const next = new Set(now);
                      if (!next.delete(target.agent)) next.add(target.agent);
                      return next;
                    })
                  }
                />
                <AgentIcon id={target.agent} />
                <span className="agent-text"><strong>{target.label}</strong>{hint && <small>{hint}</small>}</span>
              </label>
            </li>
          );
        })}
      </ul>
      {error && <Notice tone="bad">{error}</Notice>}
      <div className="row end">
        <Button kind="quiet" disabled={busy} onClick={onClose}>{t.cancel}</Button>
        <Button kind="primary" busy={busy} disabled={picked.size === 0} onClick={() => void install()}>
          {busy ? t.installing : picked.size ? t.skillInstall(picked.size) : t.skillPick}
        </Button>
      </div>
    </Dialog>
  );
}
