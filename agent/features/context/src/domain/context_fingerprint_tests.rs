//! ContentFingerprint 逐字节兼容性（#1863 内化）：context 实现必须与
//! 迁移前 runtime 侧算法完全一致，否则旧 envelope 指纹失配、幂等失效。

use super::*;
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;

fn request_fixture() -> ContextRequestData {
    ContextRequestData {
        session_id: SessionId::new("ses_compat"),
        request_id: InteractionRequestId::new("irq_compat"),
        run_id: RunId::new("run_compat"),
        step_id: RunStepId::new("stp_compat"),
        pending_messages: vec![ContextMessage::user("pending")],
        workspace_root: std::path::PathBuf::from("/tmp/workspace"),
        workspace_is_git: false,
        model_id: "test/model".to_string(),
        effective_reasoning: ReasoningLevel::Off,
        language: Language::Zh,
        agent_roles: std::collections::HashMap::new(),
        config_snapshot: ConfigSnapshot::new(Config::default()),
        context_size: 8_000,
        max_output_tokens: 100,
        last_api_total_tokens: None,
        heuristic_calibration: None,
        tool_schemas: vec![],
        tool_schema_tokens: 0,
    }
}

/// 迁移前 runtime 侧算法的逐行副本（不得修改；作为兼容基准向量）。
fn legacy_runtime_fingerprint(append: &ContextAppendData) -> String {
    let payload = serde_json::to_vec(&(
        append.session_id.as_str(),
        append.run_id.to_string(),
        append.step_id.as_str(),
        format!("{:?}", append.finalize_cause),
        append.duration_ms,
        &append.messages,
        append
            .receipts
            .iter()
            .map(|receipt| {
                (
                    receipt.call_id(),
                    receipt.index(),
                    format!("{:?}", receipt.outcome()),
                    receipt.is_agent(),
                    receipt.summary(),
                    receipt.artifact_refs(),
                    receipt.possible_side_effects(),
                    receipt.unfinished_call_ids(),
                )
            })
            .collect::<Vec<_>>(),
        append.api_input_tokens,
    ))
    .unwrap();
    format!("{:x}", Sha256::digest(payload))
}

#[test]
fn append_fingerprint_matches_legacy_runtime_algorithm_byte_for_byte() {
    let request = request_fixture();
    let append = ContextAppendData {
        session_id: request.session_id.clone(),
        expected_revision: SessionRevision::new(3),
        run_id: request.run_id.clone(),
        step_id: request.step_id.clone(),
        source_request_id: request.request_id.clone(),
        finalize_cause: FinalizeCause::Completed,
        duration_ms: Some(1200),
        messages: vec![ContextMessage::placeholder(share::message::Role::Assistant)],
        receipts: vec![],
        api_input_tokens: Some(88),
    };
    assert_eq!(
        append.fingerprint().unwrap().as_str(),
        legacy_runtime_fingerprint(&append)
    );
}

#[test]
fn accepted_input_fingerprint_matches_legacy_runtime_algorithm_byte_for_byte() {
    let request = request_fixture();
    let accepted = AcceptedInputAppendData {
        session_id: request.session_id.clone(),
        run_id: request.run_id.clone(),
        step_id: request.step_id.clone(),
        source_request_id: request.request_id.clone(),
        messages: vec![ContextMessage::user("compat check")],
    };
    // 迁移前 runtime 版：serde((session, run, step, messages))
    let payload = serde_json::to_vec(&(
        accepted.session_id.as_str(),
        accepted.run_id.to_string(),
        accepted.step_id.as_str(),
        &accepted.messages,
    ))
    .unwrap();
    let legacy = format!("{:x}", Sha256::digest(payload));
    assert_eq!(accepted.fingerprint().unwrap().as_str(), legacy);
}
