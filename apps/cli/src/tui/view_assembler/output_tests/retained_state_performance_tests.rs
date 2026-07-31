use super::super::OutputViewAssembler;
use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId};
use crate::tui::model::conversation::intent::{
    AppendUserMessage, AssistantText, ConversationIntent, RecordAgentProgress, RunCompleted,
    RunStarted, RunStepCompleted, RunStepStarted, ToolCallStart, ToolResult,
};
use crate::tui::model::conversation::interaction::{UiRunId, UiRunStepId};
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::render::output::document_renderer::OutputDocumentRenderer;
use crate::tui::render::output::spacing::MarkdownSpacingPolicy;

fn build_retained_state_workload(scale: usize) -> ConversationModel {
    let mut model = ConversationModel::default();
    for index in 0..scale {
        let chat_id = ChatId::new(format!("chat-{index}"));
        let turn_id = ChatTurnId::new(format!("turn-{index}"));
        let tool_id = ToolCallId::new(format!("tool-{index}"));
        let run_id_text = format!("run-{index}");
        let step_id_text = format!("step-{index}");
        let run_id = UiRunId::from(run_id_text.as_str());
        let step_id = UiRunStepId::from(step_id_text.as_str());

        model.apply(AppendUserMessage {
            text: format!("user-{index}"),
        });
        model.apply(AssistantText {
            chat_id: chat_id.clone(),
            turn_id: turn_id.clone(),
            text: format!("assistant-{index}"),
        });
        model.apply(ToolCallStart {
            chat_id: chat_id.clone(),
            turn_id: turn_id.clone(),
            id: tool_id.clone(),
            provider_id: None,
            name: "Agent".to_string(),
            index: 0,
        });
        model.apply(RecordAgentProgress {
            chat_id: chat_id.clone(),
            turn_id: turn_id.clone(),
            tool_id: tool_id.clone(),
            message: format!("progress-{index}"),
        });
        model.apply(ToolResult {
            chat_id: chat_id.clone(),
            turn_id: turn_id.clone(),
            id: tool_id.clone(),
            provider_id: format!("provider-{index}"),
            tool_name: "Agent".to_string(),
            output: format!("done-{index}"),
            content: serde_json::json!({ "text": format!("done-{index}") }),
            is_error: false,
            image_count: 0,
        });
        model.apply(ConversationIntent::RunStarted(RunStarted {
            run_id: run_id.clone(),
        }));
        model.apply(ConversationIntent::RunStepStarted(RunStepStarted {
            run_id: run_id.clone(),
            step_id: step_id.clone(),
            tool_reference: Some(tool_id.to_string()),
        }));
        model.apply(ConversationIntent::RunStepCompleted(RunStepCompleted {
            run_id: run_id.clone(),
            step_id,
        }));
        model.apply(ConversationIntent::RunCompleted(RunCompleted { run_id }));
    }
    model
}

#[test]
fn retained_state_workload_is_deterministic_at_representative_scales() {
    for scale in [100usize, 500, 1000] {
        let model = build_retained_state_workload(scale);
        let retained = model.retained_state_snapshot();
        let vm = OutputViewAssembler::assemble_from_conversation(&model, model.revision(), None);
        let mut renderer = OutputDocumentRenderer::default();
        renderer.render_model_document(&vm, 100, 100, 0, MarkdownSpacingPolicy::normal());
        let cold = renderer.retained_cache_capacity();
        renderer.render_model_document(&vm, 100, 100, 1, MarkdownSpacingPolicy::normal());
        renderer.render_model_document(&vm, 120, 120, 2, MarkdownSpacingPolicy::normal());
        let warm_resized = renderer.retained_cache_capacity();

        assert_eq!(retained.chats, scale);
        assert_eq!(retained.turns, scale);
        assert_eq!(retained.tool_calls, scale);
        assert_eq!(retained.agent_progress_entries, scale);
        assert_eq!(retained.agent_runs, scale);
        assert_eq!(retained.agent_run_steps, scale);
        assert_eq!(retained.terminal_agent_runs, scale);
        assert_eq!(retained.timeline_items, scale * 4);
        assert!(retained.output_view_journal_entries <= 256);
        assert!(retained.output_view_journal_item_id_bytes <= 256 * 64);
        assert_eq!(warm_resized.block_entries, cold.block_entries);
        assert_eq!(warm_resized.gutted_entries, cold.gutted_entries);
        assert_eq!(warm_resized.root_layout_entries, cold.root_layout_entries);
        assert_eq!(warm_resized.peak_block_entries, cold.peak_block_entries);
        assert_eq!(warm_resized.peak_gutted_entries, cold.peak_gutted_entries);
        assert_eq!(
            warm_resized.peak_root_layout_entries,
            cold.peak_root_layout_entries
        );
    }
}

#[test]
#[ignore = "性能/容量基线；手动运行：cargo test -p cli --release retained_state_release_workload -- --ignored --nocapture"]
#[allow(clippy::print_stdout)]
fn retained_state_release_workload() {
    for scale in [100usize, 500, 1000] {
        let model = build_retained_state_workload(scale);
        let retained = model.retained_state_snapshot();
        let assemble_started = std::time::Instant::now();
        let vm = OutputViewAssembler::assemble_from_conversation(&model, model.revision(), None);
        let assemble_ns = assemble_started.elapsed().as_nanos();
        let mut renderer = OutputDocumentRenderer::default();
        let cold_started = std::time::Instant::now();
        renderer.render_model_document(&vm, 100, 100, 0, MarkdownSpacingPolicy::normal());
        let cold_ns = cold_started.elapsed().as_nanos();
        let warm_started = std::time::Instant::now();
        renderer.render_model_document(&vm, 100, 100, 1, MarkdownSpacingPolicy::normal());
        let warm_ns = warm_started.elapsed().as_nanos();
        let cache = renderer.retained_cache_capacity();

        println!(
            "scale={scale:>4} timeline={} progress={} progress_bytes={} runs={} steps={} terminal_runs={} view_journal={} view_id_bytes={} roots={} assemble_ms={:.2} cold_ms={:.2} warm_ms={:.2} cache(block/gutted/layout)={}/{}/{} peak={}/{}/{}",
            retained.timeline_items,
            retained.agent_progress_entries,
            retained.agent_progress_bytes,
            retained.agent_runs,
            retained.agent_run_steps,
            retained.terminal_agent_runs,
            retained.output_view_journal_entries,
            retained.output_view_journal_item_id_bytes,
            vm.roots.len(),
            assemble_ns as f64 / 1_000_000.0,
            cold_ns as f64 / 1_000_000.0,
            warm_ns as f64 / 1_000_000.0,
            cache.block_entries,
            cache.gutted_entries,
            cache.root_layout_entries,
            cache.peak_block_entries,
            cache.peak_gutted_entries,
            cache.peak_root_layout_entries,
        );
    }
}
