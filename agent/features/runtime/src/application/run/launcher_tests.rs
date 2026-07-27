#[test]
fn legacy_launcher_entry_is_retired_after_production_migration() {
    let source = include_str!("launcher.rs");
    assert!(!source.contains("pub async fn launch<P>"));
    assert!(!source.contains("迁移期兼容入口"));
}
