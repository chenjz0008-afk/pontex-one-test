#!/usr/bin/env node
// PonteX One 的工程命令。只用 Node 自带模块。
//
//   node tools/pontex.mjs check            上游文件 == 上游原文 + hooks.json，一个字节都不能多
//   node tools/pontex.mjs apply            把 hooks.json 重新应用到上游文件
//   node tools/pontex.mjs replay <tag..>   在历史上游版本上试放 hook，不改工作区
//   node tools/pontex.mjs sync <tag>       跟进一个上游版本：上游文件整份取新版，再重放 hook
//   node tools/pontex.mjs dev|build [...]  用覆盖层配置启动 / 打包
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const OVERLAY = "src-tauri/pontex/tauri.pontex.json";
const readJson = (path) => JSON.parse(readFileSync(join(ROOT, path), "utf8"));
const git = (...args) =>
  execFileSync("git", args, { cwd: ROOT, encoding: "buffer", maxBuffer: 1 << 28, stdio: ["ignore", "pipe", "pipe"] });
const text = (...args) => git(...args).toString("utf8").trim();
const die = (message) => {
  console.error(`✗ ${message}`);
  process.exit(1);
};

const { hooks } = readJson("pontex/hooks.json");
const upstream = readJson("pontex/upstream.json");

/** 对一个上游文件的原文应用它的全部 hook；锚点必须恰好出现一次。 */
export function applyHooks(file, source) {
  let out = source;
  for (const hook of hooks.filter((h) => h.file === file)) {
    if (hook.prepend) {
      out = hook.replace + out;
      continue;
    }
    const count = out.split(hook.find).length - 1;
    if (count !== 1) throw new Error(`${file}：锚点出现 ${count} 次（${hook.why}）\n    ${JSON.stringify(hook.find)}`);
    out = out.replace(hook.find, () => hook.replace);
  }
  return out;
}

const hooked = [...new Set(hooks.map((h) => h.file))];
const show = (ref, file) => git("show", `${ref}:${file}`);
const treeOf = (ref) => new Map(
  text("ls-tree", "-r", "-z", ref).split("\0").filter(Boolean).map((line) => {
    const [meta, path] = line.split("\t");
    const [mode, , oid] = meta.split(" ");
    return [path, { mode, oid }];
  }),
);

/** 基线 tag 可能只在上游仓库里有。 */
function ensureTag(tag) {
  if (spawnSync("git", ["rev-parse", "-q", "--verify", `${tag}^{commit}`], { cwd: ROOT }).status === 0) return;
  git("fetch", "--no-tags", upstream.repository, `refs/tags/${tag}:refs/tags/${tag}`);
}

function check() {
  ensureTag(upstream.tag);
  const base = treeOf(upstream.tag);
  const problems = [];
  // 一次算完所有上游文件在工作区里的 oid。
  const present = [...base.keys()].filter((path) => base.get(path).mode !== "160000");
  const missing = present.filter((path) => !existsSync(join(ROOT, path)));
  missing.forEach((path) => problems.push(`上游文件被删了：${path}`));
  const list = present.filter((path) => existsSync(join(ROOT, path)) && base.get(path).mode !== "120000");
  const oids = execFileSync("git", ["hash-object", "--stdin-paths"], {
    cwd: ROOT, input: list.join("\n"), encoding: "utf8", maxBuffer: 1 << 28,
  }).trim().split("\n");
  list.forEach((path, index) => {
    if (oids[index] === base.get(path).oid) {
      if (hooked.includes(path)) problems.push(`hook 没有应用：${path}`);
      return;
    }
    if (!hooked.includes(path)) return problems.push(`改了没有登记的上游文件：${path}`);
    try {
      const want = applyHooks(path, show(upstream.tag, path).toString("utf8"));
      if (want !== readFileSync(join(ROOT, path), "utf8")) problems.push(`和「上游原文 + hook」不一致：${path}`);
    } catch (error) {
      problems.push(error.message);
    }
  });
  if (problems.length) die(`上游边界被破坏：\n  ${problems.join("\n  ")}`);
  console.log(`✓ ${list.length} 个上游文件与 ${upstream.tag} 一致；${hooked.length} 个文件、${hooks.length} 处 hook`);
}

function apply(ref = upstream.tag) {
  for (const file of hooked) writeFileSync(join(ROOT, file), applyHooks(file, show(ref, file).toString("utf8")));
  console.log(`✓ 已在 ${ref} 的 ${hooked.length} 个文件上应用 ${hooks.length} 处 hook`);
}

function replay(refs) {
  let failed = 0;
  for (const ref of refs) {
    try {
      for (const file of hooked) applyHooks(file, show(ref, file).toString("utf8"));
      console.log(`✓ ${ref}`);
    } catch (error) {
      failed += 1;
      console.log(`✗ ${ref}  ${error.message.split("\n").slice(0, 2).join(" ") || error}`);
    }
  }
  process.exit(failed ? 1 : 0);
}

/**
 * 不做三方合并：上游文件整份换成新版本，再重放 hook。我们的文件和上游没有交集，所以不存在
 * 冲突；锚点没了会在改任何东西之前停下来。
 */
function sync(tag) {
  if (!/^v\d+\.\d+\.\d+$/.test(tag ?? "")) die("用法：sync vX.Y.Z（只跟稳定版）");
  if (text("status", "--porcelain", "--untracked-files=no")) die("工作区不干净，先提交或收起");
  ensureTag(upstream.tag);
  ensureTag(tag);
  if (tag === upstream.tag) return console.log(`已经是 ${tag}`);
  const before = treeOf(upstream.tag);
  const after = treeOf(tag);
  const ours = treeOf("HEAD");
  const clash = [...after.keys()].filter((path) => !before.has(path) && ours.has(path));
  if (clash.length) die(`上游新增的文件和我们的重名：\n  ${clash.join("\n  ")}`);
  const replayed = hooked.map((file) => {
    if (!after.has(file)) die(`上游删了我们有 hook 的文件：${file}`);
    try {
      return [file, applyHooks(file, show(tag, file).toString("utf8"))];
    } catch (error) {
      return die(`${error.message}\n  上游改了这里，更新 pontex/hooks.json 后重试；工作区没有改动。`);
    }
  });
  const branch = `sync/cc-switch-${tag}`;
  git("switch", "-c", branch);
  git("merge", "--no-commit", "--no-ff", "-s", "ours", `${tag}^{commit}`);
  const changed = [...after].filter(([path, entry]) => before.get(path)?.oid !== entry.oid || before.get(path)?.mode !== entry.mode).map(([path]) => path);
  const removed = [...before.keys()].filter((path) => !after.has(path) && ours.has(path));
  for (let i = 0; i < changed.length; i += 200) git("checkout", tag, "--", ...changed.slice(i, i + 200));
  for (let i = 0; i < removed.length; i += 200) git("rm", "-q", "--", ...removed.slice(i, i + 200));
  replayed.forEach(([file, content]) => writeFileSync(join(ROOT, file), content));
  const schema = /SCHEMA_VERSION: i32 = (\d+)/.exec(show(tag, "src-tauri/src/database/mod.rs").toString())?.[1];
  const next = { ...upstream, tag, commit: text("rev-parse", "--short=8", `${tag}^{commit}`), schemaVersion: Number(schema) || upstream.schemaVersion };
  writeFileSync(join(ROOT, "pontex/upstream.json"), `${JSON.stringify(next, null, 2)}\n`);
  git("add", "-A", "--", ...hooked, "pontex/upstream.json");
  git("commit", "-q", "-m", `chore(upstream): CC Switch ${upstream.tag} → ${tag}`);
  mkdirSync(join(ROOT, "release"), { recursive: true });
  writeFileSync(join(ROOT, "release/upstream-report.md"), report(upstream, next, changed, removed));
  console.log(`✓ ${branch}：更新 ${changed.length} 个、删除 ${removed.length} 个上游文件，hook 全部重放。报告：release/upstream-report.md`);
}

/** 给评审看的：这次上游动了哪些我们依赖的地方。 */
function report(from, to, changed, removed) {
  const layer = readFileSync(join(ROOT, "src-tauri/src/pontex/upstream.rs"), "utf8");
  const watched = [...new Set([...layer.matchAll(/crate::([a-z_]+)(?:::([a-z_]+))?/g)].flatMap((m) =>
    [`src-tauri/src/${m[1]}.rs`, `src-tauri/src/${m[1]}/`, m[2] && `src-tauri/src/${m[1]}/${m[2]}.rs`].filter(Boolean)))];
  const hits = changed.filter((path) => watched.some((w) => path === w || (w.endsWith("/") && path.startsWith(w))));
  const diff = text("diff", "--unified=0", `${from.tag}`, `${to.tag}`, "--", "src-tauri/src", "src");
  const hosts = [...new Set([...diff.matchAll(/^\+.*?https?:\/\/([a-z0-9.-]+\.[a-z]{2,})/gim)].map((m) => m[1].toLowerCase()))].sort();
  const lines = [
    `# CC Switch ${from.tag} → ${to.tag}`, "",
    `- 更新 ${changed.length} 个文件，删除 ${removed.length} 个；hook 全部重放成功`,
    `- 数据库 SCHEMA_VERSION：${from.schemaVersion} → ${to.schemaVersion}${from.schemaVersion === to.schemaVersion ? "" : "  **需要旧库升级测试**"}`,
    `- 有 hook 的文件被上游改过：${hooked.filter((f) => changed.includes(f)).join("、") || "无"}`,
    "", "## 防腐层依赖的文件", "", ...(hits.length ? hits.map((p) => `- ${p}`) : ["无"]),
    "", "## 新增代码里出现的域名", "", ...(hosts.length ? hosts.map((h) => `- ${h}`) : ["无"]), "",
  ];
  return lines.join("\n");
}

/** 按用户安装的 MSI 里，附带文件必须用 HKCU 注册表值做标识（ICE38），tauri 的 `resources` 生成不了：
 *  Windows 上改用自己的片段装 CC Switch 的许可证。生成物不入库。 */
function windowsOverlay() {
  const attr = (path) => path.replaceAll("&", "&amp;").replaceAll('"', "&quot;");
  mkdirSync(join(ROOT, "release"), { recursive: true });
  writeFileSync(join(ROOT, "release/license.wxs"), `<?xml version="1.0" encoding="utf-8"?>
<Wix xmlns="http://schemas.microsoft.com/wix/2006/wi">
  <Fragment>
    <DirectoryRef Id="INSTALLDIR">
      <Component Id="CCSwitchLicense" Guid="3f0d4f4e-5c41-4b59-9a55-6f1f3c7a8e21">
        <File Id="CCSwitchLicenseFile" Name="LICENSE-CC-Switch.txt" Source="${attr(join(ROOT, "LICENSE"))}" KeyPath="no" />
        <RegistryValue Root="HKCU" Key="Software\\PonteX\\PonteX One" Name="LicenseFile" Type="integer" Value="1" KeyPath="yes" />
      </Component>
    </DirectoryRef>
  </Fragment>
</Wix>
`);
  const config = readJson(OVERLAY);
  delete config.bundle.resources;
  Object.assign(config.bundle.windows.wix, { fragmentPaths: ["../release/license.wxs"], componentRefs: ["CCSwitchLicense"] });
  // 新装的 Windows 10 可能没有 WebView2：引导程序打进安装包，装的时候不用再临时去下它。
  config.bundle.windows.webviewInstallMode = { type: "embedBootstrapper", silent: true };
  writeFileSync(join(ROOT, "release/windows.json"), JSON.stringify(config, null, 2));
  return "release/windows.json";
}

/** PE 文件导入了哪些 DLL（含延迟加载）。只读头和导入表，不依赖任何工具。 */
function imports(file) {
  const b = readFileSync(file);
  const pe = b.readUInt32LE(0x3c);
  if (b.toString("latin1", pe, pe + 4) !== "PE\0\0") die(`${file} 不是 PE 文件`);
  const sections = b.readUInt16LE(pe + 6);
  const optSize = b.readUInt16LE(pe + 20);
  const opt = pe + 24;
  const dirs = opt + (b.readUInt16LE(opt) === 0x20b ? 112 : 96);
  const table = [];
  for (let i = 0; i < sections; i++) {
    const o = opt + optSize + 40 * i;
    table.push({ va: b.readUInt32LE(o + 12), size: Math.max(b.readUInt32LE(o + 8), b.readUInt32LE(o + 16)), raw: b.readUInt32LE(o + 20) });
  }
  const at = (rva) => {
    const s = table.find((t) => rva >= t.va && rva < t.va + t.size);
    return s ? rva - s.va + s.raw : -1;
  };
  const name = (rva) => { const o = at(rva); return b.toString("latin1", o, b.indexOf(0, o)); };
  const out = [];
  for (const [index, stride, field] of [[1, 20, 12], [13, 32, 4]]) {
    const rva = b.readUInt32LE(dirs + 8 * index);
    for (let o = rva ? at(rva) : -1; o > 0 && b.readUInt32LE(o + field); o += stride) out.push(name(b.readUInt32LE(o + field)));
  }
  return out;
}

/** 需要另外安装才有的运行库。导入了其中任何一个，新装的 Windows 上就会弹「找不到 xxx.dll」。 */
const REDISTRIBUTABLE = /^(vcruntime\d|msvcp\d|concrt\d|vccorlib\d|vcomp\d|msvcr\d|mfc\d|atl\d|ucrtbased|webview2loader)/i;

function deps(files) {
  if (!files.length) die("用法：deps <exe...>");
  let bad = false;
  for (const file of files) {
    const found = imports(file).filter((dll) => REDISTRIBUTABLE.test(dll));
    if (found.length) {
      bad = true;
      console.error(`✗ ${file} 依赖需要另装的运行库：${found.join("、")}`);
    } else console.log(`✓ ${file} 只依赖 Windows 自带的 DLL`);
  }
  if (bad) die("运行库没有编进程序：新装的 Windows 上会打不开");
}

/** .app → DMG（dmgbuild，带检查）和自动更新用的 .app.tar.gz。 */
function macPackages(args) {
  const target = args.includes("--target") ? args[args.indexOf("--target") + 1] : null;
  const bundle = join(ROOT, "src-tauri/target", ...(target ? [target] : []), "release/bundle");
  const app = join(bundle, "macos/PonteX One.app");
  if (!existsSync(app)) die(`找不到 ${app}`);
  const version = JSON.parse(readFileSync(join(ROOT, "src-tauri/pontex/tauri.pontex.json"), "utf8")).version;
  const arch = { "universal-apple-darwin": "universal", "aarch64-apple-darwin": "aarch64", "x86_64-apple-darwin": "x64" }[target] ?? process.arch;
  const out = join(bundle, "dmg", `PonteX One_${version}_${arch}.dmg`);
  const made = spawnSync("bash", [join(ROOT, "tools/dmg/build.sh"), app, out], { cwd: ROOT, stdio: "inherit" });
  if (made.status !== 0) die("DMG 没打好");
  // 更新包：根目录就是 PonteX One.app；不要 ._ 资源分叉文件
  const tar = spawnSync("tar", ["-czf", `${app}.tar.gz`, "-C", join(bundle, "macos"), "PonteX One.app"], {
    cwd: ROOT,
    stdio: "inherit",
    env: { ...process.env, COPYFILE_DISABLE: "1" },
  });
  if (tar.status !== 0) die("更新包没打好");
  console.log(`✓ ${out}\n✓ ${app}.tar.gz`);
}

function tauri(command, args) {
  const bin = process.platform === "win32" ? "tauri.cmd" : "tauri";
  const local = join(ROOT, "node_modules/.bin", bin);
  if (!existsSync(local)) die("先运行 pnpm install");
  // WiX 模板沿用上游的按用户安装，只换主程序名；生成物不入库。
  const wxs = readFileSync(join(ROOT, "src-tauri/wix/per-user-main.wxs"), "utf8");
  if (!wxs.includes("cc-switch.exe")) die("上游 WiX 模板变了：找不到 cc-switch.exe");
  mkdirSync(join(ROOT, "release"), { recursive: true });
  writeFileSync(join(ROOT, "release/per-user-main.wxs"), wxs.replaceAll("cc-switch.exe", "pontex-one.exe"));
  const overlay = process.platform === "win32" ? windowsOverlay() : OVERLAY;
  const bundles = { darwin: "app,dmg", win32: "msi" }[process.platform];
  let extra = command === "build" && bundles && !args.includes("--bundles") && !args.includes("--no-bundle") ? ["--bundles", bundles] : [];
  // macOS 的 DMG 不让 tauri 打：它在 CI 上会跳过写 .DS_Store，打开是没有背景、图标乱放的窗口。
  // tauri 只出 .app，DMG 由 tools/dmg/build.sh（dmgbuild）生成并检查；同时打一份给自动更新用的 .app.tar.gz。
  let dmg = false;
  if (command === "build" && process.platform === "darwin") {
    const at = args.indexOf("--bundles");
    const list = (at >= 0 ? args[at + 1] : extra[1] ?? "").split(",").filter(Boolean);
    if (list.includes("dmg")) {
      dmg = true;
      const rest = [...new Set(list.filter((b) => b !== "dmg").concat("app"))].join(",");
      if (at >= 0) args = args.map((a, i) => (i === at + 1 ? rest : a));
      else extra = ["--bundles", rest];
    }
  }
  // Windows：C 运行库静态编进 exe，新装的系统上没有 VC++ 运行库也能打开。tauri-build 自带的
  // STATIC_VCRUNTIME 在 ARM64 上替换不掉 msvcrt.lib，所以用 Rust 官方的 crt-static，两种架构都成立。
  const env = { ...process.env };
  if (process.platform === "win32") {
    for (const triple of ["X86_64", "AARCH64"]) {
      const key = `CARGO_TARGET_${triple}_PC_WINDOWS_MSVC_RUSTFLAGS`;
      env[key] = [env[key], "-C target-feature=+crt-static"].filter(Boolean).join(" ");
    }
  }
  const result = spawnSync(local, [command, "--config", overlay, ...extra, ...args], { cwd: ROOT, stdio: "inherit", shell: process.platform === "win32", env });
  if (result.status === 0 && dmg) macPackages(args);
  if (result.status === 0 && command === "build" && process.platform === "win32") {
    const target = args[args.indexOf("--target") + 1];
    const dir = args.includes("--target") ? join(ROOT, "src-tauri/target", target, "release") : join(ROOT, "src-tauri/target/release");
    deps([join(dir, "pontex-one.exe")]);
  }
  process.exit(result.status ?? 1);
}

const [command, ...rest] = process.argv.slice(2);
if (command === "check") check();
else if (command === "apply") apply(rest[0]);
else if (command === "replay") replay(rest);
else if (command === "sync") sync(rest[0]);
else if (command === "dev" || command === "build") tauri(command, rest);
else if (command === "deps") deps(rest);
else die("用法：check | apply | replay <ref..> | sync <tag> | dev | build | deps <exe..>");
