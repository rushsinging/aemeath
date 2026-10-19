use memory::api::reflection::{ReflectionExecutionIdentity, ReflectionPrompt, ReflectionWorkflow};

use super::execution::{
    call_provider, user_prompt_messages, ReflectionExecutionError, ReflectionExecutionResultType,
    ReflectionInvocation,
};
use crate::application::constants::REFLECTION_RESPONSE_PREVIEW_CHARS;

#[derive(Debug, Clone, Copy)]
pub(super) enum ReflectionPhase {
    Noise,
    Synthesis,
}

impl ReflectionPhase {
    fn label(self) -> &'static str {
        match self {
            Self::Noise => "noise",
            Self::Synthesis => "synthesis",
        }
    }
}

/// 两阶段共用请求策略：空响应最多重试一次，解析失败最多修复一次。
/// 修复失败时保留原始响应，让各阶段按原有错误契约收口。
pub(super) async fn request_reflection_response(
    invocation: &ReflectionInvocation<'_>,
    prompt: &ReflectionPrompt,
    lang: &str,
    phase: ReflectionPhase,
    identity: &ReflectionExecutionIdentity,
    cancel: &tokio_util::sync::CancellationToken,
) -> ReflectionExecutionResultType<(String, u32, u32)> {
    let response = match call_provider(invocation, user_prompt_messages(&prompt.text), cancel).await
    {
        Err(ReflectionExecutionError::EmptyResponse) if !cancel.is_cancelled() => {
            log::debug!(
                target: crate::LOG_TARGET,
                "reflection_empty_response_retry id={} phase={}",
                identity.id,
                phase.label(),
            );
            call_provider(invocation, user_prompt_messages(&prompt.text), cancel).await
        }
        response => response,
    };
    let (mut raw_response, mut input_tokens, mut output_tokens) = response?;
    if let Err(validation_error) =
        ReflectionWorkflow::parse_for_repair(&raw_response, &prompt.references)
    {
        log_parse_failure(identity, phase, "initial", &validation_error, &raw_response);
        if !cancel.is_cancelled() {
            let messages =
                build_repair_messages(&prompt.text, &raw_response, lang, &validation_error);
            match call_provider(invocation, messages, cancel).await {
                Ok((repaired, repair_input, repair_output)) => {
                    input_tokens = input_tokens.saturating_add(repair_input);
                    output_tokens = output_tokens.saturating_add(repair_output);
                    match ReflectionWorkflow::parse_for_repair(&repaired, &prompt.references) {
                        Ok(()) => raw_response = repaired,
                        Err(error) => {
                            log_parse_failure(identity, phase, "repair", &error, &repaired);
                            log::warn!(
                                target: crate::LOG_TARGET,
                                "reflection_parse_repair_exhausted id={} phase={}",
                                identity.id,
                                phase.label(),
                            );
                        }
                    }
                }
                Err(error) => {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "reflection_parse_repair_failed id={} phase={} error={error:?}",
                        identity.id,
                        phase.label(),
                    );
                }
            }
        }
    }
    Ok((raw_response, input_tokens, output_tokens))
}

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

fn log_parse_failure(
    identity: &ReflectionExecutionIdentity,
    phase: ReflectionPhase,
    attempt: &str,
    validation_error: &str,
    raw_response: &str,
) {
    if log::log_enabled!(target: crate::LOG_TARGET, log::Level::Debug) {
        // InvalidSuggestion 的稳定校验摘要仅包含字段位置；正文从不进入错误摘要。
        // JSON/非 JSON 响应统一遮蔽全部字母和数字，只留下结构和有界空白。
        let preview = structural_preview(raw_response);
        log::debug!(
            target: crate::LOG_TARGET,
            "reflection_parse_diagnostic id={} phase={} attempt={} error={validation_error:?} response_chars={} response_preview={preview:?}",
            identity.id,
            phase.label(),
            attempt,
            raw_response.chars().count(),
        );
    }
}

fn structural_preview(source: &str) -> String {
    source
        .chars()
        .take(REFLECTION_RESPONSE_PREVIEW_CHARS)
        .map(|character| match character {
            '{' | '}' | '[' | ']' | ':' | ',' | '"' | '\\' | '`' | ' ' | '\n' | '\r' | '\t' => {
                character
            }
            _ => '*',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::structural_preview;
    use crate::application::constants::REFLECTION_RESPONSE_PREVIEW_CHARS;

    #[test]
    fn response_preview_masks_content_and_credentials_and_bounds_unicode() {
        let source = format!(
            r#"{{"outdated_memories":["私有记忆正文"],"Authorization":"Bearer sk-sensitive","token":123}}{}"#,
            "更多私有正文".repeat(100),
        );
        let preview = structural_preview(&source);
        assert_eq!(preview.chars().count(), REFLECTION_RESPONSE_PREVIEW_CHARS);
        assert!(preview.starts_with("{\""));
        for secret in ["私有", "Authorization", "Bearer", "sk-sensitive", "123"] {
            assert!(!preview.contains(secret));
        }
    }
}
