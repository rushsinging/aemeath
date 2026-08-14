/// AskUserQuestion 批量交互中的单个问题槽位。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AskUserSlot {
    /// 对应的 tool_call_id。
    pub id: String,
    /// 同一 tool call 内的问题序号，从 0 开始。
    pub question_seq: usize,
    pub question: String,
    /// 全部选项（LLM 选项 + 内建选项）。
    pub options: Vec<sdk::OptionItem>,
    /// LLM 选项数量（内建选项从该索引开始）。
    pub llm_option_count: usize,
    pub multi_select: bool,
    pub default: Option<String>,
    /// 用户回答。None=未答，Some=已答。
    pub answer: Option<String>,
}

/// AskUser 批量交互的完成状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum AskUserCompletion {
    Active,
    ReplyPending,
    CancelPending,
    Answered,
    Cancelled,
}

impl AskUserCompletion {
    pub fn is_interactive(self) -> bool {
        matches!(self, Self::Active)
    }
}

/// AskUser 批量交互的阶段。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum AskUserPhase {
    /// 逐个回答中。
    Answering,
    /// 全部答完，等待确认。
    Confirming,
}
