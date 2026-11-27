//! 产品常量。主机、协议、标识都编译进二进制，运行时没有任何入口能改。

pub const PRODUCT_ZH: &str = "PonteX One 经纬即连";
pub const PRODUCT_EN: &str = "PonteX One";
pub const API_ORIGIN: &str = "https://api.ponte-x.com";
pub const API_HOST: &str = "api.ponte-x.com";
pub const SKILLS_ORIGIN: &str = "https://skills.ponte-x.com";
pub const SCHEME: &str = "pontex-one";

/// 上游库里 PonteX 那一行的 id；每个 Agent 一行，重复连接只更新它。
pub const PROVIDER_ID: &str = "pontex-one";
/// 上游库里「用户自己的原生配置」「用户原来的别家网关」两行的 id。
pub const NATIVE_ID: &str = "pontex-one-native";
pub const PREVIOUS_ID: &str = "pontex-one-previous";

/// 请求用的来源。只有 debug 构建允许指到本机回环地址，供端到端测试使用。
pub fn api_origin() -> String {
    dev_override("PONTEX_ONE_DEV_API").unwrap_or_else(|| API_ORIGIN.to_string())
}

pub fn skills_origin() -> String {
    dev_override("PONTEX_ONE_DEV_SKILLS").unwrap_or_else(|| SKILLS_ORIGIN.to_string())
}

#[cfg(debug_assertions)]
fn dev_override(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|v| v.starts_with("http://127.0.0.1:"))
}

#[cfg(not(debug_assertions))]
fn dev_override(_name: &str) -> Option<String> {
    None
}
