use super::*;

#[test]
fn unknown_command_bilingual_and_fallback() {
    assert_eq!(unknown_command("zh", "foo"), "未知命令: /foo");
    assert_eq!(unknown_command("en", "foo"), "Unknown command: /foo");
    assert_eq!(unknown_command("fr", "foo"), unknown_command("en", "foo"));
}
