# Bug #54：LLM 过度使用 TaskListCreate，简单任务也创建 task list

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 发现日期 | 2026-05 |
| 归档日期 | 2026-05-23（active 行 2026-05-30 清理） |
| 状态 | 已确认修复 |

## 症状

LLM 在处理简单任务（如查看 bug、回答问题、单命令检查）时也会创建 TaskListCreate + TaskCreate，导致不必要的 task 管理开销。

## 根因

TaskCreate / TaskListCreate 工具描述只强调多步任务必须使用 task 管理，缺少简单任务禁止创建 task list 的反向约束。模型为避免违反 task workflow，倾向于将所有请求都包装成 task list。

## 修复

工具描述改为仅复杂多步任务（≥3 个实质步骤、多依赖变更或并行 sub-agent 协调）使用 task 管理，并明确问答、查看文件/bug 状态、单命令、小范围修改直接执行。

## 验证

2026-05-23 用户确认 bug #54 已修复。2026-05-30 同步从 `docs/bug/active.md` 表格中删除遗留行（早前归档时未清理）。
