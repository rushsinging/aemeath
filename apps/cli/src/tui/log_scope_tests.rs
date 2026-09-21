#[cfg(test)]
mod tests {
    const FOCUSED_LOG_FILES: &[&str] = &[
        "apps/cli/src/tui/adapter/agent_event.rs",
        "apps/cli/src/tui/model/conversation/tool_observe.rs",
        "apps/cli/src/tui/model/conversation/tool_flow.rs",
        "apps/cli/src/tui/render/output/blocks/tool_call.rs",
        "apps/cli/src/tui/render/output/blocks/tool_result.rs",
        "apps/cli/src/tui/view_assembler/output_tool_view.rs",
        "apps/cli/src/tui/app/update.rs",
        "apps/cli/src/tui/view_state/run_activity.rs",
    ];

    const FORBIDDEN_HIGH_VOLUME_LOG_FILES: &[&str] = &[
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
    fn test_high_volume_render_paths_do_not_emit_debug_logs() {
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
    fn activity_timing_diagnostics_include_cross_layer_identity_and_elapsed_fields() {
        let runtime_source =
            workspace_file("agent/features/runtime/src/application/activity/coordinator.rs");
        let processing_source =
            workspace_file("apps/cli/src/tui/effect/session/processing/logging.rs");
        let model_source = workspace_file("apps/cli/src/tui/model/conversation/model.rs");
        let summary_source = workspace_file("apps/cli/src/tui/app/update.rs");
        let state_source = workspace_file("apps/cli/src/tui/view_state/run_activity.rs");
        let combined = format!(
            "{runtime_source}\n{processing_source}\n{model_source}\n{summary_source}\n{state_source}"
        );

        for marker in [
            "[ACTIVITY_TIMING] runtime_snapshot",
            "[ACTIVITY_TIMING] sdk_ingress",
            "[ACTIVITY_TIMING] mirror_increment",
            "[ACTIVITY_TIMING] mirror_snapshot",
            "[ACTIVITY_TIMING] summary_selected",
            "[ACTIVITY_TIMING] state_sync",
            "root_activity_id",
            "primary_activity_id",
            "root_revision",
            "phase_revision",
            "total_elapsed_ms",
            "phase_elapsed_ms",
        ] {
            assert!(
                combined.contains(marker),
                "missing activity timing diagnostic field or marker: {marker}"
            );
        }
    }

    #[test]
    fn activity_timing_diagnostics_do_not_log_from_per_frame_render_paths() {
        const FORBIDDEN_ACTIVITY_TIMING_RENDER_FILES: &[&str] = &[
            "apps/cli/src/tui/render/output/document_renderer.rs",
            "apps/cli/src/tui/render/output_area/render.rs",
            "apps/cli/src/tui/render/output/status_line.rs",
            "apps/cli/src/tui/render/output_area/selection.rs",
            "apps/cli/src/tui/update/root_reducer.rs",
        ];
        for path in FORBIDDEN_ACTIVITY_TIMING_RENDER_FILES {
            let source = workspace_file(path);
            assert!(
                !source.contains("[ACTIVITY_TIMING]"),
                "per-frame path must not emit activity timing diagnostics: {path}"
            );
        }
    }

    #[test]
    fn test_tui_keeps_tool_and_activity_timing_diagnostic_logs() {
        let mut combined = String::new();
        for path in FOCUSED_LOG_FILES {
            combined.push_str(&workspace_file(path));
            combined.push('\n');
        }

        for marker in [
            "model observe tool_call_start",
            "render tool_call block_id",
            "render tool_result block_id",
            "[ACTIVITY_TIMING] summary_selected",
            "[ACTIVITY_TIMING] state_sync",
        ] {
            assert!(
                combined.contains(marker),
                "missing focused diagnostic marker: {marker}"
            );
        }
    }
}
