use crate::ports::ProviderPort;
use futures::StreamExt;
use memory::api::reflection::{
    ReflectionErrorCategory, ReflectionExecutionIdentity, ReflectionExecutionResult,
    ReflectionTokenUsage, ReflectionWorkflow, ReflectionWorkflowError,
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
    pub provider: &'a dyn ProviderPort,
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
    invocation: ReflectionInvocation<'_>,
    memory: &dyn MemoryPort,
    history: &dyn ReflectionHistoryStore,
    identity: &ReflectionExecutionIdentity,
    cancel: &tokio_util::sync::CancellationToken,
) -> ReflectionExecutionResultType<CompleteReflectionResult> {
    let started = std::time::Instant::now();
    let prompt = ReflectionWorkflow::build_prompt(messages, lang, memory, identity.timestamp).await;
    // 空响应有界重试：同一 prompt 再调一次 provider；provider 错误（LlmCall）
    // 与取消路径 NEVER 重试，仍按现状落 Failed(EmptyResponse)。
    let response =
        match call_provider(&invocation, user_prompt_messages(&prompt.text), cancel).await {
            Ok(response) => Ok(response),
            Err(ReflectionExecutionError::EmptyResponse) if !cancel.is_cancelled() => {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "reflection_empty_response_retry id={}", identity.id,
                );
                call_provider(&invocation, user_prompt_messages(&prompt.text), cancel).await
            }
            Err(error) => Err(error),
        };
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
    // 解析修复：调用 complete 之前先做不落盘的预解析；失败时把原反思 prompt、
    // 原始响应与精确校验错误发回模型做一次有界格式修复（上限 1 次，NEVER
    // 递归）。修复调用失败或修复后仍不合法 → 回退原始响应交 complete，按现状
    // 落 Failed(Parse) 语义；取消后 NEVER 再发起修复调用。
    if let Err(validation_error) =
        ReflectionWorkflow::parse_for_repair(&raw_response, &prompt.references)
    {
        if cancel.is_cancelled() {
            log::warn!(
                target: crate::LOG_TARGET,
                "reflection_repair_skipped reason=cancelled id={}", identity.id,
            );
        } else {
            log::warn!(
                target: crate::LOG_TARGET,
                "reflection_repair_attempt id={} validation_error={}",
                identity.id,
                validation_error,
            );
            let repair_messages =
                build_repair_messages(&prompt.text, &raw_response, lang, &validation_error);
            match call_provider(&invocation, repair_messages, cancel).await {
                Ok((repaired, repair_input_tokens, repair_output_tokens)) => {
                    input_tokens += repair_input_tokens;
                    output_tokens += repair_output_tokens;
                    if ReflectionWorkflow::parse_for_repair(&repaired, &prompt.references).is_ok() {
                        raw_response = repaired;
                    } else {
                        log::warn!(
                            target: crate::LOG_TARGET,
                            "reflection_repair_still_unparseable id={}", identity.id,
                        );
                    }
                }
                Err(error) => {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "reflection_repair_failed id={} error={:?}", identity.id, error,
                    );
                }
            }
        }
    }
    let completed: ReflectionExecutionResult = ReflectionWorkflow::complete(
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
    )
    .await?;
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
fn user_prompt_messages(prompt_text: &str) -> Vec<share::message::Message> {
    vec![share::message::Message::user(prompt_text)]
}

/// 构造一次性的格式修复消息：原反思 prompt（user）→ 原始响应（assistant）→
/// 纠错指令（user），文案按 lang 中英。system prompt 由 [`call_provider`]
/// 沿用原样（`invocation.system_prompt_text`）。
fn build_repair_messages(
    prompt_text: &str,
    raw_response: &str,
    lang: &str,
    validation_error: &str,
) -> Vec<share::message::Message> {
    let instruction = if lang == "zh" {
        format!(
            "你上一次的输出不是合法 JSON 或不符合反思输出要求，请只输出符合要求的 JSON，\
             不要输出 Markdown、解释或任何其他文字。校验错误：{validation_error}"
        )
    } else {
        format!(
            "Your previous output was not valid JSON or did not match the required reflection \
             schema. Output only a single JSON object that satisfies the requirements — no \
             Markdown fences, no prose, no extra text. Validation error: {validation_error}"
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

async fn call_provider(
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
        .invoke(request, cancel)
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
