//! SystemOne 评分事件：可复盘的领域事件与题型快照。
//!
//! 本模块只承载纯数据与映射（落盘与装配属 adapter 职责）；serde 序列化为
//! snake_case，关联字段缺省序列化为 `null`（不 skip）。
//! 设计依据：`docs/design/02-modules/systemone/03-event-stream.md` §5.1。

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::published_language::{CalibrationLevel, ScoringAnswer, ScoringQuestion};
use crate::constants::EVENT_SCHEMA_VERSION;

/// 评分事件：全量现场的可复盘记录。
///
/// `schema_version` 取 [`ScoringEvent::SCHEMA_VERSION`]；legacy 旧审计行无此字段，
/// 读侧视为 `0`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoringEvent {
    /// 事件 schema 版本。
    pub schema_version: u32,
    /// 事件唯一标识（本 crate 无 uuid 依赖：毫秒时间戳 + 进程内单调计数器）。
    pub event_id: String,
    /// 事件时刻（unix 毫秒）。
    pub ts_unix_ms: u64,
    /// RFC3339 时间串（与现审计字段兼容）。
    pub timestamp: String,
    /// 场景标签：memory_rerank | memory_recall | skill_match | policy_triage。
    pub scenario: String,
    /// 引擎 revision。
    pub engine_revision: String,
    /// 提示指纹（state + questions 序列化的 sha256）。
    pub prompt_sha256: String,
    /// 本次评分的问题数。
    pub question_count: usize,
    /// 评分耗时（毫秒）。
    pub latency_ms: u64,
    /// 结果："ok" | "unavailable"。
    pub outcome: String,
    /// 不可用类别（`outcome` 为 unavailable 时填充，否则 `None` → null）。
    pub unavailable_kind: Option<String>,
    /// 评分上下文全文。
    pub state_text: String,
    /// 问题快照（含选项全文）。
    pub questions: Vec<ScoringQuestionSnapshot>,
    /// 答案快照（概率分布 + 校准级别）。
    pub answers: Vec<ScoringAnswerSnapshot>,
    /// 重排类场景的 before/after 选项 key 序（非重排场景 → null）。
    pub ranking: Option<ScoringRankingSnapshot>,
    // 关联字段：PR1 全 None，PR2 装配透传后填充；序列化为 null（不 skip）。
    /// 关联 id（跨工具调用链）。
    pub correlation_id: Option<String>,
    /// 会话 id。
    pub session_id: Option<String>,
    /// 会话内轮次序号。
    pub run_ordinal: Option<u32>,
    /// 轮次内步骤序号。
    pub step_ordinal: Option<u32>,
    /// 工具调用 id。
    pub tool_call_id: Option<String>,
}

impl ScoringEvent {
    /// 当前事件 schema 版本（来源：`constants::EVENT_SCHEMA_VERSION`）。
    pub const SCHEMA_VERSION: u32 = EVENT_SCHEMA_VERSION;

    /// 生成事件 id：本 crate 无 uuid 依赖（约束：绝不新增 Cargo 依赖），
    /// 用 unix 毫秒时间戳 + 进程内单调计数器组成进程内唯一标识。
    pub fn generate_event_id() -> String {
        static EVENT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or(0);
        let sequence = EVENT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        format!("evt-{timestamp_ms}-{sequence}")
    }
}

/// 题型快照：按题型保留可复盘全文。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoringQuestionSnapshot {
    /// 题型标签："noul" | "choice" | "score"。
    pub kind: String,
    /// 题面指令全文。
    pub instructions: String,
    /// noul 判据句 / choice 选项 key+描述全文 / score 等级描述（key 仅 choice 有值）。
    pub criteria: Vec<CriterionSnapshot>,
}

/// 判据条目：choice 承载（选项 key，选项描述全文）；noul / score 承载无 key 的描述文本。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CriterionSnapshot {
    /// 条目 key：仅 choice 选项有值，其余题型为 `None`。
    pub key: Option<String>,
    /// 描述全文（判据句 / 选项描述 / 等级描述）。
    pub text: String,
}

/// 答案快照：概率分布（顺序与对应问题一致）与实际使用的校准级别。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoringAnswerSnapshot {
    /// 概率分布：noul 为 `[p_true]`；choice 按选项序取值；score 为各档概率。
    pub probabilities: Vec<f64>,
    /// 实际使用的校准级别。
    pub calibration: Option<CalibrationLevel>,
}

/// 排序快照：重排 / 匹配类场景的 before / after 选项 key 序（可与 answers 互相对照）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoringRankingSnapshot {
    /// 重排前的选项 key 序。
    pub before: Vec<String>,
    /// 重排后的选项 key 序。
    pub after: Vec<String>,
}

impl ScoringQuestionSnapshot {
    /// 从评分问题映射为快照（纯数据映射，永不失败）：
    /// - noul：保留 instructions + 判据两句全文；
    /// - choice：保留 instructions + 全部选项 key 与描述全文；
    /// - score：保留 instructions + 等级描述全文（原序）。
    pub fn from_question(question: &ScoringQuestion) -> Self {
        match question {
            ScoringQuestion::Noul {
                instructions,
                criteria,
            } => Self {
                kind: "noul".to_string(),
                instructions: instructions.clone(),
                criteria: criteria
                    .as_ref()
                    .map(|noul_criteria| {
                        vec![
                            CriterionSnapshot {
                                key: None,
                                text: noul_criteria.when_true().to_string(),
                            },
                            CriterionSnapshot {
                                key: None,
                                text: noul_criteria.when_false().to_string(),
                            },
                        ]
                    })
                    .unwrap_or_default(),
            },
            ScoringQuestion::Choice {
                instructions,
                criteria,
            } => Self {
                kind: "choice".to_string(),
                instructions: instructions.clone(),
                criteria: criteria
                    .iter()
                    .map(|(option_key, description)| CriterionSnapshot {
                        key: Some(option_key.clone()),
                        text: description.clone(),
                    })
                    .collect(),
            },
            ScoringQuestion::Score {
                instructions,
                levels,
            } => Self {
                kind: "score".to_string(),
                instructions: instructions.clone(),
                criteria: levels
                    .iter()
                    .map(|level| CriterionSnapshot {
                        key: None,
                        text: level.clone(),
                    })
                    .collect(),
            },
        }
    }
}

impl ScoringAnswerSnapshot {
    /// 从评分答案映射为快照（纯数据映射，永不失败）：
    /// noul 取 `[p_true]`；choice 按选项序取概率值；score 取各档概率向量。
    pub fn from_answer(answer: &ScoringAnswer) -> Self {
        match answer {
            ScoringAnswer::Noul {
                p_true,
                calibration,
            } => Self {
                probabilities: vec![*p_true],
                calibration: Some(*calibration),
            },
            ScoringAnswer::Choice {
                probabilities,
                calibration,
                ..
            } => Self {
                probabilities: probabilities
                    .iter()
                    .map(|(_, probability)| *probability)
                    .collect(),
                calibration: Some(*calibration),
            },
            ScoringAnswer::Score {
                probabilities,
                calibration,
                ..
            } => Self {
                probabilities: probabilities.clone(),
                calibration: Some(*calibration),
            },
        }
    }
}

#[cfg(test)]
#[path = "event_tests.rs"]
mod tests;
