<!-- Migrated from: docs/feature/archived/074-task-list-scope-change-guidance.md -->
# Feature #74：Guidance — 任务执行期间用户提问时同步更新 task list

| 字段 | 值 |
|------|-----|
| 优先级 | 低 |
| 登记日期 | 2026-05 |
| 归档日期 | 2026-06-04 |
| 状态 | 已确认完成 |

## 背景

复杂任务执行期间，用户可能插入问题、澄清或变更需求。旧 guidance 只强调创建和维护 task 状态，没有明确要求在用户输入改变范围时同步更新 active task list，容易导致任务描述、依赖或任务集合与最新意图不一致。

## 实现

1. 在 universal execution discipline 中新增 `<task_list_scope_changes>` 规则
2. 明确当用户在活动 task list 期间提问、澄清或改变需求时，必须先判断是否影响计划
3. 若影响计划，必须更新 active task list 和相关 task：修改描述、增删任务、调整依赖或优先级
4. 若只是回答澄清且不改变范围，则保留当前 task list，但继续保持准确 task 状态

## 涉及路径

- `agent/features/prompt/src/business/guidance/constants.rs`
- `agent/features/prompt/tests/guidance_contract.rs`

## 验证

- `cargo test -p prompt test_prompt_guidance_mentions_task_list_updates_when_user_changes_scope`
- 用户确认完成。

## 关联提交

- `98f602d feat(prompt): guidance 要求用户变更时更新 task list (refs #74)`
- `403bb0c merge: feature/task-list-guidance-question-updates (refs #74)`
