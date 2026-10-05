//! crate 身份常量（#1146 双轨归位）。

pub(crate) const LOG_TARGET: &str = "aemeath:agent:memory";
pub(crate) const LEGACY_FILE_EXT: &str = ".json";

pub(crate) const LEGACY_ARCHIVE_SUFFIX: &str = "_archive";

/// Fixed segments used by the predecessor flat-file layout.
pub(crate) const LEGACY_GLOBAL_STEM: &str = "_global";

pub(crate) const REFLECTION_HISTORY_CAS_ATTEMPTS: usize = 8;

pub(crate) const REFLECTION_RECORDS_MEMBER: &str = "records";

pub(crate) const REFLECTION_HISTORY_SEGMENT: &str = "reflection-history";

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

/// 重排 instructions（与 eval/system-one harness rank 场景同文案，保证基线可比）。
pub(crate) const RERANK_INSTRUCTIONS: &str =
    "Which option is the most relevant answer to the question?";

/// 重排 criteria 单条内容字符上限（kev 评分头按 prose 训练，截断防爆 token）。
pub(crate) const RERANK_CRITERION_MAX_CHARS: usize = 500;

/// 重排段大小：词法召回 top-N 参与 kev Choice 重排。
pub(crate) const RERANK_TOP_N: usize = 10;

/// 评分开启时的召回下限：词法多召回、重排后再截断到 query.limit，
/// 让词法 limit 之外的候选有机会被重排提升（两级架构召回边界）。
pub(crate) const RERANK_RECALL_LIMIT: usize = 20;
