use crate::tui::render::output::markdown::{is_table_row, is_table_separator};
use crate::tui::render::output::spacing::MarkdownElement;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MarkdownBlock<'a> {
    pub kind: MarkdownElement,
    pub lines: Vec<&'a str>,
    pub source_gap_before: bool,
    pub fence_language: Option<String>,
}

pub(crate) fn parse_blocks(text: &str) -> Vec<MarkdownBlock<'_>> {
    let source = text.lines().collect::<Vec<_>>();
    let mut blocks = Vec::new();
    let mut index = 0;
    let mut pending_gap = false;

    while index < source.len() {
        if source[index].trim().is_empty() {
            pending_gap = true;
            index += 1;
            continue;
        }

        let start = index;
        let (kind, end, fence_language) = if is_fence_marker(source[index].trim_start()) {
            let language = fence_language(source[index].trim_start());
            index += 1;
            while index < source.len() {
                let closing = is_fence_marker(source[index].trim_start());
                index += 1;
                if closing {
                    break;
                }
            }
            (MarkdownElement::CodeBlock, index, language)
        } else if is_table_start(&source, index) {
            index += 2;
            while index < source.len() && is_table_row(source[index]) {
                index += 1;
            }
            (MarkdownElement::Table, index, None)
        } else {
            let kind = classify_line(source[index]);
            index += 1;
            while index < source.len()
                && !source[index].trim().is_empty()
                && !is_fence_marker(source[index].trim_start())
                && !is_table_start(&source, index)
                && classify_line(source[index]) == kind
            {
                index += 1;
            }
            (kind, index, None)
        };

        blocks.push(MarkdownBlock {
            kind,
            lines: source[start..end].to_vec(), // allow unsafe_text_op: Vec slice
            source_gap_before: !blocks.is_empty() && pending_gap,
            fence_language,
        });
        pending_gap = false;
    }

    blocks
}

fn classify_line(line: &str) -> MarkdownElement {
    let trimmed = line.trim_start();
    if is_atx_heading(trimmed) {
        MarkdownElement::Heading
    } else if is_list_item(trimmed) || line.starts_with(char::is_whitespace) {
        MarkdownElement::List
    } else if trimmed.starts_with('>') {
        MarkdownElement::Blockquote
    } else {
        MarkdownElement::Paragraph
    }
}

fn is_atx_heading(line: &str) -> bool {
    let hashes = line.chars().take_while(|ch| *ch == '#').count();
    (1..=6).contains(&hashes)
        && line
            .get(hashes..)
            .and_then(|suffix| suffix.chars().next())
            .is_none_or(char::is_whitespace)
}

fn is_list_item(line: &str) -> bool {
    if ["- ", "* ", "+ "]
        .iter()
        .any(|prefix| line.starts_with(prefix))
    {
        return true;
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    digits > 0
        && line.get(digits..).is_some_and(|suffix| {
            suffix
                .strip_prefix(". ")
                .or_else(|| suffix.strip_prefix(") "))
                .is_some()
        })
}

fn is_table_start(source: &[&str], index: usize) -> bool {
    is_table_row(source[index])
        && source
            .get(index + 1)
            .is_some_and(|line| is_table_separator(line))
}

pub(crate) fn is_fence_marker(line: &str) -> bool {
    line.starts_with("```")
}

pub(crate) fn fence_language(line: &str) -> Option<String> {
    let language = line.trim_start_matches('`').trim();
    (!language.is_empty()).then(|| language.to_string())
}

#[cfg(test)]
#[path = "blocks_tests.rs"]
mod tests;
