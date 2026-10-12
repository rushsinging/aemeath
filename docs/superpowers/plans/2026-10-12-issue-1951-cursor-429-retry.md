# #1951 Cursor auto-balance 429/empty-turn 自动重试

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/1951

## Goal

Cursor SSE 流内 empty-turn / usage quota（文案含 HTTP 429）与真 HTTP 429
均分类为 `ProviderErrorKind::RateLimited` + `retryable=true`（可带 `retry_after`），
由 Runtime `RetryPolicy` 按既有退避环重试；耗尽后才表面失败。

## Boundaries

- Provider 仍只做单 attempt；跨调用重试归 Runtime（#905）。
- 不做 fallback / pool 故障转移。
- 非 rate-limit 的 `LlmError::Api` 保持 fatal。
- #1740 全量折叠 `LlmError` 不在本 issue 范围（仅收窄流路径分类）。

## Tasks

### Task 1 — RetryPolicy 对 RateLimited 退避（TDD）

- 读并改 `agent/features/runtime/src/application/model/invocation_tests.rs`：
  - 将 `retry_policy_rejects_rate_limits_*` 改为断言
    `RateLimited + retryable` → `RetryAfter`（尊重 `retry_after` / 指数退避 / 封顶）。
  - 补：`RateLimited + !retryable` → `Fail`；耗尽 attempt → `Fail`。
- 改 `invocation.rs` `RetryPolicy::decide`：删除 `RateLimited` 硬 Fail 分支；
  仅靠 `retryable` / attempt / `ContextTooLong` 判定。
- 跑：`cargo test -p runtime --lib retry_policy_`

### Task 2 — HTTP 429 → retryable=true（TDD）

- 改 `anthropic_tests.rs` 两处 429 断言：`assert!(error.retryable)`；
  保留 `request_count == 1`（provider 单 attempt）。
- 改 `http_attempt.rs` `into_provider_error`：`HttpFailureKind::RateLimited` 的
  `retryable` 改为 `true`。
- 跑：`cargo test -p provider --lib rate_limited`

### Task 3 — 流内 Api empty-turn/429 分类（TDD）

- 在 `stream_contract_tests.rs`（或新建小测）加：
  - message 含 `(HTTP 429)` / `empty turn` / `rate limit` / `quota` →
    `RateLimited + retryable`
  - 普通 `api_error` 无 rate 信号 → `UpstreamUnavailable + !retryable`
- 实现：抽取 `classify_stream_api_error(error_type, message)`，
  供 `provider_error_from_legacy` 使用；openai/anthropic/ollama 流仍产 `LlmError::Api`。
- 跑：`cargo test -p provider --lib stream`

### Task 4 — 文档口径

- 更新 `invocation.rs` 模块头注释与 #1343 spec §4.3：429/`RateLimited` 可重试。
- `03-loop` §5 已写 429 退避，可加变更记录指向 #1951。

### Task 5 — 验证

- `cargo test -p provider --lib`
- `cargo test -p runtime --lib retry_policy_ session_driver_retry`
- `xtask guard --fast`（若触及常量/导出）
