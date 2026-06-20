//! 测试专用辅助：为 skill 相关测试提供唯一的临时目录路径。
//!
//! 历史上 skill 测试普遍使用固定路径（`temp_dir().join("aemeath_test_skill_N")`），
//! 在并发测试或上次清理失败（macOS/APFS 偶发 ENOTEMPTY）时会撞上残留目录，
//! 导致 `create_dir_all` 抛 `AlreadyExists` 而 flaky（见 stop hook 复现的
//! `test_parse_skill_alias_from_dir`）。这里用 `pid + 全局原子计数器 + 名字`
//! 生成唯一路径，从根上消除冲突。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// 为 skill 测试返回唯一的临时目录路径。
///
/// 组成：`<temp_dir>/aemeath_skill_test_<pid>_<counter>_<name>`，
/// 保证跨进程、跨测试函数、跨次运行互不冲突。调用方自行 `create_dir_all`。
pub fn unique_skill_dir(name: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "aemeath_skill_test_{}_{}_{}",
        std::process::id(),
        n,
        name
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unique_skill_dir_includes_pid_name_and_unique_per_call() {
        let a = unique_skill_dir("foo");
        let b = unique_skill_dir("foo");
        // 同名连续调用必须得到不同路径（计数器递增）
        assert_ne!(a, b, "consecutive calls must differ");
        // 路径位于系统 temp_dir 之下
        assert!(
            a.starts_with(std::env::temp_dir()),
            "path should live under temp_dir"
        );
        // 文件名包含调用方提供的 name
        let a_name = a.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            a_name.contains("_foo"),
            "path file name should embed provided name: {a_name}"
        );
        // 包含 pid，避免跨进程冲突
        assert!(
            a_name.contains(&format!("_{}_", std::process::id())),
            "path file name should embed pid: {a_name}"
        );
    }
}
