//! SpanPart(现有 diff/syntax 着色单元) 与 RenderedLine 互转。

#[cfg(test)]
use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::output_area::types::SpanPart;
use ratatui::style::Style;
use ratatui::text::Span;

pub fn spanparts_to_spans(parts: &[SpanPart]) -> Vec<Span<'static>> {
    parts
        .iter()
        .map(|part| Span::styled(part.text.clone(), Style::default().fg(part.color)))
        .collect()
}

#[cfg(test)]
pub fn rendered_line_from_spanparts(parts: &[SpanPart]) -> RenderedLine {
    RenderedLine::new(spanparts_to_spans(parts))
}

#[cfg(test)]
#[path = "convert_tests.rs"]
mod tests;
