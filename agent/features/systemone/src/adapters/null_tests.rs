//! NullScoringAdapter 行为测试。

use super::*;
use crate::domain::ScoringState;
use crate::ports::ScoringPort;

#[tokio::test]
async fn null_adapter_returns_disabled_unavailable() {
    let adapter = NullScoringAdapter::new();
    let state = ScoringState::new("任意评分上下文。").expect("state 构造");
    let questions =
        vec![crate::domain::ScoringQuestion::noul("命题是否为真？", None).expect("question 构造")];

    let outcome = adapter.answer(&state, &questions).await;

    let error = outcome.expect_err("Null adapter 必须返回 Unavailable");
    assert_eq!(error.kind(), UnavailableKind::Disabled);
}

#[tokio::test]
async fn null_adapter_is_object_safe_for_composition_wiring() {
    let adapter: std::sync::Arc<dyn ScoringPort> = std::sync::Arc::new(NullScoringAdapter::new());
    let state = ScoringState::new("任意评分上下文。").expect("state 构造");
    let outcome = adapter.answer(&state, &[]).await;
    assert!(outcome.is_err());
}
