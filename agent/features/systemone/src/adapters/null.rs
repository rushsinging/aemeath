//! NullScoringAdapter：评分开关关闭时的零成本实现。
//!
//! 不做任何 IO，固定返回 `UnavailableKind::Disabled`——消费点据此静默回退原路径，
//! 日志可区分「开关关闭」与「服务故障」。

use async_trait::async_trait;

use crate::domain::{
    ScoringAnswer, ScoringQuestion, ScoringState, ScoringUnavailable, UnavailableKind,
};
use crate::ports::ScoringPort;

#[derive(Debug, Default, Clone, Copy)]
pub struct NullScoringAdapter;

impl NullScoringAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ScoringPort for NullScoringAdapter {
    async fn answer(
        &self,
        _state: &ScoringState,
        _questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, ScoringUnavailable> {
        Err(ScoringUnavailable::new(
            UnavailableKind::Disabled,
            "评分开关未开启",
        ))
    }
}

#[cfg(test)]
#[path = "null_tests.rs"]
mod tests;
