//! 日志 target 架构守卫。
//!
//! 确保各 crate 的 `log::xxx!` 调用正确携带 `target:` 参数，
//! 避免日志被路由到错误的文件。

use std::fs;
use std::path::{Path, PathBuf};

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
            if file.file_name().is_some_and(|n| n.to_string_lossy().contains("test")) {
                continue;
            }
            let source = production_source(
                &fs::read_to_string(&file).expect("read rust source"),
            );
            let violations = has_bare_log_calls(&source);
            assert!(
                violations.is_empty(),
                "{} production code must not use bare log::xxx! — use target: \"{}*\" instead.\nViolations:\n{}",
                file.display(),
                target_prefix,
                violations.join("\n")
            );
        }
    }

    #[test]
    fn tui_layer_must_not_use_bare_log_macros() {
        check_layer("apps/cli/src/tui", "cli::");
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
}
