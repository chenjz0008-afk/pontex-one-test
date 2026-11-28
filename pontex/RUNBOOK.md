# PonteX One 发布与事故手册

只写会用到的命令。原理见 `tools/publish.mjs`、`src-tauri/src/pontex/update.rs` 开头的注释。

## 密钥在哪（丢了无法补救，务必另有加密备份）

| 东西 | 位置 | 丢了 / 泄露 |
|---|---|---|
| 自动更新签名私钥 | `~/.pontex-release/pontex-one-updater.key`，密码在钥匙串「PonteX One updater key」 | 丢了：老用户再也收不到自动更新，只能去官网下载。泄露：见下文「换密钥」 |
| 自动更新公钥 | `src-tauri/pontex/tauri.pontex.json` → `plugins.updater.pubkey` | 公开的，无所谓 |
| 源站地址 | 不进仓库；发布时用环境变量 `PONTEX_REMOTE` | — |

## 发一个版本

1. 改 `src-tauri/pontex/tauri.pontex.json` 的 `version`，提交。
2. 同步到测试仓库并打包：`node tools/snapshot.mjs <测试仓库目录> --tag pontex-v<版本> --push`。
3. 打包完成后跑冒烟：`node tools/snapshot.mjs <测试仓库目录> --tag pontex-smoke-<n> --push`。
   改了安装 / 更新相关代码，再加 `--tag pontex-update-e2e-<n>`、`--tag pontex-e2e-<n>`。
4. 取回安装包和冒烟报告：`gh run download <打包 run> -D <资产目录>`、`gh run download <冒烟 run> -D <报告目录>`。
5. 写 3–5 条更新说明（用户在「有新版本」窗口里看到，一行一条），然后：
   `node tools/publish.mjs <资产目录> --notes 说明.md --smoke <报告目录> --rollout 10`
6. 一两天没问题：`node tools/publish.mjs rollout 100`。
7. 官网：`docs-pontex/config/pontex-one.json` 的 `release` 换成新的 `latest.json`，部署。

`publish.mjs` 自己保证：只发冒烟测试过、哈希一致的包；本机签名，从公网拉回来核对签名后才切换清单；版本目录不可变。

## 出事了

**新版本有问题**
1. 立刻止损：`node tools/publish.mjs rollout 0`（还没更新的人不会再拿到）。
2. 已经更新的人不会被降级：修好后发一个更高的版本（软件上次没跑稳会在启动 3 秒后查更新）。
3. 不想再让任何人拿到那个版本：`node tools/publish.mjs block <版本>`。
4. 必须尽快让所有人更新（安全问题）：发布时加 `--min-version <修复版>`，不受推送比例限制，设置里会提示必须更新。

**更新签名私钥泄露**
1. 新生成一把：`npx tauri signer generate -w <新路径>`。
2. 发一个「过渡版本」：配置里换成新公钥，但用**旧私钥**签名发布（老用户只认旧公钥）。
3. 等大多数人更新到过渡版本后，以后都用新私钥。旧私钥销毁。

**源站不可用**：官网下载和自动更新都会失败，软件本身不受影响（检查失败不打扰用户，下次再试）。恢复源站即可；清单和版本目录在 `/var/www/ponte-x-docs/shared/downloads/one/`。

**清单写坏了**：`downloads/one/.latest.json.prev`、`downloads/one/update/.stable.json.prev` 是上一份，`mv` 回去。
