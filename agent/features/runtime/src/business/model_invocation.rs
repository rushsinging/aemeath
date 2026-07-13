//! model_invocation — 调 Provider、组装流、提取 tool_calls、记录 usage。
//!
//! 对应设计：`docs/design/02-modules/runtime/02-module-boundaries.md` §2。
//!
//! 职责：
//! - 调 `ProviderPort` 发起 LLM 调用
//! - 组装流式响应
//! - 提取 tool_calls
//! - 记录 `RawUsageSnapshot` -> 构造 `UsageRecord` 经 `UsageSink.try_record`
//! - 退避重试：仅对 Retryable(超时/5xx/429/流中断) 指数退避重试
//! - Fatal(4xx) 直接失败；context 超限 -> compact
//! - 重试期 emit `ModelInvocationRetrying{attempt}`
//!
//! 状态：无（产出 `ModelInvocation` VO 交回 Run Step）
//! 消费：`ProviderPort`、`ReasoningPort`、`UsageSink`
//!
//! 实现由 #875 负责。

#![allow(dead_code)]
