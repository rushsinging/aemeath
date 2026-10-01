//! diff 原语：复用现有 build_diff_lines(产出 SpanPart) 转 RenderedLine。

use crate::tui::render::output::diff::build_diff_lines_from;
use crate::tui::render::output::primitives::spanparts_to_spans;
use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::output_area::types::SpanPart;

#[cfg(test)]
pub fn diff(old: &str, new: &str, ext: Option<&str>, width: u16) -> Vec<RenderedLine> {
    diff_from(old, new, 1, 1, ext, width)
}

pub fn diff_from(
    old: &str,
    new: &str,
    old_start: usize,
    new_start: usize,
    ext: Option<&str>,
    _width: u16,
) -> Vec<RenderedLine> {
    let mut out: Vec<Vec<SpanPart>> = Vec::new();
    build_diff_lines_from(old, new, old_start, new_start, ext, &mut out);
    out.into_iter()
        .map(|parts| RenderedLine::new(spanparts_to_spans(&parts)))
        .collect()
}

#[cfg(test)]
#[path = "diff_tests.rs"]
mod tests;
