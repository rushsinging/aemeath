#[cfg(test)]
mod tests {
    const FOCUSED_LOG_FILES: &[&str] = &[
        "apps/cli/src/tui/adapter/agent_event.rs",
        "apps/cli/src/tui/app/update/spinner.rs",
        "apps/cli/src/tui/model/conversation/tool_observe.rs",
        "apps/cli/src/tui/model/conversation/tool_flow.rs",
        "apps/cli/src/tui/render/output/blocks/tool_call.rs",
        "apps/cli/src/tui/render/output/blocks/tool_result.rs",
        "apps/cli/src/tui/view_assembler/output_tool_view.rs",
    ];

    const FORBIDDEN_HIGH_VOLUME_LOG_FILES: &[&str] = &[
        "apps/cli/src/tui/app/update.rs",
        "apps/cli/src/tui/render/output/document_renderer.rs",
        "apps/cli/src/tui/render/output_area/render.rs",
        "apps/cli/src/tui/render/output/status_line.rs",
        "apps/cli/src/tui/render/output_area/selection.rs",
        "apps/cli/src/tui/update/root_reducer.rs",
    ];

    fn workspace_file(path: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
    }

    #[test]
    fn test_tui_debug_logs_are_limited_to_tool_and_spinner_paths() {
        for path in FORBIDDEN_HIGH_VOLUME_LOG_FILES {
            let source = workspace_file(path);
            assert!(
                !source.contains("cli::tui::tool_flow")
                    && !source.contains("crate::tui::log_debug!")
                    && !source.contains("cli::tui::spinner_flow"),
                "high-volume render file must not emit TUI debug logs: {path}"
            );
        }
    }

    #[test]
    fn test_tui_keeps_tool_rendering_and_spinner_diagnostic_logs() {
        let mut combined = String::new();
        for path in FOCUSED_LOG_FILES {
            combined.push_str(&workspace_file(path));
            combined.push('\n');
        }

        for marker in [
            "map tool_call_start",
            "model observe tool_call_start",
            "render tool_call block_id",
            "render tool_result block_id",
            "[SPINNER_DEBUG] spinner_phase",
            "[SPINNER_DEBUG] spinner_stop",
        ] {
            assert!(
                combined.contains(marker),
                "missing focused diagnostic marker: {marker}"
            );
        }
    }
}
