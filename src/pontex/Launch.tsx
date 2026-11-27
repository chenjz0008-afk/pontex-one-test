// 在文件夹里打开 Agent：Agent 那一行最右边的「打开」，和它下面的弹出层。
// 给不会用终端的人：选一个文件夹，PonteX 先打开文件夹、再打开终端，Agent 已经在里面启动好。
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { api, type Agent, type Folder, type Folders } from "./api";
import { reason, type T } from "./i18n";
import { Button, useToast } from "./ui";

const isWindows = navigator.userAgent.includes("Windows");

/** 「打开」做什么：在终端里（弹文件夹）、打开应用，或者没有。 */
export function opensIn(agent: Agent): "terminal" | "app" | null {
  if (agent.install.state !== "installed") return null;
  if (agent.id === "workbuddy") return "app";
  // Codex 只装了桌面版：直接打开应用，它自己有选文件夹的地方。
  if (agent.id === "codex" && agent.app && (!agent.install.version || agent.install.version === agent.app)) return "app";
  return "terminal";
}

const base = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;

/** 文件夹显示成名字；系统文件夹用系统里的叫法。 */
function dirName(t: T, path: string) {
  const name = base(path);
  return t.dirNames[name] ?? name;
}

/** 名字的问题先在这里说，和 Rust 的检查一致；重名要等创建时 Rust 回答。 */
function nameProblem(name: string): string | null {
  const trimmed = name.trim();
  if (!trimmed) return null;
  if (/[\\/:*?"<>|]/.test(trimmed) || /\p{Cc}/u.test(trimmed)) return "FOLDER_NAME_CHARS";
  if (trimmed.startsWith(".")) return "FOLDER_NAME_DOT";
  if ([...trimmed].length > 120) return "FOLDER_NAME_LONG";
  return null;
}

export function OpenButton({ agent, t, firstTime, disabled }: { agent: Agent; t: T; firstTime: boolean; disabled?: boolean }) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const toast = useToast();
  const kind = opensIn(agent);
  const close = useCallback(() => {
    setOpen(false);
    anchor.current?.focus();
  }, []);
  if (!kind) return null;

  async function openApp() {
    setBusy(true);
    try {
      await api.open(agent.id === "workbuddy" ? "workbuddy" : "codex-app");
    } catch (cause) {
      toast({ bad: true, text: reason(t, String(cause)) });
    } finally {
      setBusy(false);
    }
  }

  async function launch(path: string) {
    setOpen(false);
    setBusy(true);
    try {
      await api.launch(agent.id, path);
      toast({ text: t.launched(agent.label, base(path)), sub: t.launchedSub });
    } catch (cause) {
      toast({ bad: true, text: reason(t, String(cause)) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <button
        ref={anchor}
        type="button"
        className="btn plain open-btn"
        aria-haspopup={kind === "terminal" ? "menu" : undefined}
        aria-expanded={kind === "terminal" ? open : undefined}
        disabled={disabled || busy}
        aria-busy={busy || undefined}
        onClick={() => (kind === "app" ? void openApp() : setOpen(!open))}
      >
        {busy ? <span className="spinner" aria-hidden /> : t.open}
      </button>
      {open && (
        <FolderMenu agent={agent} t={t} anchor={anchor} firstTime={firstTime} onClose={close} onLaunch={(path) => void launch(path)} />
      )}
    </>
  );
}

function FolderMenu(props: {
  agent: Agent;
  t: T;
  anchor: React.RefObject<HTMLButtonElement>;
  firstTime: boolean;
  onClose: () => void;
  onLaunch: (path: string) => void;
}) {
  const { agent, t, anchor, onClose, onLaunch } = props;
  const [data, setData] = useState<Folders | null>(null);
  const [mode, setMode] = useState<"list" | "new">("list");
  const [query, setQuery] = useState("");
  const [name, setName] = useState("");
  const [parent, setParent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [style, setStyle] = useState<React.CSSProperties>({ visibility: "hidden" });
  const menu = useRef<HTMLDivElement>(null);
  const toast = useToast();
  const pull = useCallback(() => api.folders().then(setData).catch(() => setData({ recents: [], default_dir: "", terminals: [] })), []);
  useEffect(() => void pull(), [pull]);

  // 挂在按钮下面；下面放不下就往上开；上下都放不下就挪到刚好放得下（和系统菜单一样）。
  useLayoutEffect(() => {
    const place = () => {
      const button = anchor.current?.getBoundingClientRect();
      const height = menu.current?.offsetHeight ?? 0;
      if (!button) return;
      const below = button.bottom + 6;
      const above = button.top - 6 - height;
      let top = below;
      if (below + height > window.innerHeight - 12) top = above >= 60 ? above : Math.max(60, window.innerHeight - 12 - height);
      setStyle({ top, right: Math.max(12, window.innerWidth - button.right) });
    };
    place();
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [anchor, data, mode, query, error]);

  // 点到外面、按 Esc 就收起（新建时 Esc 先回到列表）。
  useEffect(() => {
    const down = (event: MouseEvent) => {
      const target = event.target as Node;
      if (!menu.current?.contains(target) && !anchor.current?.contains(target)) onClose();
    };
    document.addEventListener("mousedown", down);
    return () => document.removeEventListener("mousedown", down);
  }, [anchor, onClose]);
  const onKey = (event: KeyboardEvent) => {
    if (event.key !== "Escape") return;
    event.stopPropagation();
    if (mode === "new") setMode("list");
    else onClose();
  };

  const recents = data?.recents ?? [];
  const q = query.trim().toLowerCase();
  const shown = q ? recents.filter((f) => f.path.toLowerCase().includes(q)) : recents;

  async function forget(folder: Folder) {
    const removed = await api.forgetFolder(folder.path).catch(() => null);
    await pull();
    if (!removed) return;
    const [index, recent] = removed;
    toast({
      text: t.forgotFolder(folder.name),
      sub: t.folderUntouched,
      undo: () => api.restoreFolder(index, recent).then(pull),
    });
  }

  async function pick() {
    const path = await api.pickFolder(data?.default_dir ?? null).catch((cause) => {
      toast({ bad: true, text: reason(t, String(cause)) });
      return null;
    });
    if (path) onLaunch(path);
  }

  async function changeParent() {
    const path = await api.pickFolder(parent ?? data?.default_dir ?? null).catch(() => null);
    if (path) {
      setParent(path);
      setError(null);
    }
  }

  const problem = nameProblem(name) ?? error;
  async function create() {
    if (!name.trim() || problem || creating) return;
    setCreating(true);
    try {
      const path = await api.newFolder(name, parent);
      onLaunch(path);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setCreating(false);
    }
  }

  if (mode === "new") {
    const where = parent ?? data?.default_dir ?? "";
    return (
      <div ref={menu} className="pop" style={style} onKeyDown={onKey}>
        <div className="pop-new">
          <button type="button" className="back" onClick={() => setMode("list")}>‹ {t.back}</button>
          <input
            autoFocus
            placeholder={t.newFolderName}
            value={name}
            spellCheck={false}
            autoComplete="off"
            aria-invalid={Boolean(problem)}
            onChange={(e) => {
              setName(e.target.value);
              setError(null);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.nativeEvent.isComposing) {
                e.preventDefault();
                void create();
              }
            }}
          />
          <span className={problem ? "where bad" : "where"} title={where}>
            {problem ? (
              reason(t, problem)
            ) : (
              <>
                {t.newFolderIn} <b>{dirName(t, where)}</b> {t.newFolderInTail}
                {"　"}
                <button type="button" className="linkish" onClick={() => void changeParent()}>{t.changeOnce}</button>
              </>
            )}
          </span>
          <div className="go-row">
            <Button kind="primary" busy={creating} disabled={!name.trim() || Boolean(problem)} onClick={() => void create()}>
              {t.createAndOpen}
            </Button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div ref={menu} className="pop" style={style} role="menu" aria-label={t.openWhere(agent.label)} onKeyDown={onKey}>
      <p className="pop-h">{t.openWhere(agent.label)}</p>
      {recents.length > 6 && (
        <input
          className="pop-q"
          autoFocus
          placeholder={t.searchFolders(recents.length)}
          value={query}
          spellCheck={false}
          autoComplete="off"
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.nativeEvent.isComposing && shown[0]) {
              e.preventDefault();
              onLaunch(shown[0].path);
            }
          }}
        />
      )}
      {recents.length > 0 && (
        <>
          <div className={shown.length > 6 ? "pop-list scrolls" : "pop-list"}>
            {shown.map((folder, index) => (
              <div className="pr" key={folder.path}>
                <button
                  type="button"
                  role="menuitem"
                  className="pi"
                  title={folder.path}
                  autoFocus={recents.length <= 6 && index === 0}
                  onClick={() => onLaunch(folder.path)}
                >
                  <FolderIcon />
                  <span className="nm">{folder.name}</span>
                  <small>{dirName(t, folder.parent)}</small>
                </button>
                <button type="button" className="px" aria-label={t.forgetFolder} title={t.forgetFolder} onClick={() => void forget(folder)}>×</button>
              </div>
            ))}
            {!shown.length && <p className="pop-none">{t.noFolderMatch(query)}</p>}
          </div>
          <hr />
        </>
      )}
      <button type="button" role="menuitem" className="pi" autoFocus={!recents.length} onClick={() => void pick()}>
        <PickIcon />
        <span className="nm">{t.pickFolder}</span>
      </button>
      <button
        type="button"
        role="menuitem"
        className="pi"
        onClick={() => {
          setMode("new");
          setName("");
          setParent(null);
          setError(null);
        }}
      >
        <PlusIcon />
        <span className="nm">{t.newFolder}</span>
      </button>
      {props.firstTime && <p className="pop-foot">{t.firstLaunch(agent.label)}</p>}
    </div>
  );
}

function FolderIcon() {
  return (
    <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true">
      <path d="M1.5 4.2c0-.8.6-1.4 1.4-1.4h3.3l1.5 1.6h5.4c.8 0 1.4.6 1.4 1.4v6.1c0 .8-.6 1.4-1.4 1.4H2.9c-.8 0-1.4-.6-1.4-1.4z" fill="none" stroke="currentColor" strokeWidth="1.3" />
    </svg>
  );
}

function PickIcon() {
  return (
    <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true">
      <path d="M1.5 4.2c0-.8.6-1.4 1.4-1.4h3.3l1.5 1.6h5.4c.8 0 1.4.6 1.4 1.4v6.1c0 .8-.6 1.4-1.4 1.4H2.9c-.8 0-1.4-.6-1.4-1.4z" fill="none" stroke="currentColor" strokeWidth="1.3" />
      <path d="M6 9h4M8.6 7.4 10.2 9l-1.6 1.6" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function PlusIcon() {
  return (
    <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true">
      <path d="M8 3.5v9M3.5 8h9" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
    </svg>
  );
}

export { isWindows };
