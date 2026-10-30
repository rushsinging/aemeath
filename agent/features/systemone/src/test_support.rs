//! System One 测试实现面（#2146 实现权治理：ScoringPort 实现权归本
//! crate，跨 crate 测试经此获得替身；生产不编译）。

use std::sync::Arc;

use crate::domain::{
    ScoringAnswer, ScoringQuestion, ScoringState, ScoringUnavailable, UnavailableKind,
};
use crate::ports::ScoringPort;

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

// #2146 密封解锁。
impl crate::sealed::Sealed for ScriptedScoring {}
