//! crate 身份常量（#1146 双轨归位）。

pub(crate) const LOG_TARGET: &str = "aemeath:agent:memory";
pub(crate) const LEGACY_FILE_EXT: &str = ".json";

pub(crate) const LEGACY_ARCHIVE_SUFFIX: &str = "_archive";

/// Fixed segments used by the predecessor flat-file layout.
pub(crate) const LEGACY_GLOBAL_STEM: &str = "_global";

pub(crate) const REFLECTION_HISTORY_CAS_ATTEMPTS: usize = 8;

pub(crate) const REFLECTION_RECORDS_MEMBER: &str = "records";

pub(crate) const REFLECTION_HISTORY_SEGMENT: &str = "reflection-history";

/// 悬挂 running 记录收口（reap）扫描的历史窗口上限：反思记录按最新在前排序，
/// 超龄未收口的 Running 只会出现在最近的有限窗口内。
pub(crate) const REFLECTION_REAP_SCAN_LIMIT: usize = 200;

/// Fixed, project-independent segment for the shared global layer generation.
/// Fixed, project-independent segment for the shared global layer generation.
pub(crate) const GLOBAL_DATASET_SEGMENT: &str = "global";

/// Members are canonicalized into name order by Storage; "active" sorts before
/// "archive".
/// Members are canonicalized into name order by Storage; "active" sorts before
/// "archive".
pub(crate) const MEMORY_MEMBER_NAMES: [&str; 2] = [ACTIVE_MEMBER, ARCHIVE_MEMBER];

pub(crate) const ARCHIVE_MEMBER: &str = "archive";

pub(crate) const ACTIVE_MEMBER: &str = "active";

pub(crate) const SCHEMA_VERSION: u32 = 1;

/// 生产事件流 envelope 的 schema 版本（`MemoryEvent::schema_version` 写入值）。
pub(crate) const EVENT_SCHEMA_VERSION: u32 = 1;

/// 事件流日切 segment 目录名（`memory/{project_key}/events/` 下的固定段）。
pub(crate) const EVENTS_SEGMENT: &str = "events";

/// 事件流日切 segment 文件后缀（`{yyyy-mm-dd}.jsonl`）。
pub(crate) const EVENTS_JSONL_SUFFIX: &str = ".jsonl";

/// 事件 segment 默认保留天数（`0` 表示禁用 GC，NEVER 表示关闭事件写入）。
/// retention 配置接线（composition 落点）前仅测试引用，dead_code 暂时放行。
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const DEFAULT_EVENT_RETENTION_DAYS: u32 = 30;

/// 重排 instructions（与 eval/system-one harness rank 场景同文案，保证基线可比；
/// Qwen3-Reranker instruct 定向口径——该引擎对任务定向 instruct 敏感，实测 R@1 +5pp）。
pub(crate) const RERANK_INSTRUCTIONS: &str =
    "Given a user message from a coding-agent session, retrieve the most relevant memory.";

/// 重排 criteria 单条内容字符上限（kev 评分头按 prose 训练，截断防爆 token）。
pub(crate) const RERANK_CRITERION_MAX_CHARS: usize = 500;

/// 重排段大小：词法召回 top-N 参与 kev Choice 重排。
pub(crate) const RERANK_TOP_N: usize = 10;

/// 评分开启时的召回下限：词法多召回、重排后再截断到 query.limit，
/// 让词法 limit 之外的候选有机会被重排提升（两级架构召回边界）。
pub(crate) const RERANK_RECALL_LIMIT: usize = 20;

/// 单候选召回的 Noul 门 instructions（唯一候选无法构成 Choice，转 Noul 判定）。
pub(crate) const RECALL_SINGLE_NOUL_INSTRUCTIONS: &str = "Is the candidate memory entry      relevant to the user's current message and worth recalling into context?";
