#!/usr/bin/env node
// 把安装包发布到 docs.ponte-x.com/downloads/one/。
//
//   node tools/publish.mjs <资产目录> [--notes "..."] [--dry-run]
//
// 顺序：本机校验 → 上传到新的版本目录（不可变）→ 从公网拉回来逐个比对 → 最后才换 latest.json。
// 中途任何一步失败，线上清单不变。资产按文件名认：
//   PonteX-One-<版本>-macOS-universal.dmg / -Windows-x64.msi / -Windows-arm64.msi / …
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
// 服务器地址不写进仓库（站点在 Cloudflare 后面，源站地址不公开）
const REMOTE = process.env.PONTEX_REMOTE;
const SITE = process.env.PONTEX_URL ?? "https://docs.ponte-x.com";
const BASE = "/var/www/ponte-x-docs/shared/downloads/one";
const SSH = ["-o", "ServerAliveInterval=15", "-o", "ConnectTimeout=20"];
const KINDS = {
  "macOS-universal.dmg": "macos",
  "Windows-x64.msi": "windows-x64",
  "Windows-arm64.msi": "windows-arm64",
  "Windows-x64-Portable.zip": "windows-x64-portable",
  "Windows-arm64-Portable.zip": "windows-arm64-portable",
  "Linux-x86_64.deb": "linux-x64-deb",
  "Linux-x86_64.rpm": "linux-x64-rpm",
  "Linux-x86_64.AppImage": "linux-x64-appimage",
  "Linux-arm64.deb": "linux-arm64-deb",
  "Linux-arm64.rpm": "linux-arm64-rpm",
  "Linux-arm64.AppImage": "linux-arm64-appimage",
};
const die = (message) => {
  console.error(`✗ ${message}`);
  process.exit(1);
};
if (!REMOTE) die("先设置 PONTEX_REMOTE=<用户>@<服务器地址>");
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
async function retry(label, work, times = 6) {
  for (let n = 1; ; n += 1) {
    try {
      return await work();
    } catch (error) {
      if (n >= times) throw error;
      console.log(`  ${label}：第 ${n} 次失败，重试…`);
      await sleep(n * 2000);
    }
  }
}
const ssh = (command, input) =>
  execFileSync("ssh", [...SSH, REMOTE, command], { input, stdio: [input ? "pipe" : "ignore", "pipe", "inherit"], encoding: "utf8" });
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

const [dir, ...flags] = process.argv.slice(2);
if (!dir) die("用法：publish <资产目录> [--dry-run]");
const dryRun = flags.includes("--dry-run");
const version = JSON.parse(readFileSync(join(ROOT, "src-tauri/pontex/tauri.pontex.json"), "utf8")).version;
const engine = JSON.parse(readFileSync(join(ROOT, "pontex/upstream.json"), "utf8")).tag.replace(/^v/, "");
if (!/^\d+\.\d+\.\d+$/.test(version)) die(`版本号不对：${version}`);

const prefix = `PonteX-One-${version}-`;
const assets = {};
for (const file of readdirSync(dir).filter((f) => f.startsWith(prefix))) {
  const id = KINDS[file.slice(prefix.length)];
  if (!id) die(`不认识的资产：${file}`);
  const bytes = readFileSync(join(dir, file));
  assets[id] = { url: `/downloads/one/${version}/${file}`, file, size: statSync(join(dir, file)).size, sha256: sha256(bytes) };
}
if (!Object.keys(assets).length) die(`${dir} 里没有 ${prefix}* 的资产`);

// 预览期没有平台签名；签名齐了把 channel 改成「正式版」。
const channel = version.startsWith("0.") ? "预览版" : "正式版";

// 同一版本后补平台（比如 Windows 晚一天出）：保留已经发布的，不改它们。
const current = await fetch(`${SITE}/downloads/one/latest.json?t=${Date.now()}`)
  .then((r) => (r.ok ? r.json() : null))
  .catch(() => null);
if (current?.version === version) {
  for (const [id, old] of Object.entries(current.assets)) {
    if (assets[id] && assets[id].sha256 !== old.sha256) die(`${version} 的 ${id} 已经发布过且内容不同：版本目录不可变，请提升版本号`);
    assets[id] ??= old;
  }
} else if (current && compare(version, current.version) < 0) die(`线上是 ${current.version}，不能发更旧的 ${version}`);

function compare(a, b) {
  const [x, y] = [a, b].map((v) => v.split(".").map(Number));
  return x.map((n, i) => n - (y[i] ?? 0)).find((n) => n !== 0) ?? 0;
}

const latest = { version, channel, date: new Date().toISOString().slice(0, 10), engine, assets };
console.log(JSON.stringify(latest, null, 2));
if (dryRun) process.exit(0);

console.log(`[1/4] 上传到 ${BASE}/${version}/`);
await retry("建目录", async () => ssh(`sudo mkdir -p ${BASE}/${version} && sudo chown $(id -un):$(id -gn) ${BASE} ${BASE}/${version}`));
for (const { file } of Object.values(assets)) {
  const local = join(dir, file);
  try {
    statSync(local);
  } catch {
    continue;
  }
  await retry(file, async () => execFileSync("rsync", ["-az", "--partial", "--ignore-existing", "-e", `ssh ${SSH.join(" ")}`, local, `${REMOTE}:${BASE}/${version}/`], { stdio: "inherit" }));
}

console.log("[2/4] 服务器上核对");
for (const { file, sha256: want } of Object.values(assets)) {
  const got = (await retry(file, async () => ssh(`sha256sum ${BASE}/${version}/${file}`))).split(/\s/)[0];
  if (got !== want) die(`${file} 上传后的哈希不对：${got}`);
  console.log(`  ✓ ${file}`);
}

// 网站部署会把文件属主改成 www-data：目录是自己的，删掉旧留底再复制即可。
console.log("[3/4] 切换 latest.json（原子替换，旧的留底）");
const body = `${JSON.stringify(latest, null, 2)}\n`;
const tmp = join(mkdtempSync(join(tmpdir(), "px-")), "latest.json");
writeFileSync(tmp, body);
await retry("清单", async () => ssh(`cat > ${BASE}/.latest.json.new && { [ ! -f ${BASE}/latest.json ] || cp --remove-destination ${BASE}/latest.json ${BASE}/.latest.json.prev; } && mv ${BASE}/.latest.json.new ${BASE}/latest.json`, body));

console.log("[4/4] 从公网拉回来核对");
if (flags.includes("--no-public-check")) {
  console.log("  跳过（网站还没有放行 /downloads/）");
} else {
  for (const asset of Object.values(assets)) {
    const bytes = await retry(asset.file, async () => {
      const response = await fetch(SITE + asset.url);
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      return Buffer.from(await response.arrayBuffer());
    });
    if (sha256(bytes) !== asset.sha256) {
      ssh(`[ ! -f ${BASE}/.latest.json.prev ] || mv ${BASE}/.latest.json.prev ${BASE}/latest.json`);
      die(`${asset.file} 公网内容不对，已恢复上一份清单`);
    }
    console.log(`  ✓ ${asset.url}  ${(asset.size / 1048576).toFixed(1)} MB`);
  }
}
console.log(`✓ 已发布 ${version}`);
