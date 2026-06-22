//! Reasoning Graph——阶段驱动的 reasoning effort 动态调节状态机。
//!
//! 设计文档：`docs/design/agent-reasoning-graph.md` §3.2
//!
//! 核心原则：Graph 是 effort 调节器，不是流程约束器——只推断阶段调 effort，
//! 不阻塞 tool、不改 agent loop 控制流、不依赖 LLM 配合。
//!
//! ## Workflow 演进路线
//!
//! 当前（Phase 1）是**被动观察模式**：根据上一个 tool 的类型和结果猜测当前
//! 阶段，调 effort 档位。根本局限：意图 ≠ tool 类型——`git diff` 可能是
//! Explore 也可能是 Verify，靠关键词猜不到 100% 准确。
//!
//! ### Phase 2: 轻量声明（计划中）
//!
//! 让 LLM 在 tool call 中带一个可选的 `phase` 字段声明当前意图。Graph 优先
//! 使用 LLM 声明的 phase，关键词分类器（`classify.rs`）降级为兜底 fallback。
//!
//! - 成本：每次 tool call 多几个 token
//! - 收益：准确率从 ~85% → 95%+，不再需要维护关键词列表
//! - 风险：LLM 可能不配合或幻觉，但有 fallback 兜底
//! - 对本模块的影响：`next_node()` 增加 phase 优先判断分支，
//!   `classify.rs` 的 `classify_bash` / `infer_node_from_tool` 变成 fallback
//!
//! ### Phase 3: 完整 Workflow（远期，独立模块）
//!
//! 如果需要更强的流程控制（可阻塞 tool、可重试、可分支恢复、可持久化恢复），
//! 新建 `business/workflow/` 模块，与本模块并行共存：
//!
//! ```text
//!   agent loop ←→ ReasoningGraph (effort 调节，保持纯观察)
//!        ↑
//!   WorkflowEngine (流程控制，独立层)
//!        ↓
//!   可阻塞/重试/分支/持久化
//! ```
//!
//! 关键区别：Workflow Engine 拥有控制权（可阻塞 tool 执行），ReasoningGraph
//! 只做观察调 effort。两者不合并——职责不同、生命周期不同、LLM 交互模型不同。
//! 详见设计文档 §6.3「Workflow 扩展空间」。

pub mod classify;
pub mod config;

pub use config::GraphRuntimeConfig;

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
    config: GraphRuntimeConfig,
}

impl ReasoningGraph {
    /// 创建 graph 实例。初始节点为 `Idle`。
    pub fn new(config: GraphRuntimeConfig) -> Self {
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
                // TODO(轻量声明 phase 2): 优先使用 LLM 通过 tool call `phase`
                // 字段声明的意图。仅当 LLM 未声明 phase 时，才回退到关键词推断。
                classify::infer_node_from_tool(tool_name, bash_command.as_deref(), self.current)
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
#[path = "reasoning_graph/reasoning_graph_tests.rs"]
mod tests;
