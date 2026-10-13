# #1962 Agent 转后台后 Watch 过程日志（B1）

## Goal
Agent 后台任务等待期，任务日志有过程摘要行增长，`BackgroundProcessWatch` 能流式跟踪。

## Decisions
- 方案 B + Q1=B1 + Q2=C1
- 过程：Started / ToolCalls / ToolResult / Terminal(outcome kind) 一行摘要 append
- 跳过 Thinking / Message / ToolOutput（防洪水）
- 终态总结仍只经 `notify_terminal` → `append_terminal` 一次
- 完成通知保持瘦身（仅 id+状态）
- 写入点：`ProgressSink::emit` 包装（后台后 forward 循环 500ms 超时杀死，不能只靠 agent_calls forward）

## Modules
1. `application/constants.rs` — `AGENT_ACTIVITY_LOG_LINE_MAX_CHARS`
2. `background_process/activity_log.rs` — format + sink 包装
3. `supervisor.rs` — `append_activity_line_for_runtime_call`
4. `agent_calls.rs` — 接线包装 sink
5. `docs/design/.../09-background-processes.md` — 改 Agent 输出形态约定

## Test plan
- format：各 kind 行长 ≤ 预算；跳过洪水 kind 返回 None
- supervisor：间接 log 追加；log_direct 不追加；终态后再 append_terminal 文件含过程+总结且总结只一次
- sink：emit 后文件增长
