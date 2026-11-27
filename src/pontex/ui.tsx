import { useEffect, useRef, useState, type ReactNode } from "react";
import mark from "@/assets/pontex-mark.svg";
import claude from "@/icons/extracted/claude.svg";
import codex from "@/icons/extracted/openai.svg";
import gemini from "@/icons/extracted/gemini.svg";
import grok from "@/icons/extracted/grok.svg";
import workbuddy from "./workbuddy.svg";
import type { AgentId } from "./api";
import type { T } from "./i18n";

const ICONS: Record<AgentId, { src: string; mono?: boolean }> = {
  "claude-code": { src: claude },
  codex: { src: codex, mono: true },
  "gemini-cli": { src: gemini },
  "grok-build": { src: grok, mono: true },
  workbuddy: { src: workbuddy },
};

export function AgentIcon({ id }: { id: AgentId }) {
  const icon = ICONS[id];
  return (
    <span className={icon.mono ? "agent-icon mono" : "agent-icon"} aria-hidden>
      <img src={icon.src} alt="" />
    </span>
  );
}

export function Brand({ t }: { t: T }) {
  return (
    <span className="brand" data-tauri-drag-region>
      <img src={mark} alt="" draggable={false} />
      <strong data-tauri-drag-region>PonteX One</strong>
      {t.lang === "zh" && <span data-tauri-drag-region>经纬即连</span>}
    </span>
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
