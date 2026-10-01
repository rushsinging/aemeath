use crate::tui::render::output::rendered::RenderCtx;
use crate::tui::view_model::output::{OutputBlockKind, TextBlockView};
use crate::tui::view_model::style::SemanticStyle;

fn ctx() -> RenderCtx {
    RenderCtx::for_width(80)
}

#[test]
fn test_component_dispatch_renders_self_lines() {
    let kind = OutputBlockKind::SystemNotice(TextBlockView {
        key: "s".into(),
        text: "ok".into(),
        style: SemanticStyle::Muted,
    });
    let block = kind.component().render_self("s", &ctx());
    assert_eq!(block.block_id, "s");
    assert_eq!(block.lines[0].plain, "ok");
}

#[test]
fn test_cache_version_stable_for_same_content() {
    let a = OutputBlockKind::SystemNotice(TextBlockView {
        key: "s".into(),
        text: "ok".into(),
        style: SemanticStyle::Muted,
    });
    let b = a.clone();
    assert_eq!(a.cache_version(), b.cache_version());
}

#[test]
fn test_cache_version_differs_for_different_content() {
    let a = OutputBlockKind::SystemNotice(TextBlockView {
        key: "s".into(),
        text: "ok".into(),
        style: SemanticStyle::Muted,
    });
    let b = OutputBlockKind::SystemNotice(TextBlockView {
        key: "s".into(),
        text: "changed".into(),
        style: SemanticStyle::Muted,
    });
    assert_ne!(a.cache_version(), b.cache_version());
}
