<!-- Migrated from: docs/feature/active.md#85 -->
### #85 Read/TaskList tool call 单行摘要展示

**状态**：待确认

**目标**：简化 Read、TaskListCreate、TaskUpdate 等工具调用在 TUI 中的展示，让工具调用 header 和 result 区域更紧凑，减少视觉噪音；非失败场景只显示单行摘要，失败详情保留在 result 区域展开。

**设计方向**：

1. **Read tool call 单行展示**：
   - 成功时 header 只显示读取范围（如 `Read src/main.rs: L12-L34`）和总行数（如 `23 lines`）。
   - 失败时 header 只显示工具名与文件路径，具体错误原因在 result 区域展示。
   - 不展示完整文件内容摘要或多余前缀。

2. **TaskListCreate / TaskUpdate 单行展示**：
   - TaskListCreate 成功时 header 显示 `TaskListCreate: N tasks`，单行概括创建了多少任务。
   - TaskUpdate 成功时 header 显示 `TaskUpdate: task #N → <status>`，单行概括更新动作。
   - 失败时同样只保留工具名与简要动作，原因展开在 result 中。

3. **失败原因统一收敛到 result 区域**：
   - 当前部分工具在 header 或 gutter 中即展示错误信息，导致换行或过长；统一改为 result 中展开。
   - 保持 TUI 对 tool result 的结构化展示能力（Feature #83 基础），优先读取 JSON `message` / `status` 字段。

**验收标准**：
1. Read 工具调用 header 为一行，含文件路径与读取范围/行数。
2. TaskListCreate / TaskUpdate header 为一行，含任务数或状态变化。
3. 失败场景 header 不展开错误详情，错误信息可在 result 区域完整查看。
4. 不破坏现有 tool call 的 gutter marker、选中、复制行为。
5. 编译通过，`cargo test --workspace` 无回归。

**验证**：
- `cargo test -p cli tool_call -- --nocapture`
- `cargo test -p cli read_tool -- --nocapture`
- `cargo test -p cli task_list -- --nocapture`
- `cargo fmt --check`
- `cargo clippy -p cli --all-targets -- -D warnings`

**涉及路径**：
- `apps/cli/src/tui/render/output/blocks/tool_call.rs`
- `apps/cli/src/tui/view_assembler/output.rs`
- `apps/cli/src/tui/model/conversation/tool_call.rs`
- `agent/features/tools/src/file_read.rs`
- `agent/features/tools/src/task_list.rs`
