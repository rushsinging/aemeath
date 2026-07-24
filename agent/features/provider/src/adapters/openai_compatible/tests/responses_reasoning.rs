//! Chat Completions ↔ Responses API reasoning effort 一致性测试。
//!
//! #1393 契约：
//! - Minimal / Low / Medium / High / Xhigh / Max 档位在 Chat 和 Responses 两种 API 上
//!   发送完全一致的 `reasoning.effort` 值。
//! - Off 档位两种 API 都省略 `reasoning` 字段。
//! - Responses 使用 driver 的共享 effort 映射（`wire_effort`），不复制特例。

use super::super::driver::{ChatApiDriver, OpenAiDriver};
use super::super::OpenAICompatibleProvider;
use crate::adapters::client::OpenAIProviderConfig;
use crate::domain::invoke::InvocationScope;
use crate::ports::ReasoningLevel;
use crate::ProviderDriverKind;

fn openai_provider() -> OpenAICompatibleProvider {
    let config = OpenAIProviderConfig::from_driver(ProviderDriverKind::OpenAI, "openai");
    OpenAICompatibleProvider::new(
        config,
        "test-key".to_string(),
        Some("https://example.com".to_string()),
        Some("test-model".to_string()),
        8192,
        false,
        None,
        60,
    )
}

fn scope_with_level(level: ReasoningLevel) -> InvocationScope {
    InvocationScope::new("test-model", 8192, level, level).expect("valid scope")
}

fn chat_reasoning_effort(
    provider: &OpenAICompatibleProvider,
    level: ReasoningLevel,
) -> Option<String> {
    let scope = scope_with_level(level);
    let mut body = super::common::base_body();
    provider.apply_reasoning_fields(&mut body, &scope);
    body.get("reasoning")
        .and_then(|r| r.get("effort"))
        .and_then(|e| e.as_str())
        .map(ToOwned::to_owned)
}

fn responses_reasoning_effort(
    provider: &OpenAICompatibleProvider,
    level: ReasoningLevel,
) -> Option<String> {
    let scope = scope_with_level(level);
    let body = provider.build_responses_request_body(&scope, &[], &[], &[], false);
    body.get("reasoning")
        .and_then(|r| r.get("effort"))
        .and_then(|e| e.as_str())
        .map(ToOwned::to_owned)
}

// ── Off ──────────────────────────────────────────────

#[test]
fn off_chat_omits_reasoning() {
    let provider = openai_provider();
    let scope = scope_with_level(ReasoningLevel::Off);
    let mut body = super::common::base_body();
    provider.apply_reasoning_fields(&mut body, &scope);
    assert!(
        body.get("reasoning").is_none(),
        "Chat Off must omit reasoning, got {:?}",
        body.get("reasoning")
    );
}

#[test]
fn off_responses_omits_reasoning() {
    let provider = openai_provider();
    let scope = scope_with_level(ReasoningLevel::Off);
    let body = provider.build_responses_request_body(&scope, &[], &[], &[], false);
    assert!(
        body.get("reasoning").is_none(),
        "Responses Off must omit reasoning, got {:?}",
        body.get("reasoning")
    );
}

// ── 档位一致性 ──────────────────────────────────────

#[test]
fn chat_and_responses_effort_match_for_minimal() {
    let provider = openai_provider();
    let chat = chat_reasoning_effort(&provider, ReasoningLevel::Minimal);
    let resp = responses_reasoning_effort(&provider, ReasoningLevel::Minimal);
    assert_eq!(chat, resp);
    assert_eq!(chat.as_deref(), Some("minimal"));
}

#[test]
fn chat_and_responses_effort_match_for_low() {
    let provider = openai_provider();
    let chat = chat_reasoning_effort(&provider, ReasoningLevel::Low);
    let resp = responses_reasoning_effort(&provider, ReasoningLevel::Low);
    assert_eq!(chat, resp);
    assert_eq!(chat.as_deref(), Some("low"));
}

#[test]
fn chat_and_responses_effort_match_for_medium() {
    let provider = openai_provider();
    let chat = chat_reasoning_effort(&provider, ReasoningLevel::Medium);
    let resp = responses_reasoning_effort(&provider, ReasoningLevel::Medium);
    assert_eq!(chat, resp);
    assert_eq!(chat.as_deref(), Some("medium"));
}

#[test]
fn chat_and_responses_effort_match_for_high() {
    let provider = openai_provider();
    let chat = chat_reasoning_effort(&provider, ReasoningLevel::High);
    let resp = responses_reasoning_effort(&provider, ReasoningLevel::High);
    assert_eq!(chat, resp);
    assert_eq!(chat.as_deref(), Some("high"));
}

#[test]
fn chat_and_responses_effort_match_for_xhigh() {
    let provider = openai_provider();
    let chat = chat_reasoning_effort(&provider, ReasoningLevel::Xhigh);
    let resp = responses_reasoning_effort(&provider, ReasoningLevel::Xhigh);
    assert_eq!(chat, resp);
    assert_eq!(chat.as_deref(), Some("xhigh"));
}

#[test]
fn chat_and_responses_effort_match_for_max() {
    let provider = openai_provider();
    let chat = chat_reasoning_effort(&provider, ReasoningLevel::Max);
    let resp = responses_reasoning_effort(&provider, ReasoningLevel::Max);
    assert_eq!(chat, resp);
    assert_eq!(chat.as_deref(), Some("max"));
}

// ── Responses 使用 driver wire_effort 映射（不复制特例） ──

#[test]
fn responses_effort_matches_driver_wire_effort_for_all_levels() {
    let provider = openai_provider();
    let driver = OpenAiDriver;

    for level in &[
        ReasoningLevel::Minimal,
        ReasoningLevel::Low,
        ReasoningLevel::Medium,
        ReasoningLevel::High,
        ReasoningLevel::Xhigh,
        ReasoningLevel::Max,
    ] {
        let resp = responses_reasoning_effort(&provider, *level);
        let expected = driver.wire_effort(driver.reasoning_capability().resolve(*level));
        assert_eq!(
            resp.as_deref(),
            Some(expected),
            "Responses effort for {level:?} must match driver.wire_effort(resolve({level:?}))"
        );
    }
}
