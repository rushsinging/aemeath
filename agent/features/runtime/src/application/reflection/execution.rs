use super::response::{request_reflection_response, ReflectionPhase};
use crate::ports::LlmInvokePort;
use futures::StreamExt;
use memory::api::reflection::{
    ReflectionEngine, ReflectionErrorCategory, ReflectionExecutionIdentity,
    ReflectionExecutionResult, ReflectionTokenUsage, ReflectionWorkflow, ReflectionWorkflowError,
};
use memory::api::{MemoryPort, ReflectionHistoryStore};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct CompleteReflectionResult {
    pub output: memory::api::reflection::ReflectionOutput,
    pub apply_result: Option<memory::api::reflection::ReflectionApplyResult>,
    pub error_category: Option<ReflectionErrorCategory>,
    pub record_id: Option<String>,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ReflectionExecutionError {
    #[error("reflection LLM call failed")]
    LlmCall,
    #[error("reflection LLM returned an empty response")]
    EmptyResponse,
    #[error("reflection response could not be parsed")]
    Unparseable,
    #[error("reflection response contains an invalid suggestion")]
    InvalidSuggestion,
    #[error("reflection history write failed")]
    HistoryWrite,
    #[error("reflection could not acquire corpus write lock")]
    WriteLock,
    #[error("reflection noise-removal apply failed")]
    Apply,
}

pub type ReflectionExecutionResultType<T> = Result<T, ReflectionExecutionError>;

impl ReflectionExecutionError {
    pub(crate) fn category(self) -> ReflectionErrorCategory {
        match self {
            Self::LlmCall => ReflectionErrorCategory::LlmCall,
            Self::EmptyResponse => ReflectionErrorCategory::EmptyResponse,
            Self::Unparseable => ReflectionErrorCategory::Parse,
            Self::InvalidSuggestion => ReflectionErrorCategory::InvalidSuggestion,
            Self::HistoryWrite => ReflectionErrorCategory::History,
            Self::WriteLock => ReflectionErrorCategory::Apply,
            Self::Apply => ReflectionErrorCategory::Apply,
        }
    }
}

impl From<ReflectionWorkflowError> for ReflectionExecutionError {
    fn from(error: ReflectionWorkflowError) -> Self {
        match error {
            ReflectionWorkflowError::Unparseable => Self::Unparseable,
            ReflectionWorkflowError::InvalidSuggestion => Self::InvalidSuggestion,
            ReflectionWorkflowError::HistoryWrite => Self::HistoryWrite,
        }
    }
}

pub(crate) struct ReflectionInvocation<'a> {
    pub provider: &'a dyn LlmInvokePort,
    pub model: &'a provider::ModelInfo,
    pub max_tokens: u32,
    pub requested_reasoning: share::reasoning::ReasoningLevel,
    pub system_prompt_text: &'a str,
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_reflection(
    messages: &[share::message::Message],
    lang: &str,
    auto_apply: bool,
    target_active_entries: usize,
    max_entries: usize,
    default_ttl_secs: u64,
    invocation: ReflectionInvocation<'_>,
    memory: &dyn MemoryPort,
    history: &dyn ReflectionHistoryStore,
    identity: &ReflectionExecutionIdentity,
    cancel: &tokio_util::sync::CancellationToken,
) -> ReflectionExecutionResultType<CompleteReflectionResult> {
    let started = std::time::Instant::now();
    // #1981：Reflect 全程持 corpus 写锁（含 LLM / 补问窗口）；同 task 可重入 apply。
    let write_permit = memory.acquire_write_permit().await.map_err(|error| {
        log::error!(
            target: crate::LOG_TARGET,
            "reflection_write_lock_failed id={} error={error:?}",
            identity.id
        );
        ReflectionExecutionError::WriteLock
    })?;
    // #1987 两步反思 Step 1（去噪）：不带对话摘要，输出仅 outdated_memories；
    // 无产出跳过 apply（D4）；失败整次 Failed（D2）；CapacityNotMet 降级继续
    // （本步只降不升，收敛责任由 Step 2 收口）。
    let noise_prompt = ReflectionWorkflow::build_noise_prompt(
        lang,
        memory,
        identity.timestamp,
        target_active_entries,
        max_entries,
        default_ttl_secs,
    )
    .await;
    // #1987/#2007 残留清扫：active 中已失效（outdated/被取代）且未 pinned 的
    // 条目系统侧直接归档；pinned 驻留（D4）与 TTL 过期（D1，非失效）不归档，
    // 由反思 prompt 的失效驻留段与 TTL 标注催促 LLM 裁决。
    let swept = memory
        .sweep_stale_under_permit(identity.timestamp, &write_permit)
        .await
        .unwrap_or_else(|error| {
            log::warn!(target: crate::LOG_TARGET, "reflection_sweep_failed {error:?}");
            0
        });
    if swept > 0 {
        log::info!(target: crate::LOG_TARGET, "reflection_swept_stale count={swept}");
    }

    // 取消优先：已取消时跳过 Step 1，交 Step 2 原取消路径收口。
    let noise_result = if cancel.is_cancelled() {
        Ok((
            0,
            0,
            memory::api::reflection::ReflectionApplyResult::default(),
        ))
    } else {
        call_noise_phase(
            &invocation,
            &noise_prompt,
            lang,
            identity,
            memory,
            &write_permit,
            cancel,
        )
        .await
    };
    let (noise_input_tokens, noise_output_tokens, noise_apply) = match noise_result {
        Ok(usage) => usage,
        Err(error) => {
            ReflectionWorkflow::record_failure(
                history,
                identity,
                error.category(),
                started.elapsed().as_millis() as u64,
            )
            .await?;
            return Err(error);
        }
    };
    let prompt = ReflectionWorkflow::build_prompt(
        messages,
        lang,
        memory,
        identity.timestamp,
        target_active_entries,
        max_entries,
        default_ttl_secs,
    )
    .await;
    let response = request_reflection_response(
        &invocation,
        &prompt,
        lang,
        ReflectionPhase::Synthesis,
        identity,
        cancel,
    )
    .await;
    let (mut raw_response, mut input_tokens, mut output_tokens) = match response {
        Ok(response) => response,
        Err(error) => {
            ReflectionWorkflow::record_failure(
                history,
                identity,
                error.category(),
                started.elapsed().as_millis() as u64,
            )
            .await?;
            return Err(error);
        }
    };
    input_tokens = input_tokens.saturating_add(noise_input_tokens);
    output_tokens = output_tokens.saturating_add(noise_output_tokens);
    let mut completed: ReflectionExecutionResult = ReflectionWorkflow::complete_with_prior_apply(
        history,
        memory,
        identity,
        &raw_response,
        &prompt.references,
        lang,
        auto_apply,
        ReflectionTokenUsage {
            input_tokens,
            output_tokens,
        },
        started.elapsed().as_millis() as u64,
        Some(&write_permit),
        &noise_apply,
        swept as u32,
    )
    .await?;

    // 容量未达标：同轮补问至多 1 次，目标仍为同一 target（NEVER 降比例）；仍超则保持 Failed。
    if auto_apply
        && completed.error_category == Some(ReflectionErrorCategory::CapacityNotMet)
        && !cancel.is_cancelled()
    {
        log::warn!(
            target: crate::LOG_TARGET,
            "reflection_capacity_retry id={} target={}",
            identity.id,
            target_active_entries,
        );
        let capacity_messages = build_capacity_repair_messages(
            &prompt.text,
            &raw_response,
            lang,
            target_active_entries,
        );
        match call_provider(&invocation, capacity_messages, cancel).await {
            Ok((repaired, repair_input_tokens, repair_output_tokens)) => {
                input_tokens = input_tokens.saturating_add(repair_input_tokens);
                output_tokens = output_tokens.saturating_add(repair_output_tokens);
                raw_response = repaired;
                completed = ReflectionWorkflow::complete_with_prior_apply(
                    history,
                    memory,
                    identity,
                    &raw_response,
                    &prompt.references,
                    lang,
                    auto_apply,
                    ReflectionTokenUsage {
                        input_tokens,
                        output_tokens,
                    },
                    started.elapsed().as_millis() as u64,
                    Some(&write_permit),
                    &noise_apply,
                    swept as u32,
                )
                .await?;
            }
            Err(error) => {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "reflection_capacity_retry_failed id={} error={:?}",
                    identity.id,
                    error,
                );
            }
        }
    }

    Ok(CompleteReflectionResult {
        output: completed.output,
        apply_result: completed.apply_result,
        error_category: completed.error_category,
        record_id: Some(completed.record_id),
        input_tokens,
        output_tokens,
    })
}

/// 初次请求的单条 user 消息（重试沿用同一 prompt 原样重发）。
pub(super) fn user_prompt_messages(prompt_text: &str) -> Vec<share::message::Message> {
    vec![share::message::Message::user(prompt_text)]
}

/// 容量未达标时构造一次补问，保留原请求和响应并重申同一收敛目标。
/// 容量未达标时的同轮补问：目标仍为同一 `target`（NEVER 降比例）。
fn build_capacity_repair_messages(
    prompt_text: &str,
    raw_response: &str,
    lang: &str,
    target_active_entries: usize,
) -> Vec<share::message::Message> {
    let instruction = if lang == "zh" {
        format!(
            "你上一次反思建议落盘后 project active 仍大于收敛目标 {target_active_entries}。\n\
             请只输出符合要求的 JSON：通过 outdated_memories（归档过时条目）/ supersedes / synthesizes\n\
             将 active 压到 ≤ {target_active_entries}；目标不得降低；不要机械凑数说明；不要 Markdown。"
        )
    } else {
        format!(
            "Your previous reflection suggestions would leave project active above the converge \
             target {target_active_entries}. Output only valid JSON that uses outdated_memories \
             (archives outdated entries) / supersedes / synthesizes to bring active to ≤ \
             {target_active_entries}. Do not lower the target. No Markdown."
        )
    };
    vec![
        share::message::Message::user(prompt_text),
        share::message::Message {
            role: share::message::Role::Assistant,
            content: vec![share::message::ContentBlock::Text {
                text: raw_response.to_string(),
            }],
            metadata: None,
        },
        share::message::Message::user(instruction),
    ]
}

pub(super) async fn call_provider(
    invocation: &ReflectionInvocation<'_>,
    messages: Vec<share::message::Message>,
    cancel: &tokio_util::sync::CancellationToken,
) -> ReflectionExecutionResultType<(String, u32, u32)> {
    use crate::ports::provider_port::ProviderRequestData;

    // 整段 system prompt 直收（#1861 v4）；反射提示词不参与 prompt caching
    // ——static_prefix_len 保持 0（原 Text 块语义等价）。
    let request = ProviderRequestData {
        model: invocation.model.model.clone(),
        cancellation: cancel.clone(),
        messages: messages.into(),
        system: invocation.system_prompt_text.to_string(),
        static_prefix_len: 0,
        tools: vec![],
        max_output_tokens: invocation.max_tokens,
        reasoning: invocation.requested_reasoning,
    };
    let mut stream = invocation
        .provider
        .invoke_stream(invocation.model, &request)
        .await
        .map_err(|_| ReflectionExecutionError::LlmCall)?;
    // 流式端口 → 本地聚合到终止帧（非流式语义经 Stop 帧获得）。
    let mut text = String::new();
    let mut usage = provider::TokenUsageData::default();
    while let Some(chunk) = stream.next().await {
        match chunk {
            provider::ProviderResponseChunk::Content(provider::ProviderContentData::Text(part)) => {
                text.push_str(&part);
            }
            provider::ProviderResponseChunk::Content(_) => {}
            provider::ProviderResponseChunk::Usage(reported) => {
                usage.merge_reported(reported);
            }
            provider::ProviderResponseChunk::Stop(_) => {
                let text = text.trim().to_string();
                if text.is_empty() {
                    return Err(ReflectionExecutionError::EmptyResponse);
                }
                return Ok((
                    text,
                    usage.input_tokens.unwrap_or(0),
                    usage.output_tokens.unwrap_or(0),
                ));
            }
            provider::ProviderResponseChunk::Error(_) => {
                return Err(ReflectionExecutionError::LlmCall);
            }
        }
    }
    Err(ReflectionExecutionError::LlmCall)
}

/// 去噪与归纳共用有界请求/修复策略；只应用去噪结果中的过时条目。
async fn call_noise_phase(
    invocation: &ReflectionInvocation<'_>,
    noise_prompt: &memory::api::reflection::ReflectionPrompt,
    lang: &str,
    identity: &ReflectionExecutionIdentity,
    memory: &dyn memory::api::MemoryPort,
    write_permit: &memory::api::MemoryWritePermit,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<(u32, u32, memory::api::reflection::ReflectionApplyResult), ReflectionExecutionError> {
    use memory::api::reflection::{ReflectionApplyResult, ReflectionOutput};

    let (raw, input_tokens, output_tokens) = request_reflection_response(
        invocation,
        noise_prompt,
        lang,
        ReflectionPhase::Noise,
        identity,
        cancel,
    )
    .await?;
    let parsed = match ReflectionEngine.parse_output(&raw, &noise_prompt.references) {
        Ok(parsed) => parsed,
        Err(_) => return Err(ReflectionExecutionError::Unparseable),
    };
    if parsed.output.outdated_memories.is_empty() {
        return Ok((
            input_tokens,
            output_tokens,
            ReflectionApplyResult::default(),
        ));
    }
    let noise_output = ReflectionOutput {
        deviations: Vec::new(),
        suggested_memories: Vec::new(),
        outdated_memories: parsed.output.outdated_memories,
    };
    let apply_result = match memory
        .apply_reflection_under_permit(&noise_output, write_permit)
        .await
    {
        Ok(applied) => applied,
        Err(memory::api::MemoryError::CapacityNotMet { active, target }) => {
            // 去噪只降不升：门禁未过说明存量本就超标，交 Step 2 / 补问收口。
            log::warn!(
                target: crate::LOG_TARGET,
                "reflection_noise_capacity_deferred active={active} target={target}",
            );
            ReflectionApplyResult::default()
        }
        Err(error) => {
            log::warn!(target: crate::LOG_TARGET, "reflection_noise_apply_failed {error:?}");
            return Err(ReflectionExecutionError::Apply);
        }
    };
    Ok((input_tokens, output_tokens, apply_result))
}

#[cfg(test)]
#[path = "execution_tests.rs"]
mod tests;

#[cfg(test)]
mod error_boundary_tests {
    use super::ReflectionExecutionError as ReflectionError;

    #[test]
    fn provider_failures_have_stable_display() {
        assert_eq!(
            ReflectionError::LlmCall.to_string(),
            "reflection LLM call failed"
        );
    }
}
