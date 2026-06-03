use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::tui::render::theme;

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
    /// Build the animated spinner line (called during render)
    ///
    /// 真相边界：spinner 镜像（`self.spinner`）由 `adapter/live_status_widget.rs`
    /// 据 Model（active/phase）+ view_state（frame/verb）单向写回。本函数只读镜像渲染，
    /// 不再自持 verb 选择/动画推进。
    pub fn build_spinner_line(&self) -> Option<Line<'static>> {
        let s = self.spinner.as_ref()?;

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

        let elapsed = s.start.elapsed().as_secs();
        if elapsed >= 1 {
            spans.push(Span::styled(
                format!("  {}s", elapsed),
                Style::default().fg(theme::TEXT_DIM),
            ));
        }

        if let Some(phase) = s.phase.as_deref().filter(|p| !p.is_empty()) {
            spans.push(Span::styled("  (", Style::default().fg(theme::TEXT_DIM)));
            spans.push(Span::styled(
                phase.to_string(),
                Style::default().fg(theme::WARNING),
            ));
            spans.push(Span::styled(
                format!("  ⏱ {}s", elapsed),
                Style::default().fg(theme::TEXT_DIM),
            ));
            spans.push(Span::styled(")", Style::default().fg(theme::TEXT_DIM)));
        }

        Some(Line::from(spans))
    }
}
