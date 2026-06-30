use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::tui::render::theme;
use crate::tui::view_model::live_status::CompactProgressView;
use crate::tui::view_model::SpinnerLineView;

/// Spinner glyph frames — forward then reverse for a breathing effect
const SPINNER_FRAMES: &[char] = &['·', '✢', '✳', '✶', '✻', '✽', '✻', '✶', '✳', '✢', '·'];

/// Spinner colors (theme accent)
const SPINNER_BASE: Color = theme::SPINNER_BASE;
const SPINNER_HIGHLIGHT: Color = theme::SPINNER_HIGHLIGHT;
const SPINNER_DIM: Color = theme::SPINNER_DIM;

/// Linear interpolation between two RGB colors
fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    if let (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) = (a, b) {
        let r = (r1 as f32 + (r2 as f32 - r1 as f32) * t) as u8;
        let g = (g1 as f32 + (g2 as f32 - g1 as f32) * t) as u8;
        let b = (b1 as f32 + (b2 as f32 - b1 as f32) * t) as u8;
        Color::Rgb(r, g, b)
    } else {
        a
    }
}

impl super::OutputArea {
    /// Build the animated spinner line (called during render).
    ///
    /// 真相边界：spinner active/phase 来自 RuntimeModel，frame/verb/elapsed 来自
    /// view_state.spinner，经 LiveStatusViewModel 投影到渲染层；OutputArea 不再自持
    /// spinner widget mirror。
    pub fn build_spinner_line(
        &self,
        s: &SpinnerLineView,
        compact_progress: Option<&CompactProgressView>,
    ) -> Line<'static> {
        let mut spans = Vec::new();

        let glyph = SPINNER_FRAMES[(s.frame / 3) as usize % SPINNER_FRAMES.len()];
        spans.push(Span::styled(
            format!(" {} ", glyph),
            Style::default()
                .fg(SPINNER_BASE)
                .add_modifier(Modifier::BOLD),
        ));

        let text = format!("{}...", s.verb);
        let text_len = text.chars().count() as i32;
        let cycle_len = text_len + 16;
        let glimmer_pos = ((s.frame / 2) as i32) % cycle_len - 8;

        for (i, ch) in text.chars().enumerate() {
            let dist = (i as i32 - glimmer_pos).abs();
            let color = if dist == 0 {
                SPINNER_HIGHLIGHT
            } else if dist <= 2 {
                lerp_color(SPINNER_HIGHLIGHT, SPINNER_BASE, dist as f32 / 3.0)
            } else if dist <= 4 {
                SPINNER_BASE
            } else {
                SPINNER_DIM
            };
            spans.push(Span::styled(ch.to_string(), Style::default().fg(color)));
        }

        let elapsed = s.elapsed_secs;
        if elapsed >= 1 {
            spans.push(Span::styled(
                format!("  {}s", elapsed),
                Style::default().fg(theme::TEXT_DIM),
            ));
        }

        if let Some(phase) = s.phase_text.as_deref().filter(|p| !p.is_empty()) {
            spans.push(Span::styled("  (", Style::default().fg(theme::TEXT_DIM)));
            spans.push(Span::styled(
                phase.to_string(),
                Style::default().fg(theme::WARNING),
            ));
            let phase_elapsed = s.phase_elapsed_secs;
            spans.push(Span::styled(
                format!("  ⏱ {}s", phase_elapsed),
                Style::default().fg(theme::TEXT_DIM),
            ));
            spans.push(Span::styled(")", Style::default().fg(theme::TEXT_DIM)));
        }

        if let Some(cp) = compact_progress {
            spans.extend(compact_progress_spans(cp));
        }

        Line::from(spans)
    }
}

/// Compact 进度条的最大宽度（字符）。
const COMPACT_BAR_MAX_WIDTH: usize = 30;

/// 由 `CompactProgressView` 构造手绘 Span 进度条片段。
///
/// 格式：`  ████████░░░░░░░░░░░░░░░░░░ 25%`
/// 填充 `█` 用 `YELLOW`，未填充 `░` 用 `TEXT_DIM`，百分比用 `YELLOW`。
fn compact_progress_spans(cp: &CompactProgressView) -> Vec<Span<'static>> {
    let ratio = (cp.ratio_millis as f64) / 1000.0;
    let pct = (ratio * 100.0).round().clamp(0.0, 100.0) as u16;
    let bar_width = COMPACT_BAR_MAX_WIDTH;
    let filled = (ratio * bar_width as f64)
        .round()
        .clamp(0.0, bar_width as f64) as usize;
    let empty = bar_width - filled;

    let mut spans = vec![Span::raw("  ")]; // 与前一个片段隔开
    if filled > 0 {
        spans.push(Span::styled(
            "█".repeat(filled),
            Style::default().fg(theme::YELLOW),
        ));
    }
    if empty > 0 {
        spans.push(Span::styled(
            "░".repeat(empty),
            Style::default().fg(theme::TEXT_DIM),
        ));
    }
    spans.push(Span::styled(
        format!(" {pct}%"),
        Style::default().fg(theme::YELLOW),
    ));
    spans
}
