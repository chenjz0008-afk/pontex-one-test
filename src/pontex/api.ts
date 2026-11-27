// 和 Rust 的全部接口。界面不保存业务状态：命令改，`px://state` 来了就重画。
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type AgentId =
  | "claude-code"
  | "codex"
  | "gemini-cli"
  | "grok-build"
  | "workbuddy";
export type Target = "pontex" | "native" | "previous";

export type Install =
  | { state: "checking" }
  | { state: "missing" }
  | { state: "broken"; path: string }
  | { state: "installed"; version: string | null };

export type StatusCode =
  | "checking"
  | "not-installed"
  | "waiting"
  | "broken"
  | "unreadable"
  | "failed"
  | "overridden"
  | "drifted"
  | "other-key"
  | "other"
  | "verified"
  | "written"
  | "verify-failed"
  | "native"
  | "added"
  | "not-added";

export interface Status {
  code: StatusCode;
  detail: string | null;
  action: "install" | "verify" | "apply" | "open-app" | "show-detail" | null;
}

export interface Route {
  kind: string;
  name?: string;
  via?: string;
  model?: string | null;
  key_hint?: string | null;
  models?: number;
  file?: string;
}

export interface Override {
  name: string;
  source: string;
  blocking: boolean;
}

export interface Verify {
  at: number;
  ok: boolean;
  reason: string | null;
  request_id: string | null;
  model: string;
}

export interface Agent {
  id: AgentId;
  label: string;
  install: Install;
  route: Route;
  current: Target | null;
  switchable: boolean;
  previous: string | null;
  status: Status;
  model: string | null;
  options: string[];
  overrides: Override[];
  verify: Verify | null;
  waiting: boolean;
  connected: boolean;
  installable: boolean;
  mirror: string | null;
  managed: string | null;
  update: string | null;
  /** Codex 桌面版：装着的版本。 */
  app: string | null;
  /** Codex 桌面版：这台电脑能装、还没装。 */
  app_installable: boolean;
}

export type Phase =
  | "queued"
  | "resolving"
  | "downloading"
  | "verifying"
  | "authorizing"
  | "unpacking"
  | "testing"
  | "activating"
  | "done"
  | "failed";

export interface Progress {
  agent: AgentId;
  phase: Phase;
  version: string | null;
  received: number;
  total: number | null;
  source: string | null;
  runtime: boolean;
  /** 这一轮没下完，几秒后自动重试。 */
  wait: number | null;
  /** 装的是 Codex 桌面版。 */
  desktop: boolean;
  error: string | null;
}

export type PathPlan =
  | { kind: "ready" }
  | { kind: "profile"; file: string }
  | { kind: "user-path" };

export interface InstallPlan {
  agent: AgentId;
  version: string;
  unpacked: number;
  dir: string;
  path: PathPlan;
  needs_node: boolean;
}

export interface AppPlan {
  version: string;
  size: number;
  dir: string | null;
  admin: boolean;
}

export interface Planned {
  agent: AgentId;
  plan: InstallPlan | null;
  error: string | null;
  app: AppPlan | null;
  app_error: string | null;
}

export interface Prefs {
  language: string | null;
  close_to_tray: boolean;
  coexist_notice_seen: boolean;
}

export interface Snapshot {
  version: string;
  engine: string;
  connected: boolean;
  key_hint: string | null;
  agents: Agent[];
  prefs: Prefs;
  cc_switch_present: boolean;
  data_dir: string;
}

export type View =
  | { kind: "ready"; snapshot: Snapshot }
  | { kind: "failed"; error: { code: string; detail: string } };

export interface KeyReport {
  usable: boolean;
  listed: boolean;
  reason: string | null;
  request_id: string | null;
  rotated_until: string | null;
  hint: string;
}

export interface Outcome {
  agent: AgentId;
  result: "applied" | "restored" | "staged" | "waiting" | "no-model" | "failed" | "stage";
  error: string | null;
  locked_sessions: number | null;
}

export type Place =
  | { kind: "new" | "renamed" | "up-to-date" | "linked" }
  | { kind: "update" | "modified"; from: string };

export interface SkillTarget {
  agent: AgentId;
  label: string;
  installed: boolean;
  verified: boolean;
  state: Place;
  dir: string;
}

export interface SkillPreview {
  slug: string;
  title: string;
  description: string;
  version: string;
  file_count: number;
  bytes: number;
  scripts: string[];
  safety_note: string | null;
  targets: SkillTarget[];
}

export interface SkillInfo {
  title: string;
  description: string;
  version: string;
}

export interface SkillOwned {
  slug: string;
  title: string;
  places: { agent: AgentId; label: string; version: string; dir: string; modified: boolean }[];
}

export interface SkillInstalled {
  agent: AgentId;
  ok: boolean;
  dir: string | null;
  backup: string | null;
  error: string | null;
}

export const api = {
  snapshot: () => invoke<View>("px_snapshot"),
  refresh: (force = false) => invoke<void>("px_refresh", { force }),
  checkKey: (key: string) => invoke<KeyReport>("px_check_key", { key }),
  connect: (key: string, choices: { agent: AgentId; apply: boolean }[]) =>
    invoke<{ report: KeyReport; outcomes: Outcome[] }>("px_connect", {
      key,
      choices,
    }),
  switch: (agent: AgentId, target: Target) =>
    invoke<Outcome>("px_switch", { agent, target }),
  setModel: (agent: AgentId, model: string) =>
    invoke<Outcome>("px_set_model", { agent, model }),
  verify: (agent: AgentId) => invoke<Verify | null>("px_verify", { agent }),
  disconnect: (agent: AgentId | null) =>
    invoke<Outcome[]>("px_disconnect", { agent }),
  setPrefs: (prefs: Prefs) => invoke<void>("px_set_prefs", { prefs }),
  autostart: (enable: boolean | null) =>
    invoke<boolean>("px_autostart", { enable }),
  open: (target: string, agent?: AgentId) =>
    invoke<void>("px_open", { target, agent: agent ?? null }),
  diagnose: () => invoke<string>("px_diagnose"),
  retry: () => invoke<void>("px_retry"),
  pendingSkill: () => invoke<string | null>("px_pending_skill"),
  skillInfo: (slug: string) => invoke<SkillInfo>("px_skill_info", { slug }),
  skills: () => invoke<SkillOwned[]>("px_skills"),
  skillPrepare: (slug: string) =>
    invoke<SkillPreview>("px_skill_prepare", { slug }),
  skillCommit: (slug: string, agents: AgentId[]) =>
    invoke<SkillInstalled[]>("px_skill_commit", { slug, agents }),
  installPlan: (agents: AgentId[]) =>
    invoke<Planned[]>("px_install_plan", { agents }),
  /** `desktop`：Codex 装桌面版（否则装命令行）。 */
  install: (agents: AgentId[], desktop = false) => invoke<void>("px_install", { agents, desktop }),
  /** 用户自己去装：装好了就接入。 */
  expect: (agent: AgentId) => invoke<void>("px_expect", { agent }),
  installCancel: (agent: AgentId) => invoke<void>("px_install_cancel", { agent }),
  installState: () => invoke<Progress[]>("px_install_state"),
  uninstall: (agent: AgentId) => invoke<void>("px_uninstall", { agent }),
  skillRemove: (slug: string, agent: AgentId) =>
    invoke<void>("px_skill_remove", { slug, agent }),
};

export const onState = (handler: (view: View) => void) =>
  listen<View>("px://state", (event) => handler(event.payload));
export const onInstall = (handler: (p: Progress) => void) =>
  listen<Progress>("px://install", (event) => handler(event.payload));
export const onSkill = (handler: () => void) =>
  listen("px://skill", () => handler());
