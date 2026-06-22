use super::*;
use crate::tui::render::theme;
use crate::tui::view_model::{
    StatusContextViewModel, StatusNoticeViewKind, StatusNoticeViewModel, StatusRuntimeViewModel,
    StatusViewModel, StatusWorktreeKind,
};
use crate::tui::view_state::StatusSelectionViewState;

fn row_text(buf: &Buffer, y: u16, width: u16) -> String {
    (0..width)
        .filter_map(|x| buf.cell((x, y)).map(|cell| cell.symbol().to_string()))
        .collect::<String>()
}

fn status_view(
    path_base: &str,
    workspace_root: &str,
    kind: StatusWorktreeKind,
    branch: &str,
    session_id: Option<&str>,
) -> StatusViewModel {
    StatusViewModel {
        notice: StatusNoticeViewModel {
            text: "Ready".to_string(),
            kind: StatusNoticeViewKind::Success,
        },
        runtime: StatusRuntimeViewModel {
            session_id: session_id.map(str::to_string),
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

#[test]
fn test_runtime_row_shows_token_in_out_tps_ctx_and_api_without_cost_or_session() {
    let bar = StatusBar::new();
    let view = StatusViewModel {
        notice: StatusNoticeViewModel {
            text: "Ready".to_string(),
            kind: StatusNoticeViewKind::Success,
        },
        runtime: StatusRuntimeViewModel {
            model: Some("zhipu/glm-5.1".to_string()),
            session_id: Some("019-session".to_string()),
            input_tokens: 12_400,
            output_tokens: 1_800,
            last_input_tokens: 74_000,
            api_calls: 7,
            context_size: 200_000,
            tps: 42.0,
            context: StatusContextViewModel::default(),
        },
        ..StatusViewModel::default()
    };

    let text = bar.build_full_text(&view);

    assert!(text.contains("Ready"));
    assert!(text.contains("zhipu/glm-5.1"));
    assert!(text.contains("in 12k"));
    assert!(text.contains("out 1.8k"));
    assert!(text.contains("42 t/s"));
    assert!(text.contains("ctx 37%"));
    assert!(text.contains("api 7"));
    assert!(!text.to_ascii_lowercase().contains("session"));
    assert!(!text.contains("019-session"));
    assert!(!text.to_ascii_lowercase().contains("cost"));
    assert!(!text.contains('$'));
}

#[test]
fn test_context_row_uses_real_path_not_ctx_label_when_paths_match() {
    let mut bar = StatusBar::new();
    let view = status_view(
        "~/Nextcloud/work/claudecode/aemeath",
        "~/Nextcloud/work/claudecode/aemeath",
        StatusWorktreeKind::Main,
        "main",
        Some("019-session-full"),
    );
    bar.set_permission_mode("AskMe");

    let row = bar.context_row_text_for_view(120, &view);

    assert_eq!(
        row,
        "~/Nextcloud/work/claudecode/aemeath │ main │ AskMe │ session 019-session-full"
    );
    assert!(!row.contains("ctx "));
    assert!(!row.contains("root "));
    assert!(!row.contains("Perm:"));
}

#[test]
fn test_context_row_shows_root_only_when_different() {
    let mut bar = StatusBar::new();
    let view = status_view(
        "~/Nextcloud/work/claudecode/aemeath/cli",
        "~/Nextcloud/work/claudecode/aemeath",
        StatusWorktreeKind::Main,
        "main",
        Some("019-session-full"),
    );
    bar.set_permission_mode("AskMe");

    let row = bar.context_row_text_for_view(140, &view);

    assert!(row.contains("~/Nextcloud/work/claudecode/aemeath/cli"));
    assert!(row.contains("root ~/Nextcloud/work/claudecode/aemeath"));
    assert!(row.contains(" │ main │ AskMe"));
    assert!(row.contains("session 019-session-full"));
}

#[test]
fn test_context_row_worktree_uses_worktree_branch_label() {
    let mut bar = StatusBar::new();
    let view = status_view(
        "~/Nextcloud/work/claudecode/aemeath/.worktrees/redesign-46-status-line-v2",
        "~/Nextcloud/work/claudecode/aemeath/.worktrees/redesign-46-status-line-v2",
        StatusWorktreeKind::Worktree,
        "redesign/46-status-line-v2",
        Some("019-session-full"),
    );
    bar.set_permission_mode("AskMe");

    let row = bar.context_row_text_for_view(140, &view);

    assert!(row.contains('~'));
    assert!(row.contains(".worktrees/redesign-46-status-line-v2"));
    assert!(row.contains("worktree:redesign/46-status-line-v2"));
    assert!(row.ends_with("session 019-session-full"));
}

#[test]
fn test_context_row_narrow_preserves_path_permission_and_session() {
    let mut bar = StatusBar::new();
    let view = status_view(
        "~/Nextcloud/work/claudecode/aemeath/.worktrees/redesign-46-status-line-v2/cli/src/tui",
        "~/Nextcloud/work/claudecode/aemeath/.worktrees/redesign-46-status-line-v2",
        StatusWorktreeKind::Worktree,
        "redesign/46-status-line-v2",
        Some("019-session-full"),
    );
    bar.set_permission_mode("AllowAll");

    let row = bar.context_row_text_for_view(72, &view);

    assert!(row.starts_with('~') || row.starts_with('/'));
    assert!(row.contains("AllowAll"));
    assert!(row.ends_with("session 019-session-full"));
}

#[test]
fn test_context_row_renders_path_git_permission_and_session_with_distinct_colors() {
    let mut bar = StatusBar::new();
    let view = status_view(
        "~/Nextcloud/work/claudecode/aemeath",
        "~/Nextcloud/work/claudecode/aemeath",
        StatusWorktreeKind::Main,
        "main",
        Some("019-session-full"),
    );
    bar.set_permission_mode("AskMe");
    let area = Rect::new(0, 0, 120, 2);
    let mut buf = Buffer::empty(area);

    bar.render(area, &mut buf, &StatusSelectionViewState::default(), &view);

    assert_eq!(buf.cell((0, 1)).unwrap().style().fg, Some(theme::ACCENT));
    assert_eq!(buf.cell((38, 1)).unwrap().style().fg, Some(theme::SUCCESS));
    assert_eq!(buf.cell((45, 1)).unwrap().style().fg, Some(theme::WARNING));
    assert_eq!(
        buf.cell((53, 1)).unwrap().style().fg,
        Some(theme::TEXT_MUTED)
    );
    assert_eq!(buf.cell((0, 1)).unwrap().style().bg, Some(theme::STATUS_BG));
}

#[test]
fn test_context_row_narrow_keeps_path_prefix_without_invalid_splice() {
    let mut bar = StatusBar::new();
    let view = status_view(
        "~/Nextcloud/work/claudecode/aemeath/.worktrees/redesign-46-status-line-v2/cli/src/tui",
        "~/Nextcloud/work/claudecode/aemeath/.worktrees/redesign-46-status-line-v2",
        StatusWorktreeKind::Worktree,
        "redesign/46-status-line-v2",
        Some("019-session-full"),
    );
    bar.set_permission_mode("AllowAll");

    let row = bar.context_row_text_for_view(72, &view);

    assert!(row.starts_with("~…") || row.starts_with("/…"));
    assert!(!row.starts_with("~e "));
    assert!(row.contains("AllowAll"));
    assert!(row.contains("session 019-session-full"));
}

#[test]
fn test_context_row_cjk_path_uses_display_width_budget() {
    let mut bar = StatusBar::new();
    let view = status_view(
        "~/项目/状态栏/aemeath",
        "~/项目/状态栏/aemeath",
        StatusWorktreeKind::Main,
        "main",
        Some("019-session-full"),
    );
    bar.set_permission_mode("AskMe");

    let row = bar.context_row_text_for_view(40, &view);

    assert!(
        crate::tui::render::display::safe_text::str_display_width(&row) <= 40
            || row.starts_with('~')
    );
    assert!(row.starts_with('~'));
    assert!(row.contains("AskMe"));
    assert!(row.contains("session 019-session-full"));
}

#[test]
fn test_context_row_without_session_uses_correct_semantic_colors() {
    let mut bar = StatusBar::new();
    let view = status_view(
        "~/aemeath",
        "~/aemeath",
        StatusWorktreeKind::Main,
        "main",
        None,
    );
    bar.set_permission_mode("AskMe");
    let area = Rect::new(0, 0, 80, 2);
    let mut buf = Buffer::empty(area);

    bar.render(area, &mut buf, &StatusSelectionViewState::default(), &view);

    assert_eq!(buf.cell((0, 1)).unwrap().style().fg, Some(theme::ACCENT));
    assert_eq!(buf.cell((12, 1)).unwrap().style().fg, Some(theme::SUCCESS));
    assert_eq!(buf.cell((19, 1)).unwrap().style().fg, Some(theme::WARNING));
}

#[test]
fn test_context_row_without_session_with_root_uses_correct_semantic_colors() {
    let mut bar = StatusBar::new();
    let view = status_view(
        "~/aemeath/cli",
        "~/aemeath",
        StatusWorktreeKind::Main,
        "main",
        None,
    );
    bar.set_permission_mode("AskMe");
    let area = Rect::new(0, 0, 80, 2);
    let mut buf = Buffer::empty(area);

    bar.render(area, &mut buf, &StatusSelectionViewState::default(), &view);

    assert_eq!(buf.cell((0, 1)).unwrap().style().fg, Some(theme::ACCENT));
    assert_eq!(
        buf.cell((17, 1)).unwrap().style().fg,
        Some(theme::TEXT_MUTED)
    );
    assert_eq!(buf.cell((34, 1)).unwrap().style().fg, Some(theme::SUCCESS));
    assert_eq!(buf.cell((41, 1)).unwrap().style().fg, Some(theme::WARNING));
}

#[test]
fn test_status_bar_render_two_rows_v2() {
    let bar = StatusBar::new();
    let mut view = status_view(
        "/workspace/projects/example/.worktrees/topic-46-status-line/cli/src/tui",
        "/workspace/projects/example/.worktrees/topic-46-status-line",
        StatusWorktreeKind::Worktree,
        "feature/46-status-line",
        None,
    );
    view.runtime.api_calls = 3;
    let area = Rect::new(0, 0, 120, 2);
    let mut buf = Buffer::empty(area);

    bar.render(area, &mut buf, &StatusSelectionViewState::default(), &view);

    let runtime = row_text(&buf, 0, area.width);
    let context = row_text(&buf, 1, area.width);
    assert!(runtime.contains("api 3"));
    assert!(!runtime.contains("Think:"));
    assert!(!runtime.contains("Session"));
    assert!(!context.contains("ctx "));
    assert!(context.starts_with('/'));
    assert!(context.contains("worktree:feature/46-status-line"));
}
