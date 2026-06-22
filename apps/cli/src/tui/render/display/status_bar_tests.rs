use super::*;
use crate::tui::render::theme;
use crate::tui::view_model::{
    StatusContextViewModel, StatusNoticeViewKind, StatusNoticeViewModel, StatusRuntimeViewModel,
    StatusViewModel, StatusWorktreeKind,
};
use crate::tui::view_state::StatusSelectionViewState;

fn status_view(status: &str) -> StatusViewModel {
    StatusViewModel {
        notice: StatusNoticeViewModel {
            text: status.to_string(),
            kind: StatusNoticeViewKind::Normal,
        },
        ..StatusViewModel::default()
    }
}

fn runtime_context_view(path_base: &str, workspace_root: &str, branch: &str) -> StatusViewModel {
    let kind = if branch == "main" {
        StatusWorktreeKind::Main
    } else {
        StatusWorktreeKind::Worktree
    };
    StatusViewModel {
        runtime: StatusRuntimeViewModel {
            context: StatusContextViewModel {
                path_base: path_base.to_string(),
                workspace_root: workspace_root.to_string(),
                branch: Some(branch.to_string()),
                kind,
            },
            ..StatusRuntimeViewModel::default()
        },
        ..StatusViewModel::default()
    }
}

/// 屏幕列经只读折算后写入 view_state，驱动 render/copy 投影。
#[cfg(test)]
fn select_via_view_state(
    bar: &StatusBar,
    row: StatusBarRow,
    sc: u16,
    ec: u16,
    width: u16,
    view: &StatusViewModel,
) -> StatusSelectionViewState {
    let start = bar.screen_col_to_char_idx(row, sc, width, view);
    let end = bar.screen_col_to_char_idx(row, ec, width, view);
    let mut selection = StatusSelectionViewState::default();
    selection.begin_selection(row, start, width);
    selection.update_selection(end);
    selection.end_selection();
    selection
}

#[test]
fn test_status_bar_selection_maps_cjk_screen_col_to_char_index() {
    let bar = StatusBar::new();
    let view = status_view("你好a");
    let prefix_width = 1;

    let selection = select_via_view_state(
        &bar,
        StatusBarRow::Runtime,
        prefix_width + 2,
        prefix_width + 6,
        0,
        &view,
    );

    assert_eq!(
        bar.selected_text_for_view(&selection, &view),
        Some("好a ".to_string())
    );
}

#[test]
fn test_status_bar_selection_maps_emoji_screen_col_to_char_index() {
    let bar = StatusBar::new();
    let view = status_view("a🚀b");
    let prefix_width = 1;

    let selection = select_via_view_state(
        &bar,
        StatusBarRow::Runtime,
        prefix_width + 1,
        prefix_width + 4,
        0,
        &view,
    );

    assert_eq!(
        bar.selected_text_for_view(&selection, &view),
        Some("🚀b".to_string())
    );
}

fn row_text(buf: &Buffer, y: u16, width: u16) -> String {
    (0..width)
        .filter_map(|x| buf.cell((x, y)).map(|cell| cell.symbol().to_string()))
        .collect::<String>()
}

#[test]
fn test_status_bar_render_uses_status_background() {
    let bar = StatusBar::new();
    let view = StatusViewModel::default();
    let area = Rect::new(0, 0, 40, 1);
    let mut buf = Buffer::empty(area);

    bar.render(area, &mut buf, &StatusSelectionViewState::default(), &view);

    assert_eq!(buf.cell((0, 0)).unwrap().style().bg, Some(theme::STATUS_BG));
    assert_eq!(
        buf.cell((39, 0)).unwrap().style().bg,
        Some(theme::STATUS_BG)
    );
}

#[test]
fn test_status_bar_render_two_rows_includes_context_when_height_two() {
    let bar = StatusBar::new();
    let view = runtime_context_view(
        "/workspace/projects/example/.worktrees/topic-46-status-line/cli/src/tui",
        "/workspace/projects/example/.worktrees/topic-46-status-line",
        "feature/46-status-line",
    );
    let area = Rect::new(0, 0, 100, 2);
    let mut buf = Buffer::empty(area);

    bar.render(area, &mut buf, &StatusSelectionViewState::default(), &view);

    let runtime = row_text(&buf, 0, area.width);
    let context = row_text(&buf, 1, area.width);
    assert!(!runtime.contains("Think:"));
    assert!(!runtime.contains("ctx "));
    assert!(context.starts_with('/'));
    assert!(context.contains("worktree:feature/46-status-line"));
}

#[test]
fn test_status_bar_render_one_row_omits_context() {
    let bar = StatusBar::new();
    let view = runtime_context_view(
        "/workspace/projects/example/.worktrees/topic-46-status-line/cli/src/tui",
        "/workspace/projects/example/.worktrees/topic-46-status-line",
        "feature/46-status-line",
    );
    let area = Rect::new(0, 0, 100, 1);
    let mut buf = Buffer::empty(area);

    bar.render(area, &mut buf, &StatusSelectionViewState::default(), &view);

    let runtime = row_text(&buf, 0, area.width);
    assert!(!runtime.contains("Think:"));
    assert!(!runtime.contains("ctx "));
    assert!(!runtime.contains("worktree:feature/46-status-line"));
}

#[test]
fn test_status_line_context_defaults_to_balanced_row() {
    let mut bar = StatusBar::new();
    let view = runtime_context_view(
        "/workspace/projects/example/.worktrees/topic-46-status-line/cli/src/tui",
        "/workspace/projects/example/.worktrees/topic-46-status-line",
        "feature/46-status-line",
    );
    bar.set_permission_mode("AskMe");

    let row = bar.context_row_text_for_view(120, &view);

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
    let view = runtime_context_view(
        "/workspace/projects/example/.worktrees/topic-46-status-line/cli/src/tui/app/update",
        "/workspace/projects/example/.worktrees/topic-46-status-line",
        "feature/46-status-line",
    );
    bar.set_permission_mode("AllowAll");

    let row = bar.context_row_text_for_view(56, &view);

    assert!(row.starts_with('/'));
    assert!(row.starts_with('/'));
    assert!(row.contains("AllowAll"));
}

#[test]
fn test_status_line_context_wide_truncates_to_width() {
    let mut bar = StatusBar::new();
    let view = runtime_context_view(
        "/workspace/projects/example/.worktrees/topic-46-status-line/cli/src/tui/app/update/deep/path",
        "/workspace/projects/example/.worktrees/topic-46-status-line/with/an/extra/long/root/path",
        "feature/46-status-line-with-a-very-long-branch-name",
    );
    bar.set_permission_mode("AllowAllWithExtraLongModeName");

    let row = bar.context_row_text_for_view(70, &view);

    assert!(row.starts_with('/'));
}

#[test]
fn test_status_bar_render_second_row_uses_muted_color() {
    let bar = StatusBar::new();
    let view = runtime_context_view("~/aemeath", "~/aemeath", "main");
    let area = Rect::new(0, 0, 80, 2);
    let mut buf = Buffer::empty(area);

    bar.render(area, &mut buf, &StatusSelectionViewState::default(), &view);

    assert_eq!(buf.cell((0, 1)).unwrap().style().fg, Some(theme::ACCENT));
    assert_eq!(buf.cell((0, 1)).unwrap().style().bg, Some(theme::STATUS_BG));
}

#[test]
fn test_screen_to_status_anchor_runtime_row_folds_relative_col() {
    let bar = StatusBar::new();
    let view = status_view("你好a");
    // bar_y=10：row==10 非 Context（!=bar_y+1）→ Runtime；列相对 bar_x=0 偏移后折算。
    let (row, char_idx, width) = bar.screen_to_status_anchor(10, 2, 10, 0, 0, &view);
    assert_eq!(row, StatusBarRow::Runtime);
    // 屏幕列 2（"你"宽 2）→ char_idx 1（"好" 起点）。
    assert_eq!(char_idx, 1);
    assert_eq!(width, 0);
}

#[test]
fn test_screen_to_status_anchor_context_row_and_col_offset() {
    let bar = StatusBar::new();
    let view = runtime_context_view("~/aemeath", "~/aemeath", "main");
    // bar_y=5、bar_x=3、bar_width=80：row==6(==bar_y+1) → Context；列减去 bar_x。
    let (row, char_idx, width) = bar.screen_to_status_anchor(6, 3 + 2, 5, 3, 80, &view);
    assert_eq!(row, StatusBarRow::Context);
    // 相对列 2 在 Context 行折算（与 start_selection_at(Context,2,80) 一致）。
    assert_eq!(
        char_idx,
        bar.screen_col_to_char_idx(StatusBarRow::Context, 2, 80, &view)
    );
    assert_eq!(width, 80);
}

#[test]
fn test_screen_to_status_anchor_does_not_mutate_widget_state() {
    let bar = StatusBar::new();
    let view = status_view("abc");
    // 只读折算 NEVER 触动 widget 选区字段。
    let _ = bar.screen_to_status_anchor(0, 1, 0, 0, 0, &view);
    assert!(bar
        .selected_text_for_view(&StatusSelectionViewState::default(), &view)
        .is_none());
}

#[test]
fn test_status_bar_selection_supports_context_row() {
    let bar = StatusBar::new();
    let view = runtime_context_view("~/aemeath", "~/aemeath", "main");
    let width = 80;

    let selection = select_via_view_state(&bar, StatusBarRow::Context, 2, 9, width, &view);

    assert_eq!(
        bar.selected_text_for_view(&selection, &view),
        Some("aemeath".to_string())
    );
}

#[test]
fn test_status_bar_render_highlights_context_row_selection() {
    let bar = StatusBar::new();
    let view = runtime_context_view("~/aemeath", "~/aemeath", "main");
    let area = Rect::new(0, 0, 80, 2);
    let mut buf = Buffer::empty(area);

    let selection = select_via_view_state(&bar, StatusBarRow::Context, 4, 11, area.width, &view);
    bar.render(area, &mut buf, &selection, &view);

    assert_eq!(
        buf.cell((4, 1)).unwrap().style().bg,
        Some(theme::SELECTION_BG)
    );
    assert_eq!(buf.cell((4, 0)).unwrap().style().bg, Some(theme::STATUS_BG));
}

#[test]
fn test_main_branch_does_not_repeat_main_main() {
    let bar = StatusBar::new();
    let view = runtime_context_view("~/aemeath", "~/aemeath", "main");

    let row = bar.context_row_text_for_view(80, &view);

    assert!(row.contains(" │ main │ "));
    assert!(!row.contains("main:main"));
}

#[test]
fn test_status_line_context_keeps_permission_when_space_is_tight() {
    let mut bar = StatusBar::new();
    let view = runtime_context_view("aemeath", "aemeath", "main");
    bar.set_permission_mode("AskMe");

    let row = bar.context_row_text_for_view(24, &view);

    assert!(row.chars().count() <= 24);
    assert!(row.contains("AskMe"));
}
