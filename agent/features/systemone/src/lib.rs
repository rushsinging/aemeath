//! SystemOne：System One 决策模型评分服务（状态 × 候选集 → 概率分布）。
//!
//! # Published Language
//!
//! | 类 | 实体 | 消费者 |
//! |---|---|---|
//! | `wire_*` 工厂 | 随 adapter 落地补充 | composition |
//! | 数据 | `ScoringQuestion` / `ScoringAnswer` / `ScoringState` / `CalibrationLevel` | 消费场景（memory / skills / policy） |
//! | 端口 | `ScoringPort` / `CalibrationPort` | 消费场景只依赖端口，NEVER 感知引擎型号 |
//! | 错误 | `ScoringUnavailable` | 消费点据此静默回退原路径 |
//!
//! 设计依据：`docs/design/02-modules/systemone/01-systemone-scoring.md`。

mod constants;
pub(crate) use constants::LOG_TARGET;

mod adapters;
mod domain;
mod ports;

pub use adapters::audited::{AuditedScoringAdapter, ScoringAuditEvent};
pub use adapters::calibrated::CalibratedScoringAdapter;
pub use adapters::calibration_store::{CalibrationArtifact, CalibrationStore};
pub use adapters::jev_http::JevHttpScoringAdapter;
pub use adapters::null::NullScoringAdapter;

pub use domain::{
    AnswerRejected, CalibrationLevel, NoulCriteria, QuestionRejected, ScoringAnswer,
    ScoringQuestion, ScoringState, ScoringUnavailable, UnavailableKind,
};
pub use ports::{CalibrationObservation, CalibrationPort, ScoringPort};
