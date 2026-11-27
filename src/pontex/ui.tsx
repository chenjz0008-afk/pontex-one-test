import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import markSvg from "@/assets/brand/pontex-mark.svg?raw";
import wordmarkSvg from "@/assets/brand/pontex-wordmark.svg?raw";
import oneSvg from "@/assets/brand/pontex-one.svg?raw";
import claude from "@/icons/extracted/claude.svg";
import codex from "@/icons/extracted/openai.svg";
import gemini from "@/icons/extracted/gemini.svg";
import grok from "@/icons/extracted/grok.svg";
import workbuddy from "@/assets/brand/workbuddy-icon.svg";
import type { AgentId } from "./api";
import type { T } from "./i18n";

/** Codex、Grok 是单色图形（暗色下反白）；WorkBuddy 是自带底色的应用图标，两种主题都不动。 */
const ICONS: Record<AgentId, { src: string; kind?: "mono" | "app" }> = {
  "claude-code": { src: claude },
  codex: { src: codex, kind: "mono" },
  "gemini-cli": { src: gemini },
  "grok-build": { src: grok, kind: "mono" },
  workbuddy: { src: workbuddy, kind: "app" },
};

export function AgentIcon({ id }: { id: AgentId }) {
  const icon = ICONS[id];
  return (
    <span className={icon.kind ? `agent-icon ${icon.kind}` : "agent-icon"} aria-hidden>
      <img src={icon.src} alt="" />
    </span>
  );
}

/** 小号的 Agent 图标（开关、菜单里用）。 */
export function AgentGlyph({ id }: { id: AgentId }) {
  const icon = ICONS[id];
  return <img src={icon.src} alt="" className={icon.kind} draggable={false} />;
}

/** brand 里的真图形：颜色换成变量，暗色下跟着换；字标的黑色跟着正文色。 */
function inline(svg: string, className: string, fills: Record<string, string>) {
  let out = svg.replace(/<svg width="\d+(\.\d+)?" height="\d+(\.\d+)?"/, `<svg class="${className}" aria-hidden="true" focusable="false"`);
  for (const [from, to] of Object.entries(fills)) out = out.split(`fill="${from}"`).join(`fill="${to}"`);
  return out;
}
const MARK = inline(markSvg, "mark", { "#5D2AA6": "var(--mk1)", "#3C1867": "var(--mk2)", "#391765": "var(--mk3)" });
const WORDMARK = inline(wordmarkSvg, "wm", { black: "currentColor" });
const ONE = inline(oneSvg, "one", { "#7459D9": "var(--one)" });

export function Brand(_: { t: T }) {
  return (
    <span className="brand" data-tauri-drag-region aria-label="PonteX One" role="img">
      <span data-tauri-drag-region dangerouslySetInnerHTML={{ __html: MARK }} style={{ display: "contents" }} />
      <span className="words" data-tauri-drag-region>
        <span dangerouslySetInnerHTML={{ __html: WORDMARK }} style={{ display: "contents" }} />
        <span dangerouslySetInnerHTML={{ __html: ONE }} style={{ display: "contents" }} />
      </span>
    </span>
  );
}

// ---------- 底部提示条 ----------

export interface ToastMessage {
  text: string;
  /** 第二行小字。 */
  sub?: string;
  bad?: boolean;
  undo?: () => void | Promise<unknown>;
}

const ToastContext = createContext<(message: ToastMessage | null) => void>(() => undefined);
export const useToast = () => useContext(ToastContext);

/** 全窗口一条：新的顶掉旧的。能撤销的多留一会儿，出错的留到点掉。 */
export function ToastProvider({ t, children }: { t: T; children: ReactNode }) {
  const [message, setMessage] = useState<ToastMessage | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>();
  const show = useCallback((next: ToastMessage | null) => {
    clearTimeout(timer.current);
    setMessage(next);
    if (next && !next.bad) timer.current = setTimeout(() => setMessage(null), next.undo ? 6500 : next.sub ? 4500 : 2600);
  }, []);
  useEffect(() => () => clearTimeout(timer.current), []);
  return (
    <ToastContext.Provider value={show}>
      {children}
      {message && (
        <div className={message.bad ? "toast bad" : "toast"} role={message.bad ? "alert" : "status"}>
          <span>
            {message.text}
            {message.sub && <small>{message.sub}</small>}
          </span>
          {message.undo && (
            <button type="button" onClick={() => { const undo = message.undo!; show(null); void undo(); }}>{t.undo}</button>
          )}
          {message.bad && (
            <button type="button" onClick={() => show(null)}>{t.dismiss}</button>
          )}
        </div>
      )}
    </ToastContext.Provider>
  );
}

export function Button(props: {
  kind?: "primary" | "plain" | "quiet" | "danger";
  disabled?: boolean;
  busy?: boolean;
  autoFocus?: boolean;
  type?: "button" | "submit";
  onClick?: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type={props.type ?? "button"}
      className={`btn ${props.kind ?? "plain"}`}
      disabled={props.disabled || props.busy}
      aria-busy={props.busy || undefined}
      autoFocus={props.autoFocus}
      onClick={props.onClick}
    >
      {props.busy && <span className="spinner" aria-hidden />}
      {props.children}
    </button>
  );
}

export function Notice({ tone = "info", children, action }: { tone?: "info" | "warn" | "bad"; children: ReactNode; action?: ReactNode }) {
  return (
    <div className={`notice ${tone}`} role={tone === "bad" ? "alert" : "status"}>
      <span>{children}</span>
      {action}
    </div>
  );
}

/** 一行可以复制的内容。 */
export function Copyable({ text, t, onCopy }: { text: string; t: T; onCopy?: () => void }) {
  const [done, setDone] = useState(false);
  const code = useRef<HTMLElement>(null);
  useEffect(() => {
    if (!done) return;
    const timer = setTimeout(() => setDone(false), 2000);
    return () => clearTimeout(timer);
  }, [done]);
  async function copy() {
    onCopy?.();
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // 剪贴板接口不可用：选中那一行，走系统的复制。
      const selection = window.getSelection();
      selection?.selectAllChildren(code.current!);
      if (!document.execCommand("copy")) return;
      selection?.removeAllRanges();
    }
    setDone(true);
  }
  return (
    <div className="copyable">
      <code ref={code}>{text}</code>
      <Button onClick={() => void copy()}>{done ? t.copied : t.copy}</Button>
    </div>
  );
}

/** 用原生 dialog：焦点留在框内，Esc 关闭。 */
export function Dialog({ title, onClose, locked, children }: { title: string; onClose: () => void; locked?: boolean; children: ReactNode }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
  }, []);
  return (
    <dialog
      ref={ref}
      aria-label={title}
      onCancel={(event) => {
        event.preventDefault();
        if (!locked) onClose();
      }}
    >
      <h2>{title}</h2>
      {children}
    </dialog>
  );
}
