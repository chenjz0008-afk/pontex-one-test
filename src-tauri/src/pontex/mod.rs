//! PonteX One 经纬即连。
//!
//! 和上游在同一个 crate 里，但有自己的入口、命令、托盘和界面：上游的 `run()`、界面、托盘、
//! 启动流程、深链接一概不跑，只用它的引擎（供应商行、live 写入、崩溃恢复、工具检测）。

pub mod agents;
mod autostart;
mod baseline;
mod diagnose;
pub mod consts;
pub mod core;
mod installer;
mod key;
mod links;
mod live;
mod models;
mod overrides;
mod safe_path;
mod skills;
mod status;
mod store;
mod templates;
mod tray;
mod upstream;
mod verify;

#[cfg(test)]
mod architecture;
#[cfg(test)]
mod network_tests;
#[cfg(test)]
mod tests;

pub use app::run;

/// 给集成测试用：按正式入口的方式打开数据，返回快照。
#[doc(hidden)]
pub fn open_for_test() -> Result<String, String> {
    let core = core::Core::new(upstream::Engine::open()?);
    serde_json::to_string(&core.snapshot()).map_err(|e| e.to_string())
}
mod app;
