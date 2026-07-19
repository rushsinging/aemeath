//! context_coordination — 构建本轮 Context Window。
//!
//! 对应设计：`docs/design/02-modules/runtime/02-module-boundaries.md` §2。
//!
//! 职责：
//! - 取历史消息
//! - compact 家族（L2 snip / L3 microcompact / L4 collapse / L5 auto-compact）
//! - memory 注入
//! - prompt/guidance 装配
//! - token budget 计算
//!
//! 消费：`ContextPort`（Context Management BC）、`MemoryPort`
//!
//! 注：Session 对话历史属 Context Management，本模块只是 Runtime 侧调用协调。
//! Memory 边界：检索归 Memory（`MemoryPort.retrieve`），注入进 Context Window 归
//! Context Management——记忆本体是独立 BC，不是 Context 的一部分。
//!
//! Runtime 只在本模块协调 Context-owned OHS；冻结 request 的字段映射留在调用方，
//! 不扩展第五个 ContextPort 方法。

use std::sync::Arc;

use crate::ports::{
    AppendReceipt, CompactOutcome, CompactRequest, CompactTrigger, ContentFingerprint,
    ContextAppend, ContextAppendError, ContextPort, ContextPortError, ContextRequest,
    ContextWindow, FinalizeCause, ManualCompactRequest, SessionId, SessionRevision, StepReceipt,
};
use sdk::RunStepId;
use sha2::{Digest, Sha256};
use share::message::Message;

/// Runtime 对 Context-owned 四方法端口的单一协调 façade。
#[derive(Clone)]
pub(crate) struct ContextCoordinator {
    port: Arc<dyn ContextPort>,
}

impl ContextCoordinator {
    pub(crate) fn new(port: Arc<dyn ContextPort>) -> Self {
        Self { port }
    }

    pub(crate) async fn build_window(
        &self,
        request: &ContextRequest,
    ) -> Result<ContextWindow, ContextPortError> {
        self.port.build_window(request).await
    }

    pub(crate) async fn needs_compaction(
        &self,
        request: &ContextRequest,
    ) -> Result<bool, ContextPortError> {
        Ok(self.port.needs_compaction(request).await?.needed)
    }

    pub(crate) async fn compact(
        &self,
        request: &ContextRequest,
        source_revision: SessionRevision,
    ) -> Result<CompactOutcome, ContextPortError> {
        self.port
            .compact(&CompactRequest {
                run_id: request.run_id.clone(),
                source_revision,
                source: request.clone(),
                trigger: CompactTrigger::Automatic,
            })
            .await
    }

    pub(crate) async fn manual_compact(
        &self,
        request: &ManualCompactRequest,
    ) -> Result<CompactOutcome, ContextPortError> {
        self.port.manual_compact(request).await
    }

    pub(crate) async fn clear_session(
        &self,
        session_id: &SessionId,
    ) -> Result<(), ContextPortError> {
        self.port.clear_session(session_id).await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn append_finalized(
        &self,
        request: &ContextRequest,
        step_id: RunStepId,
        expected_revision: SessionRevision,
        finalize_cause: FinalizeCause,
        messages: Vec<Message>,
        receipts: Vec<StepReceipt>,
        api_input_tokens: Option<u64>,
    ) -> Result<AppendReceipt, ContextAppendError> {
        let fingerprint = fingerprint(
            request,
            &step_id,
            finalize_cause,
            &messages,
            &receipts,
            api_input_tokens,
        )?;
        self.port
            .append_and_persist(&ContextAppend {
                session_id: request.session_id.clone(),
                expected_revision,
                run_id: request.run_id.clone(),
                step_id,
                source_request_id: request.request_id.clone(),
                finalize_cause,
                messages,
                receipts,
                api_input_tokens,
                fingerprint,
            })
            .await
    }
}

fn fingerprint(
    request: &ContextRequest,
    step_id: &RunStepId,
    finalize_cause: FinalizeCause,
    messages: &[Message],
    receipts: &[StepReceipt],
    api_input_tokens: Option<u64>,
) -> Result<ContentFingerprint, ContextAppendError> {
    let payload = serde_json::to_vec(&(
        request.session_id.as_str(),
        request.run_id.to_string(),
        step_id.as_str(),
        format!("{finalize_cause:?}"),
        messages,
        receipts
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
        api_input_tokens,
    ))
    .map_err(|error| ContextAppendError::Storage(format!("ContextAppend 指纹编码失败：{error}")))?;
    let digest = Sha256::digest(payload);
    Ok(ContentFingerprint::new(format!("{digest:x}")))
}

#[cfg(test)]
#[path = "context_coordination_tests.rs"]
mod tests;
