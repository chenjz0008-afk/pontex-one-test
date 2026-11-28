import { useCallback, useEffect, useMemo, useState } from "react";
import { api, onSkill, onState, onUpdate, type Snapshot, type View } from "./api";
import { pickLanguage, type T } from "./i18n";
import { Connect } from "./Connect";
import { Home } from "./Home";
import { Settings } from "./Settings";
import { Toolbox } from "./Toolbox";
import { UpdateDialog } from "./Update";
import { SkillDialog } from "./Skill";
import { Fatal } from "./Fatal";
import { Brand, Button, ToastProvider, useToast } from "./ui";

type Page = "home" | "toolbox" | "settings" | "connect";

const isMac = navigator.userAgent.includes("Mac");

export function App() {
  const [view, setView] = useState<View | null>(null);
  const [page, setPage] = useState<Page>("home");
  const [skill, setSkill] = useState<string | null>(null);
  const [toolboxTab, setToolboxTab] = useState<"skills" | "mcp">("skills");
  const [showUpdate, setShowUpdate] = useState(false);

  useEffect(() => {
    let alive = true;
    const pull = () => void api.pendingSkill().then((slug) => alive && slug && setSkill(slug));
    const subscriptions = [
      onState((next) => alive && setView(next)),
      onSkill(pull),
      // 新版本下好了：后台已经把主窗口叫出来，这里弹「有新版本」窗口。
      onUpdate(() => alive && setShowUpdate(true)),
    ];
    void api.snapshot().then((next) => alive && setView(next));
    pull();
    const focus = () => void api.refresh().catch(() => undefined);
    window.addEventListener("focus", focus);
    return () => {
      alive = false;
      window.removeEventListener("focus", focus);
      subscriptions.forEach((s) => void s.then((off) => off()));
    };
  }, []);

  const snapshot = view?.kind === "ready" ? view.snapshot : null;
  const t = useMemo(() => pickLanguage(snapshot?.prefs.language), [snapshot?.prefs.language]);
  useEffect(() => {
    document.documentElement.lang = t.lang === "zh" ? "zh-CN" : "en";
  }, [t]);
  // 接入一成功广播就到了，但结果页还要给用户看：点了「完成」或去了设置才算走完。
  const connected = snapshot?.connected;
  const [onboarding, setOnboarding] = useState(false);
  useEffect(() => {
    if (connected === false) setOnboarding(true);
  }, [connected]);
  const go = useCallback(
    (next: Page) => {
      if (connected) setOnboarding(false);
      setPage(next);
    },
    [connected],
  );
  // 还没连上 PonteX：只有连接页和设置，不显示 Agent / AI 工具箱。
  const tabs = Boolean(snapshot?.connected) && !onboarding && page !== "connect";

  return (
    <ToastProvider t={t}>
    <div className={isMac ? "app mac" : "app"}>
      <header data-tauri-drag-region>
        <Brand t={t} />
        {tabs && (
          <nav className="tabs" aria-label="PonteX One">
            <button type="button" className={page === "home" ? "on" : undefined} aria-current={page === "home" ? "page" : undefined} onClick={() => go("home")}>{t.tabAgents}</button>
            <button type="button" className={page === "toolbox" ? "on" : undefined} aria-current={page === "toolbox" ? "page" : undefined} onClick={() => go("toolbox")}>{t.tabToolbox}</button>
          </nav>
        )}
        <div className="header-end">
          {snapshot && page !== "connect" && (
            <button type="button" className={`btn quiet${page === "settings" ? " on" : ""}${snapshot.update.state === "ready" ? " has-dot" : ""}`} aria-current={page === "settings" ? "page" : undefined} onClick={() => go(page === "settings" && !tabs ? "home" : "settings")}>
              {t.settings}
            </button>
          )}
          {page === "connect" && <Button kind="quiet" onClick={() => go("home")}>{t.back}</Button>}
        </div>
      </header>
      <main>
        <Body view={view} snapshot={snapshot} t={t} page={page} setPage={go} onboarding={onboarding} toolboxTab={toolboxTab} onToolboxTab={setToolboxTab} />
      </main>
      {snapshot && <UpdatedToast snapshot={snapshot} t={t} />}
      {showUpdate && snapshot?.update.state === "ready" && (
        <UpdateDialog update={snapshot.update} t={t} onClose={() => setShowUpdate(false)} />
      )}
      {skill && snapshot && (
        <SkillDialog
          key={skill}
          slug={skill}
          t={t}
          onClose={() => setSkill(null)}
          onManage={() => {
            setSkill(null);
            setToolboxTab("skills");
            go("toolbox");
          }}
        />
      )}
    </div>
    </ToastProvider>
  );
}

/** 更新后第一次打开：提示一句「已更新到 …」。 */
function UpdatedToast({ snapshot, t }: { snapshot: Snapshot; t: T }) {
  const toast = useToast();
  const { version, prefs } = snapshot;
  useEffect(() => {
    if (!version || prefs.last_version === version) return;
    if (prefs.last_version) toast({ text: t.updated(version) });
    void api.setPrefs({ ...prefs, last_version: version });
  }, [version, prefs, t, toast]);
  return null;
}

function Body(props: {
  view: View | null;
  snapshot: Snapshot | null;
  t: T;
  page: Page;
  setPage: (page: Page) => void;
  onboarding: boolean;
  toolboxTab: "skills" | "mcp";
  onToolboxTab: (tab: "skills" | "mcp") => void;
}) {
  const { view, snapshot, t, page, setPage, onboarding } = props;
  if (!view) return <p className="center muted">{t.checking}</p>;
  if (view.kind === "failed") return <Fatal error={view.error} t={t} />;
  if (!snapshot) return null;
  if (page === "settings") return <Settings snapshot={snapshot} t={t} onConnect={() => setPage("connect")} />;
  if (page === "connect" || onboarding || !snapshot.connected)
    return <Connect snapshot={snapshot} t={t} onDone={() => setPage("home")} />;
  if (page === "toolbox") return <Toolbox snapshot={snapshot} t={t} tab={props.toolboxTab} onTab={props.onToolboxTab} />;
  return <Home snapshot={snapshot} t={t} />;
}
