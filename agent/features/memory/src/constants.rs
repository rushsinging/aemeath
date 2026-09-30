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
