//! 共享值类型。

use serde::{Deserialize, Serialize};
use std::ops;

/// 字符位置索引。
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub struct CharIdx(usize);

impl CharIdx {
    pub const ZERO: Self = CharIdx(0);

    pub fn new(n: usize) -> Self {
        CharIdx(n)
    }

    pub fn count_in(s: &str) -> Self {
        CharIdx(s.chars().count())
    }

    pub fn advance(self, n: usize) -> Self {
        CharIdx(self.0 + n)
    }

    pub fn checked_add(self, n: usize, s: &str) -> Option<Self> {
        let total = s.chars().count();
        let result = self.0 + n;
        if result <= total {
            Some(CharIdx(result))
        } else {
            None
        }
    }

    pub fn saturating_sub(self, other: CharIdx) -> usize {
        self.0.saturating_sub(other.0)
    }

    pub fn as_usize(self) -> usize {
        self.0
    }
}

impl ops::Add<usize> for CharIdx {
    type Output = Self;

    fn add(self, rhs: usize) -> Self::Output {
        CharIdx(self.0 + rhs)
    }
}

impl ops::Sub for CharIdx {
    type Output = usize;

    fn sub(self, rhs: CharIdx) -> usize {
        self.0.saturating_sub(rhs.0)
    }
}

/// 字节偏移索引。
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub struct ByteIdx(usize);

impl ByteIdx {
    pub const ZERO: Self = ByteIdx(0);

    pub fn new(n: usize) -> Self {
        ByteIdx(n)
    }

    pub fn end_of(s: &str) -> Self {
        ByteIdx(s.len())
    }

    pub fn after_str(self, lit: &str) -> Self {
        ByteIdx(self.0 + lit.len())
    }

    pub fn new_at_boundary(s: &str, n: usize) -> Option<Self> {
        if s.is_char_boundary(n) {
            Some(ByteIdx(n))
        } else {
            None
        }
    }

    pub fn as_usize(self) -> usize {
        self.0
    }

    pub fn checked_add(self, n: usize) -> Option<Self> {
        self.0.checked_add(n).map(ByteIdx)
    }
}

pub fn char_to_byte(s: &str, c: CharIdx) -> ByteIdx {
    s.char_indices()
        .nth(c.0)
        .map(|(b, _)| ByteIdx::new(b))
        .unwrap_or_else(|| ByteIdx::end_of(s))
}

/// 使用类型化的索引对 str 进行安全切片。
pub trait StrSlice {
    fn bslice(&self, range: ops::Range<ByteIdx>) -> &str;
    fn bslice_to(&self, end: ByteIdx) -> &str;
    fn bslice_from(&self, start: ByteIdx) -> &str;
}

impl StrSlice for str {
    fn bslice(&self, range: ops::Range<ByteIdx>) -> &str {
        &self[range.start.0..range.end.0]
    }

    fn bslice_to(&self, end: ByteIdx) -> &str {
        &self[..end.0]
    }

    fn bslice_from(&self, start: ByteIdx) -> &str {
        &self[start.0..]
    }
}

/// 成本信息（Atomic 读取，纳秒级）。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct CostInfo {
    /// 输入 token 数。
    pub input_tokens: u64,
    /// 输出 token 数。
    pub output_tokens: u64,
    /// 估算费用（USD）。
    pub cost_usd: f64,
}

/// 权限确认请求。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionPrompt {
    /// 工具名称。
    pub tool_name: String,
    /// 操作描述。
    pub description: String,
    /// 风险等级。
    pub risk_level: String,
}

/// 状态信息（用于 TUI status line）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatusInfo {
    /// 状态文本。
    pub text: String,
    /// 进度百分比（0-100）。
    pub progress: Option<u8>,
}

/// 任务状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskState {
    Pending,
    InProgress,
    Completed,
    Deleted,
}

/// 任务摘要。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummary {
    /// 任务 ID。
    pub id: String,
    /// 任务标题。
    pub subject: String,
    /// 任务状态（兼容展示字符串）。
    pub status: String,
    /// 类型化任务状态。
    pub state: TaskState,
    /// 优先级。
    pub priority: String,
    /// 负责人。
    pub owner: Option<String>,
    /// 更新时间戳。
    pub updated_at: u64,
}

impl TaskSummary {
    pub fn pending(id: impl Into<String>, subject: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            subject: subject.into(),
            status: "pending".to_string(),
            state: TaskState::Pending,
            priority: "normal".to_string(),
            owner: None,
            updated_at: 0,
        }
    }
}

pub fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        let m = tokens as f64 / 1_000_000.0;
        if m >= 10.0 || m.fract() < 0.05 {
            format!("{m:.0}m")
        } else {
            format!("{m:.1}m")
        }
    } else if tokens >= 1_000 {
        let k = tokens as f64 / 1_000.0;
        if k >= 10.0 || k.fract() < 0.05 {
            format!("{k:.0}k")
        } else {
            format!("{k:.1}k")
        }
    } else {
        tokens.to_string()
    }
}
