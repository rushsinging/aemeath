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
mod tests {
    use super::*;

    #[test]
    fn unknown_command_bilingual_and_fallback() {
        assert_eq!(unknown_command("zh", "foo"), "未知命令: /foo");
        assert_eq!(unknown_command("en", "foo"), "Unknown command: /foo");
        assert_eq!(unknown_command("fr", "foo"), unknown_command("en", "foo"));
    }
}
