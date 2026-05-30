# Bug #64：Agent 未绑定 taskId 仍启动导致 TaskList 无 doing 状态

| 字段 | 值 |
|------|-----|
| 优先级 | 高 |
| 发现日期 | 2026-05 |
| 归档日期 | 2026-05-30 |
| 状态 | 已确认修复 |
| 根因类别 | Agent/Task 集成 |

## 症状

Session `019e4ea6-6f8a-7049-a812-0ab60653770e` 中，主 LLM 创建 task list 并填充多个任务。Task 1 完成后，系统提示 Task 2 已解除阻塞；随后主 LLM 启动 Task 2 subagent，subagent 实际运行并修改代码，但 task list 中 Task 2 仍保持 `pending`，界面上只看到 `done` 与 `pending`，没有 `doing / in_progress`。

## 日志证据

1. `TaskUpdate taskId=1 status=completed` 返回 `Unblocked tasks now ready: → #2`。
2. 后续 `Agent(description="Implement Task 2", ...)` 调用缺少结构化 `taskId` 字段。
3. subagent 返回 `DONE_WITH_CONCERNS` 并产生文件修改，证明任务实际执行。
4. 因 AgentTool 未绑定 task，TaskStore 未自动执行 `Pending → InProgress → Completed/Pending` 生命周期。

## 根因

AgentTool 只在输入包含 `taskId` 时桥接 task 生命周期；当 active task batch 存在未完成任务时，未传 `taskId` 的 Agent 仍会启动。工具描述要求模型传 `taskId`，但没有工具层强制约束，LLM 遗漏参数时系统不会阻止，导致 subagent 执行和 task 状态投影脱钩。

## 修复

- 当 TaskStore 存在 active list 且有 pending/in_progress 任务时，AgentTool 缺少 `taskId` 直接返回错误，提示传入 `taskId` 或先完成/关闭 task list。
- 保留无 active task list 时的自由 Agent 调用能力，避免破坏普通并行调研/review 场景。
- 保留已有 `taskId` 生命周期管理：绑定 task 成功时标记 InProgress，成功后 Completed，失败后 Pending。
- 新增回归覆盖 active task list + missing taskId 拒绝启动，且验证 subagent runner 未被调用。

## 相关提交

- `5046f69` fix: 要求 Agent 绑定活动任务 (refs #64)

## 验证

2026-05-30 用户确认 bug #64 已修复。
