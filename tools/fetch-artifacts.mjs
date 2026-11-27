#!/usr/bin/env node
// 取回 GitHub 打好的包：node tools/fetch-artifacts.mjs [Windows-x64 Windows-arm64 macOS-universal]
// 成品放进 release/<版本>/；失败的打印日志结尾。
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const git = (args, opts = {}) => execFileSync("git", args, { cwd: ROOT, maxBuffer: 1 << 28, ...opts });
const version = JSON.parse(readFileSync(join(ROOT, "src-tauri/pontex/tauri.pontex.json"), "utf8")).version;
const names = process.argv.slice(2).length ? process.argv.slice(2) : ["Windows-x64", "Windows-arm64"];
for (const name of names) {
  const ref = `refs/pontex-artifacts/${name}`;
  try {
    git(["fetch", "-q", "--depth=1", "origin", `+refs/heads/pontex-artifacts/${name}:${ref}`], { stdio: ["ignore", "ignore", "ignore"] });
  } catch {
    console.log(`… ${name}：还没有结果（或网络不通）`);
    continue;
  }
  const result = git(["show", `${ref}:RESULT`], { encoding: "utf8" }).trim();
  console.log(`${result.startsWith("success") ? "✓" : "✗"} ${name}：${result}`);
  for (const file of git(["ls-tree", "--name-only", "-z", ref], { encoding: "utf8" }).split("\0").filter(Boolean)) {
    if (file.startsWith(`PonteX-One-${version}-`)) {
      const dir = join(ROOT, "release", version);
      mkdirSync(dir, { recursive: true });
      writeFileSync(join(dir, file), git(["show", `${ref}:${file}`]));
      console.log(`  → release/${version}/${file}`);
    } else if (file.endsWith(".log") && !result.startsWith("success")) {
      const log = git(["show", `${ref}:${file}`], { encoding: "utf8" });
      console.log(log.split("\n").slice(-60).join("\n"));
    }
  }
}
