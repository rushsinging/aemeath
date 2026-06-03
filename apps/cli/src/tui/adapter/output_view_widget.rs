//! 输出区滚动 adapter：把 `OutputViewState` 的滚动真相单向写回 `OutputArea`
//! 的 `scroll_offset` / `auto_scroll` 镜像。这是这两个镜像字段的唯一生产写入路径。
//!
//! 时序（每帧渲染前）：
//! 1. 把上一帧 render 回填的 `output_area.last_visible_height` 反喂回 view_state，
//!    供滚动钳制使用（view_state 不持有 document/可见高度，由 render 期回填）；
//! 2. 检测文档行数增长量，`auto_scroll=false` 时补偿 `view_state.scroll_offset`，
//!    保持用户视窗内容固定（不受底部新增内容影响）；
//! 3. 据 document 总行数与可见高度钳制 view_state.scroll_offset（迁自旧
//!    `output_widget.rs::clamp_scroll_state`，真相归 view_state）；
//! 4. 把钳制后的 view_state 滚动态写回 widget 镜像。

use crate::tui::render::output_area::OutputArea;
use crate::tui::view_state::output::OutputViewState;

/// 据 view_state 滚动真相写回 widget 镜像（含 last_visible_height 反喂 + 内容增长补偿 + 钳制）。
///
/// 时序（每帧渲染前）：
/// 1. 把上一帧 render 回填的可见高度反喂回 view_state；
/// 2. 检测文档行数增长，auto_scroll=false 时补偿 scroll_offset，保持视窗内容固定；
/// 3. 钳制 scroll_offset 到 max_offset；
/// 4. 把钳制后的 view_state 滚动态写回 widget 镜像。
pub(crate) fn apply_output_scroll_to_widget(
    view: &mut OutputViewState,
    output_area: &mut OutputArea,
) {
    // ① 反喂上一帧渲染回填的可见高度。
    view.last_visible_height = output_area.last_visible_height;

    // ② 内容增长补偿：auto_scroll=false 时保持视窗顶部行号不变。
    let new_total = output_area.document().total_lines();
    if !view.auto_scroll {
        let growth = new_total.saturating_sub(view.last_document_total_lines);
        view.scroll_offset = view.scroll_offset.saturating_add(growth);
    }
    view.last_document_total_lines = new_total;

    // ③ 钳制 stale offset（迁自旧 clamp_scroll_state，真相归 view_state）。
    let max_offset = new_total.saturating_sub(view.last_visible_height);
    view.scroll_offset = view.scroll_offset.min(max_offset);
    // offset 归零即回到底部：与 scroll_down 一致（被动调整），
    // 区别于 scroll_up 用 max_offset==0（主动操作）。
    if view.scroll_offset == 0 {
        view.auto_scroll = true;
    }

    // ④ 单向写回 widget 镜像。
    output_area.scroll_offset = view.scroll_offset;
    output_area.auto_scroll = view.auto_scroll;
}

/// 据 view_state 选区真相单向写回 widget 选区镜像。
///
/// 生产路径只在渲染前同步镜像，供 output rendering 高亮使用；复制取文直接读取
/// `OutputViewState` + document，避免依赖 widget 选区镜像时序。
pub(crate) fn apply_output_selection_to_widget(
    view: &OutputViewState,
    output_area: &mut OutputArea,
) {
    output_area.is_selecting = view.is_selecting;
    output_area.selection_start = view.selection_start;
    output_area.selection_end = view.selection_end;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_writes_view_scroll_to_widget() {
        let mut view = OutputViewState {
            scroll_offset: 5,
            auto_scroll: false,
            last_document_total_lines: 100,
            ..Default::default()
        };
        let mut output = OutputArea::new();
        output.last_visible_height = 20;
        output.set_plain_document_lines(100);

        apply_output_scroll_to_widget(&mut view, &mut output);

        // 正常路径：可见高度反喂 + 有效 offset 原样写回 widget。
        assert_eq!(view.last_visible_height, 20);
        assert_eq!(output.scroll_offset, 5);
        assert!(!output.auto_scroll);
    }

    #[test]
    fn test_apply_clamps_stale_offset_and_reenables_auto_scroll() {
        let mut view = OutputViewState {
            scroll_offset: 100,
            auto_scroll: false,
            ..Default::default()
        };
        let mut output = OutputArea::new();
        output.last_visible_height = 2;
        output.set_plain_document_lines(1);

        apply_output_scroll_to_widget(&mut view, &mut output);

        // 边界：内容不足一屏（max_offset==0）→ 钳到 0 且恢复 auto_scroll，view 与 widget 一致。
        assert_eq!(view.scroll_offset, 0);
        assert!(view.auto_scroll);
        assert_eq!(output.scroll_offset, 0);
        assert!(output.auto_scroll);
    }

    #[test]
    fn test_apply_clamps_to_max_offset_when_offset_exceeds() {
        let mut view = OutputViewState {
            scroll_offset: 50,
            auto_scroll: false,
            ..Default::default()
        };
        let mut output = OutputArea::new();
        output.last_visible_height = 10;
        output.set_plain_document_lines(30);

        apply_output_scroll_to_widget(&mut view, &mut output);

        // 边界：offset 超过 max_offset(=20) 时钳到 max_offset，auto_scroll 保持关闭。
        assert_eq!(view.scroll_offset, 20);
        assert!(!view.auto_scroll);
        assert_eq!(output.scroll_offset, 20);
        assert!(!output.auto_scroll);
    }

    #[test]
    fn test_apply_selection_writes_view_anchors_to_widget() {
        use sdk::CharIdx;
        let mut view = OutputViewState::default();
        view.begin_selection(1, CharIdx::new(2));
        view.update_selection(3, CharIdx::new(7));
        let mut output = OutputArea::new();

        apply_output_selection_to_widget(&view, &mut output);

        // 正常路径：view_state 选区真相单向写回 widget 镜像。
        assert!(output.is_selecting);
        assert_eq!(output.selection_start, Some((1, CharIdx::new(2))));
        assert_eq!(output.selection_end, Some((3, CharIdx::new(7))));
    }

    #[test]
    fn test_apply_selection_clears_widget_when_view_empty() {
        use sdk::CharIdx;
        let view = OutputViewState::default();
        let mut output = OutputArea::new();
        // widget 先持有旧镜像，模拟上一帧选区。
        output.is_selecting = true;
        output.selection_start = Some((0, CharIdx::new(0)));
        output.selection_end = Some((0, CharIdx::new(5)));

        apply_output_selection_to_widget(&view, &mut output);

        // 边界/清空路径：view_state 无选区 → 镜像被清空（下帧同步清 widget）。
        assert!(!output.is_selecting);
        assert_eq!(output.selection_start, None);
        assert_eq!(output.selection_end, None);
    }

    #[test]
    fn test_apply_compensates_for_content_growth_when_not_auto_scroll() {
        let mut view = OutputViewState {
            scroll_offset: 5,
            auto_scroll: false,
            last_document_total_lines: 50,
            ..Default::default()
        };
        let mut output = OutputArea::new();
        output.last_visible_height = 20;
        // 内容 60 行（比上一帧多 10 行）
        output.set_plain_document_lines(60);

        apply_output_scroll_to_widget(&mut view, &mut output);

        // 正常路径：内容增长 10 行，offset 应从 5 补偿到 15（保持视窗内容固定）。
        // max_offset = 60 - 20 = 40，15 < 40 不触发钳制，auto_scroll 保持 false。
        assert_eq!(view.scroll_offset, 15);
        assert!(!view.auto_scroll);
        assert_eq!(view.last_document_total_lines, 60);
        assert_eq!(output.scroll_offset, 15);
        assert!(!output.auto_scroll);
    }

    #[test]
    fn test_apply_no_compensation_when_auto_scroll() {
        let mut view = OutputViewState {
            scroll_offset: 0,
            auto_scroll: true,
            last_document_total_lines: 50,
            ..Default::default()
        };
        let mut output = OutputArea::new();
        output.last_visible_height = 20;
        // 内容从 50 → 70（增长 20 行），但 auto_scroll=true 不补偿
        output.set_plain_document_lines(70);

        apply_output_scroll_to_widget(&mut view, &mut output);

        // auto_scroll=true：scroll_offset 保持 0（贴尾），不受增长影响。
        assert_eq!(view.scroll_offset, 0);
        assert!(view.auto_scroll);
        assert_eq!(view.last_document_total_lines, 70);
        assert_eq!(output.scroll_offset, 0);
        assert!(output.auto_scroll);
    }

    #[test]
    fn test_apply_clamps_small_offset_to_zero_when_content_shrinks() {
        // 内容收缩到不足可见高度：小 offset 被钳到 0 并恢复 auto_scroll。
        let mut view = OutputViewState {
            scroll_offset: 3,
            auto_scroll: false,
            last_document_total_lines: 50,
            ..Default::default()
        };
        let mut output = OutputArea::new();
        output.last_visible_height = 10;
        // 内容从 50 收缩到 5 行（不足一屏）
        output.set_plain_document_lines(5);

        apply_output_scroll_to_widget(&mut view, &mut output);

        // max_offset=0（内容不足可见高度），offset 被钳到 0，auto_scroll 恢复。
        assert_eq!(view.scroll_offset, 0);
        assert!(view.auto_scroll);
        assert_eq!(view.last_document_total_lines, 5);
        assert_eq!(output.scroll_offset, 0);
        assert!(output.auto_scroll);
    }
}
