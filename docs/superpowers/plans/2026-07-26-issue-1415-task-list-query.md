# Issue #1415 Task List 查询闭环实施计划

> 对应 Issue：https://github.com/rushsinging/aemeath/issues/1415

## 目标

让 Task List 的发现、按 ID 查询、批次局部统计与 LLM reminder 形成闭环：`TaskList` 默认查询当前 Batch、可按 `task_list_id` 查询历史；`TaskLists` 发现全部 Batch；所有统计由 Task BC 按 Batch 计算；Context 使用结构化 reminder 输入生成最终 system block。

## 根因与边界

- `TaskListTool` 当前按 `current_batch` 过滤任务，却使用全局 `TaskStore::stats()` 生成摘要，造成跨历史 Batch 累计。
- Task BC 已持有 Batch ID、summary、status 和历史 Task，但只发布原始 `list()` / `list_batches()`，没有稳定的 Batch-scoped read model。
- Runtime 当前把 Task reminder 提前压扁为固定字符串，Context 只能原样放置，违反“Context 独占 reminder 格式”的目标边界。
- 归档历史属于 Session 快照语义，不能通过删除历史 Task 规避统计错误。

## Target 对照

- `docs/design/02-modules/task/01-domain-model.md`：Batch 是 Task-owned 有标识实体，历史归档保留；本实现不改状态机和持久化 wire。
- `docs/design/02-modules/task/02-ports-and-published-language.md`：扩展 Task-owned 查询 PL；Tools/Runtime 只消费窄 `TaskAccess`，不自行计算 Batch 统计。
- `specs/tools.md`：新工具进入同一 Catalog/Execution 注册规格，使用 `TaskRead` capability，仅 Main 可见。
- `specs/runtime.md`：Runtime 只冻结结构化 Task reminder snapshot，不拥有最终提示文本。

## 任务 1：Task BC 发布 Batch 查询投影

**文件**：
- `agent/features/task/src/domain/query.rs`
- `agent/features/task/src/domain/task_access.rs`
- `agent/features/task/src/domain.rs`
- `agent/features/task/src/lib.rs`
- `agent/features/task/src/adapters/store.rs`
- 对应 `*_tests.rs` 与契约测试

**TDD**：
1. 先增加失败测试：多个 Batch 下 `batch_snapshot(id)` 只统计目标 Batch；Deleted 不计入；不存在 ID 返回 `None`；`list_batch_snapshots()` 按 BatchId 稳定排序。
2. 新增 Task-owned 只读 PL：`TaskBatchStats`、`TaskBatchSnapshot`。Snapshot 包含 Batch 元数据、局部统计和稳定排序的 live Tasks。
3. `TaskAccess` 增加 `batch_snapshot(BatchId)` 与 `list_batch_snapshots()`；`TaskStoreState` 在同一次锁内构造一致投影。
4. 保留全局 `stats()` 既有语义，避免隐式破坏其他消费者。

**验证**：`cargo test -p task`。

## 任务 2：扩展 TaskList wire 与新增 TaskLists

**文件**：
- `agent/features/tools/src/domain/types/task_list.rs`
- 新增 `agent/features/tools/src/domain/types/task_lists.rs`
- `agent/features/tools/src/domain/types.rs`
- `agent/features/tools/src/adapters/task_list.rs`
- 新增 `agent/features/tools/src/adapters/task_lists.rs`
- `agent/features/tools/src/adapters.rs`
- `agent/features/tools/src/adapters/registry.rs`
- `agent/shared/src/i18n/tools/task.rs`
- `packages/sdk/src/tool_input.rs`
- `packages/sdk/src/tool_result.rs` 及薄 re-export 模块
- 对应测试

**TDD**：
1. 先锁定 schema：`TaskListInput.task_list_id` 可选字符串，`status` 可选；旧空输入仍合法；`TaskList` 不发布 priority 过滤；`TaskListsInput.status` 可选。
2. 先锁定 wire：`TaskListResult` 含 list 元数据、batch-local stats、tasks；`TaskListsResult` 含稳定排序列表。
3. `TaskList`：未传 ID 时查询 current；无 current 返回稳定空/提示结果；传 ID 时使用十进制 Batch ID，解析失败或不存在返回 tool error；status 只过滤结构化 `tasks`。text summary 始终包含 Batch ID/summary、完整 batch-local stats，以及过滤后的 Batch 内任务状态/标题/依赖列表。
4. `TaskLists`：可按 active/paused/archived 过滤，非法状态报错；注册为 Main-only、`TaskRead`、只读并发安全。
5. 更新中英文 description，明确先发现后按 ID 查询历史。

**验证**：`cargo test -p tools`、`cargo test -p sdk`。

## 任务 3：结构化 Task reminder

**文件**：
- `agent/features/context/src/domain.rs`
- `agent/features/context/src/application/service.rs`
- `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`
- 对应 Context/Runtime 测试

**TDD**：
1. Context 契约测试先表达：结构化 reminder 含 list ID、summary、pending/in_progress 时，生成非缓存 `task_reminder` system block；全完成或无 current 时不生成。
2. Runtime 相邻测试先表达：freeze 只投影 Task-owned current Batch snapshot，不拼最终文案。
3. 扩展 Context `TaskReminderSnapshot` 为结构化字段；最终中文/英文文本由 Context application service 根据 `request.language` 生成。
4. 文案包含 ID、summary、pending/in_progress，并保留“与最新请求相关才继续，否则优先最新请求”的约束。
5. Subagent 保持默认空 reminder，不继承 Main Task 状态。

**验证**：`cargo test -p context`、`cargo test -p runtime`。

## 任务 4：SDK/TUI 显示与跨层契约

**文件**：
- `apps/cli/src/tui/view_model/tool_name.rs`
- `apps/cli/src/tui/render/output/tool_display/task_impls.rs`
- 对应 TUI display/assembler 测试

**TDD**：
1. 新增 `TaskLists` display lookup 和 display-name 测试。
2. `TaskList` header 在显式 ID 时可显示目标列表，结果预览继续走 Plain、有界行数。
3. 新增 TaskLists 稳定只读结果展示，不泄漏 raw input 或产生诊断噪声。
4. 用跨层已有 harness 验证工具名、结构化结果和 TUI 投影字段不丢失。

**验证**：`cargo test -p cli`。

## 任务 5：文档、门禁与 PR

**文件/命令**：
- 更新 `docs/design/02-modules/task/01-domain-model.md`
- 更新 `docs/design/02-modules/task/02-ports-and-published-language.md`
- 更新 Issue checklist/status
- `cargo fmt --check`
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- 运行仓库架构守卫

**完成条件**：
1. 搜索确认没有 Tools/Runtime 重复计算 Batch stats 的生产路径。
2. Session TaskSnapshot wire 未变化，历史数据兼容测试通过。
3. Issue 全部 check 完成或在 PR 中记录可验证 N/A。
4. `git pull origin main` 后无冲突，提交、推送并创建 `Closes #1415` PR；不自行合并或关闭 Issue。

## 风险与兼容策略

- `TaskListResult` 增加字段属于 additive wire change；保留 `tasks` 字段，SDK re-export 指向同一权威类型。
- `TaskListInput` 新字段可选，旧调用 `{}` 行为保持默认 current。
- Batch ID 对 Tool wire 使用十进制字符串；不复用当前 build.rs 对 `BatchId` 的 integer 映射。
- stats 描述目标 Batch 全量 live Task；status 过滤影响结构化 `tasks` 和 text 中的任务明细，但不改变列表总体统计语义。
- 不修改 Session schema、不清理 Archived Task、不引入独立 Storage 路径。
