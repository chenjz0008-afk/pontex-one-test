// 「有新版本」窗口：新版在后台下好、签名核对过之后才弹（点「立即更新」马上生效，不用等下载）。
// 每个版本弹一次；点「稍后」一天后再弹，菜单栏和设置里一直留着「重启更新」。
import { useState } from "react";
import { api, type SelfUpdate } from "./api";
import { reason, type T } from "./i18n";
import { Button, Dialog, Notice } from "./ui";

const isMac = navigator.userAgent.includes("Mac");

/** 发布时写的说明：一行一条，「- 」开头的去掉符号。 */
export function noteLines(notes: string | null | undefined): string[] {
  return (notes ?? "")
    .split(/\r?\n/)
    .map((line) => line.trim().replace(/^[-*•·]\s*/, ""))
    .filter(Boolean);
}

export function UpdateDialog({ update, t, onClose }: { update: Extract<SelfUpdate, { state: "ready" }>; t: T; onClose: () => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const lines = noteLines(t.lang === "zh" ? update.notes ?? update.notes_en : update.notes_en ?? update.notes);
  async function now() {
    setBusy(true);
    setError(null);
    try {
      // macOS：换好 .app 后重开；Windows：交给安装程序，装完自动重开。
      await api.updateInstall();
    } catch (cause) {
      setError(reason(t, String(cause)));
      setBusy(false);
    }
  }
  return (
    <Dialog title={t.updateTitle(update.version)} onClose={onClose} locked={busy}>
      <div className="update-dialog">
        <p className="muted">{t.updateLead(update.current)}</p>
        {lines.length ? (
          <ul className="update-notes">
            {lines.map((line) => <li key={line}>{line}</li>)}
          </ul>
        ) : (
          <p className="update-notes">{t.updateNoNotes}</p>
        )}
        <p className="note">{t.updateSafe}{isMac && ` ${t.updateOnQuit}`}</p>
        {error && <Notice tone="bad">{error}</Notice>}
        <div className="row end">
          <Button kind="quiet" disabled={busy} onClick={onClose}>{t.updateLater}</Button>
          <Button kind="primary" busy={busy} autoFocus onClick={() => void now()}>{busy ? t.updateInstalling : t.updateNow}</Button>
        </div>
      </div>
    </Dialog>
  );
}
