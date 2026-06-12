use crate::tui::render::display::safe_text::col_to_char_idx;
use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WrappedInputLine {
    pub original_row: usize,
    pub original_col_start: usize,
    pub text: String,
}

pub(crate) fn wrap_input_lines_for_width(lines: Vec<&str>, width: usize) -> Vec<WrappedInputLine> {
    if width == 0 {
        return lines
            .into_iter()
            .enumerate()
            .map(|(row, text)| WrappedInputLine {
                original_row: row,
                original_col_start: 0,
                text: text.to_string(),
            })
            .collect();
    }

    let mut out = Vec::new();
    for (row, line) in lines.into_iter().enumerate() {
        let mut current = String::new();
        let mut current_width = 0usize;
        let mut col_start = 0usize;
        for (col, ch) in line.chars().enumerate() {
            let ch_width = ch.width().unwrap_or(0);
            if !current.is_empty() && current_width + ch_width > width {
                out.push(WrappedInputLine {
                    original_row: row,
                    original_col_start: col_start,
                    text: std::mem::take(&mut current),
                });
                current_width = 0;
                col_start = col;
            }
            current.push(ch);
            current_width += ch_width;
        }
        out.push(WrappedInputLine {
            original_row: row,
            original_col_start: col_start,
            text: current,
        });
    }
    out
}

pub(crate) fn display_position_for_anchor(
    lines: &[WrappedInputLine],
    original_row: usize,
    original_col: usize,
) -> (usize, usize) {
    let mut best_idx = 0usize;
    let mut best = None;
    for (idx, line) in lines.iter().enumerate() {
        if line.original_row != original_row || line.original_col_start > original_col {
            continue;
        }
        best_idx = idx;
        best = Some(line);
    }
    let Some(line) = best else {
        return (best_idx, 0);
    };
    let offset = original_col.saturating_sub(line.original_col_start);
    (best_idx, offset)
}

pub(crate) fn anchor_for_display_position(
    lines: &[WrappedInputLine],
    display_row: usize,
    screen_col: usize,
) -> (usize, usize) {
    let Some(line) = lines.get(display_row) else {
        return (display_row, 0);
    };
    (
        line.original_row,
        line.original_col_start + col_to_char_idx(&line.text, screen_col),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_input_lines_tracks_original_row_and_col() {
        let lines = wrap_input_lines_for_width(vec!["abcdef"], 4);

        assert_eq!(
            lines,
            vec![
                WrappedInputLine {
                    original_row: 0,
                    original_col_start: 0,
                    text: "abcd".to_string(),
                },
                WrappedInputLine {
                    original_row: 0,
                    original_col_start: 4,
                    text: "ef".to_string(),
                },
            ]
        );
    }

    #[test]
    fn display_position_for_anchor_maps_wrapped_col() {
        let lines = wrap_input_lines_for_width(vec!["abcdef"], 4);

        assert_eq!(display_position_for_anchor(&lines, 0, 5), (1, 1));
    }

    #[test]
    fn display_position_for_anchor_uses_textarea_char_column_for_cjk() {
        let lines = wrap_input_lines_for_width(vec!["你好ab"], 10);

        assert_eq!(display_position_for_anchor(&lines, 0, 1), (0, 1));
    }

    #[test]
    fn anchor_for_display_position_maps_wrapped_row_back_to_original_anchor() {
        let lines = wrap_input_lines_for_width(vec!["abcdef"], 4);

        assert_eq!(anchor_for_display_position(&lines, 1, 1), (0, 5));
    }

    #[test]
    fn wrap_input_lines_uses_display_width_for_cjk() {
        let lines = wrap_input_lines_for_width(vec!["你好ab"], 4);

        assert_eq!(lines[0].text, "你好");
        assert_eq!(lines[1].original_col_start, 2);
    }
}
