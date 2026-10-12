# #1944 AskUser 等待期间 queued message 触发 ActiveStepAlreadyExists

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/1944

## Goal
AwaitingUser + pending AskUser 时，session queued user message 必须 defer 回流，不得 UserResumed/begin_step。

## Root Cause
`handle_suspensions` 不 complete_step；`await_run_ingress` select 到 Ready 后 `engine.rs` 直接 resume+execute_step → ActiveStepAlreadyExists。

## Approach
抽出 `defer_user_batch_if_pending_interaction`，Ready/InternalContinuation 在 single-purpose defer 之后调用；为真则 continue。

## Tasks

### Task 1 — L2 失败复现测试
- File: `engine_input_tests.rs`
- AskUser → AwaitingUser；await_user_input Ready(queued)；断言不报 ActiveStep、deferred 含消息、pending 仍在；reply 后 Completed。
- 先红后绿。

### Task 2 — helper + 接线
- `phases.rs` helper；`engine.rs` Ready/InternalContinuation 调用。

### Task 3 — L4 scenario
- CLI scenario：AskUser 中提交排队消息 → 无 ApiError。

### Task 4 — L5 PTY
- `apps/cli/tests/ask_user_queued_pty.rs` + slow-test-matrix 登记。

## Verification
- runtime engine_input 定点测试
- CLI L4 scenario
- L5 ignored PTY
