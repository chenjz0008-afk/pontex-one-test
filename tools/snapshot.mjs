#!/usr/bin/env node
// 把 PonteX_One 已提交的内容同步到公开测试仓库（只为了用 GitHub 的免费真机打包、测试）。
//
//   node tools/snapshot.mjs <测试仓库本地目录> [--tag <标签> ...] [--push]
//
// 规则固定、可重复，不靠手工复制：
//   · 只同步 HEAD 里已提交的文件；测试仓库里多出来的删掉。
//   · 工作流只留 pontex-*（上游的不跑）。
//   · 两处测试仓库专用的补丁，对不上就失败（上游改了要重新看）：
//       清空 Gemini OAuth 常量（GitHub 推送保护会拦）；打包流程跳过上游边界检查（清空的常量会让它失败）。
//   · 推之前扫一遍：不许出现作者邮箱、源站地址（取自 PONTEX_REMOTE，不打印）。
//   · 提交作者是 GitHub 的 noreply 地址。
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const die = (message) => {
  console.error(`✗ ${message}`);
  process.exit(1);
};
const [target, ...flags] = process.argv.slice(2);
if (!target || !existsSync(join(target, ".git"))) die("用法：snapshot <测试仓库本地目录> [--tag 标签 ...] [--push]");
const tags = flags.flatMap((f, i) => (f === "--tag" ? [flags[i + 1]] : []));
const git = (cwd, args, options = {}) => execFileSync("git", args, { cwd, encoding: "utf8", ...options });

const AUTHOR = ["-c", "user.name=chenjz0008-afk", "-c", "user.email=chenjz0008-afk@users.noreply.github.com"];

/** 测试仓库专用的补丁：每条都必须恰好命中。 */
const PATCHES = {
  "src-tauri/src/services/subscription.rs": (text) =>
    replaceOnce(
      replaceOnce(text, /(const GEMINI_OAUTH_CLIENT_ID: &str =\s*)"[^"]+";/, '$1"snapshot-removed";'),
      /(const GEMINI_OAUTH_CLIENT_SECRET: &str =\s*)"[^"]+";/,
      '$1"snapshot-removed";',
    ),
  ".github/workflows/pontex-release.yml": (text) =>
    replaceOnce(text, /^ {6}- run: node tools\/pontex\.mjs check\n/m, "      # 测试快照里清空过上游的两个值，边界检查在 PonteX_One 里跑\n"),
};

function replaceOnce(text, pattern, replacement) {
  const hits = text.match(new RegExp(pattern.source, `${pattern.flags.replace("g", "")}g`))?.length ?? 0;
  if (hits !== 1) die(`补丁对不上（命中 ${hits} 次）：${pattern}`);
  return text.replace(pattern, replacement);
}

const keep = (path) => !path.startsWith(".github/workflows/") || /^\.github\/workflows\/pontex-[^/]+$/.test(path);
const files = git(ROOT, ["ls-tree", "-r", "--name-only", "HEAD"]).split("\n").filter(Boolean).filter(keep);
const wanted = new Set(files);

// 不许出现的东西：作者邮箱、源站地址。只比较，不打印。
const forbidden = [git(ROOT, ["config", "user.email"]).trim(), process.env.PONTEX_REMOTE?.split("@").pop()].filter((v) => v && v.length > 4);

let patched = 0;
for (const path of files) {
  let bytes = execFileSync("git", ["show", `HEAD:${path}`], { cwd: ROOT, maxBuffer: 1 << 30 });
  if (PATCHES[path]) {
    bytes = Buffer.from(PATCHES[path](bytes.toString("utf8")));
    patched += 1;
  }
  for (const secret of forbidden) if (bytes.includes(secret)) die(`${path} 里有不该公开的内容，没有同步`);
  const out = join(target, path);
  mkdirSync(dirname(out), { recursive: true });
  if (!existsSync(out) || !readFileSync(out).equals(bytes)) writeFileSync(out, bytes);
}
if (patched !== Object.keys(PATCHES).length) die("有补丁没有用上：对应的文件不在了");
for (const path of git(target, ["ls-files"]).split("\n").filter(Boolean)) {
  if (!wanted.has(path)) rmSync(join(target, path), { force: true });
}

git(target, ["add", "-A"]);
const changed = git(target, ["status", "--porcelain"]).trim();
const head = git(ROOT, ["rev-parse", "--short", "HEAD"]).trim();
if (changed) {
  git(target, [...AUTHOR, "commit", "-q", "-m", `snapshot ${head}`]);
  console.log(`✓ 同步到 ${head}：${changed.split("\n").length} 个文件有变化`);
} else console.log(`✓ 已经是 ${head}`);
for (const tag of tags) git(target, ["tag", "-f", tag], { stdio: "ignore" });

if (flags.includes("--push")) {
  for (let n = 1; ; n += 1) {
    try {
      git(target, ["push", "-q", "origin", "HEAD"], { stdio: "inherit" });
      for (const tag of tags) git(target, ["push", "-q", "-f", "origin", tag], { stdio: "inherit" });
      console.log(`✓ 已推送${tags.length ? `，标签 ${tags.join(" ")}` : ""}`);
      break;
    } catch (error) {
      if (n >= 6) throw error;
      console.log(`  推送第 ${n} 次失败，重试…`);
      execFileSync("sleep", [String(n * 3)]);
    }
  }
}
