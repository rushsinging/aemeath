//! Command（slash 命令）执行文案。
//!
//! T8-4：trait_command.rs 的 "未知命令" 错误（原中文硬编码）双语化。

/// 未知命令错误（返回给 CLI/UI）。
pub fn unknown_command(lang: &str, name: &str) -> String {
    match lang {
        "zh" => format!("未知命令: /{name}"),
        _ => format!("Unknown command: /{name}"),
    }
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod tests;
