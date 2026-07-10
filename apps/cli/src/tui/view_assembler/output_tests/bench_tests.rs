#[test]
#[ignore = "性能基准；手动运行：cargo test -p cli --release bench_refresh_cost_by_conversation_size -- --ignored --nocapture"]
#[allow(clippy::print_stdout)]
fn bench_refresh_cost_by_conversation_size() {
    use crate::tui::model::conversation::ids::{ChatId, ChatTurnId};
    use crate::tui::render::output::document_renderer::OutputDocumentRenderer;
    use std::time::Instant;

    fn build_conversation(n_turns: usize) -> ConversationModel {
        let mut conv = ConversationModel::default();
        let chat_id = ChatId::new("chat-main");
        for i in 0..n_turns {
            conv.apply(AppendUserMessage {
                text: format!("用户第 {i} 问，带一些上下文细节。"),
            });
            let turn_id = ChatTurnId::new(format!("turn-{i}"));
            conv.apply(AssistantText {
                chat_id: chat_id.clone(),
                turn_id: turn_id.clone(),
                text: format!("助手第 {i} 段回复。\n第二行内容稍长触发换行处理。\n第三行结束。"),
            });
            let tool_id = ToolCallId::new(format!("tool-{i}"));
            conv.apply(ToolCallStart {
                chat_id: chat_id.clone(),
                turn_id: turn_id.clone(),
                id: tool_id.clone(),
                provider_id: Some(format!("p{i}")),
                name: "Read".to_string(),
                index: 0,
            });
            conv.apply(ToolResult {
                chat_id: chat_id.clone(),
                turn_id: turn_id.clone(),
                id: tool_id.clone(),
                provider_id: format!("p{i}"),
                tool_name: "Read".to_string(),
                output: format!("文件内容片段 {i}\n第二行\n第三行"),
                content: serde_json::json!({ "text": format!("文件内容片段 {i}") }),
                is_error: false,
                image_count: 0,
            });
        }
        conv
    }

    println!("\n=== refresh 成本基准（width=100；每 turn = user+assistant+completed tool）===");
    for n in [500usize, 1000, 2000, 4000] {
        let conv = build_conversation(n);
        let blocks = conv.timeline.items().len();
        let rev = conv.revision();

        // assemble 全量（A3 memo miss 时、即每个 streaming chunk 的成本）
        let t = Instant::now();
        let vm = OutputViewAssembler::assemble_from_conversation(&conv, rev, None);
        let assemble_ms = t.elapsed().as_secs_f64() * 1000.0;
        let roots = vm.roots.len();

        // render cold：BlockCache 空（冷启动 / resize 成本）
        let mut renderer = OutputDocumentRenderer::default();
        let t = Instant::now();
        let _ = renderer.render_model_document(&vm, 100, 100, 0);
        let render_cold_ms = t.elapsed().as_secs_f64() * 1000.0;

        // render warm：BlockCache 命中、仅 frame 变（动画 tick 成本：遍历+gutter+clone+trim）
        let t = Instant::now();
        let _ = renderer.render_model_document(&vm, 100, 100, 1);
        let render_warm_ms = t.elapsed().as_secs_f64() * 1000.0;

        println!(
            "turns={n:>5} blocks={blocks:>6} roots={roots:>6} | assemble={assemble_ms:>8.2}ms  render_cold={render_cold_ms:>8.2}ms  render_warm={render_warm_ms:>8.2}ms  | streaming_chunk≈assemble+warm={:>8.2}ms",
            assemble_ms + render_warm_ms
        );
    }
    println!();
}
