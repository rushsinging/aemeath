//! systemone 测试替身（#2146 治理后统一由 testkit 维护）。
//!
//! ScoringPort 实现权归 systemone crate，跨 crate 测试经本模块获得替身；
//! 消费方经 `[dev-dependencies] testkit` 引用，不进生产依赖图。

use std::sync::Arc;

use systemone::{
    ScoringAnswer, ScoringPort, ScoringQuestion, ScoringState, ScoringUnavailable, UnavailableKind,
};

/// 脚本化评分端口：`answer` 按脚本返回（可按 state/questions 定制答案、
/// 错误或不可用）。
pub fn scripted_scoring(
    answer: impl Fn(&ScoringState, &[ScoringQuestion]) -> Result<Vec<ScoringAnswer>, ScoringUnavailable>
        + Send
        + Sync
        + 'static,
) -> Arc<dyn ScoringPort> {
    Arc::new(ScriptedScoring {
        answer: Box::new(answer),
    })
}

/// 评分脚本：按 (state, questions) 产出答案或不可用。
pub type ScoringAnswerFn = dyn Fn(&ScoringState, &[ScoringQuestion]) -> Result<Vec<ScoringAnswer>, ScoringUnavailable>
    + Send
    + Sync;

struct ScriptedScoring {
    answer: Box<ScoringAnswerFn>,
}

#[async_trait::async_trait]
impl ScoringPort for ScriptedScoring {
    async fn answer(
        &self,
        state: &ScoringState,
        questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, ScoringUnavailable> {
        (self.answer)(state, questions)
    }
}

/// 恒不可用评分端口（降级路径验证）。
pub fn unavailable_scoring(
    reason: impl Into<String> + Send + Sync + 'static,
) -> Arc<dyn ScoringPort> {
    let reason = reason.into();
    scripted_scoring(move |_state, _questions| {
        Err(ScoringUnavailable::new(
            UnavailableKind::Connect,
            reason.clone(),
        ))
    })
}
