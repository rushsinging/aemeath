use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::ModelStreamPlaceholderBlockView;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use std::rc::Rc;

const DOT_FRAMES: [&str; 3] = [".", "..", "..."];
pub const THINKING_DOT_FRAME_DIVISOR: u64 = 4;

pub fn render_model_stream_placeholder(
    block_id: &str,
    view: &ModelStreamPlaceholderBlockView,
    ctx: &RenderCtx,
    animation_frame: u64,
) -> RenderedBlock {
    let dots = animated_thinking_dots(animation_frame);
    let header = format!("Thinking{dots}");
    let body = placeholder_body_for_phase(&view.phase);
    let style = Style::default().fg(theme::THINKING);
    let muted_style = Style::default().fg(theme::TEXT_MUTED);

    let mut lines = vec![RenderedLine::new(vec![Span::styled(
        header,
        style.add_modifier(Modifier::ITALIC),
    )])];
    lines.extend(
        wrap_placeholder_body(body, ctx.text_width)
            .into_iter()
            .map(|line| RenderedLine::new(vec![Span::styled(line, muted_style)])),
    );

    RenderedBlock {
        block_id: block_id.to_string(),
        lines: Rc::new(lines),
    }
}

pub fn animated_thinking_dots(animation_frame: u64) -> &'static str {
    let index = ((animation_frame / THINKING_DOT_FRAME_DIVISOR) as usize) % DOT_FRAMES.len();
    DOT_FRAMES[index]
}

pub fn placeholder_body_for_phase(phase: &str) -> &'static str {
    match phase {
        "waiting_model_response" => "Waiting for model response...",
        "waiting_model_output" | "waiting_first_model_delta" => "Waiting for model output...",
        "thinking" => "Model is thinking...",
        "writing" => "Model is writing...",
        "preparing_tool_arguments" => "Model is preparing tool arguments...",
        _ => "Waiting for model output...",
    }
}

fn wrap_placeholder_body(text: &str, width: u16) -> Vec<String> {
    let max_width = width as usize;
    if max_width == 0 || text.chars().count() <= max_width {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    for ch in text.chars() {
        if current_width >= max_width && !current.is_empty() {
            lines.push(current);
            current = String::new();
            current_width = 0;
        }
        current.push(ch);
        current_width += 1;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thinking_dots_cycle_between_one_two_three_dots() {
        assert_eq!(animated_thinking_dots(0), ".");
        assert_eq!(animated_thinking_dots(4), "..");
        assert_eq!(animated_thinking_dots(8), "...");
        assert_eq!(animated_thinking_dots(12), ".");
    }

    #[test]
    fn test_placeholder_header_and_body_render() {
        let view = ModelStreamPlaceholderBlockView {
            key: "p".into(),
            elapsed_secs: 10,
            phase: "preparing_tool_arguments".into(),
        };
        let block = render_model_stream_placeholder("p", &view, &RenderCtx { text_width: 80 }, 8);

        assert_eq!(block.lines[0].plain, "Thinking...");
        assert_eq!(block.lines[1].plain, "Model is preparing tool arguments...");
    }

    #[test]
    fn test_placeholder_body_maps_phase_without_elapsed_text() {
        assert_eq!(
            placeholder_body_for_phase("waiting_model_response"),
            "Waiting for model response..."
        );
        assert_eq!(
            placeholder_body_for_phase("thinking"),
            "Model is thinking..."
        );
        assert_eq!(placeholder_body_for_phase("writing"), "Model is writing...");
        assert_eq!(
            placeholder_body_for_phase("preparing_tool_arguments"),
            "Model is preparing tool arguments..."
        );
    }
}
