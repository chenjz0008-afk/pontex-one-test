#!/usr/bin/env node
// 把安装包发布到 docs.ponte-x.com/downloads/one/。
//
//   node tools/publish.mjs <资产目录> [--notes "..."] [--dry-run]
//
// 顺序：本机校验、给更新包签名 → 上传到新的版本目录（不可变）→ 从公网拉回来逐个比对哈希、核对签名
// → 最后才换 latest.json（官网下载）和 update/stable.json（软件自动更新）。中途任何一步失败，线上清单不变。
//   node tools/publish.mjs <资产目录> --notes 说明.md [--notes-en notes.md] [--rollout 10] [--min-version x.y.z] [--dry-run]
//   node tools/publish.mjs rollout <0-100>      改已发布版本的推送比例（先 10，没问题再 100）
//   node tools/publish.mjs block <版本>          撤回一个版本：还没更新的不会再拿到它
// 资产按文件名认：PonteX-One-<版本>-macOS-universal.dmg / -Windows-x64.msi / -Windows-arm64.msi / …
// 自动更新的签名：私钥只在发布者这台电脑（~/.pontex-release/pontex-one-updater.key，密码在钥匙串
// 「PonteX One updater key」），CI 拿不到；公钥编进软件（tauri.pontex.json）。
import { createHash, createPublicKey, verify as edVerify } from "node:crypto";
import { homedir } from "node:os";
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, statSync } from "node:fs";
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
/** 自动更新用的包 → 更新清单里的平台。msi 既是下载包也是更新包。 */
const UPDATE_KINDS = {
  "macOS-universal.app.tar.gz": ["darwin-aarch64", "darwin-x86_64"],
  "Windows-x64.msi": ["windows-x86_64"],
  "Windows-arm64.msi": ["windows-aarch64"],
};
const KEY_FILE = process.env.PONTEX_UPDATE_KEY ?? join(homedir(), ".pontex-release/pontex-one-updater.key");
const KEYCHAIN_ITEM = "PonteX One updater key";
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
if (!dir) die("用法：publish <资产目录> --notes 说明.md [--dry-run] ｜ rollout <0-100> ｜ block <版本>");
const flag = (name) => {
  const at = flags.indexOf(name);
  return at >= 0 ? flags[at + 1] : undefined;
};
const text = (value) => (value && statSafe(value) ? readFileSync(value, "utf8") : value ?? "").trim();
function statSafe(path) {
  try {
    return statSync(path).isFile();
  } catch {
    return false;
  }
}
const UPDATE_MANIFEST = `${BASE}/update/stable.json`;
if (dir === "rollout" || dir === "block") await adjust(dir, flags[0]);
const dryRun = flags.includes("--dry-run");
const version = JSON.parse(readFileSync(join(ROOT, "src-tauri/pontex/tauri.pontex.json"), "utf8")).version;
const engine = JSON.parse(readFileSync(join(ROOT, "pontex/upstream.json"), "utf8")).tag.replace(/^v/, "");
if (!/^\d+\.\d+\.\d+$/.test(version)) die(`版本号不对：${version}`);

const prefix = `PonteX-One-${version}-`;
const assets = {};
for (const file of readdirSync(dir).filter((f) => f.startsWith(prefix))) {
  const kind = file.slice(prefix.length);
  if (kind.endsWith(".sig")) continue;
  const id = KINDS[kind] ?? (UPDATE_KINDS[kind] ? `update:${kind}` : null);
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

// 官网下载页只列安装包；更新包（.app.tar.gz）只进更新清单。
const downloads = Object.fromEntries(Object.entries(assets).filter(([id]) => !id.startsWith("update:")));
const latest = { version, channel, date: new Date().toISOString().slice(0, 10), engine, assets: downloads };

// ---------- 自动更新：签名、清单 ----------
const pubkey = JSON.parse(readFileSync(join(ROOT, "src-tauri/pontex/tauri.pontex.json"), "utf8")).plugins?.updater?.pubkey;
if (!pubkey) die("tauri.pontex.json 里没有更新公钥");
const notes = text(flag("--notes"));
const notesEn = text(flag("--notes-en"));
if (!notes) die("写几条这次更新了什么：--notes 说明.md（一行一条，用户会在「有新版本」窗口里看到）");
const platforms = {};
const signatures = {};
for (const [kind, targets] of Object.entries(UPDATE_KINDS)) {
  const file = `${prefix}${kind}`;
  if (!statSafe(join(dir, file))) continue;
  signatures[file] = dryRun ? "(dry-run)" : sign(join(dir, file));
  if (!dryRun && !verifySignature(readFileSync(join(dir, file)), signatures[file])) die(`${file} 签完核对不过：私钥和软件里的公钥不是一对`);
  for (const target of targets) platforms[target] = { url: `${SITE}/downloads/one/${version}/${file}`, signature: signatures[file] };
}
// 同一版本后补平台：保留线上更新清单里已有的平台（签名不变），只加新的。
if (current?.version === version) {
  const online = await fetch(`${SITE}/downloads/one/update/stable.json?t=${Date.now()}`).then((r) => (r.ok ? r.json() : null)).catch(() => null);
  if (online?.version === version) for (const [target, entry] of Object.entries(online.platforms ?? {})) platforms[target] ??= entry;
}
// 新版本必须带齐 macOS 和 Windows 的更新包（同一版本后补平台时例外）。
if (current?.version !== version) for (const need of ["darwin-aarch64", "windows-x86_64", "windows-aarch64"]) if (!platforms[need]) die(`缺少 ${need} 的更新包`);
const rollout = Number(flag("--rollout") ?? 100);
if (!(rollout >= 0 && rollout <= 100)) die("--rollout 是 0 到 100");
const update = {
  version,
  notes,
  ...(notesEn ? { notes_en: notesEn } : {}),
  pub_date: new Date().toISOString(),
  rollout,
  min_version: flag("--min-version") ?? null,
  blocked: [],
  platforms,
};
console.log(JSON.stringify(latest, null, 2));
console.log(JSON.stringify({ ...update, platforms: Object.fromEntries(Object.entries(platforms).map(([k, v]) => [k, v.url])) }, null, 2));
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
// 先核对公网上的包（版本目录不依赖清单），全对了才换清单：用户永远不会拿到对不上的包。
console.log("[3/4] 从公网拉回来核对（哈希、更新包的签名）");
for (const asset of Object.values(assets)) {
  const bytes = await retry(asset.file, async () => {
    const response = await fetch(`${SITE}${asset.url}?t=${Date.now()}`);
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return Buffer.from(await response.arrayBuffer());
  });
  if (sha256(bytes) !== asset.sha256) die(`${asset.file} 公网内容不对，清单没有切换`);
  if (signatures[asset.file] && !verifySignature(bytes, signatures[asset.file])) die(`${asset.file} 公网上的包签名核对不过，清单没有切换`);
  console.log(`  ✓ ${asset.url}  ${(asset.size / 1048576).toFixed(1)} MB${signatures[asset.file] ? "  签名对" : ""}`);
}

console.log("[4/4] 切换清单：latest.json（官网下载）和 update/stable.json（自动更新），原子替换、旧的留底");
await putJson(`${BASE}/latest.json`, latest);
await putJson(UPDATE_MANIFEST, update);
console.log(`✓ 已发布 ${version}（自动更新推送给 ${rollout}% 的用户${rollout < 100 ? "；没问题后运行 node tools/publish.mjs rollout 100" : ""}）`);

// ---------- 工具 ----------

async function putJson(path, value) {
  const body = `${JSON.stringify(value, null, 2)}\n`;
  const dirName = path.slice(0, path.lastIndexOf("/"));
  const name = path.slice(path.lastIndexOf("/") + 1);
  await retry(name, async () =>
    ssh(`mkdir -p ${dirName} && cat > ${dirName}/.${name}.new && { [ ! -f ${path} ] || cp --remove-destination ${path} ${dirName}/.${name}.prev; } && mv ${dirName}/.${name}.new ${path}`, body),
  );
}

/** 用本机私钥给更新包签名（tauri signer = minisign）。返回 .sig 的内容。 */
function sign(file) {
  if (!statSafe(KEY_FILE)) die(`找不到更新签名私钥 ${KEY_FILE}`);
  const password = execFileSync("security", ["find-generic-password", "-s", KEYCHAIN_ITEM, "-w"], { encoding: "utf8" }).trim();
  const bin = join(ROOT, "node_modules/.bin/tauri");
  execFileSync(bin, ["signer", "sign", "-f", KEY_FILE, "-p", password, file], { stdio: ["ignore", "ignore", "inherit"] });
  return readFileSync(`${file}.sig`, "utf8").trim();
}

/** 按 minisign 的格式核对签名（和软件里 tauri 更新组件做的一样）：先核对文件，再核对受信任的注释。 */
function verifySignature(bytes, signature) {
  try {
    const keyLine = Buffer.from(pubkey, "base64").toString("utf8").trim().split("\n").pop();
    const key = Buffer.from(keyLine, "base64");
    const raw = Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), key.subarray(10, 42)]);
    const publicKey = createPublicKey({ key: raw, format: "der", type: "spki" });
    const lines = Buffer.from(signature, "base64").toString("utf8").trim().split("\n");
    const sig = Buffer.from(lines[1], "base64");
    if (!sig.subarray(2, 10).equals(key.subarray(2, 10))) return false; // 不是这把钥匙签的
    const algorithm = sig.subarray(0, 2).toString("latin1");
    const message = algorithm === "ED" ? createHash("blake2b512").update(bytes).digest() : bytes;
    if (!edVerify(null, message, publicKey, sig.subarray(10, 74))) return false;
    const trusted = lines[2].replace(/^trusted comment: /, "");
    return edVerify(null, Buffer.concat([sig.subarray(10, 74), Buffer.from(trusted)]), publicKey, Buffer.from(lines[3], "base64"));
  } catch {
    return false;
  }
}

/** 已发布版本：改推送比例，或撤回一个版本。 */
async function adjust(what, value) {
  const manifest = await fetch(`${SITE}/downloads/one/update/stable.json?t=${Date.now()}`).then((r) => (r.ok ? r.json() : null));
  if (!manifest) die("线上还没有更新清单");
  if (what === "rollout") {
    const n = Number(value);
    if (!(n >= 0 && n <= 100)) die("用法：rollout <0-100>");
    manifest.rollout = n;
  } else {
    if (!/^\d+\.\d+\.\d+$/.test(value ?? "")) die("用法：block <版本>");
    manifest.blocked = [...new Set([...(manifest.blocked ?? []), value])];
  }
  await putJson(UPDATE_MANIFEST, manifest);
  console.log(`✓ ${what === "rollout" ? `${manifest.version} 推送给 ${manifest.rollout}% 的用户` : `已撤回 ${value}`}`);
  process.exit(0);
}
