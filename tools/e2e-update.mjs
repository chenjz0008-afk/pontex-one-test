#!/usr/bin/env node
// 自动更新真机测试的小工具（只给 .github/workflows/pontex-update-e2e.yml 用）。
//   config <e2e.key.pub> <版本>                 换成临时公钥、改版本号
//   manifest <目录> <版本> <平台> <更新包文件名>  在目录里写 stable.json（指向 127.0.0.1:8765）
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const [command, ...args] = process.argv.slice(2);
if (command === "config") {
  const [pub, version] = args;
  const file = "src-tauri/pontex/tauri.pontex.json";
  const config = JSON.parse(readFileSync(file, "utf8"));
  config.version = version;
  config.plugins.updater.pubkey = readFileSync(pub, "utf8").trim();
  // 测试清单在 127.0.0.1 上走 http；只有这种测试包允许，正式包一律 https。
  config.plugins.updater.dangerousInsecureTransportProtocol = true;
  writeFileSync(file, `${JSON.stringify(config, null, 2)}\n`);
  console.log(`config: version ${version}, e2e pubkey`);
} else if (command === "manifest") {
  const [dir, version, platform, name] = args;
  const manifest = {
    version,
    notes: "- e2e：自动更新真机测试",
    pub_date: new Date().toISOString(),
    rollout: 100,
    min_version: null,
    blocked: [],
    platforms: { [platform]: { url: `http://127.0.0.1:8765/${name}`, signature: readFileSync(join(dir, `${name}.sig`), "utf8").trim() } },
  };
  writeFileSync(join(dir, "stable.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`manifest: ${platform} → ${name}`);
} else {
  console.error("用法：config <pub> <版本> | manifest <目录> <版本> <平台> <文件>");
  process.exit(2);
}
