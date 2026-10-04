//! 评分与校准端口：消费方只依赖端口，NEVER 直接感知引擎型号。

use async_trait::async_trait;

use crate::domain::{
    CalibrationLevel, ScoringAnswer, ScoringQuestion, ScoringState, ScoringUnavailable,
};

/// 评分端口：批量评分；服务不可用返回 `ScoringUnavailable`（消费点据此回退）。
#[async_trait]
pub trait ScoringPort: Send + Sync {
    async fn answer(
        &self,
        state: &ScoringState,
        questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, ScoringUnavailable>;
}

/// 一条「评分 + 后续观测标签」记录，供离线/在线校准拟合。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CalibrationObservation {
    pub question: ScoringQuestion,
    pub probabilities: Vec<f64>,
    pub label: String,
    pub engine_revision: String,
    pub timestamp: String,
}

/// 校准端口：observe 回路落盘 + 当前生效校准 artifact。
#[async_trait]
pub trait CalibrationPort: Send + Sync {
    /// 落盘一条观测记录，供校准拟合。
    async fn observe(&self, record: CalibrationObservation);

    /// 当前生效的校准级别；无 artifact 时为 `CalibrationLevel::Raw`。
    fn current(&self) -> CalibrationLevel;
}
