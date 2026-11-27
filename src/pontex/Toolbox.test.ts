import { describe, expect, it } from "vitest";
import { pickLanguage } from "./i18n";
import { opensIn } from "./Launch";
import { parseTarget } from "./Toolbox";
import type { Agent } from "./api";

const t = pickLanguage("zh");

describe("添加 MCP 的输入框", () => {
  it("命令：按空格切，引号里的不切，名字从包名里取", () => {
    const p = parseTarget(t, 'npx -y @upstash/context7-mcp@latest --dir "My Docs"');
    expect(p.server).toEqual({ type: "stdio", command: "npx", args: ["-y", "@upstash/context7-mcp@latest", "--dir", "My Docs"] });
    expect(parseTarget(t, "uvx mcp-server-fetch").name).toBe("fetch");
    expect(parseTarget(t, "npx -y @upstash/context7-mcp").name).toBe("context7");
  });

  it("网址：用 http，名字取域名", () => {
    const p = parseTarget(t, "https://mcp.notion.com/mcp");
    expect(p.server).toEqual({ type: "http", url: "https://mcp.notion.com/mcp" });
    expect(p.name).toBe("notion");
  });

  it("README 里的 JSON：认出名字和环境变量", () => {
    const p = parseTarget(t, JSON.stringify({ mcpServers: { github: { command: "docker", args: ["run"], env: { GITHUB_TOKEN: "x" } } } }));
    expect(p.name).toBe("github");
    expect(p.pairs).toEqual(["GITHUB_TOKEN"]);
    expect(p.server).toMatchObject({ type: "stdio", command: "docker" });
    expect(parseTarget(t, '{"mcpServers": {').hint).toBe(t.mcpJsonIncomplete);
  });

  it("空的什么都不做", () => {
    expect(parseTarget(t, "   ").server).toBeNull();
  });
});

describe("「打开」做什么", () => {
  const agent = (o: Partial<Agent>) => ({ id: "claude-code", install: { state: "installed", version: "2.1.40" }, app: null, ...o }) as Agent;
  it("命令行 Agent 在终端里打开；没装的没有按钮", () => {
    expect(opensIn(agent({}))).toBe("terminal");
    expect(opensIn(agent({ install: { state: "missing" } }))).toBeNull();
  });
  it("WorkBuddy 和只装了桌面版的 Codex 直接打开应用", () => {
    expect(opensIn(agent({ id: "workbuddy" }))).toBe("app");
    expect(opensIn(agent({ id: "codex", app: "26.1120", install: { state: "installed", version: "26.1120" } }))).toBe("app");
    expect(opensIn(agent({ id: "codex", app: "26.1120", install: { state: "installed", version: "0.171.0" } }))).toBe("terminal");
  });
});
