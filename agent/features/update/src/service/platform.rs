/// 返回当前平台的 Rust target triple（用于匹配 artifact 文件名）。
///
/// 支持的平台：
/// - macOS aarch64 → `aarch64-apple-darwin`
/// - macOS x86_64  → `x86_64-apple-darwin`
/// - Linux x86_64  → `x86_64-unknown-linux-gnu`
pub(super) fn platform_target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        _ => None,
    }
}

/// 构造 GitHub Release 下载 URL。
pub(super) fn download_url(version: &str, filename: &str) -> String {
    format!("https://github.com/rushsinging/aemeath/releases/download/v{version}/{filename}")
}
