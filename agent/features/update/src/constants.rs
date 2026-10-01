//! Update crate 常量（#1146 双轨归位）。

pub(crate) const GITHUB_API_URL: &str =
    "https://api.github.com/repos/rushsinging/aemeath/releases/latest";

/// HTTP 请求超时（秒）—— 用于元数据请求（GitHub API JSON / checksums.txt）。
pub(crate) const REQUEST_TIMEOUT_SECS: u64 = 5;

/// 二进制下载超时（秒）—— 用于 tar.gz artifact（可达数 MB）。
/// 5s 对 6.5MB tar.gz 经常不够（含 TLS 握手 + GitHub 302 跳转），
/// 过短会中断 body 流导致 `error decoding response body`。见 issue #350。
pub(crate) const DOWNLOAD_TIMEOUT_SECS: u64 = 120;

pub(crate) const LOG_TARGET: &str = "aemeath:agent:update";
