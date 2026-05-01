use rand::prelude::IndexedRandom;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::tui::output_area::SpinnerState;

/// Spinner glyph frames — forward then reverse for a breathing effect
const SPINNER_FRAMES: &[char] = &[
    '·', '✢', '✳', '✶', '✻', '✽',
    '✻', '✶', '✳', '✢', '·',
];

/// Fun verbs shown while the LLM is thinking
const SPINNER_VERBS: &[&str] = &[
    "Thinking",    "Pondering",    "Crafting",     "Computing",
    "Brewing",     "Weaving",      "Conjuring",    "Forging",
    "Hatching",    "Cooking",      "Channeling",   "Ruminating",
    "Composing",   "Imagining",    "Processing",   "Puzzling",
    "Mulling",     "Noodling",     "Tinkering",    "Crystallizing",
    "Synthesizing","Architecting", "Orchestrating","Incubating",
    "Fermenting",  "Simmering",    "Percolating",  "Cogitating",
    "Meandering",  "Harmonizing",
];

/// Spinner colors (warm orange theme)
const SPINNER_BASE: Color = Color::Rgb(204, 152, 87);
const SPINNER_HIGHLIGHT: Color = Color::Rgb(255, 210, 140);
const SPINNER_DIM: Color = Color::Rgb(140, 105, 60);

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
    /// Start the animated spinner in the output area
    pub fn start_spinner(&mut self) {
        if self.spinner.is_some() {
            log::debug!("[SPINNER] start_spinner: already running, frame={}", self.spinner.as_ref().unwrap().frame);
            return;
        }
        let mut rng = rand::rng();
        self.spinner = Some(SpinnerState {
            frame: 0,
            verb: SPINNER_VERBS.choose(&mut rng).unwrap_or(&"Thinking").to_string(),
            start: std::time::Instant::now(),
            phase: None,
        });
        log::debug!("[SPINNER] start_spinner: started");
    }

    /// Update the detailed phase shown on the spinner line.
    pub fn set_spinner_phase(&mut self, phase: impl Into<String>) {
        let phase = phase.into();
        if self.spinner.is_none() {
            self.start_spinner();
        }
        if let Some(ref mut spinner) = self.spinner {
            spinner.phase = Some(phase);
        }
    }

    /// Stop the animated spinner
    pub fn stop_spinner(&mut self) {
        if self.spinner.is_some() {
            log::debug!("[SPINNER] stop_spinner: stopping (was running)");
        }
        self.spinner = None;
    }

    /// Build the animated spinner line (called during render)
    pub fn build_spinner_line(&self) -> Option<Line<'static>> {
        let s = self.spinner.as_ref()?;

        let mut spans = Vec::new();

        let glyph = SPINNER_FRAMES[(s.frame / 3) as usize % SPINNER_FRAMES.len()];
        spans.push(Span::styled(
            format!(" {} ", glyph),
            Style::default().fg(SPINNER_BASE).add_modifier(Modifier::BOLD),
        ));

        let text = format!("{}…", s.verb);
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
                Style::default().fg(Color::DarkGray),
            ));
        }

        if let Some(phase) = s.phase.as_deref().filter(|p| !p.is_empty()) {
            spans.push(Span::styled("  (", Style::default().fg(Color::DarkGray)));
            spans.push(Span::styled(
                phase.to_string(),
                Style::default().fg(Color::Yellow),
            ));
            spans.push(Span::styled(")", Style::default().fg(Color::DarkGray)));
        }

        Some(Line::from(spans))
    }
}
