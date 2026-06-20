# 全项目 LLM 文案 i18n 统一 + 工作区路径上下文通知机制

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/421
> 收编: #413 / #414 / #415
> 状态: 设计稿，待 review

## 背景

排查 #342（tool header 路径相对化）时发现三个关联设计问题：#413（path_base 语义不透明）、#414（Bash cd 不通知 LLM）、#415（ExitWorktree 缺 guidance）。深入后确认两个层面的结构性问题：

1. 路径上下文通知不对称（#413/#414/#415 核心）：worktree/bash 等操作改变 path_base/working_root 后，对 LLM 的通知不完整、不对称。
2. LLM 文案 i18n 未统一：i18n 本是项目级横切能力，但现状是 prompt 层已完整双语、runtime 层有基础设施但文案漏译、tools 层连基础设施都没有。没有统一的文案集中点，散落各处。

本设计把 i18n 定位为项目级能力，建立集中文案 catalog（单一真相），解决路径上下文通知不对称，并把散落文案全量收敛。

## 现状调查

### 1. 三层 i18n 成熟度不一

| 层 | i18n 基础设施 | 文案双语情况 |
|---|---|---|
| prompt | 完整（`constants.rs` 成对 EN/ZH 常量 + `universal_execution_discipline(lang)`） | 就绪 |
| runtime | 有（`lang: &str` 到处透传） | **4 处文案漏译**（见 §1.2） |
| tools | **无**（trait 无 lang 入参、ctx 无 lang 字段） | 混乱硬编码（见 §1.3） |

#### 1.1 tools 层无 i18n 基础设施

- `TypedTool::description(&self) -> &str`（`contract/tool.rs:105`）返回静态引用，**无 lang 入参**。
- `ToolExecutionContext`（`contract/context.rs:10-42`）**无 lang 字段**。

#### 1.2 runtime 层文案漏译（基础设施已有，文案漏写 match lang）

| 文件 | 内容 | 语言 |
|---|---|---|
| `business/reflection/prompt.rs` | 整个 Reflection 引擎 system prompt | 中文硬编码 |
| `compact/restore/assemble.rs:28` | compaction 历史摘要恢复注入 `[Conversation summary of N earlier messages]` | 英文硬编码 |
| `compact/restore/restore_files.rs:76` | compaction 文件恢复注入 `[Post-compaction file restoration...]` | 英文硬编码 |
| `core/client/trait_command.rs:57/62` | "未知命令" | 中文硬编码（CLI 提示） |

#### 1.3 tools 层文案语言混乱（189 处 TypedToolResult 调用点）

- 英文硬编码：task_*、file_*、bash、glob、grep、lsp、web_*、agent_tool、mcp_tool、sleep、ask_user、plan_mode、brief、skill_tool、tool_search
- 中文硬编码：worktree.rs（guidance + 部分错误）、memory_tool.rs + handlers.rs（全中文）
- 同文件混杂：worktree.rs 里 `Invalid input`（英）与 `进入 worktree 失败`（中）并存

### 2. TypedTool 实现清单（31 个）

内置工具（29 个，本地 description）：
FileReadTool FileWriteTool FileEditTool GlobTool GrepTool BashTool LspTool AgentTool WebSearchTool WebFetchTool SkillTool ToolSearchTool SleepTool BriefTool AskUserQuestionTool EnterPlanModeTool ExitPlanModeTool EnterWorktreeTool ExitWorktreeTool MemoryTool TaskCreateTool TaskGetTool TaskListTool TaskListCreateTool TaskListCompleteTool TaskStopTool TaskUpdateTool ListMcpResourcesTool ReadMcpResourceTool

MCP 工具（2 个，description 来自远端 server 动态字符串）：
- `McpTool`（`mcp_tool.rs`）
- `McpToolWrapper`（`mcp_manager/wrapper.rs:75` 返回 `&self.description`）

关键约束：MCP 工具的 description 是远端 server 提供的，无法按 lang 切换。i18n 机制必须对 MCP 工具优雅降级（原样透传远端文案）。

### 3. 结构化 guidance 字段现状

- 仅 `EnterWorktreeResult`（`share/tool/types/enter_worktree.rs:12`）有 `guidance: String`。
- `ExitWorktreeResult`、`BashResult`（仅 `stdout/stderr/exit_code/signal`）、其它 typed result 均无。

### 4. 路径上下文通知不对称

- Bash `cd subdir`：path_base 变、working_root 不变，完全不通知（#414）
- EnterWorktree：path_base 变、working_root 变，result + guidance
- ExitWorktree：path_base 变、working_root 变，result（无 guidance，#415）
- guidance 只提 working_root，不解释 path_base 语义（#413）

## 设计目标

1. i18n 作为项目级横切能力，建立集中文案 catalog（单一真相），三层共用。
2. tools 层接入项目已有 i18n 机制（lang 透传 + 按 lang 取文案）。
3. 路径上下文通知对称、语义透明（解决 #413/#414/#415）。
4. runtime 层 4 处漏译文案补齐。
5. MCP 动态工具不受 i18n 改造影响（降级透传）。
6. 改造分阶段可独立验证、独立 PR，不阻塞主分支。

## 设计决策

### 决策 1：集中文案 catalog（方案 B）

采用集中 catalog，位置 `agent/shared/src/i18n/`（模块 `share::i18n`）。三层（prompt/runtime/tools）共同依赖 `share`，都能访问。单一真相，文案收敛一处。

#### API 形态：强类型函数式（不用字符串 key）

不用 `i18n::t("tools.worktree.enter", lang)` 字符串 key 方案。理由：

- key 拼写错误只能运行期暴露（panic 或返回空文案），强类型函数编译期就抓到。
- 无运行期查表（HashMap/Match）开销。
- 与现有 `prompt/constants.rs` 的 `universal_execution_discipline(lang)` 风格一致，只是从散落各模块收敛到 `share::i18n` 单点。

两种函数签名：

```rust
// share/src/i18n/tools/worktree.rs

/// 无参文案：返回 &'static str（零分配）
pub fn enter_description(lang: &str) -> &'static str {
    match lang {
        "zh" => "进入或创建 git worktree 目录 ...",
        _ =>    "Enter or create a git worktree directory ...",
    }
}

/// 带参文案：返回 String，内部 {placeholder} + replace（沿用现有 prompt_build 模式）
pub fn commit_guidance(lang: &str, trailer: &str) -> String {
    let tmpl = match lang {
        "zh" => "...Co-Authored-By... {trailer}",
        _ =>    "...Co-Authored-By... {trailer}",
    };
    tmpl.replace("{trailer}", trailer)
}
```

默认分支（`_`）返回英文，`"zh"` 分支返回中文（见「已定问题」）。

#### 目录组织：按功能域分层

```
agent/shared/src/i18n/
├── mod.rs              // pub use 汇总，暴露 Lang 类型别名与默认 lang 常量
├── prompt/             // 系统 prompt、guidance、commit guidance
│   ├── mod.rs
│   ├── discipline.rs   // 迁自 prompt/constants.rs 的 UNIVERSAL_EXECUTION_DISCIPLINE_*
│   └── guidance.rs
├── runtime/            // reflection prompt、compact restore、claudeMd reminder 等
│   ├── mod.rs
│   ├── reflection.rs
│   └── compact.rs
└── tools/              // 29 个工具的 description + result/error 文案
    ├── mod.rs
    ├── worktree.rs
    ├── bash.rs
    └── ...（按工具分文件）
```

#### 迁移既有文案

- `prompt/constants.rs` 的 `UNIVERSAL_EXECUTION_DISCIPLINE_EN/ZH` 迁入 `share::i18n::prompt::discipline`，`universal_execution_discipline` 在 prompt feature 改为 re-export（`pub use share::i18n::prompt::discipline::*;`），调用点零改动。
- runtime 层 4 处漏译文案迁入 `share::i18n::runtime`（同时补齐缺失的语言分支）。
- tools 层 189 处文案迁入 `share::i18n::tools`（同时补齐缺失的语言分支）。

### 决策 2：tools 层 lang 注入（call 内文案）

`ToolExecutionContext`（`contract/context.rs`）增加 `pub lang: String` 字段，工具在 `call()` 内通过 `ctx.lang` 获取语言，文案调用 `share::i18n::tools::xxx(&ctx.lang)`。

理由：`ToolExecutionContext` 是工具执行态的唯一上下文，lang 作为执行态属性天然归属此处。runtime 层（`loop_runner.rs:284`）构造 ctx 时已有 `language` 变量，透传零成本；sub-agent（`runner/setup.rs:167`）从 parent chat 透传。

注意：description 在注册期消费（不在 call 内），无 ctx 访问，见决策 3。

### 决策 3：description 签名改造

description 在注册期被调用生成 tool schema 发给 LLM。现有签名 `fn description(&self) -> &str`。

采用「保留无参 description + 新增带 lang 的 description_for」：
- 保留 `fn description(&self) -> &str` 返回默认语言（英文），向后兼容。
- 新增带默认实现的 trait 方法 `fn description_for(&self, lang: &str) -> Cow<'_, str>`，默认委托 `description()`。
- 需要双语的工具覆盖 `description_for`，从 `share::i18n::tools::xxx(lang)` 取文案；不覆盖的（含 2 个 MCP 工具）自动走默认，优雅降级。

不采用直接改 `description` 签名的原因：会强制 31 个实现（含无法按 lang 切换的 MCP 工具）全部改动，且 MCP 工具无合理双语文案来源。

### 决策 4：schema 生成期的 lang 来源

description 在注册期消费，但 lang 是会话级属性。采用「schema 组装时显式传 lang」：
- `ToolRegistry` 新增 `schemas_for(lang: &str)`，内部调用 `description_for(lang)`。
- 旧 `schemas()` 保留，委托 `schemas_for("en")`（默认英文）兼容。
- runtime（`loop_runner.rs:290` 附近调用 `registry.schemas()`）改为传当前 `language`。

理由：lang 是会话级、非全局态，显式传参比全局变量清晰，registry 无需持可变 lang 状态。

### 决策 5：path_base 语义透明化（#413）

在 EnterWorktree/ExitWorktree 的 guidance 中明确区分两个字段角色：
- path_base = 相对路径解析基（LLM 传相对路径时，系统按 path_base.join 拼绝对路径）
- working_root = 安全边界（canonicalize 后检查 starts_with(working_root)，越界拒绝）

同步在系统提示（`prompt_build.rs:106/171` 的 Environment 段）补充一行简短语义说明。

### 决策 6：ExitWorktree guidance 对称（#415）

- 为 `ExitWorktreeResult` 增加 `guidance: String` 字段。
- 在 worktree.rs 两个退出分支（switch_to / exit）构造 payload 时填入 guidance，提示「已恢复到 XX，后续路径以当前 path_base/working_root 为准」。
- guidance 文案从 `share::i18n::tools::worktree` 取，按 lang 双语。

### 决策 7：Bash cd 回传 path_base（#414）

- `BashResult` 增加 `path_base: Option<PathBuf>` 字段。
- 仅当命令执行后 path_base 发生变化时回填（`Some`），未变时为 `None`，减少噪音。
- 回填点：`bash.rs:264-268` 现有 set_path_base 调用处，同时把 new_path_base 写入 result。

## 实施阶段与任务拆分

### 阶段一：i18n catalog 基础设施（先行阻塞，独立 PR）

| 任务 | 文件 | 内容 |
|---|---|---|
| T1 | `shared/src/i18n/`（新建 mod.rs + prompt/runtime/tools 子目录） | 建 catalog 骨架，定义 Lang 类型别名、默认 lang 常量 |
| T2 | `shared/src/i18n/prompt/discipline.rs` | 迁 `prompt/constants.rs` 的 UNIVERSAL_EXECUTION_DISCIPLINE_EN/ZH；prompt feature re-export |
| T3 | `tools/contract/context.rs` | `ToolExecutionContext` 增加 `pub lang: String` 字段 |
| T3 | `loop_runner.rs:259`、`runner/setup.rs:167`、所有测试构造点 | 透传 lang |
| T4 | `tools/contract/tool.rs:105` | TypedTool 新增 `fn description_for(&self, lang) -> Cow` 默认实现 |
| T4 | `tools/core/tool_registry.rs` | 新增 `schemas_for(lang)`，旧 `schemas()` 委托默认英文 |
| T4 | `loop_runner.rs:290` | `registry.schemas()` 改传当前 language |

验证：`cargo build` + `cargo test --workspace` + `cargo clippy --workspace`。本阶段不改变任何文案（只搭 catalog 骨架 + 基础设施 + 默认走英文），行为零变化。

### 阶段二：路径上下文通知（依赖阶段一，三个子 issue 并行 PR）

| 任务 | 子 issue | 文件 | 内容 |
|---|---|---|---|
| T5 | #413 | `i18n/tools/worktree.rs`、`prompt_build.rs` | guidance 区分 path_base/working_root 语义；系统提示补语义说明 |
| T6 | #415 | `exit_worktree.rs`、`worktree.rs`、`i18n/tools/worktree.rs` | ExitWorktreeResult 加 guidance 字段；两退出分支填双语 guidance |
| T7 | #414 | `bash.rs`、`shared/tool/types/bash.rs` | BashResult 加 path_base: Option<PathBuf>；变化时回填 |

验证：每个子 PR 独立 `cargo test` + clippy + 终端冒烟（EnterWorktree/ExitWorktree/Bash cd 各场景）。

### 阶段三：全量文案收敛（依赖阶段一，收尾，可分多个 PR）

| 任务 | 范围 | 内容 |
|---|---|---|
| T8 | runtime 4 处漏译（reflection prompt、compact restore×2、trait_command） | 迁入 `i18n/runtime`，补齐缺失语言分支 |
| T9 | tools 189 处文案（29 个内置工具） | 迁入 `i18n/tools`，补齐缺失语言分支；各工具覆盖 description_for |
| T10 | prompt 其余散落文案（如 build_commit_guidance 的 EN/ZH 分支） | 迁入 `i18n/prompt`，调用点改引用 catalog |

验证：`cargo test --workspace` + clippy + 终端冒烟（en/zh 两套语言全量过一遍主要工具）。

## 影响面

- 新增：`agent/shared/src/i18n/` 模块（catalog 单一真相）。
- trait 级：`TypedTool`（+1 默认方法 `description_for`）、`ToolRegistry`（+1 方法 `schemas_for`）。
- 横切类型：`ToolExecutionContext`（+lang 字段），波及所有构造点（主循环、sub-agent、测试，约 5-8 处）。
- typed result struct：`ExitWorktreeResult`（+guidance）、`BashResult`（+path_base）。schema 透传给 LLM，属面向 LLM 的契约变更。
- 文案：迁移 prompt 层 guidance 常量、runtime 4 处漏译、tools 189 处文案到集中 catalog。
- 系统提示：`prompt_build.rs` 两段（中/英）各补一行 path_base 语义说明。
- MCP 工具：零改动（走 description_for 默认实现降级）。

## 风险与缓解

1. catalog 新增 + 迁移既有文案编译波及面大。缓解：阶段一只搭骨架 + re-export（prompt feature 零行为变化），先合并再逐域迁移。
2. 阶段一改 trait + 横切类型，编译波及面大。缓解：阶段一零文案变更，纯加字段/方法 + 默认实现，可快速编译验证。
3. BashResult schema 变更可能影响已序列化的历史 tool result（session 回放）。缓解：新增字段用 Option + serde default，向后兼容反序列化。
4. 阶段三文案量大、易遗漏。缓解：以工具/模块为单位逐个迁移 + 测试，分多个小 PR，不追求一次性全量。
5. MCP 工具 description 无法双语是已知限制，文档与 issue 中明确标注，不作为本伞范围。

## 已定问题

- **description 默认语言：英文**。29 个内置工具中绝大多数 description 已是英文硬编码，默认英文迁移成本最低；只 worktree/memory 等少数中文文案需补英文分支。match 的默认分支（`_`）返回英文。
- **不引入文案 lint**。文案量有限（约 30 个工具 + runtime/prompt 散落文案），自建 i18n lint 成本高、误报率高；靠 review + 终端冒烟（en/zh 各跑一遍）覆盖。漏译暴露时是「文案显示成错的语言」，冒烟一眼可见，非隐蔽 bug。
- **集中 catalog 放 `share::i18n`**（不放 shared 外或某 feature 内）。理由：`share` 是三层共同依赖的唯一横切 crate，catalog 放此处三层都能访问，不引入新的跨 feature 依赖。
- **强类型函数式 API，不用字符串 key**。编译期发现拼写错误，无运行期查表开销，与现有 prompt 风格一致。
