use super::*;
use crate::tui::display::theme;

#[cfg(test)]
pub(crate) fn set_test_status_text(bar: &mut StatusBar, status: &str) {
    bar.status = status.to_string();
    bar.status_type = StatusType::Normal;
    bar.input_tokens = 0;
    bar.output_tokens = 0;
    bar.last_input_tokens = 0;
    bar.session_id = None;
    bar.api_calls = 0;
    bar.model = None;
    bar.context_size = 0;
    bar.tps = 0.0;
    bar.thinking = false;
}

#[test]
fn test_status_bar_selection_maps_cjk_screen_col_to_char_index() {
    let mut bar = StatusBar::new();
    set_test_status_text(&mut bar, "你好a");
    let prefix_width = 1;

    bar.start_selection(prefix_width + 2);
    bar.update_selection(prefix_width + 6);

    assert_eq!(bar.get_selected_text(), Some("好a ".to_string()));
}

#[test]
fn test_status_bar_selection_maps_emoji_screen_col_to_char_index() {
    let mut bar = StatusBar::new();
    set_test_status_text(&mut bar, "a🚀b");
    let prefix_width = 1;

    bar.start_selection(prefix_width + 1);
    bar.update_selection(prefix_width + 4);

    assert_eq!(bar.get_selected_text(), Some("🚀b".to_string()));
}

fn row_text(buf: &Buffer, y: u16, width: u16) -> String {
    (0..width)
        .filter_map(|x| buf.cell((x, y)).map(|cell| cell.symbol().to_string()))
        .collect::<String>()
}

#[test]
fn test_status_bar_render_uses_status_background() {
    let bar = StatusBar::new();
    let area = Rect::new(0, 0, 40, 1);
    let mut buf = Buffer::empty(area);

    bar.render(area, &mut buf);

    assert_eq!(buf.cell((0, 0)).unwrap().style().bg, Some(theme::STATUS_BG));
    assert_eq!(
        buf.cell((39, 0)).unwrap().style().bg,
        Some(theme::STATUS_BG)
    );
}

#[test]
fn test_status_bar_render_two_rows_includes_context_when_height_two() {
    let mut bar = StatusBar::new();
    bar.set_context_paths(
        "/workspace/projects/example/.worktrees/topic-46-status-line/cli/src/tui",
        "/workspace/projects/example/.worktrees/topic-46-status-line",
    );
    bar.set_git_context(WorktreeKind::Worktree, "feature/46-status-line");
    let area = Rect::new(0, 0, 100, 2);
    let mut buf = Buffer::empty(area);

    bar.render(area, &mut buf);

    let runtime = row_text(&buf, 0, area.width);
    let context = row_text(&buf, 1, area.width);
    assert!(!runtime.contains("Think:"));
    assert!(!runtime.contains("ctx "));
    assert!(context.starts_with('/'));
    assert!(context.contains("worktree:feature/46-status-line"));
}

#[test]
fn test_status_bar_render_one_row_omits_context() {
    let mut bar = StatusBar::new();
    bar.set_context_paths(
        "/workspace/projects/example/.worktrees/topic-46-status-line/cli/src/tui",
        "/workspace/projects/example/.worktrees/topic-46-status-line",
    );
    bar.set_git_context(WorktreeKind::Worktree, "feature/46-status-line");
    let area = Rect::new(0, 0, 100, 1);
    let mut buf = Buffer::empty(area);

    bar.render(area, &mut buf);

    let runtime = row_text(&buf, 0, area.width);
    assert!(!runtime.contains("Think:"));
    assert!(!runtime.contains("ctx "));
    assert!(!runtime.contains("worktree:feature/46-status-line"));
}

#[test]
fn test_status_line_context_defaults_to_balanced_row() {
    let mut bar = StatusBar::new();
    bar.set_model("claude-sonnet");
    bar.set_context_paths(
        "/workspace/projects/example/.worktrees/topic-46-status-line/cli/src/tui",
        "/workspace/projects/example/.worktrees/topic-46-status-line",
    );
    bar.set_git_context(WorktreeKind::Worktree, "feature/46-status-line");
    bar.set_permission_mode("AskMe");

    let row = bar.context_row_text(120);

    assert!(!row.contains("ctx "));
    assert!(row.starts_with('/'));
    assert!(row.contains("root /"));
    assert!(row.contains("topic-46-status-line"));
    assert!(row.contains("worktree:feature/46-status-line"));
    assert!(row.contains("AskMe"));
}

#[test]
fn test_status_line_context_narrow_keeps_path_branch_and_permission() {
    let mut bar = StatusBar::new();
    bar.set_context_paths(
        "/workspace/projects/example/.worktrees/topic-46-status-line/cli/src/tui/app/update",
        "/workspace/projects/example/.worktrees/topic-46-status-line",
    );
    bar.set_git_context(WorktreeKind::Worktree, "feature/46-status-line");
    bar.set_permission_mode("AllowAll");

    let row = bar.context_row_text(56);

    assert!(row.starts_with('/'));
    assert!(row.starts_with('/'));
    assert!(row.contains("AllowAll"));
}

#[test]
fn test_status_line_context_wide_truncates_to_width() {
    let mut bar = StatusBar::new();
    bar.set_context_paths(
        "/workspace/projects/example/.worktrees/topic-46-status-line/cli/src/tui/app/update/deep/path",
        "/workspace/projects/example/.worktrees/topic-46-status-line/with/an/extra/long/root/path",
    );
    bar.set_git_context(
        WorktreeKind::Worktree,
        "feature/46-status-line-with-a-very-long-branch-name",
    );
    bar.set_permission_mode("AllowAllWithExtraLongModeName");

    let row = bar.context_row_text(70);

    assert!(row.starts_with('/'));
}

#[test]
fn test_status_bar_render_second_row_uses_muted_color() {
    let mut bar = StatusBar::new();
    bar.set_current_dir("~/aemeath");
    let area = Rect::new(0, 0, 80, 2);
    let mut buf = Buffer::empty(area);

    bar.render(area, &mut buf);

    assert_eq!(buf.cell((0, 1)).unwrap().style().fg, Some(theme::ACCENT));
    assert_eq!(buf.cell((0, 1)).unwrap().style().bg, Some(theme::STATUS_BG));
}

#[test]
fn test_status_bar_selection_supports_context_row() {
    let mut bar = StatusBar::new();
    bar.set_current_dir("~/aemeath");
    let width = 80;

    bar.start_selection_at(StatusBarRow::Context, 2, width);
    bar.update_selection_at(9, width);

    assert_eq!(bar.get_selected_text(), Some("aemeath".to_string()));
}

#[test]
fn test_status_bar_render_highlights_context_row_selection() {
    let mut bar = StatusBar::new();
    bar.set_current_dir("~/aemeath");
    let area = Rect::new(0, 0, 80, 2);
    let mut buf = Buffer::empty(area);

    bar.start_selection_at(StatusBarRow::Context, 4, area.width);
    bar.update_selection_at(11, area.width);
    bar.render(area, &mut buf);

    assert_eq!(
        buf.cell((4, 1)).unwrap().style().bg,
        Some(theme::SELECTION_BG)
    );
    assert_eq!(
        buf.cell((4, 0)).unwrap().style().bg,
        Some(theme::SELECTION_BG)
    );
}

#[test]
fn test_main_branch_does_not_repeat_main_main() {
    let mut bar = StatusBar::new();
    bar.set_current_dir("~/aemeath");
    bar.set_git_context(WorktreeKind::Main, "main");

    let row = bar.context_row_text(80);

    assert!(row.contains(" │ main │ "));
    assert!(!row.contains("main:main"));
}

#[test]
fn test_status_line_context_keeps_permission_when_space_is_tight() {
    let mut bar = StatusBar::new();
    bar.set_context_paths("aemeath", "aemeath");
    bar.set_git_context(WorktreeKind::Main, "main");
    bar.set_permission_mode("AskMe");

    let row = bar.context_row_text(24);

    assert!(row.chars().count() <= 24);
    assert!(row.contains("AskMe"));
}
