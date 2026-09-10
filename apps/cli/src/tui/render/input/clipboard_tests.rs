use super::*;

#[test]
fn test_copy_text_empty_is_ok() {
    assert!(copy_text("").is_ok());
}

#[test]
fn test_copy_text_command_failure_returns_chinese_error() {
    let error = copy_text("测试").err();

    if cfg!(target_os = "macos") {
        assert!(error.is_none() || error.unwrap().contains("剪贴板"));
    } else {
        assert!(error
            .expect("non-macOS pbcopy should fail")
            .contains("无法启动剪贴板命令 pbcopy"));
    }
}
