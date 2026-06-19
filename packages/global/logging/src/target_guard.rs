//! 日志 target 架构守卫。
//!
//! 确保各 crate 的 `log::xxx!` 调用正确携带 `target:` 参数，
//! 避免日志被路由到错误的文件。

use std::fs;
use std::path::{Path, PathBuf};

/// 合法 target 白名单 —— 所有 `LOG_TARGET` 常量的值。
/// 每条必须以 `aemeath:` 开头，最多 3 段（`aemeath:<domain>:<crate>` 或 `aemeath:<name>`）。
const ALLOWED_TARGETS: &[&str] = &[
    "aemeath:tui",
    "aemeath:composition",
    "aemeath:shared",
    "aemeath:agent:policy",
    "aemeath:agent:project",
    "aemeath:agent:hook",
    "aemeath:agent:storage",
    "aemeath:agent:provider",
    "aemeath:agent:audit",
    "aemeath:agent:prompt",
    "aemeath:agent:runtime",
    "aemeath:agent:tools",
];

/// 递归收集目录下所有 `.rs` 文件。
fn rust_files_under(path: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    files
}

/// 从源码中剥离 `#[cfg(test)] mod` 块，只保留生产代码。
fn production_source(source: &str) -> String {
    let mut output = String::new();
    let mut skip_test_module = false;
    let mut brace_depth = 0usize;

    for line in source.lines() {
        if line.trim() == "#[cfg(test)]" {
            skip_test_module = true;
            continue;
        }
        if skip_test_module {
            let opens = line.matches('{').count();
            let closes = line.matches('}').count();
            if opens > 0 || brace_depth > 0 {
                brace_depth = brace_depth.saturating_add(opens).saturating_sub(closes);
                if brace_depth == 0 {
                    skip_test_module = false;
                }
            }
            continue;
        }
        output.push_str(line);
        output.push('\n');
    }
    output
}

/// 检查源码中是否有裸 `log::xxx!(` 调用（不含 `target:`）。
fn has_bare_log_calls(source: &str) -> Vec<String> {
    let patterns = [
        "log::trace!(",
        "log::debug!(",
        "log::info!(",
        "log::warn!(",
        "log::error!(",
    ];
    let lines: Vec<&str> = source.lines().collect();
    let mut violations = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with("/*") {
            continue;
        }
        for pat in &patterns {
            if trimmed.contains(pat) {
                // Check if target: appears on this line or the next 3 lines
                let context = lines[i..(i + 4).min(lines.len())].join(" ");
                if !context.contains("target:") {
                    violations.push(trimmed.to_string());
                }
            }
        }
    }
    violations
}

/// 从单行中提取 `target: "xxx"` 的字符串值。
fn extract_target_value(line: &str) -> Option<String> {
    // 查找 target: "..." 模式
    let target_idx = line.find("target:")?;
    let after = &line[target_idx + 7..];
    // 跳过空格
    let after = after.trim_start();
    // 检查是否有引号（字符串字面量）
    if !after.starts_with('"') {
        // target: 不是字符串字面量 → 引用常量（如 LOG_TARGET），合法
        return None;
    }
    let inner = &after[1..];
    let end_quote = inner.find('"')?;
    Some(inner[..end_quote].to_string())
}

/// 检查 target 字符串字面量是否合法（在白名单内）。
fn is_valid_target(target: &str) -> bool {
    // 必须以 aemeath: 开头
    if !target.starts_with("aemeath:") {
        return false;
    }
    // 最多 3 段
    let parts: Vec<&str> = target.split(':').collect();
    if parts.len() > 3 {
        return false;
    }
    // 必须在白名单内
    ALLOWED_TARGETS.contains(&target)
}

/// 检查源码中所有 `target: "xxx"` 字符串字面量是否合规。
fn validate_target_values(source: &str) -> Vec<String> {
    let mut violations = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with("/*") {
            continue;
        }
        if let Some(target_val) = extract_target_value(line) {
            if !is_valid_target(&target_val) {
                violations.push(format!("invalid target: \"{}\"", target_val));
            }
        }
    }
    violations
}

/// workspace 根目录。
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf()
}

mod tests {
    use super::*;

    fn check_layer(dir_name: &str, target_prefix: &str) {
        let root = workspace_root();
        let dir = root.join(dir_name);
        if !dir.exists() {
            return;
        }
        for file in rust_files_under(&dir) {
            if file
                .file_name()
                .is_some_and(|n| n.to_string_lossy().contains("test"))
            {
                continue;
            }
            let raw_source = fs::read_to_string(&file).expect("read rust source");
            let source = production_source(&raw_source);

            // 1. 检查裸 log::xxx! 调用（无 target）
            let bare_violations = has_bare_log_calls(&source);
            assert!(
                bare_violations.is_empty(),
                "{} production code must not use bare log::xxx! — use target: \"{}*\" instead.\nViolations:\n{}",
                file.display(),
                target_prefix,
                bare_violations.join("\n")
            );

            // 2. 检查 target 字符串字面量是否合规
            let target_violations = validate_target_values(&source);
            assert!(
                target_violations.is_empty(),
                "{} production code uses invalid log target string literal.\nAllowed: {:#?}\nViolations:\n{}",
                file.display(),
                ALLOWED_TARGETS,
                target_violations.join("\n")
            );
        }
    }

    #[test]
    fn tui_layer_must_not_use_bare_log_macros() {
        check_layer("apps/cli/src/tui", "cli::");
    }

    #[test]
    fn chat_layer_must_not_use_bare_log_macros() {
        check_layer("apps/cli/src/chat", "cli::");
    }

    #[test]
    fn hook_layer_must_not_use_bare_log_macros() {
        check_layer("agent/features/hook/src", "hook::");
    }

    #[test]
    fn runtime_layer_must_not_use_bare_log_macros() {
        check_layer("agent/features/runtime/src", "runtime::");
    }

    #[test]
    fn provider_layer_must_not_use_bare_log_macros() {
        check_layer("agent/features/provider/src", "provider::");
    }

    #[test]
    fn tools_layer_must_not_use_bare_log_macros() {
        check_layer("agent/features/tools/src", "tools::");
    }

    #[test]
    fn prompt_layer_must_not_use_bare_log_macros() {
        check_layer("agent/features/prompt/src", "prompt::");
    }

    #[test]
    fn storage_layer_must_not_use_bare_log_macros() {
        check_layer("agent/features/storage/src", "storage::");
    }

    #[test]
    fn allowed_targets_whitelist_is_valid() {
        // 自检：白名单中每个 target 必须以 aemeath: 开头且最多 3 段
        for target in ALLOWED_TARGETS {
            assert!(
                target.starts_with("aemeath:"),
                "ALLOWED_TARGETS entry '{}' must start with 'aemeath:'",
                target
            );
            let parts: Vec<&str> = target.split(':').collect();
            assert!(
                parts.len() <= 3,
                "ALLOWED_TARGETS entry '{}' has more than 3 colon-separated segments",
                target
            );
        }
    }
}
