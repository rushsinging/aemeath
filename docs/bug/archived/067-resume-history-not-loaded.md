# Bug #67：`--resume` 失效：进入 TUI 后未加载历史会话

| 字段 | 值 |
|------|-----|
| 优先级 | 高 |
| 发现日期 | 2026-05 |
| 归档日期 | 2026-05-25 |
| 状态 | 已确认修复 |

## 症状

使用 `aemeath --resume <session-id>` 启动后进入 TUI，预期应自动加载指定 session 的历史消息并显示在 output area 中；实际 TUI 起始为空白，与新建会话表现一致，历史内容未被加载/回放。

## 根因

`apps/cli/src/run_orchestration.rs` 在调用 `runtime::api::client::from_args(args.into())` 时已把 `args.resume` 移入 runtime bootstrap，runtime 因此会复用指定 session id；但随后启动 TUI 的 `app.run(..., resume_id, ...)` 参数仍硬编码为 `None`。`apps/cli/src/tui/session/session_lifecycle.rs` 中历史加载与回放逻辑只在 `resume_id` 为 `Some` 时执行，所以 CLI `--resume` 虽然影响了 session id，却没有触发 TUI 历史加载。

## 修复

在 `args` 被 move 给 runtime bootstrap 前保存 `initial_resume_id`，并传给 `app.run()`；新增回归测试 `test_initial_tui_resume_id_uses_cli_resume` 覆盖 CLI resume id 不应在 TUI 启动路径丢失。

## 验证

2026-05-25 用户确认 bug #67 已修复。活动列表中移除 #67，并保留此归档记录。
