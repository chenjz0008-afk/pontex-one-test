import { useState } from "react";
import { api } from "./api";
import { reason, type T } from "./i18n";
import { Button, Copyable, Notice } from "./ui";

/** 打不开的时候：一句人话、一句放心话、一个主要动作；技术信息折起来。 */
export function Fatal({ error, t }: { error: { code: string; detail: string }; t: T }) {
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ bad: boolean; text: string } | null>(null);
  const outdated = error.code === "DB_TOO_NEW";
  async function retry() {
    setBusy(true);
    setMessage(null);
    await api.retry().catch((cause) => setMessage({ bad: true, text: reason(t, String(cause)) }));
    setBusy(false);
  }
  async function diagnose() {
    try {
      setMessage({ bad: false, text: t.diagnosed(await api.diagnose()) });
    } catch (cause) {
      setMessage({ bad: true, text: String(cause) });
    }
  }
  return (
    <section className="fatal" role="alert">
      <svg viewBox="0 0 48 48" width="56" height="56" aria-hidden>
        <circle cx="24" cy="24" r="23" className="fatal-ring" />
        <path d="M24 13v14" className="fatal-mark" />
        <circle cx="24" cy="34" r="2" className="fatal-dot" />
      </svg>
      <h1>{t.fatalTitle}</h1>
      <p className="lead">{t.fatal[error.code] ?? t.fatal.STARTUP}</p>
      <p className="muted">{t.fatalSafe}</p>
      <div className="row">
        {outdated ? (
          <Button kind="primary" autoFocus onClick={() => void api.open("download")}>{t.download}</Button>
        ) : (
          <Button kind="primary" autoFocus busy={busy} onClick={() => void retry()}>{t.fatalRetry}</Button>
        )}
        <Button kind="plain" onClick={() => void diagnose()}>{t.diagnose}</Button>
        <Button kind="quiet" onClick={() => void api.open("data")}>{t.openData}</Button>
      </div>
      {message && <Notice tone={message.bad ? "bad" : "info"}>{message.text}</Notice>}
      <details className="scripts">
        <summary>{t.fatalDetail}</summary>
        <Copyable text={`${error.code}: ${error.detail}`} t={t} />
      </details>
    </section>
  );
}
