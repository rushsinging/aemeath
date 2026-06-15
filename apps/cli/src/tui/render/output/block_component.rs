//! BlockComponent trait：统一 block 渲染模板与缓存指纹。
//!
//! - `render_self`：仅渲染自身内容（不含子块、不含 gutter/缩进），产 depth=0 行。
//!   不变式：每行 plain == spans 可见文本拼接。
//! - `cache_version`：自身语义指纹，作为 block 缓存 key 的 version 分量。
//!
//! gutter（缩进 + marker）由渲染器在组合期注入，组件永不自写（见 spec section 6.5）。

use crate::tui::render::output::blocks;
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock};
use crate::tui::view_model::output::OutputBlockKind;
use std::hash::{Hash, Hasher};

pub trait BlockComponent {
    fn render_self(&self, block_id: &str, ctx: &RenderCtx) -> RenderedBlock;
}

impl OutputBlockKind {
    /// enum → trait 分发入口。
    pub fn component(&self) -> &dyn BlockComponent {
        self
    }

    /// block 自身语义指纹（block 缓存 key 的 version 分量）。
    pub fn cache_version(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }
}

impl BlockComponent for OutputBlockKind {
    fn render_self(&self, block_id: &str, ctx: &RenderCtx) -> RenderedBlock {
        match self {
            OutputBlockKind::AssistantMessage(text) => {
                blocks::assistant_message::render_assistant_message(block_id, text, ctx)
            }
            OutputBlockKind::ToolCall(tool) => {
                blocks::tool_call::render_tool_call(block_id, tool, ctx)
            }
            OutputBlockKind::ToolResult(result) => {
                blocks::tool_result::render_tool_result(block_id, result, ctx)
            }
            OutputBlockKind::ThinkingMessage(text) => {
                blocks::thinking::render_thinking(block_id, text, ctx)
            }
            OutputBlockKind::UserMessage(text) => {
                blocks::user_message::render_user_message(block_id, text, ctx)
            }
            OutputBlockKind::AskUserBatch(ask) => {
                blocks::ask_user::render_ask_user_batch(block_id, ask, ctx)
            }
            OutputBlockKind::HookNotice(notice) => {
                blocks::diagnostic::render_hook_notice(block_id, notice, ctx)
            }
            OutputBlockKind::SystemNotice(text) | OutputBlockKind::DiagnosticNotice(text) => {
                blocks::diagnostic::render_diagnostic(block_id, text, ctx)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::render::output::rendered::RenderCtx;
    use crate::tui::view_model::output::{OutputBlockKind, TextBlockView};
    use crate::tui::view_model::style::SemanticStyle;

    fn ctx() -> RenderCtx {
        RenderCtx { width: 80 }
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
}
