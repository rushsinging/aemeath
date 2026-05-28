use crate::tui::output_area::{LineStyle, OutputArea};
use crate::tui::view_model::{OutputBlockView, OutputViewModel, SemanticStyle, TextBlockView};

pub struct OutputViewAssembler;

impl OutputViewAssembler {
    pub fn assemble_from_output_area(output: &OutputArea, version: u64) -> OutputViewModel {
        let blocks = output
            .lines
            .iter()
            .enumerate()
            .map(|(idx, line)| {
                let style = match line.style {
                    LineStyle::Error | LineStyle::ToolCallError => SemanticStyle::Error,
                    LineStyle::ToolCallSuccess => SemanticStyle::Success,
                    LineStyle::ToolCallRunning => SemanticStyle::Running,
                    LineStyle::System | LineStyle::Thinking => SemanticStyle::Muted,
                    _ => SemanticStyle::Normal,
                };
                OutputBlockView::SystemNotice(TextBlockView {
                    key: format!("legacy-line-{idx}"),
                    text: line.content.clone(),
                    style,
                })
            })
            .collect();
        OutputViewModel {
            blocks,
            version,
            follow_tail_hint: output.auto_scroll,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::output_area::{LineStyle, OutputArea};

    use super::OutputViewAssembler;

    #[test]
    fn test_output_assembler_converts_existing_lines_to_blocks() {
        let mut output = OutputArea::new();
        output.push_system("hello");
        let vm = OutputViewAssembler::assemble_from_output_area(&output, 1);
        assert_eq!(vm.version, 1);
        assert_eq!(vm.blocks.len(), 1);
        assert!(matches!(
            output.lines.front().map(|line| line.style),
            Some(LineStyle::System)
        ));
    }
}
