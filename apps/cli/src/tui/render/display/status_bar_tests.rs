use super::*;
use crate::tui::render::theme;
use crate::tui::view_model::{StatusContextViewModel, StatusRuntimeViewModel, StatusWorktreeKind};

#[cfg(test)]
pub(crate) fn set_test_status_text(bar: &mut StatusBar, status: &str) {
    bar.status = status.to_string();
    bar.status_type = StatusType::Normal;
    bar.vm = crate::tui::view_model::StatusRuntimeViewModel::default();
    bar.thinking = false;
}

/// 屏幕列经只读折算 + 镜像写回（adapter 唯一生产写入路径），驱动 plain 取文本。
/// 替代已删除的 `start_selection*`/`update_selection*` 状态变更方法，覆盖不弱化。
#[cfg(test)]
fn select_via_mirror(bar: &mut StatusBar, row: StatusBarRow, sc: u16, ec: u16, width: u16) {
    let start = bar.screen_col_to_char_idx(row, sc, width);
    let end = bar.screen_col_to_char_idx(row, ec, width);
    bar.apply_selection_mirror(true, Some(start), Some(end), row, width);
}

#[test]
fn test_status_bar_selection_maps_cjk_screen_col_to_char_index() {
    let mut bar = StatusBar::new();
    set_test_status_text(&mut bar, "你好a");
    let prefix_width = 1;

    select_via_mirror(
        &mut bar,
        StatusBarRow::Runtime,
        prefix_width + 2,
        prefix_width + 6,
        0,
    );

    assert_eq!(bar.get_selected_text(), Some("好a ".to_string()));
}

#[test]
fn test_status_bar_selection_maps_emoji_screen_col_to_char_index() {
    let mut bar = StatusBar::new();
    set_test_status_text(&mut bar, "a🚀b");
    let prefix_width = 1;

    select_via_mirror(
        &mut bar,
        StatusBarRow::Runtime,
        prefix_width + 1,
        prefix_width + 4,
        0,
    );

    assert_eq!(bar.get_selected_text(), Some("🚀b".to_string()));
}

fn row_text(buf: &Buffer, y: u16, width: u16) -> String {
    (0..width)
        .filter_map(|x| buf.cell((x, y)).map(|cell| cell.symbol().to_string()))
        .collect::<String>()
}

fn apply_runtime_context(bar: &mut StatusBar, path_base: &str, working_root: &str, branch: &str) {
    let kind = if branch == "main" {
        StatusWorktreeKind::Main
    } else {
        StatusWorktreeKind::Worktree
    };
    bar.apply_runtime_view(StatusRuntimeViewModel {
        context: StatusContextViewModel {
            path_base: path_base.to_string(),
            working_root: working_root.to_string(),
            branch: Some(branch.to_string()),
            kind,
        },
        ..StatusRuntimeViewModel::default()
    });
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
    apply_runtime_context(
        &mut bar,
        "/workspace/projects/example/.worktrees/topic-46-status-line/cli/src/tui",
        "/workspace/projects/example/.worktrees/topic-46-status-line",
        "feature/46-status-line",
    );
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
    apply_runtime_context(
        &mut bar,
        "/workspace/projects/example/.worktrees/topic-46-status-line/cli/src/tui",
        "/workspace/projects/example/.worktrees/topic-46-status-line",
        "feature/46-status-line",
    );
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
    apply_runtime_context(
        &mut bar,
        "/workspace/projects/example/.worktrees/topic-46-status-line/cli/src/tui",
        "/workspace/projects/example/.worktrees/topic-46-status-line",
        "feature/46-status-line",
    );
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
    apply_runtime_context(
        &mut bar,
        "/workspace/projects/example/.worktrees/topic-46-status-line/cli/src/tui/app/update",
        "/workspace/projects/example/.worktrees/topic-46-status-line",
        "feature/46-status-line",
    );
    bar.set_permission_mode("AllowAll");

    let row = bar.context_row_text(56);

    assert!(row.starts_with('/'));
    assert!(row.starts_with('/'));
    assert!(row.contains("AllowAll"));
}

#[test]
fn test_status_line_context_wide_truncates_to_width() {
    let mut bar = StatusBar::new();
    bar.apply_runtime_view(StatusRuntimeViewModel {
        context: StatusContextViewModel {
            path_base:
                "/workspace/projects/example/.worktrees/topic-46-status-line/cli/src/tui/app/update/deep/path"
                    .to_string(),
            working_root:
                "/workspace/projects/example/.worktrees/topic-46-status-line/with/an/extra/long/root/path"
                    .to_string(),
            branch: Some("feature/46-status-line-with-a-very-long-branch-name".to_string()),
            kind: StatusWorktreeKind::Worktree,
        },
        ..StatusRuntimeViewModel::default()
    });
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
fn test_screen_to_status_anchor_runtime_row_folds_relative_col() {
    let mut bar = StatusBar::new();
    set_test_status_text(&mut bar, "你好a");
    // bar_y=10：row==10 非 Context（!=bar_y+1）→ Runtime；列相对 bar_x=0 偏移后折算。
    let (row, char_idx, width) = bar.screen_to_status_anchor(10, 2, 10, 0, 0);
    assert_eq!(row, StatusBarRow::Runtime);
    // 屏幕列 2（"你"宽 2）→ char_idx 1（"好" 起点）。
    assert_eq!(char_idx, 1);
    assert_eq!(width, 0);
}

#[test]
fn test_screen_to_status_anchor_context_row_and_col_offset() {
    let mut bar = StatusBar::new();
    bar.set_current_dir("~/aemeath");
    // bar_y=5、bar_x=3、bar_width=80：row==6(==bar_y+1) → Context；列减去 bar_x。
    let (row, char_idx, width) = bar.screen_to_status_anchor(6, 3 + 2, 5, 3, 80);
    assert_eq!(row, StatusBarRow::Context);
    // 相对列 2 在 Context 行折算（与 start_selection_at(Context,2,80) 一致）。
    assert_eq!(
        char_idx,
        bar.screen_col_to_char_idx(StatusBarRow::Context, 2, 80)
    );
    assert_eq!(width, 80);
}

#[test]
fn test_screen_to_status_anchor_does_not_mutate_widget_state() {
    let mut bar = StatusBar::new();
    set_test_status_text(&mut bar, "abc");
    // 只读折算 NEVER 触动 widget 选区字段。
    let _ = bar.screen_to_status_anchor(0, 1, 0, 0, 0);
    assert!(!bar.is_selecting());
    assert!(bar.get_selected_text().is_none());
}

#[test]
fn test_status_bar_selection_supports_context_row() {
    let mut bar = StatusBar::new();
    bar.set_current_dir("~/aemeath");
    let width = 80;

    select_via_mirror(&mut bar, StatusBarRow::Context, 2, 9, width);

    assert_eq!(bar.get_selected_text(), Some("aemeath".to_string()));
}

#[test]
fn test_status_bar_render_highlights_context_row_selection() {
    let mut bar = StatusBar::new();
    bar.set_current_dir("~/aemeath");
    let area = Rect::new(0, 0, 80, 2);
    let mut buf = Buffer::empty(area);

    select_via_mirror(&mut bar, StatusBarRow::Context, 4, 11, area.width);
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
    apply_runtime_context(&mut bar, "~/aemeath", "~/aemeath", "main");

    let row = bar.context_row_text(80);

    assert!(row.contains(" │ main │ "));
    assert!(!row.contains("main:main"));
}

#[test]
fn test_status_line_context_keeps_permission_when_space_is_tight() {
    let mut bar = StatusBar::new();
    apply_runtime_context(&mut bar, "aemeath", "aemeath", "main");
    bar.set_permission_mode("AskMe");

    let row = bar.context_row_text(24);

    assert!(row.chars().count() <= 24);
    assert!(row.contains("AskMe"));
}
