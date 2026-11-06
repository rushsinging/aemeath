//! 审计装饰器：每次评分决策经日切事件流落盘到 `events/{yyyy-mm-dd}.jsonl`。
//!
//! 事件携带：schema 版本、事件 id、时间戳、引擎 revision、prompt sha256、
//! state/questions/answers 全量现场、校准级别、延迟与结果分类（吸收 semif 的
//! 溯源要求）。构造期一次性完成 legacy `audit.jsonl` 迁移与保留期 GC；
//! 落盘失败 NEVER 影响评分返回（fail-open，仅记运行日志）。
//!
//! 设计依据：`docs/design/02-modules/systemone/03-event-stream.md` §4–§7。

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;

use crate::adapters::event_jsonl::JsonlSegmentScoringEventStore;
use crate::domain::{
    ScoringAnswer, ScoringAnswerSnapshot, ScoringCallContext, ScoringEvent, ScoringQuestion,
    ScoringQuestionSnapshot, ScoringState, ScoringUnavailable,
};
use crate::ports::ScoringPort;

/// 审计装饰器：透传评分结果，同时落评分事件（best-effort）。
pub struct AuditedScoringAdapter {
    inner: Arc<dyn ScoringPort>,
    scenario: &'static str,
    engine_revision: String,
    store: Arc<JsonlSegmentScoringEventStore>,
    clock: Arc<dyn Fn() -> String + Send + Sync>,
    /// 可选关联上下文源（设计 §5.3）；缺省 None → 事件关联字段写 null。
    context_source: Option<Arc<dyn Fn() -> ScoringCallContext + Send + Sync>>,
}

impl AuditedScoringAdapter {
    /// `store` 为日切事件流 store（scoring_dir + retention_days 由装配期注入）；
    /// `clock` 注入时间源（生产：RFC3339 系统时钟；测试：固定值）；
    /// `scenario` 为消费场景标签（composition 装配期注入，事件逐条携带）。
    ///
    /// 构造期一次性执行 legacy `audit.jsonl` 迁移与保留期 GC，两者失败均
    /// fail-open（仅 `log::warn!`，绝不 panic、绝不阻断评分链装配）。
    pub fn new(
        inner: Arc<dyn ScoringPort>,
        engine_revision: impl Into<String>,
        store: JsonlSegmentScoringEventStore,
        clock: Arc<dyn Fn() -> String + Send + Sync>,
        scenario: &'static str,
    ) -> Self {
        let store = Arc::new(store);
        if let Err(error) = store.migrate_legacy_audit() {
            log::warn!(
                target: crate::LOG_TARGET,
                "scoring_event_migration_failed error={error}"
            );
        }
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        store.retain_segments(&today);
        Self {
            inner,
            scenario,
            engine_revision: engine_revision.into(),
            store,
            clock,
            context_source: None,
        }
    }

    /// 注入关联上下文源：每次 `answer` 调用一次以快照写入事件关联字段。
    ///
    /// 缺省不调用本方法时关联字段写 null；**NEVER** 因源缺失或返回空而阻断评分。
    pub fn with_context_source(
        mut self,
        context_source: Arc<dyn Fn() -> ScoringCallContext + Send + Sync>,
    ) -> Self {
        self.context_source = Some(context_source);
        self
    }
}

/// prompt 指纹：state + 题型结构序列化的 sha256（与实际发送内容同源）。
fn prompt_fingerprint(state: &ScoringState, questions: &[ScoringQuestion]) -> String {
    let mut prompt_bytes = Vec::with_capacity(state.as_str().len());
    prompt_bytes.extend_from_slice(state.as_str().as_bytes());
    for question in questions {
        if let Ok(serialized) = serde_json::to_vec(question) {
            prompt_bytes.extend_from_slice(&serialized);
        }
    }
    utils::sha256_hex(&prompt_bytes)
}

#[async_trait]
impl ScoringPort for AuditedScoringAdapter {
    async fn answer(
        &self,
        state: &ScoringState,
        questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, ScoringUnavailable> {
        let started = Instant::now();
        let outcome = self.inner.answer(state, questions).await;
        let latency_ms = started.elapsed().as_millis();

        // 时间戳同刻：RFC3339 时间串与其 unix 毫秒取同一瞬间。
        let timestamp = (self.clock)();
        let ts_unix_ms = chrono::DateTime::parse_from_rfc3339(&timestamp)
            .map(|instant| u64::try_from(instant.timestamp_millis()).unwrap_or(0))
            .unwrap_or(0);

        let (outcome_label, unavailable_kind, answer_snapshots) = match &outcome {
            Ok(answers) => (
                "ok",
                None,
                answers
                    .iter()
                    .map(ScoringAnswerSnapshot::from_answer)
                    .collect(),
            ),
            Err(error) => (
                "unavailable",
                Some(format!("{:?}", error.kind())),
                Vec::new(),
            ),
        };

        // 关联字段：有 context_source 则快照写入；缺省全 None（serde null）。
        let call_context = self
            .context_source
            .as_ref()
            .map(|source| source())
            .unwrap_or_default();

        // 全量现场：state/questions/answers 全文 + 可选关联字段。
        let event = ScoringEvent {
            schema_version: ScoringEvent::SCHEMA_VERSION,
            event_id: ScoringEvent::generate_event_id(),
            ts_unix_ms,
            timestamp,
            scenario: self.scenario.to_owned(),
            engine_revision: self.engine_revision.clone(),
            prompt_sha256: prompt_fingerprint(state, questions),
            question_count: questions.len(),
            latency_ms: u64::try_from(latency_ms).unwrap_or(u64::MAX),
            outcome: outcome_label.to_owned(),
            unavailable_kind,
            state_text: state.as_str().to_owned(),
            questions: questions
                .iter()
                .map(ScoringQuestionSnapshot::from_question)
                .collect(),
            answers: answer_snapshots,
            ranking: None,
            correlation_id: call_context.correlation_id,
            session_id: call_context.session_id,
            run_ordinal: call_context.run_ordinal,
            step_ordinal: call_context.step_ordinal,
            tool_call_id: call_context.tool_call_id,
        };

        let store = Arc::clone(&self.store);
        let append_outcome = tokio::task::spawn_blocking(move || store.append(&event)).await;
        match append_outcome {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "scoring_event_append_failed error={error}"
                );
            }
            Err(join_error) => {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "scoring_event_append_failed error={join_error}"
                );
            }
        }

        outcome
    }
}

#[cfg(test)]
#[path = "audited_tests.rs"]
mod tests;
