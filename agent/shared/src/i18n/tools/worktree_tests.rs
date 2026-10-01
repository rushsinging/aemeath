use super::*;

#[test]
fn descriptions_are_bilingual_and_fallback_en() {
    assert!(enter_description("zh").contains("进入"));
    assert!(enter_description("en").contains("Enter"));
    assert_eq!(enter_description("fr"), enter_description("en"));
    assert!(exit_description("zh").contains("退出"));
    assert!(exit_description("en").contains("Exit"));
    assert_eq!(exit_description("xx"), exit_description("en"));
}

#[test]
fn enter_guidance_distinguishes_path_base_and_workspace_root() {
    let zh = enter_guidance("zh");
    let en = enter_guidance("en");
    for s in [&zh, &en] {
        assert!(s.contains("path_base"), "guidance must mention path_base");
        assert!(
            s.contains("workspace_root"),
            "guidance must mention workspace_root"
        );
    }
    assert_eq!(enter_guidance("fr"), en);
}

#[test]
fn exit_guidance_includes_restored_target() {
    let g = exit_guidance("en", std::path::Path::new("/tmp/foo"));
    assert!(g.contains("/tmp/foo"));
    assert!(g.contains("path_base"));
    let zh = exit_guidance("zh", std::path::Path::new("/tmp/foo"));
    assert!(zh.contains("/tmp/foo"));
    assert!(zh.contains("path_base"));
}

#[test]
fn errors_are_bilingual() {
    assert!(enter_error("zh", "x").contains("失败"));
    assert!(enter_error("en", "x").contains("Failed"));
}
