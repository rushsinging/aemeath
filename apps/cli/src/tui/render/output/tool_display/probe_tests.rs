#[test]
fn probe_snake_lookup() {
    let hit = crate::tui::render::output::tool_display::registry::lookup_display("workflow_create");
    let hit2 = crate::tui::render::output::tool_display::registry::lookup_display("workflow_query");
    let hit3 = crate::tui::render::output::tool_display::registry::lookup_display("workflow_node_update");
    let hit4 = crate::tui::render::output::tool_display::registry::lookup_display("workflow_update");
    println!("snake hits: {} {} {} {}", hit.is_some(), hit2.is_some(), hit3.is_some(), hit4.is_some());
    let (line, _) = crate::tui::render::output::tool_display::format_tool_call(
        "workflow_create",
        r#"{"name":"显示验收"}"#,
        None,
        None,
    );
    println!("header spans: {:?}", line.spans.iter().map(|s| s.content.to_string()).collect::<Vec<_>>());
}
