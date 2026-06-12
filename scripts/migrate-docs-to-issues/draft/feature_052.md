<!-- Migrated from: docs/feature/active.md#52 -->
### #52 Tool 描述英文化：所有 tool 给 LLM 的 description 统一为英文

**状态**：未开始

**背景**：当前所有内置 tool 中，大部分 `description()` 已是英文，仅 `EnterWorktree` 和 `ExitWorktree` 两个 tool 的 description 和 input_schema 参数描述仍为中文。LLM 对英文描述的语义理解更精确，统一为英文有助于减少工具调用错误。

**目标**：将所有内置 tool 给 LLM 的 description 和 input_schema 参数描述统一为英文。

**范围**：
1. 内置 tool：`EnterWorktree`、`ExitWorktree` 的 description 和 input_schema 参数描述改为英文。
2. 审查所有内置 tool 的 `input_schema` 参数 `description` 字段，确认无中文残留。
3. MCP tool 的 description 由 MCP server 返回，不在本 feature 范围内（透传不改动）。

**涉及文件**：
- `agent/tools/src/worktree.rs`：`EnterWorktree`、`ExitWorktree` 的 `fn description()` 和 `fn input_schema()` 实现
- 可能涉及：`agent/tools/src/` 下其他 tool 的 `input_schema` 参数描述审查

**验收标准**：
1. `EnterWorktree` 和 `ExitWorktree` 的 `description()` 返回纯英文描述。
2. `EnterWorktree` 和 `ExitWorktree` 的 `input_schema()` 中所有参数 `description` 字段为英文。
3. 全量审查通过：29 个内置 tool 的 description 和 input_schema 参数描述均为英文。
4. 编译通过（`cargo build -p aemeath-tools`）。
