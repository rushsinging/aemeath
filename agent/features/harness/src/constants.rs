//! Harness 模块常量。

/// 本 crate 日志 target（路由规则见 `specs/3.15-logging.md`；
/// feature crate 前缀约定 `aemeath:agent:<crate>`）。
pub(crate) const LOG_TARGET: &str = "aemeath:agent:harness";

/// 边界错误来源标识（`share::error::DomainError` 的 domain 字段）。
pub const ERROR_DOMAIN: &str = "harness";

/// 控制帧长度前缀（u32 大端）上限：控制帧远小于此，超限视为协议破坏。
pub const CONTROL_FRAME_MAX_BYTES: usize = 64 * 1024;

/// 组件文件 symlink 解析的最大层数（防循环链与恶意深链）。
pub(crate) const MAX_COMPONENT_SYMLINK_DEPTH: usize = 32;

/// 经验条目单条 lesson 文本上限（对齐 memory content 上限族）。
pub const LESSON_MAX_CHARS: usize = 500;

/// 经验条目证据摘录上限。
pub const EXCERPT_MAX_CHARS: usize = 200;

/// 单 SKILL.md body 大小上限（64 KiB，#2018）。
pub const SKILL_MAX_BYTES: u64 = 64 * 1024;

/// 单 plugin.md 大小上限（64 KiB，#2030）。
pub const PLUGIN_MAX_BYTES: u64 = 64 * 1024;

/// 超长结果判定阈值（证据投影，对齐 materializer floor 3000）。
pub const OVERSIZED_RESULT_CHARS: usize = 3000;

/// 证据摘录上限（对齐 lesson excerpt 常量族）。
pub(crate) const EVIDENCE_EXCERPT_BOUND: usize = 200;

/// 单条聚合信号保留的可回读来源上限。
pub(crate) const EVIDENCE_SOURCE_REF_LIMIT: usize = 3;

/// 单个来源 step 回读的上下文 token 上限（含相邻 step）。
pub(crate) const SESSION_CONTEXT_SOURCE_TOKENS: usize = 1200;

/// 目标 step 的上下文 token 预算（保证目标事实不被超大邻居挤掉）。
pub(crate) const SESSION_CONTEXT_TARGET_TOKENS: usize = 800;

/// 单个相邻 step 的上下文 token 预算。
pub(crate) const SESSION_CONTEXT_NEIGHBOR_TOKENS: usize = 200;

/// Compose 相似候选 pair 上限。
pub(crate) const COMPOSE_SIMILAR_PAIR_LIMIT: usize = 8;

/// Compose 相似候选文本召回阈值（仅召回，不作为合并判定）。
pub(crate) const COMPOSE_SIMILARITY_CANDIDATE_THRESHOLD: f64 = 0.28;

/// Compose 相似候选注入的 session 上下文总 token 预算。
pub(crate) const COMPOSE_SIMILAR_CONTEXT_TOTAL_TOKENS: usize = 12_000;

/// 跨 scope（global ↔ project overlay）重复召回阈值。相似度只用于召回，
/// 最终由结构化判定区分 duplicate/specialization/conflict/distinct。
pub(crate) const CROSS_SCOPE_DUPLICATE_RECALL_THRESHOLD: f64 = 0.24;

/// 单个 project overlay 的跨 scope 重复判定候选上限。
pub(crate) const CROSS_SCOPE_DUPLICATE_CANDIDATE_LIMIT: usize = 24;

/// RSI 可落盘的内建 prose 组件 body 预算（token；低于 runtime 注入预算，
/// 为 source 标签包装保留余量；超限走 LLM 压缩，NEVER 硬截断）。
pub(crate) const RSI_BUILTIN_COMPONENT_BODY_BUDGET_TOKENS: usize = 3800;

/// RSI 可落盘的 overlay body 预算（token；低于 runtime overlay 注入预算）。
pub(crate) const RSI_OVERLAY_COMPONENT_BODY_BUDGET_TOKENS: usize = 7800;

/// 组件维护审计：rule 数量上限。
pub(crate) const COMPONENT_AUDIT_MAX_RULES: usize = 40;

/// 组件维护审计：单条 rule token 上限。
pub(crate) const COMPONENT_AUDIT_MAX_RULE_TOKENS: usize = 600;

/// 组件维护审计：单条 rule 字符上限（防把多条规则堆进一条）。
pub(crate) const COMPONENT_AUDIT_MAX_RULE_CHARS: usize = 1000;

/// 组件维护审计：body token 达到预算的百分比阈值。
pub(crate) const COMPONENT_AUDIT_BODY_BUDGET_PERCENT: usize = 70;

/// 维护 Compose 分块：每块最多规则数（超预算组件按块维护）。
pub(crate) const COMPONENT_MAINTENANCE_CHUNK_RULES: usize = 30;

/// checkpoint 落盘文件名（`~/.agents/harness/checkpoint.json`，#2049）。
pub const CHECKPOINT_FILE_NAME: &str = "checkpoint.json";

/// 已完成 session 条目的 checkpoint 保留上限（自动清理，#2057）。
pub const COMPLETE_SESSION_RETENTION: usize = 50;

/// RSI 过程事件流 GC 保留天数（#2070）。
pub const EVENT_RETENTION_DAYS: u64 = 30;

/// lesson 过期阈值：距上次判定超过该秒数且未被引用 → 退役（#2092）。
pub const LESSON_STALE_AFTER_SECS: u64 = 30 * 24 * 3600;

/// 可参与组件路由的 signal kind 集合。
pub(crate) const ROUTABLE_SIGNAL_KINDS: [crate::domain::evidence::SignalKind; 3] = [
    crate::domain::evidence::SignalKind::RepeatedFailure,
    crate::domain::evidence::SignalKind::RetryPattern,
    crate::domain::evidence::SignalKind::OversizedResult,
];
