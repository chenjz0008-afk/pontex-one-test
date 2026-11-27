import { useCallback, useEffect, useMemo, useState } from "react";
import { api, onSkill, onState, type Snapshot, type View } from "./api";
import { pickLanguage, type T } from "./i18n";
import { Connect } from "./Connect";
import { Home } from "./Home";
import { Settings } from "./Settings";
import { SkillDialog } from "./Skill";
import { Fatal } from "./Fatal";
import { Brand, Button } from "./ui";

const isMac = navigator.userAgent.includes("Mac");

export function App() {
  const [view, setView] = useState<View | null>(null);
  const [page, setPage] = useState<"home" | "settings" | "connect">("home");
  const [skill, setSkill] = useState<string | null>(null);
  const [focus, setFocus] = useState<"skills" | undefined>();

  useEffect(() => {
    let alive = true;
    const pull = () => void api.pendingSkill().then((slug) => alive && slug && setSkill(slug));
    const subscriptions = [
      onState((next) => alive && setView(next)),
      onSkill(pull),
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
  const home = useCallback(() => setPage("home"), []);
  // 接入一成功广播就到了，但结果页还要给用户看：点了「完成」或去了设置才算走完。
  const connected = snapshot?.connected;
  const [onboarding, setOnboarding] = useState(false);
  useEffect(() => {
    if (connected === false) setOnboarding(true);
  }, [connected]);
  const go = useCallback(
    (next: "home" | "settings" | "connect") => {
      if (connected) setOnboarding(false);
      setFocus(undefined);
      setPage(next);
    },
    [connected],
  );

  return (
    <div className={isMac ? "app mac" : "app"}>
      <header data-tauri-drag-region>
        <Brand t={t} />
        <div className="header-end">
          {snapshot?.connected && page === "home" && (
            <>
              <span className="key-chip" title={t.keySection}>{snapshot.key_hint}…</span>
              <Button kind="quiet" onClick={() => go("settings")}>{t.settings}</Button>
            </>
          )}
          {snapshot && !snapshot.connected && page === "home" && (
            <Button kind="quiet" onClick={() => go("settings")}>{t.settings}</Button>
          )}
          {page !== "home" && <Button kind="quiet" onClick={home}>{t.back}</Button>}
        </div>
      </header>
      <main>
        <Body view={view} snapshot={snapshot} t={t} page={page} setPage={go} onboarding={onboarding} focus={focus} />
      </main>
      {skill && snapshot && (
        <SkillDialog
          key={skill}
          slug={skill}
          t={t}
          onClose={() => setSkill(null)}
          onManage={() => {
            setSkill(null);
            go("settings");
            setFocus("skills");
          }}
        />
      )}
    </div>
  );
}

function Body(props: {
  view: View | null;
  snapshot: Snapshot | null;
  t: T;
  page: "home" | "settings" | "connect";
  setPage: (page: "home" | "settings" | "connect") => void;
  onboarding: boolean;
  focus?: "skills";
}) {
  const { view, snapshot, t, page, setPage, onboarding, focus } = props;
  if (!view) return <p className="center muted">{t.checking}</p>;
  if (view.kind === "failed") return <Fatal error={view.error} t={t} />;
  if (!snapshot) return null;
  if (page === "settings") return <Settings key={focus ?? ""} snapshot={snapshot} t={t} focus={focus} onConnect={() => setPage("connect")} />;
  if (page === "connect" || onboarding || !snapshot.connected)
    return <Connect snapshot={snapshot} t={t} onDone={() => setPage("home")} />;
  return <Home snapshot={snapshot} t={t} />;
}
