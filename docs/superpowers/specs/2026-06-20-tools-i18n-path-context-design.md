# Tools 层文案双语化 + 工作区路径上下文通知机制统一

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/421
> 收编: #413 / #414 / #415
> 状态: 设计稿，待 review

## 背景

排查 #342（tool header 路径相对化）时发现三个关联设计问题：#413（path_base 语义不透明）、#414（Bash cd 不通知 LLM）、#415（ExitWorktree 缺 guidance）。深入后确认更根本的结构问题：**tools 层完全没有语言切换机制**，面向 LLM 的文案全局混乱硬编码。本设计统一建立 tools 层 i18n 基础设施，并解决路径上下文通知不对称。

## 现状调查

### 1. tools 层无 i18n 基础设施

- `TypedTool::description(&self) -> &str`（`contract/tool.rs:105`）返回静态引用，**无 lang 入参**，结构上无法按语言切换。
- `ToolExecutionContext`（`contract/context.rs:10-42`）**无 lang 字段**，工具执行时拿不到当前语言。
- 对比：runtime 层（prompt_build / loop_phases / finalize / task_reminder / git_context）已统一用 `lang: &str`（`"en"/"zh"`）切换。

### 2. TypedTool 实现清单（31 个）

内置工具（29 个，本地 description）：
FileReadTool FileWriteTool FileEditTool GlobTool GrepTool BashTool LspTool AgentTool WebSearchTool WebFetchTool SkillTool ToolSearchTool SleepTool BriefTool AskUserQuestionTool EnterPlanModeTool ExitPlanModeTool EnterWorktreeTool ExitWorktreeTool MemoryTool TaskCreateTool TaskGetTool TaskListTool TaskListCreateTool TaskListCompleteTool TaskStopTool TaskUpdateTool ListMcpResourcesTool ReadMcpResourceTool

MCP 工具（2 个，description 来自远端 server 动态字符串）：
- `McpTool`（`mcp_tool.rs`）
- `McpToolWrapper`（`mcp_manager/wrapper.rs:75` 返回 `&self.description`）

关键约束：MCP 工具的 description 是远端 server 提供的，无法按 lang 切换。i18n 机制必须对 MCP 工具优雅降级（原样透传远端文案）。

### 3. 文案语言混乱（189 处 TypedToolResult 调用点）

- 英文硬编码：task_*、file_*、bash、glob、grep、lsp、web_*、agent_tool、mcp_tool、sleep、ask_user、plan_mode、brief、skill_tool、tool_search
- 中文硬编码：worktree.rs（guidance + 部分错误）、memory_tool.rs + handlers.rs（全中文）
- 同文件混杂：worktree.rs 里 `Invalid input`（英）与 `进入 worktree 失败`（中）并存

### 4. 结构化 guidance 字段现状

- 仅 `EnterWorktreeResult`（`share/tool/types/enter_worktree.rs:12`）有 `guidance: String`。
- `ExitWorktreeResult`、`BashResult`（仅 `stdout/stderr/exit_code/signal`）、其它 typed result 均无。

### 5. 路径上下文通知不对称

- Bash `cd subdir`：path_base 变、working_root 不变，完全不通知（#414）
- EnterWorktree：path_base 变、working_root 变，result + guidance
- ExitWorktree：path_base 变、working_root 变，result（无 guidance，#415）
- guidance 只提 working_root，不解释 path_base 语义（#413）

## 设计目标

1. tools 层具备与 runtime 层对等的 i18n 能力（按 lang 切换面向 LLM 文案）。
2. 路径上下文通知对称、语义透明（解决 #413/#414/#415）。
3. MCP 动态工具不受 i18n 改造影响（降级透传）。
4. 改造分阶段可独立验证、独立 PR，不阻塞主分支。

## 设计决策

### 决策 1：lang 字段注入方式（call 内文案）

采用：`ToolExecutionContext` 增加 `pub lang: String` 字段，工具在 `call()` 内通过 `ctx.lang` 获取语言。

理由：
- `ToolExecutionContext` 是工具执行态的唯一上下文，lang 作为执行态属性天然归属此处。
- runtime 层（`loop_runner.rs:284`）构造 ctx 时已有 `language` 变量，透传零成本。
- sub-agent（`runner/setup.rs:167`）从 parent chat 透传 lang。

注意：description 在注册期消费（不在 call 内），无 ctx 访问，见决策 2/3。

### 决策 2：description 签名改造

description 在注册期被调用生成 tool schema 发给 LLM。现有签名 `fn description(&self) -> &str`。

采用「保留无参 description + 新增带 lang 的 description_for」：
- 保留 `fn description(&self) -> &str` 返回默认语言（英文），向后兼容。
- 新增带默认实现的 trait 方法 `fn description_for(&self, lang: &str) -> Cow<'_, str>`，默认委托 `description()`。
- 需要双语的工具覆盖 `description_for`，按 lang 返回对应文案；不覆盖的（含 2 个 MCP 工具）自动走默认，优雅降级。

默认语言选英文的理由：29 个内置工具中绝大多数 description 已是英文硬编码，默认英文使阶段一/二的迁移成本最低，只 worktree/memory 等少数中文文案需补英文分支。

不采用直接改 `description` 签名的原因：会强制 31 个实现（含无法按 lang 切换的 MCP 工具）全部改动，且 MCP 工具无合理双语文案来源。

### 决策 3：schema 生成期的 lang 来源

description 在注册期消费，但 lang 是会话级属性。

采用「schema 组装时显式传 lang」：
- `ToolRegistry` 新增 `schemas_for(lang: &str)`，内部调用 `description_for(lang)`。
- 旧 `schemas()` 保留，委托 `schemas_for("en")`（默认英文）兼容。
- runtime（`loop_runner.rs:290` 附近调用 `registry.schemas()`）改为传当前 `language`。

理由：lang 是会话级、非全局态，显式传参比全局变量清晰，registry 无需持可变 lang 状态。

### 决策 4：i18n 文案组织

采用「模块级 match lang + 常量表」，不引入 fluent/gettext 等外部依赖。

理由：
- 文案量有限（每个工具 description + 若干 result/error 文案），match 足够。
- 避免引入 .ftl 文件、编译时检查等复杂度。
- 与 runtime 层现有 `match lang { "zh" => ..., _ => ... }` 风格一致。

文案表定义在每个工具文件内（或同模块新建 i18n.rs）。默认分支（`_`）返回英文，`"zh"` 分支返回中文。示例（worktree.rs）：

```rust
fn enter_worktree_description(lang: &str) -> &'static str {
    match lang {
        "zh" => "进入或创建 git worktree 目录 ...",
        _ =>    "Enter or create a git worktree directory ...",
    }
}
```

### 决策 5：path_base 语义透明化（#413）

在 EnterWorktree/ExitWorktree 的 guidance 中明确区分两个字段角色：
- path_base = 相对路径解析基（LLM 传相对路径时，系统按 path_base.join 拼绝对路径）
- working_root = 安全边界（canonicalize 后检查 starts_with(working_root)，越界拒绝）

同步在系统提示（`prompt_build.rs:106/171` 的 Environment 段）补充一行简短语义说明。

### 决策 6：ExitWorktree guidance 对称（#415）

- 为 `ExitWorktreeResult` 增加 `guidance: String` 字段。
- 在 worktree.rs 两个退出分支（switch_to / exit）构造 payload 时填入 guidance，提示「已恢复到 XX，后续路径以当前 path_base/working_root 为准」。
- guidance 文案按 lang 双语（随决策 1-4 一起落地）。

### 决策 7：Bash cd 回传 path_base（#414）

- `BashResult` 增加 `path_base: Option<PathBuf>` 字段。
- 仅当命令执行后 path_base 发生变化时回填（`Some`），未变时为 `None`，减少噪音。
- 回填点：`bash.rs:264-268` 现有 set_path_base 调用处，同时把 new_path_base 写入 result。

## 实施阶段与任务拆分

### 阶段一：i18n 基础设施（先行阻塞，独立 PR）

| 任务 | 文件 | 内容 |
|---|---|---|
| T1 | `contract/context.rs` | `ToolExecutionContext` 增加 `pub lang: String` 字段 |
| T1 | `loop_runner.rs:259`、`runner/setup.rs:167`、所有测试构造点（`runner/tests.rs:251`、`agent_tests.rs:49` 等） | 透传 lang |
| T2 | `contract/tool.rs:105` | TypedTool 新增 `fn description_for(&self, lang: &str) -> Cow<'_, str>` 默认实现 |
| T2 | `core/tool_registry.rs` | 新增 `schemas_for(lang)`，旧 `schemas()` 委托默认英文 |
| T2 | `loop_runner.rs:290` | `registry.schemas()` 改传当前 language |

验证：`cargo build` + `cargo test -p aemeath-tools` + `cargo clippy --workspace`。本阶段不改变任何文案（只搭基础设施 + 默认走英文），行为零变化。

### 阶段二：路径上下文通知（依赖阶段一，三个子 issue 并行 PR）

| 任务 | 子 issue | 文件 | 内容 |
|---|---|---|---|
| T3 | #413 | `worktree.rs`、`prompt_build.rs` | guidance 区分 path_base/working_root 语义；系统提示补语义说明 |
| T4 | #415 | `exit_worktree.rs`、`worktree.rs` | ExitWorktreeResult 加 guidance 字段；两退出分支填双语 guidance |
| T5 | #414 | `bash.rs`、`shared/tool/types/bash.rs` | BashResult 加 path_base: Option<PathBuf>；变化时回填 |

验证：每个子 PR 独立 `cargo test` + clippy + 终端冒烟（EnterWorktree/ExitWorktree/Bash cd 各场景）。

### 阶段三：全量文案双语化（依赖阶段一，收尾 PR）

| 任务 | 范围 | 内容 |
|---|---|---|
| T6 | worktree.rs、memory_tool.rs + handlers.rs | 中文硬编码文案补英文分支 |
| T7 | task_*、file_*、bash、glob、grep、lsp、web_*、agent_tool、mcp_tool、sleep、ask_user、plan_mode、brief、skill_tool、tool_search | 英文硬编码文案补中文分支；各工具覆盖 description_for |

验证：`cargo test --workspace` + clippy + 终端冒烟（en/zh 两套语言全量过一遍主要工具）。

## 影响面

- trait 级：`TypedTool`（新增 1 个默认方法）、`ToolRegistry`（新增 1 个方法）。
- 横切类型：`ToolExecutionContext`（+1 字段），波及所有构造点（主循环、sub-agent、测试，约 5-8 处）。
- typed result struct：`ExitWorktreeResult`（+guidance）、`BashResult`（+path_base）。schema 透传给 LLM，属面向 LLM 的契约变更。
- 文案：29 个内置工具的 description + 189 处 TypedToolResult 调用点文案。
- 系统提示：`prompt_build.rs` 两段（中/英）各补一行 path_base 语义说明。
- MCP 工具：零改动（走 description_for 默认实现降级）。

## 风险与缓解

1. 阶段一改 trait + 横切类型，编译波及面大。缓解：阶段一零文案变更，纯加字段/方法 + 默认实现，可快速编译验证；先合并阶段一再开阶段二/三。
2. BashResult schema 变更可能影响已序列化的历史 tool result（session 回放）。缓解：新增字段用 Option + serde default，向后兼容反序列化。
3. 阶段三文案量大、易遗漏。缓解：以工具为单位逐个迁移 + 测试，不追求一次性全量；可分多个小 PR。
4. MCP 工具 description 无法双语是已知限制，文档与 issue 中明确标注，不作为本伞范围。

## 已定问题

- **description 默认语言：英文**。29 个内置工具中绝大多数 description 已是英文硬编码，默认英文迁移成本最低；只 worktree/memory 等少数中文文案需补英文分支。match 的默认分支（`_`）返回英文。
- **不引入文案 lint**。文案量有限（约 30 个工具），自建 i18n lint 成本高、误报率高；靠 review + 终端冒烟（en/zh 各跑一遍）覆盖。漏译暴露时是「文案显示成错的语言」，冒烟一眼可见，非隐蔽 bug。
