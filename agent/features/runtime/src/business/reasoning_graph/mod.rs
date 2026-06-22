//! Reasoning Graph——阶段驱动的 reasoning effort 动态调节状态机。
//!
//! 设计文档：`docs/design/agent-reasoning-graph.md` §3.2
//!
//! 核心原则：Graph 是 effort 调节器，不是流程约束器——只推断阶段调 effort，
//! 不阻塞 tool、不改 agent loop 控制流、不依赖 LLM 配合。

pub mod config;

pub use config::ReasoningGraphConfig;

use provider::api::ReasoningLevel;

/// 推理阶段节点。
///
/// 对应设计文档 §2.2 五节点定义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReasoningNode {
    /// 空闲，等待用户输入（不调 LLM）
    Idle,
    /// 探索：收集信息，理解现状
    Explore,
    /// 规划：深度推理，定方案，处理异常
    Plan,
    /// 执行：机械执行已确定的改动
    Execute,
    /// 验证：检查执行结果
    Verify,
}

impl ReasoningNode {
    /// 节点的默认 effort 值（设计文档 §3.2 `default_effort()`）。
    pub fn default_effort(&self) -> ReasoningLevel {
        match self {
            Self::Idle => ReasoningLevel::Off,
            Self::Explore => ReasoningLevel::Medium,
            Self::Plan => ReasoningLevel::High,
            Self::Execute => ReasoningLevel::Low,
            Self::Verify => ReasoningLevel::Medium,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Explore => "explore",
            Self::Plan => "plan",
            Self::Execute => "execute",
            Self::Verify => "verify",
        }
    }
}

impl std::fmt::Display for ReasoningNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 转移信号——runtime 观察到的事件，驱动节点转换。
///
/// 对应设计文档 §2.3 转移信号表。
#[derive(Debug, Clone)]
pub enum GraphSignal {
    /// 新 user message 到达。`text` 用于判断初始节点（EXPLORE vs PLAN），
    /// `turn_count` 用于识别首个 turn。
    UserMessage { text: String, turn_count: usize },
    /// tool 执行完成。`tool_name` 用于判断 tool→node 映射，
    /// `bash_command` 仅在 tool=Bash 时提供，用于细分类。
    ToolCompleted {
        tool_name: String,
        bash_command: Option<String>,
        is_error: bool,
    },
    /// LLM 回复无 tool call（纯文本回复）。
    TextOnly,
    /// agent loop 新轮次（保持上一轮节点）。
    TurnBoundary,
}

/// Reasoning Graph 状态机。
pub struct ReasoningGraph {
    current: ReasoningNode,
    config: ReasoningGraphConfig,
}

impl ReasoningGraph {
    /// 创建 graph 实例。初始节点为 `Idle`。
    pub fn new(config: ReasoningGraphConfig) -> Self {
        Self {
            current: ReasoningNode::Idle,
            config,
        }
    }

    /// graph 是否启用。
    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    /// 当前节点。
    pub fn current_node(&self) -> ReasoningNode {
        self.current
    }

    /// 当前节点对应的 effort（优先用 config 覆盖，否则用节点默认值）。
    pub fn current_effort(&self) -> ReasoningLevel {
        self.config.effort_for(self.current)
    }

    /// 用户配置的最大 reasoning 深度。
    pub fn user_max_level(&self) -> ReasoningLevel {
        self.config.max_reasoning
    }

    /// 消费信号，更新当前节点。返回是否发生变化。
    pub fn transition(&mut self, signal: GraphSignal) -> bool {
        let old = self.current;
        let new = self.next_node(&signal);

        if new != old {
            log::info!(
                target: crate::LOG_TARGET,
                "reasoning_graph transition: {} → {} (effort: {:?}, signal: {})",
                old,
                new,
                self.config.effort_for(new),
                signal_name(&signal)
            );
            self.current = new;
            true
        } else {
            false
        }
    }

    /// 根据当前节点和信号计算下一节点（纯函数，不修改状态）。
    fn next_node(&self, signal: &GraphSignal) -> ReasoningNode {
        match signal {
            GraphSignal::UserMessage { text, turn_count } => {
                initial_node_for_message(text, *turn_count)
            }
            GraphSignal::ToolCompleted {
                tool_name,
                bash_command,
                is_error,
            } => {
                // tool_error 优先：任何节点都可能触发 PLAN
                if *is_error {
                    return ReasoningNode::Plan;
                }
                infer_node_from_tool(tool_name, bash_command.as_deref())
            }
            GraphSignal::TextOnly => ReasoningNode::Idle,
            GraphSignal::TurnBoundary => self.current, // 保持
        }
    }
}

/// 根据用户消息文本和 turn 计数推断初始节点。
///
/// 设计文档 §2.3「Turn 开始时的初始节点」：
/// - 首个 turn → EXPLORE
/// - 复杂意图关键词 → PLAN
/// - 简单指令 → EXPLORE
fn initial_node_for_message(text: &str, turn_count: usize) -> ReasoningNode {
    // 首个 turn 默认从 EXPLORE 开始
    if turn_count <= 1 {
        // 但如果首条消息就含复杂意图关键词，直接 PLAN
        if has_complex_intent(text) {
            return ReasoningNode::Plan;
        }
        return ReasoningNode::Explore;
    }

    // 后续 turn：用户追加信息时，按意图分类
    if has_complex_intent(text) {
        ReasoningNode::Plan
    } else {
        ReasoningNode::Explore
    }
}

/// 检测用户消息是否含复杂意图关键词（设计文档 §2.3）。
fn has_complex_intent(text: &str) -> bool {
    // 注意：中英文双语关键词
    let keywords = [
        "设计",
        "重构",
        "架构",
        "排查",
        "为什么",
        "分析",
        "调研",
        "评估",
        "方案",
        "design",
        "refactor",
        "architect",
        "investigate",
        "why",
        "analyze",
        "debug root cause",
    ];
    let lower = text.to_lowercase();
    keywords.iter().any(|kw| lower.contains(kw))
}

/// tool → node 推断。
///
/// 设计文档 §2.3 转移信号表 + Bash 分类规则。
fn infer_node_from_tool(tool_name: &str, bash_command: Option<&str>) -> ReasoningNode {
    match tool_name {
        "Read" | "Grep" | "Glob" | "LSP" => ReasoningNode::Explore,
        "Edit" | "Write" => ReasoningNode::Execute,
        "Bash" => {
            let cmd = bash_command.unwrap_or("");
            match classify_bash(cmd) {
                BashCategory::Verify => ReasoningNode::Verify,
                BashCategory::Explore => ReasoningNode::Explore,
                BashCategory::Execute => ReasoningNode::Execute,
            }
        }
        // Agent / Task / 其他 tool 不改变阶段（保持上一轮节点）
        _ => ReasoningNode::Explore, // 默认保守：视为探索
    }
}

/// Bash 命令分类（设计文档 §2.3 Bash 分类规则）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BashCategory {
    Verify,
    Explore,
    Execute,
}

fn classify_bash(command: &str) -> BashCategory {
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

/// 信号的简短名称（用于日志）。
fn signal_name(signal: &GraphSignal) -> &'static str {
    match signal {
        GraphSignal::UserMessage { .. } => "UserMessage",
        GraphSignal::ToolCompleted { .. } => "ToolCompleted",
        GraphSignal::TextOnly => "TextOnly",
        GraphSignal::TurnBoundary => "TurnBoundary",
    }
}

#[cfg(test)]
#[path = "reasoning_graph_tests.rs"]
mod tests;
