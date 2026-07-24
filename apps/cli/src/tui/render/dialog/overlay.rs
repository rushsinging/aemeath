//! Dialog components for user interactions
//!
//! Provides a modal selection dialog for the TUI.

use crate::tui::model::conversation::interaction::{
    InteractionBody, InteractionDraft, InteractionPhase, InteractionState, UiRiskLevel,
};
use crate::tui::render::theme;
use crate::tui::view_model::DialogViewModel;
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};

/// A modal selection dialog widget
pub struct Dialog {
    title: String,
    options: Vec<String>,
    selected: usize,
    /// Whether the dialog is visible
    pub visible: bool,
}

impl Dialog {
    /// Create a new selection dialog
    pub fn select(title: &str, options: Vec<String>) -> Self {
        Self {
            title: title.to_string(),
            options,
            selected: 0,
            visible: true,
        }
    }

    /// Get the selected option index
    pub fn get_selected(&self) -> Option<usize> {
        if self.options.is_empty() {
            None
        } else {
            Some(self.selected)
        }
    }

    /// Move selection up (wraps around)
    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        } else {
            self.selected = self.options.len().saturating_sub(1);
        }
    }

    /// Move selection down (wraps around)
    pub fn select_next(&mut self) {
        self.selected = (self.selected + 1) % self.options.len().max(1);
    }

    /// Render the dialog centered on the screen
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if !self.visible || self.options.is_empty() {
            return;
        }

        // Calculate dimensions
        let max_option_len = self.options.iter().map(|o| o.len()).max().unwrap_or(20);
        let content_width = (max_option_len as u16 + 8)
            .max(self.title.len() as u16 + 6)
            .min(area.width.saturating_sub(4));
        let height = (self.options.len() as u16 + 3) // +3 for border + hint line
            .min(area.height.saturating_sub(2));

        // Center
        let x = (area.width.saturating_sub(content_width)) / 2;
        let y = (area.height.saturating_sub(height)) / 2;
        let dialog_area = Rect::new(x, y, content_width, height);

        // Clear background
        Clear.render(dialog_area, buf);

        // Border
        let block = Block::default()
            .title(Span::styled(
                format!(" {} ", self.title),
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::ACCENT));

        let inner = block.inner(dialog_area);
        block.render(dialog_area, buf);

        // Options
        let mut lines: Vec<Line> = Vec::new();
        for (i, option) in self.options.iter().enumerate() {
            if i == self.selected {
                lines.push(Line::styled(
                    format!(" > {}", option),
                    Style::default()
                        .fg(theme::WARNING)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                lines.push(Line::styled(
                    format!("   {}", option),
                    Style::default().fg(theme::TEXT_MUTED),
                ));
            }
        }

        // Hint line
        lines.push(Line::styled(
            " Enter=select  Esc=cancel",
            Style::default().fg(theme::TEXT_DIM),
        ));

        let paragraph = Paragraph::new(lines).alignment(Alignment::Left);
        paragraph.render(inner, buf);
    }
}

/// Render a DialogViewModel as a modal dialog
pub fn render_dialog_vm(vm: &DialogViewModel, area: Rect, buf: &mut Buffer) {
    // Calculate dimensions
    let max_action_len = vm.actions.iter().map(|a| a.label.len()).max().unwrap_or(20);
    let content_width = (max_action_len as u16 + 8)
        .max(vm.title.len() as u16 + 6)
        .min(area.width.saturating_sub(4));
    let height = (vm.actions.len() as u16 + 3) // +3 for border + hint line
        .min(area.height.saturating_sub(2));

    // Center
    let x = (area.width.saturating_sub(content_width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let dialog_area = Rect::new(x, y, content_width, height);

    // Clear background
    Clear.render(dialog_area, buf);

    // Border style based on severity
    let border_style = match vm.severity {
        crate::tui::view_model::status::StatusSeverity::Error => Style::default().fg(theme::ERROR),
        crate::tui::view_model::status::StatusSeverity::Warning => {
            Style::default().fg(theme::WARNING)
        }
        _ => Style::default().fg(theme::ACCENT),
    };

    // Border
    let block = Block::default()
        .title(Span::styled(
            format!(" {} ", vm.title),
            border_style.add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(dialog_area);
    block.render(dialog_area, buf);

    // Body
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::styled(
        vm.body.clone(),
        Style::default().fg(theme::TEXT),
    ));
    lines.push(Line::raw(""));

    // Actions
    for action in vm.actions.iter() {
        let is_default = vm.default_action.as_ref() == Some(&action.id);
        if is_default {
            lines.push(Line::styled(
                format!(" > {}", action.label),
                Style::default()
                    .fg(theme::WARNING)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            lines.push(Line::styled(
                format!("   {}", action.label),
                Style::default().fg(theme::TEXT_MUTED),
            ));
        }
    }

    // Hint line
    lines.push(Line::styled(
        " Enter=select  Esc=cancel",
        Style::default().fg(theme::TEXT_DIM),
    ));

    let paragraph = Paragraph::new(lines).alignment(Alignment::Left);
    paragraph.render(inner, buf);
}

/// Render an AskUserQuestion interaction as a modal overlay.
///
/// Layout:
/// ```text
/// ┌─ {title} ──────────────────────┐
/// │  Question prompt text           │
/// │                                 │
/// │  > Option A                     │
/// │    Option B                     │
/// │    Option C                     │
/// │                                 │
/// │  Tab=cycle  Enter=confirm  Esc=cancel │
/// └─────────────────────────────────┘
/// ```
pub fn render_interaction_overlay(
    state: &InteractionState,
    selected: usize,
    area: Rect,
    buf: &mut Buffer,
) {
    let body = state.body();
    let draft = state.draft();
    let phase = state.phase();

    // ── Build content lines ──
    let mut lines: Vec<Line> = Vec::new();
    let title;

    match body {
        InteractionBody::UserQuestions(questions) => {
            // For single-question with options, render as a selection list.
            // For text-input questions, render prompt + current draft.
            if questions.len() == 1 && !questions[0].options.is_empty() {
                let q = &questions[0];
                title = if q.prompt.len() > 40 {
                    " Question ".to_string()
                } else {
                    format!(" {} ", q.prompt)
                };
                lines.push(Line::styled(
                    q.prompt.clone(),
                    Style::default().fg(theme::TEXT),
                ));
                lines.push(Line::raw(""));
                for (i, opt) in q.options.iter().enumerate() {
                    let marker = if i == selected { "▶ " } else { "  " };
                    let style = if i == selected {
                        Style::default()
                            .fg(theme::WARNING)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme::TEXT_MUTED)
                    };
                    lines.push(Line::styled(format!("{marker}{opt}"), style));
                }
            } else {
                title = " Input ".to_string();
                for (i, q) in questions.iter().enumerate() {
                    lines.push(Line::styled(
                        q.prompt.clone(),
                        Style::default().fg(theme::TEXT),
                    ));
                    if let InteractionDraft::UserAnswers(answers) = draft {
                        if let Some(ans) = answers.get(i) {
                            lines.push(Line::styled(
                                format!("  › {ans}"),
                                Style::default().fg(theme::ACCENT),
                            ));
                        }
                    }
                    lines.push(Line::raw(""));
                }
            }
        }
        InteractionBody::ToolApproval(prompt) => {
            title = format!(" {} ", prompt.title);
            lines.push(Line::styled(
                prompt.detail.clone(),
                Style::default().fg(theme::TEXT),
            ));
            lines.push(Line::raw(""));
            let risk_label = match prompt.risk {
                UiRiskLevel::Low => ("Low", theme::SUCCESS),
                UiRiskLevel::Medium => ("Medium", theme::WARNING),
                UiRiskLevel::High => ("High", theme::ERROR),
            };
            lines.push(Line::styled(
                format!("Risk: {}", risk_label.0),
                Style::default().fg(risk_label.1),
            ));
            lines.push(Line::raw(""));
            let opts = [
                ("▶ Approve", "  Approve"),
                ("  Deny", "▶ Deny"),
            ];
            let approved = matches!(draft, InteractionDraft::Approval { approved: Some(true), .. });
            let denied = matches!(draft, InteractionDraft::Approval { approved: Some(false), .. });
            if approved {
                lines.push(Line::styled(opts[0].0, Style::default().fg(theme::WARNING).add_modifier(Modifier::BOLD)));
                lines.push(Line::styled(opts[1].1, Style::default().fg(theme::TEXT_MUTED)));
            } else if denied {
                lines.push(Line::styled(opts[0].1, Style::default().fg(theme::TEXT_MUTED)));
                lines.push(Line::styled(opts[1].0, Style::default().fg(theme::WARNING).add_modifier(Modifier::BOLD)));
            } else {
                lines.push(Line::styled(opts[selected].0, Style::default().fg(theme::WARNING).add_modifier(Modifier::BOLD)));
                lines.push(Line::styled(opts[1 - selected].1, Style::default().fg(theme::TEXT_MUTED)));
            }
        }
        InteractionBody::PlanApproval(prompt) => {
            title = format!(" {} ", prompt.title);
            for step in &prompt.steps {
                lines.push(Line::styled(
                    format!(" • {step}"),
                    Style::default().fg(theme::TEXT),
                ));
            }
            lines.push(Line::raw(""));
            let approved = matches!(draft, InteractionDraft::Approval { approved: Some(true), .. });
            let denied = matches!(draft, InteractionDraft::Approval { approved: Some(false), .. });
            if approved {
                lines.push(Line::styled("▶ Approve", Style::default().fg(theme::WARNING).add_modifier(Modifier::BOLD)));
                lines.push(Line::styled("  Deny", Style::default().fg(theme::TEXT_MUTED)));
            } else if denied {
                lines.push(Line::styled("  Approve", Style::default().fg(theme::TEXT_MUTED)));
                lines.push(Line::styled("▶ Deny", Style::default().fg(theme::WARNING).add_modifier(Modifier::BOLD)));
            } else {
                let labels = ["▶ Approve", "  Deny"];
                lines.push(Line::styled(labels[selected], Style::default().fg(theme::WARNING).add_modifier(Modifier::BOLD)));
                lines.push(Line::styled(labels[1 - selected], Style::default().fg(theme::TEXT_MUTED)));
            }
        }
        InteractionBody::HardPause(diag) => {
            title = " Stuck — Hard Pause ".to_string();
            lines.push(Line::styled(
                diag.reason.clone(),
                Style::default().fg(theme::ERROR),
            ));
            lines.push(Line::raw(""));
            if !diag.recent_actions.is_empty() {
                lines.push(Line::styled(
                    "Recent actions:",
                    Style::default().fg(theme::TEXT_DIM),
                ));
                for action in &diag.recent_actions {
                    lines.push(Line::styled(
                        format!("  {action}"),
                        Style::default().fg(theme::TEXT_MUTED),
                    ));
                }
                lines.push(Line::raw(""));
            }
            if matches!(draft, InteractionDraft::HardPause { continue_run: true }) {
                lines.push(Line::styled("▶ Continue", Style::default().fg(theme::WARNING).add_modifier(Modifier::BOLD)));
            } else {
                lines.push(Line::styled("  Press Enter to continue", Style::default().fg(theme::TEXT)));
            }
        }
    }

    // ── Hint line ──
    lines.push(Line::raw(""));
    let hint = match body {
        InteractionBody::UserQuestions(qs)
            if qs.len() == 1 && !qs[0].options.is_empty() =>
        {
            " Tab/↑↓=cycle  Enter=confirm  Esc=cancel"
        }
        InteractionBody::ToolApproval(_) | InteractionBody::PlanApproval(_) => {
            " ←/→=select  Enter=confirm  Esc=cancel"
        }
        InteractionBody::UserQuestions(_) => " Type answer  Enter=confirm  Esc=cancel",
        InteractionBody::HardPause(_) => " Enter=continue  Esc=cancel",
    };
    lines.push(Line::styled(hint, Style::default().fg(theme::TEXT_DIM)));

    // ── Phase indicator ──
    if !matches!(phase, InteractionPhase::Collecting | InteractionPhase::Confirming) {
        lines.push(Line::styled(
            format!(" [{:?}...]", phase),
            Style::default().fg(theme::TEXT_DIM),
        ));
    }

    // ── Dimensions ──
    let content_width = lines
        .iter()
        .map(|l| l.width())
        .max()
        .unwrap_or(20)
        .max(title.len())
        .min(area.width.saturating_sub(4) as usize) as u16;
    let height = (lines.len() as u16 + 2).min(area.height.saturating_sub(2));

    let x = (area.width.saturating_sub(content_width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let dialog_area = Rect::new(x, y, content_width, height);

    Clear.render(dialog_area, buf);

    let block = Block::default()
        .title(Span::styled(
            title,
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::ACCENT));

    let inner = block.inner(dialog_area);
    block.render(dialog_area, buf);

    Paragraph::new(lines)
        .alignment(Alignment::Left)
        .render(inner, buf);
}
