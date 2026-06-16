//! 消息压缩工具
//!
//! 提供消息历史压缩以减少上下文占用。
//!
//! ## 上下文管理策略（3 层）
//!
//! 1. **工具结果截断** — 每条结果的大小限制。超长结果在加入对话历史前截断为预览。
//! 2. **微压缩 (Microcompact)** — 清除旧消息中的工具结果内容。
//! 3. **完整压缩** — 基于 LLM 的早期对话历史摘要。

// 子模块声明
pub mod autocompact;
pub mod micro;
pub mod restore;
pub mod summary;
mod token_estimation;
pub mod truncate;

// ---- 向后兼容的 re-exports ----

// Token 估算函数（原始 compact.rs 中的 re-export）
pub use token_estimation::*;

// truncate 模块
pub use truncate::{
    apply_tool_result_budget, truncate_tool_result, truncate_tool_results,
    MAX_TOOL_RESULTS_PER_MESSAGE_CHARS, MAX_TOOL_RESULT_CHARS, TRUNCATION_PREVIEW_HEAD,
    TRUNCATION_PREVIEW_TAIL,
};

// autocompact 模块
pub use autocompact::{AutoCompactState, MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES};

// micro 模块
pub use micro::microcompact;

// summary 模块
pub use summary::{
    build_compact_request, build_summary_text, compact_messages, compact_messages_with_llm,
    compact_window, messages_selected_for_precompact_memory, parse_compact_response,
    COMPACT_PROMPT,
};

// restore 模块
pub use restore::{
    assemble_compacted, assemble_compacted_with_files, build_file_restoration,
    fix_role_alternation, sanitize_tool_pairs, POST_COMPACT_MAX_FILES,
    POST_COMPACT_MAX_TOKENS_PER_FILE, POST_COMPACT_TOKEN_BUDGET,
};
