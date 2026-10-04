use std::collections::HashMap;

use async_trait::async_trait;
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use share::reasoning::ReasoningLevel;

use super::*;

struct FakeContextPort;

fn request() -> ContextRequestData {
    ContextRequestData {
        session_id: SessionId::new("session"),
        request_id: ContextRequestId::new("request"),
        run_id: sdk::RunId::new("run"),
        step_id: sdk::RunStepId::new("step"),
        pending_messages: vec![],
        system_prompt: SystemPromptSpecData::new("system"),
        model_id: "fake/model".into(),
        effective_reasoning: ReasoningLevel::Off,
        language: Language::new("zh"),
        agent_roles: HashMap::new(),
        config_snapshot: ConfigSnapshot::new(Config::default()),
        context_size: 128_000,
        max_output_tokens: 8_192,
        last_api_total_tokens: None,
        heuristic_calibration: None,
        tool_schemas: vec![],
        tool_schema_tokens: 0,
    }
}

#[async_trait]
impl ContextPort for FakeContextPort {
    async fn build_window(
        &self,
        request: &ContextRequestData,
    ) -> Result<ContextWindowData, ContextPortError> {
        Ok(ContextWindowData {
            backing_revision: SessionRevision::new(0),
            system_blocks: vec![],
            messages: request.pending_messages.clone().into(),
            tool_schemas: request.tool_schemas.clone(),
            token_estimation: TokenBudget::default(),
            compaction_decision: self.needs_compaction(request).await?,
        })
    }

    async fn needs_compaction(
        &self,
        _request: &ContextRequestData,
    ) -> Result<CompactionDecisionData, ContextPortError> {
        Ok(CompactionDecisionData {
            needed: false,
            urgency: Urgency::None,
            decision_token_count: 0,
            threshold: 1,
            context_size: 200_000,
            effective_window: 180_000,
            reason: DecisionReason::HeuristicFallback,
        })
    }

    async fn compact(
        &self,
        _request: &CompactRequestData,
    ) -> Result<CompactOutcome, ContextPortError> {
        Ok(CompactOutcome::Skipped(CompactSkipReason::ResumeProtection))
    }

    async fn manual_compact(
        &self,
        request: &ManualCompactRequestData,
    ) -> Result<CompactOutcome, ContextPortError> {
        Ok(CompactOutcome::Committed(CompactResult {
            summary: format!("manual summary for {}", request.session_id.as_str()),
            recent_messages: vec![],
            source_revision: SessionRevision::new(2),
            quality: context::CompactSummaryQuality::LocalOnly,
        }))
    }

    async fn clear_session(&self, _session_id: &SessionId) -> Result<(), ContextPortError> {
        Ok(())
    }

    async fn append_and_persist(
        &self,
        append: &ContextAppendData,
    ) -> Result<AppendReceiptData, ContextAppendError> {
        Ok(AppendReceiptData {
            run_id: append.run_id.clone(),
            step_id: append.step_id.clone(),
            committed_revision: SessionRevision::new(1),
            fingerprint: append.fingerprint.clone(),
        })
    }
}

#[tokio::test]
async fn runtime_fake_compiles_against_context_owned_port() {
    let request = request();
    let window = FakeContextPort.build_window(&request).await.unwrap();
    assert!(window.messages.is_empty());

    let manual = FakeContextPort
        .manual_compact(&ManualCompactRequestData {
            session_id: request.session_id.clone(),
            run_id: request.run_id.clone(),
            system_prompt: request.system_prompt.clone(),
            context_size: request.context_size,
            progress: None,
            task_snapshot: None,
        })
        .await
        .unwrap();
    assert!(matches!(
        manual,
        CompactOutcome::Committed(ref result)
            if result.source_revision == SessionRevision::new(2)
    ));

    assert_eq!(
        FakeContextPort
            .clear_session(&request.session_id)
            .await
            .unwrap(),
        ()
    );
}
