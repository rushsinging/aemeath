//! Bash 命令分类 + tool 类型到 node 的推断。
//!
//! 设计文档 §2.3 Bash 分类规则 + 转移信号表。
//!
//! ## [Phase 2] 本模块现为 fallback——LLM 声明 phase 时不会走到这里
//!
//! 当前方案是**被动观察**：tool 执行完后靠关键词猜测意图。根本局限在于
//! 同一个 tool 在不同意图下属于不同阶段——`git diff` 可能是 Explore
//! （理解现状）也可能是 Verify（确认编辑结果），关键词无法区分。
//!
//! 计划引入**轻量声明协议**：LLM 在 tool call 中带一个可选的 `phase`
//! 字段（`"explore"` / `"plan"` / `"execute"` / `"verify"`），Graph 优
//! 先使用 LLM 声明的 phase，`classify_bash` 降级为兜底（仅在 LLM 未
//! 声明 phase 时使用）。
//!
//! 更远期的演进方向见 `mod.rs` 模块文档中的「Workflow 演进路线」。

use super::ReasoningNode;

/// Bash 命令的分类结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BashCategory {
    /// 构建测试类（cargo test / clippy / build / pytest / tsc …）
    Verify,
    /// 只读探索类（git log / diff / ls / cat / grep …）
    Explore,
    /// 写入执行类（默认）
    Execute,
}

/// 对 Bash 命令做关键词分类。
///
/// 已知局限：复合命令、管道、自定义脚本可能误分类（约 15%），
/// 但误分类仅影响 effort 一档差异，不阻塞执行。
///
/// [Phase 2] 本函数为 fallback——LLM 声明 phase 时不会走到这里。
///
/// 的 `phase` 字段声明了意图时，直接用声明值，跳过关键词匹配。
/// 关键词分类仅在 LLM 未声明 phase 时作为 fallback。
pub fn classify_bash(command: &str) -> BashCategory {
    let cmd = command.to_lowercase();

    // 验证类：构建 / 测试 / lint
    let verify_keywords = [
        "cargo test",
        "cargo clippy",
        "cargo check",
        "cargo build",
        "npm test",
        "pytest",
        "go test",
        "tsc",
        "make test",
        "yarn test",
        "rustc",
    ];
    for kw in &verify_keywords {
        if cmd.contains(kw) {
            return BashCategory::Verify;
        }
    }

    // 探索类：只读命令
    let explore_keywords = [
        "git log",
        "git diff",
        "git show",
        "git status",
        "git branch",
        "ls ",
        "cat ",
        "head ",
        "tail ",
        "wc ",
        "find ",
        "grep ",
        "rg ",
        "fd ",
    ];
    for kw in &explore_keywords {
        if cmd.contains(kw) {
            return BashCategory::Explore;
        }
    }

    // 默认：执行类
    BashCategory::Execute
}

/// 根据上一个 tool 的类型推断当前阶段节点。
///
/// `current` 用于 Agent 等需要保持当前节点的 tool（不改变阶段）。
///
/// 设计文档 §2.3 转移信号表。
pub fn infer_node_from_tool(
    tool_name: &str,
    bash_command: Option<&str>,
    current: ReasoningNode,
) -> ReasoningNode {
    match tool_name {
        // 探索类 tool
        "Read" | "Grep" | "Glob" | "LSP" | "ToolSearch" => ReasoningNode::Explore,
        // 执行类 tool
        "Edit" | "Write" => ReasoningNode::Execute,
        // Bash 按 command 内容细分类
        "Bash" => {
            let cmd = bash_command.unwrap_or("");
            match classify_bash(cmd) {
                BashCategory::Verify => ReasoningNode::Verify,
                BashCategory::Explore => ReasoningNode::Explore,
                BashCategory::Execute => ReasoningNode::Execute,
            }
        }
        // Agent：子代理有自己的 graph 实例，父 agent 保持当前阶段
        "Agent" => current,
        // 未知 tool：保守视为探索（不干扰当前阶段太多）
        _ => ReasoningNode::Explore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === classify_bash ===

    #[test]
    fn test_classify_verify_keywords() {
        assert_eq!(classify_bash("cargo test"), BashCategory::Verify);
        assert_eq!(
            classify_bash("cargo clippy --workspace"),
            BashCategory::Verify
        );
        assert_eq!(classify_bash("cargo check"), BashCategory::Verify);
        assert_eq!(classify_bash("cargo build"), BashCategory::Verify);
        assert_eq!(classify_bash("npm test"), BashCategory::Verify);
        assert_eq!(classify_bash("pytest -v"), BashCategory::Verify);
        assert_eq!(classify_bash("go test ./..."), BashCategory::Verify);
        assert_eq!(classify_bash("tsc --noEmit"), BashCategory::Verify);
        assert_eq!(classify_bash("rustc main.rs"), BashCategory::Verify);
    }

    #[test]
    fn test_classify_explore_keywords() {
        assert_eq!(classify_bash("git log --oneline"), BashCategory::Explore);
        assert_eq!(classify_bash("git diff HEAD"), BashCategory::Explore);
        assert_eq!(classify_bash("git show abc123"), BashCategory::Explore);
        assert_eq!(classify_bash("ls -la"), BashCategory::Explore);
        assert_eq!(classify_bash("cat file.txt"), BashCategory::Explore);
        assert_eq!(classify_bash("head -20 file.rs"), BashCategory::Explore);
        assert_eq!(classify_bash("grep -r pattern ."), BashCategory::Explore);
        assert_eq!(classify_bash("rg 'fn main'"), BashCategory::Explore);
    }

    #[test]
    fn test_classify_default_execute() {
        assert_eq!(classify_bash("echo hello"), BashCategory::Execute);
        assert_eq!(classify_bash("rm tmp.txt"), BashCategory::Execute);
        assert_eq!(classify_bash("git add -A"), BashCategory::Execute);
        assert_eq!(classify_bash("gh pr create"), BashCategory::Execute);
        assert_eq!(classify_bash(""), BashCategory::Execute);
    }

    #[test]
    fn test_classify_case_insensitive() {
        assert_eq!(classify_bash("CARGO TEST"), BashCategory::Verify);
        assert_eq!(classify_bash("Git Log"), BashCategory::Explore);
    }

    // === infer_node_from_tool ===

    #[test]
    fn test_infer_read_goes_explore() {
        assert_eq!(
            infer_node_from_tool("Read", None, ReasoningNode::Execute),
            ReasoningNode::Explore
        );
    }

    #[test]
    fn test_infer_toolsearch_goes_explore() {
        assert_eq!(
            infer_node_from_tool("ToolSearch", None, ReasoningNode::Plan),
            ReasoningNode::Explore
        );
    }

    #[test]
    fn test_infer_edit_goes_execute() {
        assert_eq!(
            infer_node_from_tool("Edit", None, ReasoningNode::Explore),
            ReasoningNode::Execute
        );
    }

    #[test]
    fn test_infer_write_goes_execute() {
        assert_eq!(
            infer_node_from_tool("Write", None, ReasoningNode::Idle),
            ReasoningNode::Execute
        );
    }

    #[test]
    fn test_infer_bash_verify() {
        assert_eq!(
            infer_node_from_tool("Bash", Some("cargo test"), ReasoningNode::Execute),
            ReasoningNode::Verify
        );
    }

    #[test]
    fn test_infer_bash_explore() {
        assert_eq!(
            infer_node_from_tool("Bash", Some("git diff"), ReasoningNode::Execute),
            ReasoningNode::Explore
        );
    }

    #[test]
    fn test_infer_bash_execute() {
        assert_eq!(
            infer_node_from_tool("Bash", Some("echo hi"), ReasoningNode::Explore),
            ReasoningNode::Execute
        );
    }

    #[test]
    fn test_infer_agent_preserves_current() {
        assert_eq!(
            infer_node_from_tool("Agent", None, ReasoningNode::Plan),
            ReasoningNode::Plan
        );
        assert_eq!(
            infer_node_from_tool("Agent", None, ReasoningNode::Execute),
            ReasoningNode::Execute
        );
    }

    #[test]
    fn test_infer_unknown_tool_defaults_explore() {
        assert_eq!(
            infer_node_from_tool("SomeNewTool", None, ReasoningNode::Verify),
            ReasoningNode::Explore
        );
    }
}
