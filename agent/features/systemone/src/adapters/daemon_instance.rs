//! Scoring daemon 惰性拉起与 HTTP 就绪探活（#2096：UDS 退役）。
//!
//! 探活语义与原 UDS `UnixStream::connect` 等价：TCP connect 成功即 daemon
//! 在监听。端口 bind 互斥天然保证单实例，不再有 lock 文件。

use std::time::Duration;

/// 确保 local daemon HTTP 端点可用：先探活，不通则惰性拉起
/// `systemone serve`（同一进程二进制），等待端口就绪。
pub fn ensure_scoring_http_ready(endpoint: &str, connect_timeout: Duration) -> Result<(), String> {
    if scoring_http_connectable(endpoint) {
        return Ok(());
    }
    let executable = std::env::current_exe().map_err(|error| {
        format!("定位当前可执行文件失败，无法惰性拉起 systemone serve：{error}")
    })?;
    let mut command = std::process::Command::new(&executable);
    command
        .args(["systemone", "serve"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    utils::configure_std_noninteractive(&mut command)
        .map_err(|error| format!("配置 systemone serve 非交互隔离失败：{error}"))?;
    command
        .spawn()
        .map_err(|error| format!("惰性拉起 `systemone serve` 失败：{error}"))?;

    let started = std::time::Instant::now();
    while started.elapsed() < connect_timeout {
        if scoring_http_connectable(endpoint) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(format!(
        "等待 scoring daemon HTTP 就绪超时（{}s）；可手动执行 `aemeath systemone serve`，模型缺失时先 `aemeath systemone download`",
        connect_timeout.as_secs().max(1)
    ))
}

/// 从 endpoint URL 提取 `host:port` 并 TCP 探活。
fn scoring_http_connectable(endpoint: &str) -> bool {
    let Some(authority) = endpoint_authority(endpoint) else {
        return false;
    };
    std::net::TcpStream::connect(authority).is_ok()
}

/// 解析 `http[s]://host:port/path` 的 authority（host:port；缺省端口 80/443）。
fn endpoint_authority(endpoint: &str) -> Option<String> {
    let without_scheme = endpoint
        .strip_prefix("http://")
        .or_else(|| endpoint.strip_prefix("https://"))
        .unwrap_or(endpoint);
    let authority = without_scheme.split('/').next().unwrap_or("");
    if authority.is_empty() {
        return None;
    }
    if authority.contains(':') {
        Some(authority.to_string())
    } else if endpoint.starts_with("https://") {
        Some(format!("{authority}:443"))
    } else {
        Some(format!("{authority}:80"))
    }
}

#[cfg(test)]
#[path = "daemon_instance_tests.rs"]
mod tests;
