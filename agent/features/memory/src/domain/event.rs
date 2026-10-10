//! 事件领域 schema：append-only 生产事件流的可序列化类型（仅类型 + serde，无 IO）。
//!
//! 对应设计文档「生产事件流与 append-only 历史」的事件模型：envelope
//! 记录「谁/何时、变更、上下文、坐标」四要素；`EventChange` 用判别枚举
//! 区分写/读/反思/生命周期四类变更，避免一个巨型 Optional 包。
//!
//! 本模块的类型在事件写路径接线前仅测试引用，dead_code 暂时放行。

#![cfg_attr(not(test), allow(dead_code))]

use super::{MemoryEntry, MemoryLayer};
use crate::constants::EVENT_SCHEMA_VERSION;
use serde::{Deserialize, Serialize};

/// 生产事件 envelope：一段因果链（含 CAS 重试）内 `correlation_id` 共用，
/// `schema_version` 恒由构造函数写入 [`EVENT_SCHEMA_VERSION`]。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryEvent {
    /// 必填（不设 `serde(default)`）：schema 版本缺失必须报错，禁止静默补默认值。
    pub schema_version: u32,
    pub event_id: String,
    pub ts_unix_ms: u64,
    pub op: MemoryEventOp,
    pub outcome: EventOutcome,
    /// 同因果链（含 CAS 重试）共用一条 id。
    pub correlation_id: String,
    // 谁/何时：装配点拿不到的字段保持 None，不阻断 emit。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_ordinal: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_ordinal: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    pub actor: EventActor,
    // 坐标
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<MemoryLayer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corpus: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_revision: Option<String>,
    // 变更 + 上下文（含内容）
    pub change: EventChange,
    pub context: EventContext,
    pub config_fingerprint: ConfigFingerprint,
}

impl MemoryEvent {
    /// 构造事件并写入当前 `schema_version`；坐标与「谁/何时」可选字段
    /// 默认 `None`，由调用方按装配点可得信息补全。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        event_id: impl Into<String>,
        ts_unix_ms: u64,
        op: MemoryEventOp,
        outcome: EventOutcome,
        correlation_id: impl Into<String>,
        actor: EventActor,
        change: EventChange,
        context: EventContext,
        config_fingerprint: ConfigFingerprint,
    ) -> Self {
        Self {
            schema_version: EVENT_SCHEMA_VERSION,
            event_id: event_id.into(),
            ts_unix_ms,
            op,
            outcome,
            correlation_id: correlation_id.into(),
            session_id: None,
            run_ordinal: None,
            step_ordinal: None,
            tool_call_id: None,
            actor,
            layer: None,
            corpus: None,
            commit_revision: None,
            change,
            context,
            config_fingerprint,
        }
    }
}

/// 监控操作面（19 项）：serde 名称与映射表以 `event_tests` 的常量固化。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryEventOp {
    // 读
    RetrieveForInject,
    Search,
    PerMessageRecall,
    ListStats,
    // 写
    WriteAdd,
    Update,
    Delete,
    Pin,
    MarkOutdated,
    ArchiveRestore,
    Compact,
    SupersedeSynthesis,
    // 反思
    ReflectionTriggered,
    ReflectionApplied,
    ReflectionCost,
    // 生命周期
    OpenLoad,
    CommitCas,
    AssemblyFingerprint,
    EvictionWatermark,
}

/// 事件结局；`Failed.kind` 记录失败类别（如 CAS 冲突、校验失败）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventOutcome {
    Succeeded,
    Failed { kind: String },
    Skipped,
}

/// 事件发射方：区分服务路径、反思工作流、打开器与保留期 GC。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventActor {
    Service,
    ReflectionWorkflow,
    Opener,
    RetentionGc,
}

/// 变更判别枚举：按读/写/反思/生命周期分臂，受影响条目复制全文进事件
/// （**NEVER** 仅存 id 引用——`/clear` 会清会话状态）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventChange {
    /// 写：`before`/`after` 为受影响 `MemoryEntry` 全文快照（可为空列表）。
    Write {
        before: Vec<MemoryEntry>,
        after: Vec<MemoryEntry>,
    },
    /// 读：候选集含正文，另带命中数、limit、layer 过滤等指标。
    Read {
        candidates: Vec<MemoryEntry>,
        hit_count: u32,
        limit: u32,
        layer_filter: Option<MemoryLayer>,
    },
    /// 反思：触发/落地摘要 + 受影响条目全文（覆盖区间在 `EventContext`）。
    Reflection {
        summary: String,
        affected: Vec<MemoryEntry>,
    },
    /// 生命周期：`stage` 标识阶段（open_load / commit_cas / …），`affected`
    /// 为受影响条目全文（如满容淘汰候选）。
    Lifecycle {
        stage: String,
        affected: Vec<MemoryEntry>,
    },
}

/// 决策上下文：检索 query、触发摘要、反思覆盖区间。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_summary: Option<String>,
    /// 反思覆盖区间（如 `"start_ms..end_ms"`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_range: Option<String>,
}

/// 决策时的配置指纹，复盘时对照「当时开关」。
///
/// `scoring_enabled` 是关键开关，**NEVER** 省略；阈值缺省以显式 `None`
/// 表达（不静默删字段）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConfigFingerprint {
    #[serde(default)]
    pub scoring_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reflection_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reflection_model_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub similarity_threshold: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inject_token_budget: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_retention_days: Option<u32>,
}

#[cfg(test)]
#[path = "event_tests.rs"]
mod tests;
