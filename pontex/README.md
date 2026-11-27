# PonteX One 工程说明

- `src-tauri/src/pontex/`：Core。只有 `upstream.rs` 允许引用上游代码（`architecture.rs` 测试强制）。
- `src/pontex/`：界面，Rust 状态的投影。
- `src-tauri/pontex/`：覆盖层（标识、版本、图标、DMG、plist）。产品版本只在 `tauri.pontex.json`。
- `pontex/hooks.json`：对上游文件的**全部**改动，声明式。
- `pontex/upstream.json`：当前吸收的 CC Switch 版本。

```bash
node tools/pontex.mjs check            # 上游文件 == 上游原文 + hooks，一个字节都不能多
node tools/pontex.mjs sync v4.0.41     # 跟进上游：整份取新版 + 重放 hook，不做三方合并
node tools/pontex.mjs build --target universal-apple-darwin
node tools/fetch-artifacts.mjs          # 取回 GitHub 打好的包到 release/<版本>/
node tools/publish.mjs release/<版本>   # 上传 → 核对 → 原子切换 latest.json
cargo test --manifest-path src-tauri/Cargo.toml --lib pontex:: -- --test-threads=1
```

继承的上游工作流（`release.yml`、`sync-r2.yml`、`schema-version.yml`、`stale.yml`、`labeler.yml`、`wsl2-nightly.yml`）不改不删，在仓库设置里禁用。
