use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};

use crate::tui::render::theme;

const WIDE_LAYOUT_MIN_WIDTH: u16 = 72;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConfigFormInteraction {
    pub(crate) focused_field: usize,
    pub(crate) selected_option: usize,
    pub(crate) focused_action: usize,
}

pub(crate) fn render_config_form(
    frame: &mut ratatui::Frame<'_>,
    view: &sdk::ConfigFormView,
    visible_input: &str,
    scroll: u16,
    interaction: ConfigFormInteraction,
) {
    let area = frame.area();
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default().style(Style::default().bg(theme::SURFACE)),
        area,
    );
    let outer = Block::default()
        .title(format!(" {} ", view.page.title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER))
        .style(Style::default().fg(theme::TEXT).bg(theme::SURFACE));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(3),
            Constraint::Length(4),
        ])
        .split(inner);
    render_header(frame, view, rows[0]);
    if area.width >= WIDE_LAYOUT_MIN_WIDTH {
        render_wide_body(frame, view, rows[1], scroll, interaction);
    } else {
        render_narrow_body(frame, view, rows[1], scroll, interaction);
    }
    render_footer(frame, view, visible_input, rows[2], interaction);
}

fn render_header(frame: &mut ratatui::Frame<'_>, view: &sdk::ConfigFormView, area: Rect) {
    let mut spans = Vec::new();
    if let Some(step) = view.page.step {
        spans.push(Span::styled(
            format!("步骤 {}/{}  ", step.current, step.total),
            Style::default().fg(theme::ACCENT),
        ));
    }
    spans.push(Span::styled(
        view.page.description.as_deref().unwrap_or_default(),
        Style::default().fg(theme::TEXT_MUTED),
    ));
    frame.render_widget(
        Paragraph::new(Line::from(spans)).wrap(Wrap { trim: true }),
        area,
    );
}

fn render_wide_body(
    frame: &mut ratatui::Frame<'_>,
    view: &sdk::ConfigFormView,
    area: Rect,
    scroll: u16,
    interaction: ConfigFormInteraction,
) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(56), Constraint::Percentage(44)])
        .split(area);
    render_fields(frame, view, columns[0], scroll, interaction);
    render_details(frame, view, columns[1], scroll);
}

fn render_narrow_body(
    frame: &mut ratatui::Frame<'_>,
    view: &sdk::ConfigFormView,
    area: Rect,
    scroll: u16,
    interaction: ConfigFormInteraction,
) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);
    render_fields(frame, view, rows[0], scroll, interaction);
    render_details(frame, view, rows[1], scroll);
}

fn render_fields(
    frame: &mut ratatui::Frame<'_>,
    view: &sdk::ConfigFormView,
    area: Rect,
    scroll: u16,
    interaction: ConfigFormInteraction,
) {
    let items = view
        .page
        .fields
        .iter()
        .enumerate()
        .flat_map(|(field_index, field)| {
            let is_focused = field_index == interaction.focused_field;
            let marker = if is_focused { "▸ " } else { "  " };
            let label_style = if is_focused {
                Style::default().fg(theme::ACCENT_BRIGHT)
            } else {
                Style::default().fg(theme::TEXT)
            };
            let mut lines = vec![ListItem::new(Line::from(vec![
                Span::styled(marker, Style::default().fg(theme::ACCENT)),
                Span::styled(&field.label, label_style),
            ]))];
            if field.field_type == sdk::ConfigFormFieldType::SingleSelect {
                lines.extend(
                    field
                        .options
                        .iter()
                        .enumerate()
                        .map(|(option_index, option)| {
                            let selected =
                                is_focused && option_index == interaction.selected_option;
                            let option_marker = if selected { "    ● " } else { "    ○ " };
                            let option_style = if selected {
                                Style::default().fg(theme::ACCENT_BRIGHT)
                            } else {
                                Style::default().fg(theme::TEXT)
                            };
                            ListItem::new(Line::from(vec![
                                Span::styled(option_marker, Style::default().fg(theme::ACCENT)),
                                Span::styled(&option.label, option_style),
                            ]))
                        }),
                );
            } else if let Some(value) = &field.display_value {
                lines.push(ListItem::new(Line::from(Span::styled(
                    format!("    {value}"),
                    Style::default().fg(theme::TEXT_MUTED),
                ))));
            } else if field.has_value {
                lines.push(ListItem::new(Line::from(Span::styled(
                    "    已设置",
                    Style::default().fg(theme::SUCCESS),
                ))));
            }
            lines
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::RIGHT)
                    .border_style(Style::default().fg(theme::BORDER)),
            )
            .style(Style::default().bg(theme::SURFACE)),
        area,
    );
    let _ = scroll;
}

fn render_details(
    frame: &mut ratatui::Frame<'_>,
    view: &sdk::ConfigFormView,
    area: Rect,
    scroll: u16,
) {
    let mut lines = Vec::new();
    for field in &view.page.fields {
        if let Some(description) = &field.description {
            lines.push(Line::from(Span::styled(
                description,
                Style::default().fg(theme::TEXT_MUTED),
            )));
        }
        if let Some(error) = &field.error {
            lines.push(Line::from(Span::styled(
                &error.message,
                Style::default().fg(theme::ERROR),
            )));
        }
    }
    if let Some(error) = &view.page.error {
        lines.push(Line::from(Span::styled(
            &error.message,
            Style::default().fg(theme::ERROR),
        )));
    }
    if let Some(busy) = &view.busy {
        lines.push(Line::from(Span::styled(
            &busy.message,
            Style::default().fg(theme::TOOL_RUNNING),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((scroll, 0))
            .wrap(Wrap { trim: true })
            .style(Style::default().bg(theme::SURFACE)),
        area,
    );
}

fn render_footer(
    frame: &mut ratatui::Frame<'_>,
    view: &sdk::ConfigFormView,
    visible_input: &str,
    area: Rect,
    interaction: ConfigFormInteraction,
) {
    let mut action_spans = Vec::new();
    for (action_index, action) in view.page.actions.iter().enumerate() {
        if action_index > 0 {
            action_spans.push(Span::styled(" · ", Style::default().fg(theme::TEXT_DIM)));
        }
        let selected = interaction.focused_action == action_index;
        let label = if selected {
            format!("[{}]", action.label)
        } else {
            action.label.clone()
        };
        action_spans.push(Span::styled(
            label,
            if selected {
                Style::default().fg(theme::ACCENT_BRIGHT)
            } else {
                Style::default().fg(theme::TEXT_MUTED)
            },
        ));
    }
    let text = vec![
        Line::from(action_spans),
        Line::from(vec![
            Span::styled("> ", Style::default().fg(theme::ACCENT)),
            Span::styled(visible_input, Style::default().fg(theme::TEXT)),
        ]),
        Line::from(Span::styled(
            "Tab/↑↓ 切换 · ←→ 选择 · Esc 取消 · Enter 提交",
            Style::default().fg(theme::TEXT_DIM),
        )),
    ];
    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(theme::BORDER)),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

#[cfg(test)]
#[path = "config_form_scenario_tests.rs"]
mod scenario_tests;
