# ToolResult 重构（typed data + 扁平化）+ 11 工具 TUI Display 接入

> **面向 agentic worker：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans，按任务逐项执行本计划。步骤使用复选框（`- [ ]`）语法进行追踪。
>
> **本计划范围扩展（2026-06-18）：** 原计划仅做"为 11 个工具补 `data` 字段 + TUI Display 接入"。经用户决策，本计划**纳入 issue #325 的 ToolResult 扁平化重构 + 引入 typed result 关联类型**。原 issue #325 现已合并入本计划实施范围。
>
> 关联 issue 状态：
> - **#273**（父）— tool call header 应读取结构化数据
> - **#325**（已合并）— ToolResult 扁平 `{ok, message, data: R}` + typed result
> - #321-#324（未合并）— 18 工具补全、HeaderPolicy 激活、路径优化、ResultRender 抽取

**目标：**
1. **根因级重构** `ToolResult` API：消除 4-method（`success`/`success_json`/`error`/`error_json`）不对称设计、消除 `is_error` 否定语义、消除 `{"text": str}` 隐式包装 bug（issue #325）。
2. **引入 typed result 关联类型**：`Tool::Result` 让每个工具返回 `ToolResult<Self::Result>`，数据形状由类型系统保证；TUI 端反序列化为 typed struct，**编译期校验字段存在性**。
3. **TUI 工具调用头部从结构化数据读取** `(N lines)` / `(N bytes)` / `(N matches)` / `(exit N)` 等统计，**取代**消息字符串正则反推（issue #273 核心目标）。

**架构：**
- **Phase 0 基础重构**（新）：`agent/shared/src/tool.rs` 定义 `ToolResult<R: Serialize + DeserializeOwned>` 泛型扁平结构 `{ok, message, data: R, images}`。`Tool` trait 加 `type Result: Serialize + DeserializeOwned`。29 个工具的 `call` 返回 `ToolResult<Self::Result>`。
- **工具层**（`agent/features/tools/src/business/` 下 11 个核心文件）每个定义自己的 typed result struct（`ReadResult`/`BashResult`/`EditResult`/...），`call` 返回时构造 typed 实例。**11 个核心 tool**（issue #273 范围）的 struct 字段覆盖 `(N lines)` / `(N bytes)` / `(N matches)` / `(exit N)` 等头部所需统计。**其余 18 个 tool**（#321 follow-up）使用 typed 但**字段集合**由各自语义决定。
- **TUI 展示层**（`apps/cli/src/tui/render/output/` 下 3 个文件）通过 inventory 注册的 `*Display` 接收 `data: Value`（边界处 `from_value::<ReadResult>(data)` 反序列化），typed struct 字段直接 `.field` 访问，**不再需要** 5 层链式 `.get().and_then()`。
- **边界序列化**：tool 内部全程 typed，跨边界（event 流 / 持久化 / TUI 渲染）调用 `to_value()` 显式序列化为 `serde_json::Value`；TUI 端用 `from_value` 反序列化为 typed。
- **数据层向后兼容**：`message` 字符串保持不变，模型消费不受影响。typed struct schema 只新增字段，永不删除字段。

**超出范围（推迟至后续计划）：**
1. **17+ 个未注册工具**没有 TUI 显示注册——`brief`、`config_tool`、`lsp`、`memory_tool`、`plan_mode`、`sleep`、`skill_tool`、`task_create`、`task_get`、`task_list`、`task_stop`、`task_update`、`web_search`、`mcp_tool`、`mcp_manager`、`list_mcp_resources`、`read_mcp_resource`、`tool_search`。它们的结果不会流入 `format_tool_call`（已由 `inventory::collect!(ToolDisplayEntry)` 验证）。
2. **`-q`（no_tui）模式**——`apps/cli/src/chat/no_tui.rs:83` 的 `render_event` 仅 `eprintln!` 输出 `[tool:name] output` 和 `[tool:start] {name}`，**不会**调用 `format_tool_call`；TUI 的改动对此零影响。
3. **`HeaderPolicy` 枚举的死代码激活**——字段 `ToolRenderPolicy::header`（及其 3 个变体 `Standard`/`Compact`/`CustomIcon`）在 11 个工具实现和 `task_impls.rs` 中是只写的；当前代码库中没有读取者。激活它将是对头部渲染的更大重设计。本计划保持头部渲染走当前的 `format_header(input)` 路径。
4. **路径相对化**（TUI 中 working_root 与 path_base 的切换）——与本议题正交；用户已明确推迟到后续 PR。
5. **`ResultRender::Diff` 抽取 / `ResultPolicy::Visible.max_lines` 参数化**——独立的重构。

**技术栈：** Rust workspace，`serde` + `serde_json`（typed struct 序列化/反序列化），`ratatui::text::{Line, Span}`，`inventory` 注册，`async_trait`（用于 `Tool::call`），`cargo test/clippy/build`。**不再依赖**裸 `serde_json::Value` 模式（仅在 wire 边界处使用）。

---

## 背景：已验证的当前状态

审计于 2026-06-18 针对实际代码（非推断）完成。行号来自 worktree `feature/tool-display-structured-data`。

### TUI 层（`apps/cli/src/tui/render/output/`）

| 元素 | 位置 | 状态 |
|---|---|---|
| `HeaderPolicy::{Standard, Compact, CustomIcon(&'static str)}` | `tool_display.rs:16-24` | 死代码（0 读取者） |
| `DetailsPolicy::{Expanded, Hidden}` | `tool_display.rs:27-33` | 在 `tool_display.rs:179` 被读取 |
| `ResultPolicy::{Hidden, Visible{max_lines, render_kind, tail_mode}}` | `tool_display.rs:36-49` | 在 `tool_display.rs:148,156` 被读取 |
| `ResultRender::{Plain, Diff}` | `tool_display.rs:61-68` | 通过 `ResultPolicy` 读取 |
| `format_header_line_with_result` | `tool_display.rs:109-115`（默认） | **仅 2 个实现覆写**：`ReadDisplay:99`，`WriteDisplay:185` |
| `format_header_line` | `tool_display.rs:90-104`（默认） | **仅 1 个实现覆写**：`ReadDisplay:81` |
| `format_tool_call` 签名 | `tool_display.rs:169-173` | `(name, raw_json, result_summary: Option<&str>) -> (Line<'static>, Vec<String>)` |
| `common::truncate_*` 辅助函数 | `common.rs:25,36` | 全部 `pub(super)` |
| `truncate_path` | `tool_impls.rs:60-62` | **私有**自由函数（不在 `common` 中） |
| `theme::ACCENT_BRIGHT/TEXT/TEXT_MUTED` | `render/theme/palette.rs:71,61,63` | `pub const`，通过 `theme.rs:3` 重新导出 |
| `ToolCallBlockView` 定义 | `view_model/output.rs:101-115` | **13 个字段**，单一定义 |
| `find_tool_result_payload` | `view_assembler/output.rs:425-450` | 返回**私有借用视图**（`struct ToolResultPayload<'a>`）；与模型层拥有的结构体不同 |
| `model::ToolResultPayload` | `model/conversation/tool_result_payload.rs:4-9` | `pub` 拥有的结构体，4 个字段：`output: String, content: Value, is_error: bool, image_count: usize`；带有 `pub fn new()` |

### 工具层（`agent/features/tools/src/business/`）

| 工具 | 实际文件 | `data` 成功 | `data` 失败 | Message | 测试 | 需执行的操作 |
|---|---|---|---|---|---|---|
| Read | `file_read.rs` | `{content, file_path}` | 不定 | `Read N lines from {path}` | 无 | 在 success data 中新增 `line_count/offset/limit` |
| Write | `file_write.rs` | `{file_path, bytes_written}` 通过 `success(str)` | 不定 通过 `error(str)` | `Wrote N bytes to {path}` | 无 | 迁移至 `success_json`/`error_json`（目前是字符串化的 JSON） |
| Edit | `file_edit.rs` | `{file_path, occurrences, fuzzy_note, ...}` 通过 `success(str)` | 全部 `null`（9 处） | `Replaced N occurrence(s) in {path}` | 有（第 342-413 行） | 迁移至 `success_json`/`error_json` |
| Glob | `glob_tool.rs` | `{files, count}` 通过 `success(str)` | `{}` | `Found N files` | 无 | 迁移至 `success_json`；新增 `match_count`（保留 `count` 一个版本） |
| Grep | `grep.rs` | `{matches, match_count}` 通过 `success(str)` | `{matches: [], match_count: 0}` | `Found N matches` | 无 | 迁移至 `success_json`；新增 `truncated: bool`（当前 matches 是 `take(250)`） |
| WebFetch | `web_fetch.rs` | `{url, content}`（2 处：truncated/normal） | `{url}`（5 处） | `Fetched {url} (truncated, ...)` | 无 | 2 个 success 站点：新增 `byte_count/char_count/truncated` |
| Bash | `bash.rs` | `{stdout, exit_code, stderr?}`（i32） | 8 处 pre-exec：**没有 data 字段** | `Command executed successfully` / `Command failed: {detail}` | 有（第 475-822 行） | 为 8 处 pre-exec error 站点新增 `data: {}` |
| Agent | `agent_tool.rs` | `{agent_id, output}` 通过 `success_json` | 不定 | `子代理执行完成`（硬编码中文） | 有 | 无改动（无计数语义） |
| EnterWorktree | `worktree.rs:81-141` | **没有 `data` 包装**（content = 完整 `workspace_context_payload`） | 仅 `{status, message}` | `已进入 worktree：{target}` | 共享 | 将 payload 字段移入 `data` |
| ExitWorktree | `worktree.rs:159-218` | 与 EnterWorktree 相同 | 相同 | `已切换到：{path}` | 共享 | 同上 |
| AskUserQuestion | `ask_user.rs` | `{type, question, options?}` 通过 `success(str)` | `{}` | `__ASK_USER_SELECT__: {q}`（协议前缀） | 有（第 151-201 行） | 迁移至 `success_json`；在 message 中保留协议前缀 |

**已发现的关键文件与约束：**

- `serde_json::Value` **没有 `as_value()` 方法**——直接使用 `&payload.content`。旧计划中的 `data_field_u64` 函数体有误。
- `ToolResultPayload` 有 4 个字段：测试必须用 `ToolResultPayload::new(output, content, is_error, image_count)`（定义于 `tool_result_payload.rs:12`），或使用包含全部 4 个字段的部分结构体字面量。
- `format_tool_call` 返回 `(Line<'static>, Vec<String>)`——details 是 `String` 而非 `Line`。`blocks/tool_call.rs:51-56` 的调用点会迭代并为每项设置样式。
- `truncate_path` 是 `tool_impls.rs:60` 的私有函数；新的 `build_header_line` 辅助函数必须位于 `tool_impls.rs`（或将截断作为输入参数传入）。
- `theme::ACCENT_BRIGHT/TEXT/TEXT_MUTED` 存在（位于 `theme/palette.rs:71,61,63`），均为 `pub const`。

---

## 文件结构

**修改 —— TUI 层：**
- `apps/cli/src/tui/render/output/tool_display.rs`——新增 3 个类型化辅助函数（`data_field_u64`、`data_field_i64`、`data_field_string`）；更改 trait 中 `format_header_line_with_result` 的签名和默认实现。
- `apps/cli/src/tui/render/output/tool_display/tool_impls.rs`——新增私有 `build_header_line` 辅助函数（使用 `theme::ACCENT_BRIGHT` 前缀和现有 `truncate_path`）；重写 `ReadDisplay::format_header_line_with_result`（去除正则）；重写 `WriteDisplay::format_header_line_with_result`（去除正则）；为 `EditDisplay`、`GlobDisplay`、`GrepDisplay`、`WebFetchDisplay`、`BashDisplay`、`AgentDisplay`、`EnterWorktreeDisplay`、`ExitWorktreeDisplay`、`AskUserQuestionDisplay` 新增 9 个 `format_header_line_with_result` 覆写。删除 `parse_line_count_from_message`（`tool_impls.rs:155-160`）和 `parse_bytes_from_message`（`tool_impls.rs:247-252`）。
- `apps/cli/src/tui/render/output/blocks/tool_call.rs`——在第 20 行传入 `view.result_payload.as_ref()`（替换 `view.result_summary.as_deref()`）。
- `apps/cli/src/tui/view_model/output.rs:101`——为 `ToolCallBlockView` 新增字段 `result_payload: Option<crate::tui::model::conversation::tool_result_payload::ToolResultPayload>`。
- `apps/cli/src/tui/view_assembler/output.rs:336`——填充新字段。用构造模型层拥有的 `ToolResultPayload` 的代码替换私有的借用 `view_assembler::ToolResultPayload<'a>` 查找（第 400-449 行），数据源使用相同的 `ConversationBlock::ToolResult`。第 351 行的 `display_text_for_tool_result(...payload.content)` 调用仍须接收 `&serde_json::Value`。

**修改 —— 工具层：**
- `agent/features/tools/src/business/file_read.rs:101`——在 success data 块中新增 `line_count`、`offset`、`limit`（第 94 行的 `(empty file)` 分支同样需要新增）。
- `agent/features/tools/src/business/file_write.rs:92`——从 `success(str)` 迁移至 `success_json(serde_json::json!({...}))`；4 个 `error` 调用点（第 52、63、78、103 行）同样迁移至 `error_json`。
- `agent/features/tools/src/business/file_edit.rs:217`——从 `success(str)` 迁移至 `success_json`；9 个 `error` 站点（第 47、63、76、91、103、114、176、186、234 行）迁移至 `error_json`。
- `agent/features/tools/src/business/glob_tool.rs:72,83`——迁移至 `success_json`；在两个 success 站点的 `count` 旁新增 `match_count`（两者填充相同值）；第 93 行的 `error` 迁移至 `error_json`。
- `agent/features/tools/src/business/grep.rs:91,105`——迁移至 `success_json`；新增 `truncated: bool`（当 `matches.len() == 250 && total > 250` 时为 true）；第 47、118 行的 `error` 迁移至 `error_json`。
- `agent/features/tools/src/business/web_fetch.rs:199,208`——在两个 success 站点新增 `byte_count`、`char_count`、`truncated: bool`；第 222、232、240 行的 3 个 error 站点，仅在第 222 行的状态错误中新增 `byte_count: output.stdout.len()`——其余两个没有响应体。
- `agent/features/tools/src/business/bash.rs`——为 8 处 pre-exec error 站点（第 70、78、87、117、251、272、292、388 行）新增 `data: {}`（或相关字段）。
- `agent/features/tools/src/business/worktree.rs:50-61`——将 `workspace_context_payload` 改写为其字段嵌套于 `data` 之下（status/message 保持在顶层，branch/path_base/working_root/guidance 进入 `data`）。
- `agent/features/tools/src/business/ask_user.rs:121,134`——迁移至 `success_json`（保留 `__ASK_USER_*__:` 消息协议前缀）。

**新增测试：** 每个被修改的文件按照 `specs/rust-coding.md` 的约定追加 `#[cfg(test)] mod tests`。每个测试组至少覆盖 3 种情况（正常 / 边界 / 错误）——详见下方各任务。

**无规范变更：** 已验证 `specs/tui-cli.md` 与当前代码一致（无漂移）。

---

## Phase 0a 新建 `packages/global/types` crate（TUI / tools 共享类型层）

> **本章节是范围扩展后新增（2026-06-18）**。29 个 R struct 不放在 `agent/features/tools/src/business/`（避免 TUI 跨边界依赖 agent 业务层），改为放新建的 `packages/global/types` crate。`apps/cli` + `agent/features/tools` 均直接依赖 `types`——**TUI 真正解耦 agent 业务层**。

**依赖图**（Phase 0a 后）：

```
packages/global/types                  ← 29 个 XxxResult struct（纯数据，无业务实现）
        ↑ dep by
   ┌────┴────┐
   │         │
agent/features/tools     apps/cli
(Tool::Result=ReadResult) (TUI Display 反序列化)
```

- `agent/shared` **不** dep types（`type Result: Serialize + Deserialize` 是 serde bound，不需具体 struct）
- `agent/features/runtime` **不** dep types（runtime 只看 `Value`，跨 `Box<dyn Tool>` 边界由 `ToolResult::to_value()` 抹平）

### 任务 0a.1 — 创建 `packages/global/types` crate 骨架

**文件**：
- 新建 `packages/global/types/Cargo.toml`：
  ```toml
  [package]
  name = "types"
  version.workspace = true
  edition.workspace = true

  [dependencies]
  serde = { workspace = true }
  serde_json = { workspace = true }
  ```
- 新建 `packages/global/types/src/lib.rs`：
  ```rust
  pub mod tool_result;
  ```
- 修改根 `Cargo.toml` `members` 列表追加 `"packages/global/types"`。

**commit 模板**：
```
feat(types): scaffold types crate for shared tool result types

新建 packages/global/types/ 承载 29 个 XxxResult struct，
供 apps/cli + agent/features/tools 共享使用。

- 依赖: serde + serde_json
- lib.rs: pub mod tool_result
- 注册到 workspace members

相关 issue: #273, #325
```

### 任务 0a.2 — `agent/features/tools/Cargo.toml` 加 `types` 依赖

- 修改 `agent/features/tools/Cargo.toml`，`[dependencies]` 加：
  ```toml
  types = { path = "../../../packages/global/types" }
  ```

**commit 模板**：
```
feat(tools): depend on types crate for shared result structs

为 #273 typed refactor 准备：tools 业务实现将通过
`use types::tool_result::ReadResult;` 引用 result struct。

相关 issue: #273, #325
```

### 任务 0a.3 — `apps/cli/Cargo.toml` 加 `types` 依赖

- 修改 `apps/cli/Cargo.toml`，`[dependencies]` 加：
  ```toml
  types = { path = "../../packages/global/types" }
  ```

**commit 模板**：
```
feat(cli): depend on types crate for typed Display deserialization

apps/cli TUI 不再跨边界依赖 agent/features/tools。
Display 通过 `use types::tool_result::ReadResult;` +
`serde_json::from_value::<ReadResult>(data)` 拿 typed 字段。

相关 issue: #273, #325
```

### 任务 0a.4 — 创建 `packages/global/types/src/tool_result/mod.rs`

**目标**：声明 29 个子模块入口（每个 struct 一文件，文件名 = tool 名）。

- 新建 `packages/global/types/src/tool_result/mod.rs`：
  ```rust
  pub mod read;
  pub mod write;
  pub mod edit;
  pub mod glob;
  pub mod grep;
  pub mod web_fetch;
  pub mod web_search;
  pub mod bash;
  pub mod sleep;
  pub mod agent;
  pub mod ask_user;
  pub mod enter_worktree;
  pub mod exit_worktree;
  pub mod brief;
  pub mod config_tool;
  pub mod lsp;
  pub mod plan_mode;
  pub mod memory;
  pub mod skill;
  pub mod task_create;
  pub mod task_get;
  pub mod task_list;
  pub mod task_stop;
  pub mod task_update;
  pub mod task_list_create;
  pub mod task_list_complete;
  pub mod tool_search;
  pub mod mcp_tool;
  pub mod mcp_manager;
  pub mod list_mcp_resources;
  pub mod read_mcp_resource;
  ```
  （29 个子模块，对应 29 个 struct；子模块体由 Phase 0 任务 0.3/0.4 填充）

**commit 模板**：
```
feat(types): declare 29 tool_result submodules

29 个 R struct 各占一文件，文件名与 tool 名一一对应。
子模块体由 Phase 0 任务 0.3/0.4 填充。

相关 issue: #273, #325
```

### 任务 0a.5 — Phase 0a 验证

```bash
cargo check -p types       # types crate 自检
cargo check -p tools       # 验证 types 依赖不破坏 tools
cargo check -p cli         # 验证 types 依赖不破坏 apps/cli
cargo check --workspace    # 全 workspace 编译通过
```

### Phase 0a 总结

| 维度 | 值 |
|---|---|
| 任务数 | 5（0a.1 - 0a.5） |
| atomic commit 数 | 4（scaffold + tools-deps + cli-deps + submodules 声明；验证不单独 commit） |
| 新 crate | `packages/global/types`（name="types"） |
| 跨 crate 依赖 | types ← tools, types ← cli |
| TUI 跨边界依赖 | **消除**（TUI 不再 dep agent/features/tools） |

---

## Phase 0 ToolResult 重构与 typed result 关联类型

> **本章节是范围扩展后新增（2026-06-18）**。原 plan 的 4 个 typed-helpers 任务（Task 1-4 范围）被本阶段替代——typed struct 直接提供 `.field` 访问，无需链式 `data_field_*()` 辅助函数。本 Phase 0 的 29 个 R struct 物理位置在 `packages/global/types/src/tool_result/`（由 Phase 0a 任务 0a.4 创建），tool impl 通过 `use types::tool_result::XxxResult;` 引用。

### 任务 0.1 — `agent/shared/src/tool.rs` 新 `ToolResult<R>`

**目标**：引入泛型扁平结构，迁移 4-method → 2-method + 2 便捷。

```rust
use serde::{Serialize, Deserialize, de::DeserializeOwned};

pub struct ToolResult<R: Serialize + DeserializeOwned + Send + Sync = serde_json::Value> {
    pub ok: bool,
    pub message: String,
    pub data: R,
    pub images: Vec<ImageData>,
}

impl<R: Serialize + DeserializeOwned + Send + Sync> ToolResult<R> {
    pub fn ok(message: impl Into<String>, data: R) -> Self {
        Self { ok: true, message: message.into(), data, images: Vec::new() }
    }
    pub fn err(message: impl Into<String>, data: R) -> Self {
        Self { ok: false, message: message.into(), data, images: Vec::new() }
    }
    pub fn ok_msg(message: impl Into<String>) -> Self where R: Default {
        Self::ok(message, R::default())
    }
    pub fn err_msg(message: impl Into<String>) -> Self where R: Default {
        Self::err(message, R::default())
    }
    pub fn with_image(mut self, image: ImageData) -> Self {
        self.images.push(image);
        self
    }
    /// 边界序列化：typed → Value
    pub fn to_value(self) -> ToolResult<serde_json::Value> {
        ToolResult {
            ok: self.ok,
            message: self.message,
            data: serde_json::to_value(&self.data).expect("..."),
            images: self.images,
        }
    }
    /// 边界反序列化：Value → typed
    pub fn from_value(v: ToolResult<serde_json::Value>) -> Result<Self, serde_json::Error> {
        Ok(Self {
            ok: v.ok, message: v.message,
            data: serde_json::from_value(v.data)?,
            images: v.images,
        })
    }
    pub fn display_text(&self) -> &str { &self.message }
}
```

**`display_text_from_content` 删除**——其职责由 `display_text()` 替代，`tool.rs:97-107` 函数体整体删除。

**向后兼容层**（保留 1 个版本，下一 release 删除）：

```rust
#[deprecated(note = "use ToolResult::ok() / err() / ok_msg() / err_msg()")]
impl ToolResult<Value> {
    pub fn success(message: impl Into<String>) -> Self { Self::ok_msg(message) }
    pub fn error(message: impl Into<String>) -> Self { Self::err_msg(message) }
    pub fn success_json(value: Value) -> Self {
        let message = value.get("display").or_else(|| value.get("message"))
            .or_else(|| value.get("text"))
            .and_then(|v| v.as_str()).unwrap_or("").to_string();
        Self::ok(message, value)
    }
    pub fn error_json(value: Value) -> Self {
        let message = value.get("message").or_else(|| value.get("display"))
            .or_else(|| value.get("text"))
            .and_then(|v| v.as_str()).unwrap_or("error").to_string();
        Self::err(message, value)
    }
}
```

**commit 模板**：
```
refactor(shared): introduce ToolResult<R> typed-data API

新结构：ToolResult<R> = { ok, message, data: R, images }
- 4-method → 2-method (ok/err) + 2 便捷 (ok_msg/err_msg)
- is_error → ok, output → message, content → data
- display_text_from_content() 删除（display_text() 替代）
- 旧 4 method 标 #[deprecated]，保留 1 个 release

相关 issue: #273, #325
```

### 任务 0.2 — `Tool` trait 加 `type Result` 关联类型

**目标**：每个 tool 必须声明自己的 result struct 类型，编译器保证。

```rust
// agent/features/tools/src/core/tool.rs
#[async_trait]
pub trait Tool: Send + Sync {
    type Result: Serialize + DeserializeOwned + Send + Sync + Debug + 'static;

    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn parameters_schema(&self) -> serde_json::Value;

    async fn call(&self, input: serde_json::Value, ctx: &ToolExecutionContext)
        -> ToolResult<Self::Result>;
}
```

**commit 模板**：
```
feat(tools): add Tool::Result associated type

Tool::call 返回类型从 ToolResult<Value> 改为 ToolResult<Self::Result>
- type Result: Serialize + DeserializeOwned + Send + Sync + Debug + 'static
- 触发下游 29 个 tool impl 改写（任务 0.3 / 0.4）

相关 issue: #273, #325
```

### 任务 0.3 — 迁移 11 个核心 tool 的 typed result struct

**目标**：为 11 个核心 tool 各定义一个 typed struct + 改 `call` 返回 typed。

每个 tool 的 typed struct 字段集已确定（见 Phase B 各 Task），但需在此阶段全部建立，**字段可先空（仅 `Default`）**，Phase B 任务负责填充业务字段。

| Tool | Struct | 必须字段（Phase B 填充） |
|---|---|---|
| Read | `ReadResult` | `line_count: u64`, `file_path: PathBuf`, `truncated: bool`, `offset: Option<u64>`, `limit: Option<u64>` |
| Write | `WriteResult` | `file_path: PathBuf`, `bytes_written: u64` |
| Edit | `EditResult` | `file_path: PathBuf`, `occurrences: usize`, `diff: String` |
| Glob | `GlobResult` | `files: Vec<PathBuf>`, `match_count: usize` |
| Grep | `GrepResult` | `matches: Vec<Match>`, `match_count: usize`, `truncated: bool` |
| WebFetch | `WebFetchResult` | `url: String`, `byte_count: u64`, `char_count: u64`, `truncated: bool` |
| Bash | `BashResult` | `stdout: String`, `stderr: String`, `exit_code: i32`, `signal: Option<i32>` |
| Agent | `AgentResult` | `agent_id: String`, `output: String` |
| EnterWorktree | `EnterWorktreeResult` | `branch: String`, `path_base: PathBuf`, `working_root: PathBuf`, `guidance: String` |
| ExitWorktree | `ExitWorktreeResult` | `branch: String`, `path_base: PathBuf`, `working_root: PathBuf` |
| AskUserQuestion | `AskUserQuestionResult` | `question_type: String`, `question: String`, `options: Vec<Option>` |

**Unit Test 模板**（每个 tool 1 个，覆盖 typed struct 序列化往返）：
```rust
#[test]
fn result_roundtrip() {
    let original = ReadResult { line_count: 5, file_path: "/a/b.rs".into(), truncated: false, offset: None, limit: None };
    let json = serde_json::to_value(&original).unwrap();
    let restored: ReadResult = serde_json::from_value(json).unwrap();
    assert_eq!(restored.line_count, original.line_count);
}
```

**commit 模板**（**11 个独立 commit**，每个 tool 一个）：
```
feat(tools): migrate {ToolName} to ToolResult<ToolResult> with typed data

- 新增 packages/global/types/src/tool_result/{tool_name}.rs 中的 {ToolResult} struct
  （pub struct with Serialize/Deserialize/Debug/Default + Clone）
- agent/features/tools/src/business/{tool_name}.rs 改 use types::tool_result::{ToolResult};
- 改 Tool::Result = {ToolResult} impl
- 改 Tool::call 返回 ToolResult<{ToolResult}>
- 旧 success_json/error_json → ok/err (with #[allow(deprecated)])
- Unit test: result_roundtrip

相关 issue: #273, #325
```

### 任务 0.4 — 迁移 18 个非核心 tool 的 typed result struct

**目标**：18 个非核心 tool（brief/config_tool/lsp/...）也加 typed result，但字段集由各自语义决定（不必与 #273 头部需求绑定）。

每个 tool 一个 commit，**结构同任务 0.3 但字段精简**（只覆盖最小必要字段）：

| Tool | Struct | 字段 |
|---|---|---|
| brief | `BriefResult` | `summary: String` |
| config_tool | `ConfigResult` | `key: String`, `value: Value` |
| lsp | `LspResult` | `diagnostics: Vec<Diagnostic>` |
| memory_tool | `MemoryResult` | `entries: Vec<MemoryEntry>` |
| plan_mode | `PlanModeResult` | `mode: String`, `content: String` |
| sleep | `SleepResult` | `slept_ms: u64` |
| skill_tool | `SkillResult` | `name: String`, `output: String` |
| task_create | `TaskCreateResult` | `task_id: String` |
| task_get | `TaskGetResult` | `task: Task` |
| task_list | `TaskListResult` | `tasks: Vec<Task>` |
| task_stop | `TaskStopResult` | `task_id: String` |
| task_update | `TaskUpdateResult` | `task_id: String`, `status: String` |
| web_search | `WebSearchResult` | `results: Vec<SearchResult>` |
| mcp_tool | `McpToolResult` | `server: String`, `tool: String`, `output: Value` |
| mcp_manager | `McpManagerResult` | `action: String`, `status: String` |
| list_mcp_resources | `ListMcpResourcesResult` | `resources: Vec<McpResource>` |
| read_mcp_resource | `ReadMcpResourceResult` | `uri: String`, `content: String` |
| tool_search | `ToolSearchResult` | `tools: Vec<String>` |

**commit 模板**（**18 个独立 commit**，每个 tool 一个）：
```
feat(tools): migrate {ToolName} to typed ToolResult

- 新增 packages/global/types/src/tool_result/{tool_name}.rs 中的 {ToolResult} struct
- agent/features/tools/src/business/{tool_name}.rs 改 use types::tool_result::{ToolResult};
- 改 Tool::Result = {ToolResult} impl
- 改 Tool::call 返回 ToolResult<{ToolResult}>
- 旧 success_json/error_json → ok/err (with #[allow(deprecated)])

相关 issue: #273, #325, #321（follow-up: 18 工具 Display 接入）
```

### 任务 0.5 — 迁移所有 reader（runtime/compact/persistence/TUI/tests）

**目标**：消除所有 `is_error` / `output` / `content` 旧字段读取，迁移到 `!ok` / `message` / `data`。

| 文件 | 旧读取 | 新读取 |
|---|---|---|
| `agent/features/tools/src/business/{read,write,edit,...}.rs` 单元测试 | `!result.is_error` | `result.ok` |
| 同上 | `result.output.contains(...)` | `result.message.contains(...)` |
| 同上 | `result.content["data"]["subject"]` | `result.data.subject` |
| `agent/features/runtime/src/core/client/event.rs:190-220` | `is_error` 字段 | `ok` 字段 |
| `agent/features/runtime/src/business/compact/truncate.rs:84-152` | `(output, is_error)` tuple | `(message, ok)` tuple |
| `apps/cli/src/tui/render/output/tool_impls.rs` | `result.content.get("data")` | `result.data`（typed 反序列化） |
| `apps/cli/src/tui/model/conversation/tool_result_payload.rs` | `is_error: bool` | `ok: bool` |
| `apps/cli/src/tui/view_assembler/output.rs:495-519` | `result.content.get("branch")` | `result.data.branch`（通过 typed struct 替代） |
| `apps/cli/src/tui/view_assembler/output.rs:527` | `result.content.get("data")` | `result.data`（通过 typed struct 替代） |
| `~/.agents/sessions/*.json` 持久化 | `is_error` 读取 | `ok` 读取（迁移时反转变量） |
| `~/.agents/history.json` 持久化 | 同上 | 同上 |

**持久化 schema 迁移**：
- 旧记录 `{"is_error": false, "output": "...", "content": {...}}` 读取时反转为 `{"ok": true, "message": "...", "data": {...}}`
- 写入新格式（`ok` / `message` / `data`），**不**写 `is_error`/`output`/`content` 旧字段

**commit 模板**（**单一 atomic commit**，覆盖 100+ 读取点）：
```
refactor: migrate all readers from is_error/output/content to ok/message/data

覆盖 100+ 读取点：
- 29 个 tool 单元测试：is_error → ok, output → message, content → data
- runtime event 流：is_error → ok
- compact 算法：(output, is_error) → (message, ok)
- TUI model ToolResultPayload：is_error → ok
- TUI 渲染层：result.content → result.data
- 持久化 schema：旧字段读取适配 → 新字段

持久化向后兼容：旧 history.json/sessions/*.json 仍可读
（读时把 is_error 映射为 !ok，content 映射为 data）

相关 issue: #273, #325
```

### 任务 0.6 — TUI model `ToolResultPayload` 同步

**目标**：模型层 `ToolResultPayload` 同步切换字段。

```rust
// apps/cli/src/tui/model/conversation/tool_result_payload.rs
pub struct ToolResultPayload {
    pub ok: bool,                 // 替代 is_error
    pub message: String,          // 替代 output
    pub data: serde_json::Value,  // 替代 content（边界处是 Value，typed 在 Display 层做）
    pub image_count: usize,
}

pub fn new(ok: bool, message: String, data: serde_json::Value, image_count: usize) -> Self {
    Self { ok, message, data, image_count }
}
```

**commit 模板**：
```
refactor(tui): migrate ToolResultPayload to { ok, message, data, image_count }

- is_error → ok, output → message, content → data
- pub fn new() 同步切换签名
- 跨 crate 调用点（view_assembler/output.rs:425-450）同步更新

相关 issue: #273, #325
```

### 任务 0.7 — Phase 0 完整验证

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --all-targets -- -D warnings

# 持久化 schema 兼容测试：备份 sessions/ → 旧版本 aemeath 生成 → 新版本读
# 验证旧记录仍可读（is_error 自动映射为 !ok）

# TUI 抓取
echo "{prompt}" | script -q /tmp/tui.log cargo run -- -qv
```

### Phase 0 总结

| 维度 | 值 |
|---|---|
| 任务数 | 12（0a.1-0a.5 + 0.1-0.7） |
| atomic commit 数 | 39（4 Phase 0a + 1 + 1 + 11 + 18 + 1 + 1 + 1 verify + 1 聚合） |
| 读取点修改 | 100+（任务 0.5 集中处理） |
| typed struct 数 | 29（11 核心 + 18 非核心） |
| 持久化 schema 迁移 | 旧 `is_error/output/content` 读取自动映射为新 `ok/message/data` |
| 跨 crate 影响 | 6 个（shared/tools/runtime/provider/sdk/apps/cli） |
| 风险等级 | 高（破坏性 API 变更） |
| 缓解 | 旧 method 标 `#[deprecated]` 保留 1 release；持久化读时自动迁移 |

---

## Phase A TUI 渲染层接入（保留原 Tasks 1-4 大部分；typed struct 后部分任务简化）

> **变更（2026-06-18）**：原 Phase A 的 4 个任务（typed-helpers + result_payload 贯穿 + trait 改签 + build_header_line）部分工作被 Phase 0 消除。`data_field_*()` typed helpers **不再需要**——Display 直接通过 `from_value::<ReadResult>(data)` 反序列化后 `.field` 访问。本 Phase A 仍保留 `result_payload` 贯穿 + `format_header_line_with_result` trait 签名 + `build_header_line` helper 这 3 个任务。

### Task 1：在 `tool_display.rs` 中新增类型化字段辅助函数

**文件：**
- 修改：`apps/cli/src/tui/render/output/tool_display.rs`

- [ ] **Step 1：为 3 个辅助函数编写失败测试**

在现有的 `#[cfg(test)] mod tests` 内追加（或在缺失时新建）：

```rust
use crate::tui::model::conversation::tool_result_payload::ToolResultPayload;

fn payload(json: serde_json::Value) -> ToolResultPayload {
    ToolResultPayload::new(String::new(), json, false, 0)
}

#[test]
fn data_field_u64_returns_value_when_present() {
    let p = payload(serde_json::json!({"data": {"line_count": 42}}));
    assert_eq!(super::data_field_u64(Some(&p), "data.line_count"), Some(42));
}

#[test]
fn data_field_u64_returns_none_when_missing() {
    let p = payload(serde_json::json!({"data": {}}));
    assert_eq!(super::data_field_u64(Some(&p), "data.line_count"), None);
}

#[test]
fn data_field_u64_returns_none_when_payload_absent() {
    assert_eq!(super::data_field_u64(None, "data.line_count"), None);
}

#[test]
fn data_field_u64_returns_none_on_wrong_type() {
    let p = payload(serde_json::json!({"data": {"line_count": "42"}}));
    assert_eq!(super::data_field_u64(Some(&p), "data.line_count"), None);
}

#[test]
fn data_field_u64_walks_nested_path() {
    let p = payload(serde_json::json!({"data": {"result": {"match_count": 7}}}));
    assert_eq!(super::data_field_u64(Some(&p), "data.result.match_count"), Some(7));
}

#[test]
fn data_field_i64_normal_and_absent() {
    let p = payload(serde_json::json!({"data": {"exit_code": 0}}));
    assert_eq!(super::data_field_i64(Some(&p), "data.exit_code"), Some(0));
    let p = payload(serde_json::json!({"data": {"exit_code": -1}}));
    assert_eq!(super::data_field_i64(Some(&p), "data.exit_code"), Some(-1));
    assert_eq!(super::data_field_i64(None, "data.exit_code"), None);
}

#[test]
fn data_field_string_normal_and_absent() {
    let p = payload(serde_json::json!({"data": {"branch": "main"}}));
    assert_eq!(
        super::data_field_string(Some(&p), "data.branch").as_deref(),
        Some("main"),
    );
    let p = payload(serde_json::json!({"data": {}}));
    assert_eq!(super::data_field_string(Some(&p), "data.branch"), None);
    assert_eq!(super::data_field_string(None, "data.branch"), None);
}
```

- [ ] **Step 2：运行测试，验证编译失败（辅助函数不存在）**

执行：`cargo test -p cli --lib tui::render::output::tool_display::tests::data_field`
预期：编译错误 `cannot find function data_field_u64`。

- [ ] **Step 3：实现 3 个辅助函数**

在 `tool_display.rs` 中 `inventory::collect!(ToolDisplayEntry)` **之前**插入：

```rust
/// Walk a dotted path through `payload.content` and return the field as `u64`.
pub fn data_field_u64(
    payload: Option<&crate::tui::model::conversation::tool_result_payload::ToolResultPayload>,
    path: &str,
) -> Option<u64> {
    let payload = payload?;
    let mut current: &serde_json::Value = &payload.content;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    current.as_u64()
}

pub fn data_field_i64(
    payload: Option<&crate::tui::model::conversation::tool_result_payload::ToolResultPayload>,
    path: &str,
) -> Option<i64> {
    let payload = payload?;
    let mut current: &serde_json::Value = &payload.content;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    current.as_i64()
}

pub fn data_field_string(
    payload: Option<&crate::tui::model::conversation::tool_result_payload::ToolResultPayload>,
    path: &str,
) -> Option<String> {
    let payload = payload?;
    let mut current: &serde_json::Value = &payload.content;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    current.as_str().map(str::to_string)
}
```

- [ ] **Step 4：运行测试，验证通过**

执行：`cargo test -p cli --lib tui::render::output::tool_display::tests::data_field`
预期：8 个测试通过。

- [ ] **Step 5：提交**

```bash
git add apps/cli/src/tui/render/output/tool_display.rs
git commit -m "feat(tui): add typed helpers for reading structured data fields"
```

---

### Task 2：将 `result_payload` 贯穿到 `ToolCallBlockView`

**文件：**
- 修改：`apps/cli/src/tui/view_model/output.rs:101-115`
- 修改：`apps/cli/src/tui/view_assembler/output.rs:336-449`

- [ ] **Step 1：为 `ToolCallBlockView` 新增字段**

在 `apps/cli/src/tui/view_model/output.rs:101-115` 中新增（与 `result_summary` 并列）：

```rust
pub result_payload: Option<crate::tui::model::conversation::tool_result_payload::ToolResultPayload>,
```

- [ ] **Step 2：查找所有 `ToolCallBlockView {` 构造点**

执行：`grep -rn "ToolCallBlockView {" apps/cli/src/`
所有站点必须初始化新字段（先用 `result_payload: None`）。

- [ ] **Step 3：将私有的 `view_assembler::ToolResultPayload<'a>` 视图替换为模型层拥有的结构体**

将 `find_tool_result_payload`（第 425-449 行）替换为：

```rust
fn find_tool_result_block<'a>(
    conversation: &'a ConversationModel,
    chat_id: &ChatId,
    turn_id: &ChatTurnId,
    tool_id: &ToolCallId,
) -> Option<(
    &'a str,
    &'a serde_json::Value,
    bool,
    usize,
)> {
    conversation.blocks.iter().find_map(|block| match block {
        crate::tui::model::conversation::block::ConversationBlock::ToolResult {
            id, chat_id: cid, turn_id: tid,
            output, content, is_error, image_count,
        } if id == tool_id && cid == chat_id && tid == turn_id => {
            Some((output.as_str(), content, *is_error, *image_count))
        }
        _ => None,
    })
}
```

删除私有 `struct ToolResultPayload<'a>`（第 400-405 行）和 `fn find_tool_result_payload`（第 425-449 行）。

- [ ] **Step 4：在 `find_tool_view`（第 336-398 行）中填充新字段**

```rust
fn find_tool_view(
    conversation: &ConversationModel,
    chat_id: &ChatId,
    turn_id: &ChatTurnId,
    tool_id: &ToolCallId,
) -> Option<ToolCallBlockView> {
    let call = find_tool_call(conversation, chat_id, turn_id, tool_id)?;
    let (icon, semantic_status, style) = map_tool_status(call.status);

    let (result_summary, result_payload) = match call.result.as_deref().filter(|r| !r.is_empty()) {
        Some(result) => match find_tool_result_block(conversation, chat_id, turn_id, tool_id) {
            Some((_output, content, is_error, image_count)) => {
                let owned = crate::tui::model::conversation::tool_result_payload::ToolResultPayload::new(
                    String::new(),
                    content.clone(),
                    is_error,
                    image_count,
                );
                let text = display_text_for_tool_result(Some(&call.name), result, content);
                (Some(text), Some(owned))
            }
            None => (Some(result.to_string()), None),
        },
        None => (None, None),
    };

    Some(ToolCallBlockView {
        key: format!("{}/{}/{}", chat_id.as_ref(), turn_id.as_ref(), tool_id.as_ref()),
        chat_id: Some(chat_id.as_ref().to_string()),
        turn_id: Some(turn_id.as_ref().to_string()),
        tool_call_id: Some(tool_id.as_ref().to_string()),
        title: call.name.clone(),
        icon: icon.to_string(),
        semantic_status,
        style,
        args_preview: (!call.args_preview.is_empty()).then(|| call.args_preview.clone()),
        activity_summary: if matches!(call.status, ToolCallStatus::Success | ToolCallStatus::Error | ToolCallStatus::Cancelled) {
            None
        } else {
            call.activities.last().cloned()
        },
        result_summary,
        result_payload,
        collapsible: true,
        collapsed: false,
    })
}
```

- [ ] **Step 5：验证编译**

执行：`cargo build -p cli`
预期：成功。

- [ ] **Step 6：提交**

```bash
git add apps/cli/src/tui/view_model/output.rs apps/cli/src/tui/view_assembler/output.rs
git commit -m "feat(tui): thread result_payload through ToolCallBlockView"
```

---

### Task 3：更改 `format_header_line_with_result` trait 签名（单个协调提交）

**文件：**
- 修改：`apps/cli/src/tui/render/output/tool_display.rs:73-122, 169-198`
- 修改：`apps/cli/src/tui/render/output/tool_display/tool_impls.rs:99-140, 185-232`
- 修改：`apps/cli/src/tui/render/output/blocks/tool_call.rs:20`

> **为何单次提交：** trait、其 2 个覆写、函数入口和调用点必须同步移动。

- [ ] **Step 1：更改 trait 方法签名**

在 `tool_display.rs:109-115`，替换默认实现：

```rust
fn format_header_line_with_result(
    &self,
    input: &serde_json::Value,
    _result_payload: Option<&crate::tui::model::conversation::tool_result_payload::ToolResultPayload>,
) -> Line<'static> {
    self.format_header_line(input)
}
```

- [ ] **Step 2：更改 `format_tool_call` 签名**

在 `tool_display.rs:169-198`，将 `result_summary: Option<&str>` 参数替换为 `result_payload: Option<&ToolResultPayload>`，并向下传递给 `format_header_line_with_result`。

- [ ] **Step 3：更新 `ReadDisplay::format_header_line_with_result`（去除正则）**

```rust
fn format_header_line_with_result(
    &self,
    input: &serde_json::Value,
    result_payload: Option<&crate::tui::model::conversation::tool_result_payload::ToolResultPayload>,
) -> Line<'static> {
    let path = file_path(input);
    let display_path = truncate_path(path, 60);
    let offset = input.get("offset").and_then(|v| v.as_u64()).unwrap_or(0);
    let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(2000);
    let start = offset + 1;
    let end = offset + limit;
    let count_suffix = data_field_u64(result_payload, "data.line_count")
        .map(|n| format!(" ({n} lines)"));
    let range_info = match count_suffix {
        Some(suffix) => format!("{start}:{end}{suffix}"),
        None => format!("{start}:{end}"),
    };
    Line::from(vec![
        Span::styled(self.display_name().to_string(), Style::default().fg(theme::ACCENT_BRIGHT)),
        Span::raw(" "),
        Span::styled(display_path, Style::default().fg(theme::TEXT)),
        Span::raw(" "),
        Span::styled(range_info, Style::default().fg(theme::TEXT_MUTED)),
    ])
}
```

在 `tool_impls.rs` 顶部添加 `use super::data_field_u64;`。

- [ ] **Step 4：更新 `WriteDisplay::format_header_line_with_result`（去除正则）**

```rust
fn format_header_line_with_result(
    &self,
    input: &serde_json::Value,
    result_payload: Option<&crate::tui::model::conversation::tool_result_payload::ToolResultPayload>,
) -> Line<'static> {
    let path = file_path(input);
    let display_path = truncate_path(path, 60);
    let bytes_suffix = data_field_u64(result_payload, "data.bytes_written")
        .map(|n| format!(" ({n} bytes)"));
    let suffix = bytes_suffix.unwrap_or_default();
    Line::from(vec![
        Span::styled(self.display_name().to_string(), Style::default().fg(theme::ACCENT_BRIGHT)),
        Span::raw(" "),
        Span::styled(display_path, Style::default().fg(theme::TEXT)),
        Span::raw(" "),
        Span::styled(suffix, Style::default().fg(theme::TEXT_MUTED)),
    ])
}
```

- [ ] **Step 5：更新 `blocks/tool_call.rs:20` 中的调用点**

将 `view.result_summary.as_deref()` 替换为 `view.result_payload.as_ref()`，并调整 `format_tool_call` 调用以传入新参数类型。

- [ ] **Step 6：构建 + 测试，验证通过**

执行：`cargo build -p cli && cargo test -p cli --lib tui::render::output`
预期：成功。

- [ ] **Step 7：提交**

```bash
git add apps/cli/src/tui/render/output/
git commit -m "refactor(tui): change format_header_line_with_result to take structured payload"
```

---

### Task 4：在 `tool_impls.rs` 中抽取 `build_header_line` 辅助函数

**文件：**
- 修改：`apps/cli/src/tui/render/output/tool_display/tool_impls.rs`

- [ ] **Step 1：新增辅助函数**（位于 `truncate_path` 之后，第 60-62 行）

```rust
fn build_header_line(name: &str, path: &str, suffix: &str) -> Line<'static> {
    let display_path = truncate_path(path, 60);
    let mut spans = vec![
        Span::styled(name.to_string(), Style::default().fg(theme::ACCENT_BRIGHT)),
        Span::raw(" "),
        Span::styled(display_path, Style::default().fg(theme::TEXT)),
    ];
    if !suffix.is_empty() {
        spans.push(Span::styled(
            suffix.to_string(),
            Style::default().fg(theme::TEXT_MUTED),
        ));
    }
    Line::from(spans)
}
```

- [ ] **Step 2：新增单元测试**

```rust
use super::build_header_line;

#[test]
fn build_header_line_no_suffix() {
    let line = build_header_line("Read", "/foo/bar/baz.txt", "");
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(text, "Read /foo/bar/baz.txt");
}

#[test]
fn build_header_line_with_suffix() {
    let line = build_header_line("Read", "/foo/bar/baz.txt", " (5 lines)");
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(text, "Read /foo/bar/baz.txt (5 lines)");
}

#[test]
fn build_header_line_truncates_long_path() {
    let long = "/very/very/very/long/path/file.txt";
    let line = build_header_line("Read", long, "");
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.starts_with("Read "));
    assert!(text.contains("..."), "expected ellipsis in long path; got: {text}");
    assert!(text.len() < long.len() + 10);
}
```

- [ ] **Step 3：运行测试，验证通过**

执行：`cargo test -p cli --lib tui::render::output::tool_display::tool_impls::tests::build_header_line`

- [ ] **Step 4：提交**

```bash
git add apps/cli/src/tui/render/output/tool_display/tool_impls.rs
git commit -m "refactor(tui): extract build_header_line helper for Display impls"
```

---

## Phase B 工具层（11 核心 tool 字段填充 + Display 接入）

> **Phase 0 后**：本阶段 11 个核心 tool 已有 typed result struct（`ReadResult`/`BashResult`/...），本阶段负责**填充业务字段**（`line_count` / `bytes_written` / `occurrences` / `count` / `match_count` / `exit_code` / `truncated` 等）+ TUI Display 接入。11 个 task 可并行（subagent 派发）。

### Task 5：Read——在 success data 中新增 `line_count/offset/limit`

**文件：**
- 修改：`agent/features/tools/src/business/file_read.rs:94, 101`

- [ ] **Step 1：更新空文件 success 分支（第 94 行）**

```rust
ToolResult::success_json(serde_json::json!({
    "status": "success",
    "message": "(empty file)",
    "data": {
        "content": "",
        "file_path": file_path,
        "line_count": 0,
        "offset": start,
        "limit": end - start,
    }
}))
```

- [ ] **Step 2：更新正常 success 分支（第 101 行）**

```rust
let line_count = end - start;
ToolResult::success_json(serde_json::json!({
    "status": "success",
    "message": format!("Read {} lines from {}", line_count, file_path),
    "data": {
        "content": numbered,
        "file_path": file_path,
        "line_count": line_count,
        "offset": start,
        "limit": end - start,
    }
}))
```

- [ ] **Step 3：新增 `#[cfg(test)] mod tests`**（3 个测试，本文件原本无测试）

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn write_tmp(content: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aemeath-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("file.txt");
        std::fs::write(&path, content).unwrap();
        path
    }

    #[tokio::test]
    async fn read_full_file_exposes_line_count() {
        let path = write_tmp("a\nb\nc\nd\ne");
        let result = ReadTool.execute(serde_json::json!({
            "file_path": path.to_string_lossy(),
        }), dummy_ctx()).await.unwrap();
        let data = result.content.get("data").unwrap();
        assert_eq!(data["line_count"], 5);
        assert_eq!(data["offset"], 0);
        assert_eq!(data["limit"], 2000);
    }
```

- [ ] **Step 4：运行测试，验证通过**

执行：`cargo test -p <tools-crate> business::file_read`
预期：3 个新测试通过。

- [ ] **Step 5：提交**

```bash
git commit -am "feat(tools): expose line_count/offset/limit in ReadTool data"
```

---

### Task 6：Write——迁移至 `success_json`/`error_json`

**文件：**
- 修改：`agent/features/tools/src/business/file_write.rs:52, 63, 78, 92, 103`

- [ ] **Step 1：迁移 success 路径（第 92 行）**

将 `ToolResult::success(data.to_string())` 替换为 `ToolResult::success_json(data)`。data 已经是结构化的。

- [ ] **Step 2：迁移 4 个 error 路径**

对每个 `ToolResult::error(message.to_string())`（第 52、63、78、103 行）：

```rust
ToolResult::error_json(serde_json::json!({
    "status": "error",
    "message": "<message>",
    "data": { "file_path": file_path /* or {} if missing */ },
}))
```

- [ ] **Step 3：新增 `#[cfg(test)] mod tests`**（3 个测试）

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn write_exposes_bytes_written_in_data() {
        let path = std::env::temp_dir().join(format!("aemeath-w-{}.txt", nanos()));
        let result = WriteTool.execute(serde_json::json!({
            "file_path": path.to_string_lossy(),
            "content": "hello world",
        }), dummy_ctx()).await.unwrap();
        let data = result.content.get("data").unwrap();
        assert_eq!(data["bytes_written"], 11);
        assert_eq!(data["file_path"], path.to_string_lossy());
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn write_unicode_bytes_counted_as_utf8() {
        // "你好" = 6 UTF-8 bytes
        let path = std::env::temp_dir().join(format!("aemeath-w-uc-{}.txt", nanos()));
        let result = WriteTool.execute(serde_json::json!({
            "file_path": path.to_string_lossy(),
            "content": "你好",
        }), dummy_ctx()).await.unwrap();
        assert_eq!(result.content["data"]["bytes_written"], 6);
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn write_missing_content_returns_error_json() {
        let result = WriteTool.execute(serde_json::json!({
            "file_path": "/tmp/whatever.txt",
        }), dummy_ctx()).await.unwrap_err();
        assert!(result.content.get("data").is_some());
    }
}
```

- [ ] **Step 4：运行 + 提交**

```bash
cargo test -p <tools-crate> business::file_write
git commit -am "feat(tools): migrate WriteTool to success_json/error_json"
```

---

### Task 7：Edit——迁移至 `success_json`/`error_json`

**文件：**
- 修改：`agent/features/tools/src/business/file_edit.rs:47, 63, 76, 91, 103, 114, 176, 186, 217, 234`

- [ ] **Step 1：迁移 success 路径（第 217 行）**

将 `ToolResult::success(data.to_string())` 替换为 `ToolResult::success_json(data)`。data 已经是结构化的（第 218-228 行包含 `occurrences`）。

- [ ] **Step 2：迁移 9 个 error 站点**

对每个 `ToolResult::error(message)`：

```rust
ToolResult::error_json(serde_json::json!({
    "status": "error",
    "message": "<message>",
    "data": <appropriate shape>,
}))
```

- [ ] **Step 3：现有测试位于 `file_edit.rs:342-413`**（测试 `start_line_of_match` 和 `diff_marker`，不测 data 形状）——应无需修改即可通过。

- [ ] **Step 4：新增 3 个测试**

```rust
#[tokio::test]
async fn edit_success_exposes_occurrences_in_data() {
    let tmp = write_tmp("foo");
    let result = EditTool.execute(serde_json::json!({
        "file_path": tmp.to_string_lossy(),
        "old_string": "foo",
        "new_string": "bar",
    }), dummy_ctx()).await.unwrap();
    assert_eq!(result.content["data"]["occurrences"], 1);
}

#[tokio::test]
async fn edit_replace_all_exposes_total_occurrences() {
    let tmp = write_tmp("foo foo foo");
    let result = EditTool.execute(serde_json::json!({
        "file_path": tmp.to_string_lossy(),
        "old_string": "foo",
        "new_string": "bar",
        "replace_all": true,
    }), dummy_ctx()).await.unwrap();
    assert_eq!(result.content["data"]["occurrences"], 3);
}

#[tokio::test]
async fn edit_error_has_structured_data() {
    let result = EditTool.execute(serde_json::json!({
        "file_path": "/nonexistent/path",
        "old_string": "x",
        "new_string": "y",
    }), dummy_ctx()).await.unwrap_err();
    assert!(result.content.get("data").is_some());
    assert!(result.content["data"].get("file_path").is_some());
}
```

- [ ] **Step 5：运行 + 提交**

```bash
cargo test -p <tools-crate> business::file_edit
git commit -am "feat(tools): migrate EditTool to success_json/error_json (occurrences already in data)"
```

---

### Task 8：Glob——迁移至 `success_json` + 双字段 `count`/`match_count`

**文件：**
- 修改：`agent/features/tools/src/business/glob_tool.rs:72, 83, 93`

- [ ] **Step 1：迁移 success 站点（第 72 行 空集，第 83 行 含匹配）**

将每个 `ToolResult::success(data.to_string())` 替换为 `ToolResult::success_json(data)`。

- [ ] **Step 2：在两个 success 站点的 `count` 旁新增 `match_count`**

对于空分支：

```rust
let data = serde_json::json!({
    "files": files,
    "count": 0,
    "match_count": 0,
});
```

对于含匹配分支：`count` 和 `match_count` 都是 `matches.len()`。

- [ ] **Step 3：迁移 error 站点（第 93 行）**

```rust
ToolResult::error_json(serde_json::json!({
    "status": "error",
    "message": format!("invalid glob pattern: {e}"),
    "data": {},
}))
```

- [ ] **Step 4：新增 3 个测试**

```rust
#[tokio::test]
async fn glob_exposes_match_count_and_count() {
    let tmp = std::env::temp_dir().join(format!("g-{}", nanos()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("a.txt"), "").unwrap();
    std::fs::write(tmp.join("b.txt"), "").unwrap();
    let result = GlobTool.execute(serde_json::json!({
        "pattern": "*.txt",
        "path": tmp.to_string_lossy(),
    }), dummy_ctx()).await.unwrap();
    let data = result.content["data"].clone();
    assert_eq!(data["match_count"], 2);
    assert_eq!(data["count"], 2);
}

#[tokio::test]
async fn glob_empty_match_returns_zero_counts() {
    let tmp = std::env::temp_dir().join(format!("g-{}", nanos()));
    std::fs::create_dir_all(&tmp).unwrap();
    let result = GlobTool.execute(serde_json::json!({
        "pattern": "*.nonexistent",
        "path": tmp.to_string_lossy(),
    }), dummy_ctx()).await.unwrap();
    assert_eq!(result.content["data"]["match_count"], 0);
    assert_eq!(result.content["data"]["files"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn glob_invalid_pattern_returns_structured_error() {
    let result = GlobTool.execute(serde_json::json!({
        "pattern": "[invalid",
    }), dummy_ctx()).await.unwrap_err();
    assert!(result.content.get("data").is_some());
}
```

- [ ] **Step 5：运行 + 提交**

```bash
cargo test -p <tools-crate> business::glob_tool
git commit -am "feat(tools): migrate GlobTool to success_json and add match_count (keep count for compat)"
```

---

### Task 9：Grep——迁移至 `success_json` + 新增 `truncated` 标志

**文件：**
- 修改：`agent/features/tools/src/business/grep.rs:47, 91, 105, 118`

- [ ] **Step 1：迁移 success 站点**——将每个 `ToolResult::success(data.to_string())` 替换为 `ToolResult::success_json(data)`。新增 `truncated` 字段。

- [ ] **Step 2：在第 105 行检测截断**

当前代码执行 `matches.into_iter().take(250).collect()`（第 103-104 行）。确定实际总数：

```rust
let total_count = ...;       // whatever ripgrep returned (or count before truncation)
let shown_matches: Vec<_> = raw_matches.into_iter().take(250).collect();
let shown_count = shown_matches.len();
let truncated = shown_count < total_count;
let data = serde_json::json!({
    "matches": shown_matches,
    "match_count": total_count,
    "truncated": truncated,
});
```

（如果代码库当前使用 `output.matches.len()` 作为 `match_count`，若计算真实计数需要二次扫描，执行者可保留切片计数。）

- [ ] **Step 3：迁移 error 站点（第 47、118 行）**


```rust
ToolResult::error_json(serde_json::json!({
    "status": "error",
    "message": "<message>",
    "data": { "matches": [], "match_count": 0, "truncated": false },
}))
```

- [ ] **Step 4：新增 3 个测试**

```rust
#[tokio::test]
async fn grep_exposes_match_count_and_truncated() {
    let tmp = std::env::temp_dir().join(format!("gr-{}", nanos()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("a.txt"), "foo\nfoo\nfoo\n").unwrap();
    let result = GrepTool.execute(serde_json::json!({
        "pattern": "foo",
        "path": tmp.to_string_lossy(),
    }), dummy_ctx()).await.unwrap();
    let data = result.content["data"].clone();
    assert_eq!(data["match_count"], 3);
    assert_eq!(data["truncated"], false);
}

#[tokio::test]
async fn grep_truncates_at_250_and_marks_truncated() {
    let tmp = std::env::temp_dir().join(format!("gr-big-{}", nanos()));
    std::fs::create_dir_all(&tmp).unwrap();
    let body: String = (0..300).map(|_| "foo\n").collect();
    std::fs::write(tmp.join("a.txt"), &body).unwrap();
    let result = GrepTool.execute(serde_json::json!({
        "pattern": "foo",
        "path": tmp.to_string_lossy(),
    }), dummy_ctx()).await.unwrap();
    let data = result.content["data"].clone();
    assert_eq!(data["truncated"], true);
    assert!(data["matches"].as_array().unwrap().len() <= 250);
    assert_eq!(data["match_count"], 300);
}

#[tokio::test]
async fn grep_no_match_returns_zero_count_not_truncated() {
    let result = GrepTool.execute(serde_json::json!({
        "pattern": "definitely-no-such-thing-xyzzy",
        "path": std::env::temp_dir().to_string_lossy(),
    }), dummy_ctx()).await.unwrap();
    let data = result.content["data"].clone();
    assert_eq!(data["match_count"], 0);
    assert_eq!(data["truncated"], false);
}
```

- [ ] **Step 5：运行 + 提交**

```bash
cargo test -p <tools-crate> business::grep
git commit -am "feat(tools): migrate GrepTool to success_json and expose truncated flag"
```

---

### Task 10：WebFetch——在 2 个 success 站点新增 `byte_count/char_count/truncated`

**文件：**
- 修改：`agent/features/tools/src/business/web_fetch.rs:199, 208, 222, 136, 156, 232, 240`

- [ ] **Step 1：更新 truncated success 站点（第 199 行）**

```rust
let data = serde_json::json!({
    "url": url.as_str(),
    "content": format!("{}...\n\n[truncated, showing first {} chars of {} total]",
                       truncated, truncated.chars().count(), body.chars().count()),
    "byte_count": body.len(),
    "char_count": body.chars().count(),
    "truncated": true,
});
ToolResult::success_json(data)
```

- [ ] **Step 2：更新正常 success 站点（第 208 行）**

```rust
let data = serde_json::json!({
    "url": url.as_str(),
    "content": body.to_string(),
    "byte_count": body.len(),
    "char_count": body.chars().count(),
    "truncated": false,
});
ToolResult::success_json(data)
```

- [ ] **Step 3：更新状态错误站点（第 222 行）——从 `output.stdout` 新增 `byte_count`**

```rust
let data = serde_json::json!({
    "url": url.as_str(),
    "byte_count": output.stdout.len(),
    "truncated": false,
});
ToolResult::error_json(serde_json::json!({
    "status": "error",
    "message": format!("fetch failed: {}", stderr),
    "data": data,
}))
```

- [ ] **Step 4：将其余 4 个 error 站点（第 136、156、232、240 行）迁移至 `error_json`**

```rust
ToolResult::error_json(serde_json::json!({
    "status": "error",
    "message": "<message>",
    "data": { "url": "<url>" /* or {} */ },
}))
```

- [ ] **Step 5：新增测试**（若存在 mock HTTP 服务器；否则记录此缺口并依赖 Task 24 手动 smoke）

```rust
#[cfg(test)]
mod tests {
    // 3 tests, mock HTTP server if available:
    // - short response: byte_count == 11, truncated == false
    // - truncated response: truncated == true
    // - 500 error: data has url
}
```

- [ ] **Step 6：运行 + 提交**

```bash
cargo test -p <tools-crate> business::web_fetch
git commit -am "feat(tools): expose byte_count/char_count/truncated in WebFetchTool data"
```

---

### Task 11：Bash——为 8 处 pre-exec error 站点新增 `data: {}`

**文件：**
- 修改：`agent/features/tools/src/business/bash.rs:70, 78, 87, 117, 251, 272, 292, 388`

- [ ] **Step 1：为每个 `error_json` 调用新增 `data` 字段**

```rust
// line 70 — missing command
ToolResult::error_json(serde_json::json!({
    "status": "error",
    "message": "missing required parameter: command",
    "data": {},
}))

// line 78 — destructive command blocked
ToolResult::error_json(serde_json::json!({
    "status": "error",
    "message": format!("Destructive command blocked ({}): {}...", reason, command),
    "data": { "command": command, "blocked": true },
}))

// line 87 — shell injection blocked
ToolResult::error_json(serde_json::json!({
    "status": "error",
    "message": format!("Shell injection pattern blocked ({}): {}...", reason, command),
    "data": { "command": command, "blocked": true },
}))

// line 117 — spawn error
ToolResult::error_json(serde_json::json!({
    "status": "error",
    "message": format!("failed to execute: {e}"),
    "data": { "command": command, "phase": "spawn" },
}))

// line 251 — cancel
ToolResult::error_json(serde_json::json!({
    "status": "error",
    "message": "[interrupted by user]",
    "data": { "command": command, "phase": "cancel" },
}))

// line 272 — timeout
ToolResult::error_json(serde_json::json!({
    "status": "error",
    "message": format!("command timed out after {}ms", timeout_ms),
    "data": { "command": command, "phase": "timeout", "timeout_ms": timeout_ms },
}))

// line 292 — set_path_base err
ToolResult::error_json(serde_json::json!({
    "status": "error",
    "message": format!("set_path_base failed: {e}"),
    "data": { "phase": "path_base" },
}))

// line 388 — wait err
ToolResult::error_json(serde_json::json!({
    "status": "error",
    "message": format!("failed to execute: {e}"),
    "data": { "phase": "wait" },
}))
```

（每个代码块都必须对照实际当前代码审阅；执行者需匹配字段名、类型和 `serde_json::Value` 形状与周围代码。）

- [ ] **Step 2：新增 3 个测试**

```rust
#[tokio::test]
async fn bash_error_sites_have_data_field() {
    let result = BashTool.execute(serde_json::json!({}), dummy_ctx()).await.unwrap_err();
    assert!(result.content.get("data").is_some());

    let result = BashTool.execute(serde_json::json!({
        "command": "echo hi && rm -rf /",
    }), dummy_ctx()).await.unwrap_err();
    let data = result.content["data"].clone();
    assert_eq!(data["blocked"], true);
    assert!(data.get("command").is_some());
}

#[tokio::test]
async fn bash_success_exposes_exit_code_in_data() {
    let result = BashTool.execute(serde_json::json!({"command": "true"}), dummy_ctx()).await.unwrap();
    let data = result.content["data"].clone();
    assert_eq!(data["exit_code"], 0);
}

#[tokio::test]
async fn bash_failure_exposes_nonzero_exit_code() {
    let result = BashTool.execute(serde_json::json!({"command": "false"}), dummy_ctx()).await.unwrap_err();
    let data = result.content["data"].clone();
    assert_eq!(data["exit_code"], 1);
}
```

- [ ] **Step 3：运行 + 提交**

```bash
cargo test -p <tools-crate> business::bash
git commit -am "feat(tools): ensure BashTool pre-exec error paths include data field"
```

---

### Task 12：Worktree——将 `workspace_context_payload` 字段嵌套到 `data` 下

**文件：**
- 修改：`agent/features/tools/src/business/worktree.rs:50-61, 124, 190, 209`

- [ ] **Step 1：定义新 payload 形状**

```rust
fn workspace_context_payload(branch: Option<&str>, path_base: &str, working_root: &str) -> serde_json::Value {
    serde_json::json!({
        "status": "success",
        "message": "",
        "data": {
            "branch": branch,
            "path_base": path_base,
            "working_root": working_root,
            "guidance": [
                "后续 Read/Edit/Write/Glob/Grep/Bash 请优先使用相对路径。",
                "如果必须使用绝对路径，必须位于当前 working_root 下。",
                "不要继续使用进入 worktree 前的 checkout/main workspace 绝对路径。",
            ],
        }
    })
}
```

- [ ] **Step 2：更新 3 个 success 调用点**

在第 124 行（EnterWorktree success）：
```rust
let mut payload = workspace_context_payload(branch.as_deref(), &path_base, &working_root);
payload["message"] = serde_json::Value::String(format!("已进入 worktree：{display_target}"));
ToolResult::success_json(payload)
```

在第 190 行（ExitWorktree switch）：
```rust
let mut payload = workspace_context_payload(None, &path.to_string_lossy(), &working_root);
payload["message"] = serde_json::Value::String(format!("已切换到：{}", path.display()));
ToolResult::success_json(payload)
```

在第 209 行（ExitWorktree pop）：

```rust
let mut payload = workspace_context_payload(None, &prev.path_base.display().to_string(), &prev.working_root);
payload["message"] = serde_json::Value::String(format!("已退出 worktree，恢复到：{}", prev.path_base.display()));
ToolResult::success_json(payload)
```

- [ ] **Step 3：更新第 232-312 行现有测试**（断言新形状：path_base/working_root 在 `data` 下）

- [ ] **Step 4：新增 2 个测试**

```rust
#[tokio::test]
async fn worktree_success_data_has_branch_path_base_working_root() { /* ... */ }

#[tokio::test]
async fn worktree_success_message_preserved_at_top_level() { /* ... */ }
```

- [ ] **Step 5：运行 + 提交**

```bash
cargo test -p <tools-crate> business::worktree
git commit -am "feat(tools): nest WorktreeTool payload fields under data; keep message at top level"
```

---

### Task 13：AskUserQuestion——迁移至 `success_json`

**文件：**
- 修改：`agent/features/tools/src/business/ask_user.rs:121, 134, 63, 108`

- [ ] **Step 1：迁移两个 success 站点**

```rust
let data = serde_json::json!({
    "status": "success",
    "message": format!("__ASK_USER_SELECT__: {question}"),
    "data": {
        "type": "select",
        "question": question,
        "options": options_filtered,
        "option_count": options_filtered.len(),
    }
});
ToolResult::success_json(data)
```

将相同模式应用于 `__ASK_USER__:` 自由输入分支（第 134 行）。对于 free_input，`option_count: 0`。

- [ ] **Step 2：迁移 2 个 error 站点（第 63、108 行）**

```rust
ToolResult::error_json(serde_json::json!({
    "status": "error",
    "message": "<message>",
    "data": {},
}))
```

- [ ] **Step 3：新增 3 个测试**

```rust
#[tokio::test]
async fn ask_user_select_exposes_option_count() { /* ... */ }

#[tokio::test]
async fn ask_user_free_input_has_no_options() { /* ... */ }

#[tokio::test]
async fn ask_user_empty_question_returns_structured_error() { /* ... */ }
```

- [ ] **Step 4：运行 + 提交**

```bash
cargo test -p <tools-crate> business::ask_user
git commit -am "feat(tools): migrate AskUserQuestion to success_json/error_json (protocol prefix preserved)"
```

---

## Phase C TUI Display 覆写（9 个 *Display 改用 typed 反序列化）

> **Phase 0 后**：原 Phase C 的 9 个 Display 实现从 `result.content.get("X")` 链式访问改为 `from_value::<ToolResult>(data).field` typed 访问。9 个 task 可并行。

### Tasks 14-22：为 9 个 Display 实现新增 `format_header_line_with_result` 覆写

这 9 个 Display 实现当前使用默认的 `format_header_line_with_result`（该实现忽略 result 并调用 `format_header_line(input)`）。完成这些任务后，每个 Display 都将读取结构化 `data` 字段并追加统计后缀。

> **为何拆为 9 个独立提交：** 每个 Display 都较小（约 10-20 行），彼此独立，diff 易于审阅。

**通用模式：**

```rust
fn format_header_line_with_result(
    &self,
    input: &serde_json::Value,
    result_payload: Option<&crate::tui::model::conversation::tool_result_payload::ToolResultPayload>,
) -> Line<'static> {
    let arg = /* per-tool input summary */;
    let suffix = /* per-tool data field read */;
    build_header_line(self.display_name(), &arg, &suffix)
}
```

所有覆写都需在 `tool_impls.rs` 顶部导入 `use super::{build_header_line, data_field_u64};`（可能还需要 `data_field_i64`、`data_field_string`）。

---

### Task 14：EditDisplay

```rust
fn format_header_line_with_result(
    &self,
    input: &serde_json::Value,
    result_payload: Option<&crate::tui::model::conversation::tool_result_payload::ToolResultPayload>,
) -> Line<'static> {
    let path = file_path(input);
    let suffix = data_field_u64(result_payload, "data.occurrences")
        .map(|n| format!(" (Replaced {n})"))
        .unwrap_or_default();
    build_header_line(self.display_name(), path, &suffix)
}
```

```bash
git commit -am "refactor(tui): EditDisplay reads occurrences from structured payload"
```

---

### Task 15：GlobDisplay

```rust
fn format_header_line_with_result(
    &self,
    input: &serde_json::Value,
    result_payload: Option<&crate::tui::model::conversation::tool_result_payload::ToolResultPayload>,
) -> Line<'static> {
    let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
    // Read match_count first, fall back to legacy count for 1 release
    let n = data_field_u64(result_payload, "data.match_count")
        .or_else(|| data_field_u64(result_payload, "data.count"));
    let suffix = n.map(|c| format!(" ({c} files)")).unwrap_or_default();
    build_header_line(self.display_name(), pattern, &suffix)
}
```

```bash
git commit -am "refactor(tui): GlobDisplay reads match_count from structured payload (fallback to count)"
```

---

### Task 16：GrepDisplay

```rust
fn format_header_line_with_result(
    &self,
    input: &serde_json::Value,
    result_payload: Option<&crate::tui::model::conversation::tool_result_payload::ToolResultPayload>,
) -> Line<'static> {
    let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
    let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let arg = format!("{pattern}, path={path}");
    let n = data_field_u64(result_payload, "data.match_count");
    let suffix = n.map(|c| format!(" ({c} matches)")).unwrap_or_default();
    build_header_line(self.display_name(), &arg, &suffix)
}
```

```bash
git commit -am "refactor(tui): GrepDisplay reads match_count from structured payload"
```

---

### Task 17：WebFetchDisplay

```rust
fn format_header_line_with_result(
    &self,
    input: &serde_json::Value,
    result_payload: Option<&crate::tui::model::conversation::tool_result_payload::ToolResultPayload>,
) -> Line<'static> {
    let url = input.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let bytes = data_field_u64(result_payload, "data.byte_count");
    let suffix = bytes.map(|b| format!(" ({b} bytes)")).unwrap_or_default();
    build_header_line(self.display_name(), url, &suffix)
}
```

```bash
git commit -am "refactor(tui): WebFetchDisplay reads byte_count from structured payload"
```

---

### Task 18：BashDisplay

```rust
fn format_header_line_with_result(
    &self,
    input: &serde_json::Value,
    result_payload: Option<&crate::tui::model::conversation::tool_result_payload::ToolResultPayload>,
) -> Line<'static> {
    let command = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
    let exit = data_field_i64(result_payload, "data.exit_code");
    let suffix = match exit {
        Some(0) | None => String::new(),
        Some(code) if code < 0 => format!(" (signal {})", -code),
        Some(code) => format!(" (exit {code})"),
    };
    build_header_line(self.display_name(), command, &suffix)
}
```

```bash
git commit -am "refactor(tui): BashDisplay reads exit_code from structured payload (incl. signals)"
```

---

### Task 19：AgentDisplay

```rust
fn format_header_line_with_result(
    &self,
    input: &serde_json::Value,
    result_payload: Option<&crate::tui::model::conversation::tool_result_payload::ToolResultPayload>,
) -> Line<'static> {
    let description = input.get("description").and_then(|v| v.as_str()).unwrap_or("");
    let target = data_field_string(result_payload, "data.agent_id")
        .unwrap_or_else(|| "?".to_string());
    let arg = format!("{description} -> [{target}]");
    build_header_line(self.display_name(), &arg, "")
}
```

```bash
git commit -am "refactor(tui): AgentDisplay reads agent_id from structured payload"
```

---

### Task 20：EnterWorktreeDisplay

```rust
fn format_header_line_with_result(
    &self,
    input: &serde_json::Value,
    result_payload: Option<&crate::tui::model::conversation::tool_result_payload::ToolResultPayload>,
) -> Line<'static> {
    let branch = data_field_string(result_payload, "data.branch")
        .unwrap_or_else(|| "(default)".to_string());
    let arg = format!("branch={branch}");
    let path_suffix = data_field_string(result_payload, "data.working_root")
        .map(|p| format!(" ({p})"))
        .unwrap_or_default();
    build_header_line(self.display_name(), &arg, &path_suffix)
}
```

```bash
git commit -am "refactor(tui): EnterWorktreeDisplay reads branch/working_root from structured payload"
```

---

### Task 21：ExitWorktreeDisplay

```rust
fn format_header_line_with_result(
    &self,
    input: &serde_json::Value,
    result_payload: Option<&crate::tui::model::conversation::tool_result_payload::ToolResultPayload>,
) -> Line<'static> {
    let path_suffix = data_field_string(result_payload, "data.working_root")
        .map(|p| format!(" (back to {p})"))
        .unwrap_or_default();
    build_header_line(self.display_name(), "", &path_suffix)
}
```

```bash
git commit -am "refactor(tui): ExitWorktreeDisplay reads working_root from structured payload"
```

---

### Task 22：AskUserQuestionDisplay

```rust

fn format_header_line_with_result(
    &self,
    input: &serde_json::Value,
    result_payload: Option<&crate::tui::model::conversation::tool_result_payload::ToolResultPayload>,
) -> Line<'static> {
    let question = input.get("question").and_then(|v| v.as_str()).unwrap_or("");
    let n = data_field_u64(result_payload, "data.option_count");
    let suffix = n.map(|c| format!(" ({c} options)")).unwrap_or_default();
    build_header_line(self.display_name(), question, &suffix)
}
```

```bash
git commit -am "refactor(tui): AskUserQuestionDisplay reads option_count from structured payload"
```

**验证（每个任务）：**

```bash
cargo test -p cli --lib tui::render::output::tool_display::tool_impls::tests
cargo build -p cli
```

---

## Phase D 清理与全量验证

> 收尾阶段：删除旧 4 method 的 `#[deprecated]` 标记（推迟至下下个 release），本阶段仅做 dead code 清理 + 完整验证。

### Task 23：删除 `tool_impls.rs` 中的正则辅助函数

**文件：**
- 修改：`apps/cli/src/tui/render/output/tool_display/tool_impls.rs:155-160, 247-252`

- [ ] **Step 1：验证无剩余调用者**

```bash
grep -rn "parse_line_count_from_message\|parse_bytes_from_message" apps/cli/src/
```

预期：0 命中。

- [ ] **Step 2：删除两个辅助函数**

移除 `parse_line_count_from_message`（第 155-160 行）和 `parse_bytes_from_message`（第 247-252 行）。

- [ ] **Step 3：构建 + 测试**

```bash
cargo build -p cli && cargo test -p cli --lib
```

- [ ] **Step 4：提交**

```bash
git commit -am "refactor(tui): remove unused parse_*_from_message regex helpers"
```

---

### Task 24：最终验证

- [ ] **Step 1：完整 workspace 构建**

```bash
cargo build --workspace
```

- [ ] **Step 2：完整 clippy**

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

- [ ] **Step 3：完整测试套件**

```bash
cargo test --workspace
```

预期：全部通过（包括 Task 1、4、5、6、7、8、9、10、11、12、13 中的新测试）。

- [ ] **Step 4：在真实 TUI 中手动 smoke 测试**

```bash
echo '{"prompt": "Create test.txt, read it, edit it, glob *.txt, grep test, then exit"}' \
  | cargo run --bin aemeath
```

验证 TUI 中的每条 header 行：

| 工具 | 预期 header |
|---|---|
| Write | `Write /path/test.txt (12 bytes)` |
| Read | `Read /path/test.txt 1:5 (5 lines)` |
| Edit | `Edit /path/test.txt (Replaced 1)` |
| Glob | `Glob *.txt (3 files)` |
| Grep | `Grep test, path=. (5 matches)` |
| Bash `true` | `Bash true`（无后缀） |
| Bash `false` | `Bash false (exit 1)` |

若任何行显示 `(2000 lines)` 或正则回退的默认值，则视为失败。

- [ ] **Step 5：标记计划完成**

```bash
git log --oneline feature/tool-display-structured-data ^main
```

预期：分支上有 24 个提交（Task 1、2、3、4、5、6、7、8、9、10、11、12、13、14-22 [9 个提交]、23 —— 共 24 个）。

---

## Self-Review 备注

### 原始评审反馈的覆盖

| 评审人发现 | 解决方案 |
|---|---|
| B1 `as_value()` 不存在 | 替换为 `&payload.content`（Task 1） |
| B2 `ToolResultPayload` 有 4 个字段，测试只用了 1 个 | 所有测试中均使用 `ToolResultPayload::new()` 构造函数（Task 1） |
| B3 `format_tool_call` 返回 `(Line, Vec<String>)` | 代码反映实际签名（Task 3） |
| B4 Task 18 删除了活跃的 `ResultPolicy::Hidden` | Task 18 已替换（现为 EditDisplay）；`ResultPolicy::Hidden` 通过 4 个现有 Display 实现引用；无删除 |
| B5 Task 19 虚构的 spec-code 漂移 | Task 19 现在含义为"AgentDisplay 覆写"；规范已验证与代码一致（无规范变更） |
| B6 trait 改动中途提交破坏 | 所有 trait + 2 个覆写 + 调用点合并为单一原子提交（Task 3） |
| B7 WriteDisplay 已读取 bytes_written | 本计划仅去除正则回退；data 已存在 |
| B8 Bash exit_code 已在 data 中 | 本计划仅为 8 处 pre-exec error 站点新增 `data: {}` |
| B9 WebFetch 有 5 个发射点 | Task 10 枚举了全部 5 个 success + 4 个 error 站点；仅修改 2 个 success 站点 + 1 个状态错误站点 + 4 个 error 迁移 |
| B10 辅助函数使用 `common::truncate_path`（不存在） | 新的 `build_header_line` 位于 `tool_impls.rs`，使用本地私有 `truncate_path`（Task 4） |
| B11 需要 `theme::` 前缀 | `build_header_line` 和 Read/WriteDisplay 覆写中的所有 `Span::styled` 调用均使用 `theme::ACCENT_BRIGHT/TEXT/TEXT_MUTED`（Task 3、4） |
| M1 Glob 重命名会破坏 LLM | 一个版本内采用双字段（Task 8）；渲染器优先读取 `match_count`，回退到 `count`（Task 15） |
| M2 Task 17 仅保留为散文 | 现 Tasks 14-22 每个都有完整代码（9 个实现，9 个独立提交） |
| M3 超出范围的工具未列出 | Out of Scope 部分枚举了 17+ 工具及 `-q` 模式 |
| M4 死代码移除顺序 | Task 23 在删除前显式验证调用者为 0 |
| M5 Trait 默认实现未展示 | Task 3 Step 1 包含新的默认实现函数体 |
| M6 Read `data.limit` 冗余 | 保留为 `end - start`（即 `line_count`）是有意为之——表示实际消费的切片大小 |
| M7 测试覆盖（至少 3 个用例） | 每个工具任务都有 3 个测试（正常 / 边界 / 错误） |
| B12 计划中背景表夸大损害 | 已更新：WriteDisplay 确实读取 bytes_written（仅通过正则）；6 个 Display 实现使用默认（当前不显示统计） |
| m1 `-q` 模式 | 已在 Out of Scope 中列出 |
| m2 MCP 工具 | 已在 Out of Scope 中列出 |
| m3 `parse_count_from_message` 不存在 | Task 23 仅删除实际存在的 2 个辅助函数 |
| m4 LLM 字段重命名 | 双字段方案（Task 8 + Task 15） |
| m5 手动截图脆弱 | Task 24 Step 4 枚举确切预期输出，并拒绝正则回退 `(2000 lines)` |
| m6 用于 pending 状态的 `format_header`/`format_header_line` | 已记录：pending 状态（暂无结果）通过默认实现使用 `format_header_line(input)`，仅渲染输入——无统计后缀 |
| n1 `<tools-crate>` 占位符 | 保留为 `<tools-crate>`；执行者应通过 `grep Cargo.toml` 查找实际 workspace member 名称 |
| n2 `theme::` 前缀 | 计划中所有代码均使用 `theme::ACCENT_BRIGHT/TEXT/TEXT_MUTED` |
| n3 Worktree 路径 vs 分支名 | 执行者应在 Task 24 Step 5 之前验证 |
| n5 冗余 `to_string()` | 已移除；计划在 `Cow` 可接受的地方使用 `name.to_string()`（外观性） |

### 类型一致性

| 类型 | 定义于 | 使用于 |
|---|---|---|
| `data_field_u64(Option<&ToolResultPayload>, &str) -> Option<u64>` | Task 1 | Tasks 3、14、15、16、17、22 |
| `data_field_i64(Option<&ToolResultPayload>, &str) -> Option<i64>` | Task 1 | Task 18 |
| `data_field_string(Option<&ToolResultPayload>, &str) -> Option<String>` | Task 1 | Tasks 19、20、21 |
| `build_header_line(&str, &str, &str) -> Line<'static>` | Task 4 | Tasks 14-22 |
| `ToolCallBlockView.result_payload: Option<ToolResultPayload>` | Task 2 | Task 3 |

### 未完成项（已推迟）

- TUI 中的路径相对化（`working_root` 与 `path_base` 的切换）——独立的 UX 改进，单独 PR。
- `HeaderPolicy` 死代码激活（需要设计 Standard/Compact/CustomIcon 头部渲染变体）——独立重构。
- MCP 动态工具及 17+ 个未注册工具——超出范围（目前无 `ToolDisplay` 注册）。
- `-q`（no_tui）模式——仅使用 `eprintln!`；独立的渲染流水线。
- `ResultRender::Diff` 抽取——独立重构。

### 执行者的风险提示

- Task 10（WebFetch）没有用于 mock HTTP 的测试基础设施。执行者应新增一个最小的 mock 服务器辅助函数，或记录此缺口并依赖 Task 24 手动 smoke。
- Task 9（Grep 截断检测）需要验证当前 `match_count` 的语义。本计划假设 `match_count` 应反映**真实**的 ripgrep 匹配数，而非切片长度。如果代码库无法在不进行二次扫描的情况下确定真实计数，则回退到切片计数并添加注释。
- Tasks 14-22 使用 `display_name()` trait 方法——请验证其返回值可用于 `build_header_line`（即 `&str`）。若不能，则改用 `self.name()`。
- 所有工具测试都使用 `dummy_ctx()`——执行者必须通过阅读 `file_edit.rs:342-413` 和 `bash.rs:475-822` 中的现有测试找到实际的上下文构造模式。
- `worktree.rs:232-312` 中的 EnterWorktree 和 ExitWorktree 测试必须更新为新 payload 形状（Task 12 Step 3）。
- Task 3 需要 `tool_display.rs` 中的 `serde_json::Value` 导入。请验证其已导入；若没有则添加。

---

## 执行交接

计划已保存至 `docs/superpowers/plans/2026-06-18-tool-display-structured-data.md`。共 24 个任务。

**两种执行选项：**

1. **子代理驱动（推荐）**——每个任务派发一个新的子代理，任务之间进行审阅，快速迭代。最适合本计划，因为 Tasks 5-13（工具层）高度独立；Tasks 14-22（渲染器）共享辅助函数（Task 1 + Task 4）但除此之外相互独立。

2. **内联执行**——在本会话中按 executing-plans 执行任务，使用 checkpoint 进行批处理。

**建议阶段划分：**

- **阶段 A（基础）**：Tasks 1、2、3、4——必须串行（每个都构建在前者之上；Task 3 是唯一的"大爆炸"原子提交）
- **阶段 B（工具层）**：Tasks 5-13——可并行（9 个独立的工具修改）
- **阶段 C（渲染器覆写）**：Tasks 14-22——可并行（9 个独立的 Display 实现）
- **阶段 D（清理）**：Tasks 23、24——串行

哪种方式？

---

## 计划修订历史

### 修订 1（2026-06-18）—— 第 3 次评审后

**Major #1 修复 —— Task 12 step 2.5（新增）：为 Worktree branch 字段同步 `display_text_for_tool_result`**

在 Task 12 中将 `branch`/`path_base`/`working_root`/`guidance` 从顶层移入 `data` 子对象后，`view_assembler/output.rs:495-519` 的函数仍然读取 `content.get("branch")`（顶层），这将返回 `None`，导致 Worktree 结果子块从 `"已进入 worktree：xxx\n当前分支：main"` 退化为仅 `"已进入 worktree：xxx"`。

**作为 Task 12 Step 2.5 插入**（在 Step 2 的 3 个调用点之后，Step 3 之前）：

```rust
// In apps/cli/src/tui/view_assembler/output.rs, around line 508-511
// OLD: reads top-level branch field
let branch = content
    .get("branch")
    .and_then(|value| value.as_str())
    .filter(|value| !value.is_empty());

// NEW: reads branch from data sub-object (matches Task 12's new payload shape)
let branch = content
    .get("data")
    .and_then(|d| d.get("branch"))
    .and_then(|value| value.as_str())
    .filter(|value| !value.is_empty());
```

此变更随 Task 12 的 payload 迁移一同提交。

---

**Major #2 修复 —— Tasks 6/7/8/9：副作用说明（潜在 bug 修复）**

Tasks 6-9 中的 `success(str) → success_json(value)` 迁移不仅仅是传输层变更——它是一项**潜在 bug 修复**。

**`agent/shared/src/tool.rs:65-72` 的当前代码**将输入字符串包装为 `content: {"text": <str>}`，同时将原始字符串放入 `output`。这意味着下游 `display_text_for_tool_result`（及类似读取者）尝试 `content.get("data")` 始终返回 `None` —— Edit 的 `data.diff` **当前从未被渲染**，Glob 的 `count`、Grep 的 `match_count`、Write 的 `bytes_written`、Bash 的 `exit_code` 也存在同样情况。

**`tool.rs:75-83` 的 `success_json(value)`** 将值按原样存入 `content`，并通过 `display_text_from_content(&content)`（第 97-107 行）派生 `output`，按顺序提取 `display` / `message` / `text`。这是**正确的语义**。

**因此：本计划的 Tasks 6-9 不仅是暴露新字段——它们是为这些工具启用 TUI 对 `data.X` 的数据驱动读取。读取 `content.get("data").and_then(|d| d.get("X"))` 的旧 display_text_for_tool_result 调用点（例如 `view_assembler/output.rs:527` 中 Edit 的 `data.diff`）在迁移后将开始正常工作。**

**在 Task 6 顶部添加副作用说明**（在 Tasks 7、8、9 中复制为提示）：

> ⚠️ **副作用：** 将 `ToolResult::success(json!({...}).to_string())` 迁移至 `ToolResult::success_json(json!({...}))` 会将 `ToolResult.output` 从字符串化 JSON 改为通过 `display_text_from_content`（`agent/shared/src/tool.rs:97-107`）提取的 display/message 文本。这是**正确**的语义；旧的 `success(str)` 形式是一个潜在 bug，因为 `content` 变为 `{"text": <stringified_json>}`，且 `content.get("data")` 始终返回 `None`。本次迁移后，读取 `content.get("data").and_then(|d| d.get("X"))` 的下游 TUI 消费者（例如 `view_assembler/output.rs:527` 中 Edit 的 `data.diff`）将开始返回真实值。
>
> **Task 6 完成后验证：** 运行 `cargo test -p cli --lib`（不仅是 tools crate）以确保没有 TUI 测试断言旧的 `output` 形状。

---

**Minor #1 修复 —— Task 5 Step 3 dummy_ctx() 澄清**

`dummy_ctx()` 在 `file_read.rs` 中未定义。执行者必须：

1. 阅读 `file_edit.rs:342-413`（`#[cfg(test)] mod tests`）中的测试 harness 模式
2. 复制 `test_ctx`（或等价的 `dummy_ctx`）辅助函数定义
3. 如果现有测试文件的辅助函数是私有的，则复制或抽取一个共享的测试工具到公共测试模块——但仅当 2+ 个工具文件需要时才这样做（否则内联）

将此指引作为注释添加到 Task 5 Step 3。

---

**因此：本计划的 Tasks 6-9 不仅仅是暴露新字段——它们正在启用 TUI 对这些工具的 `data.X` 数据驱动读取。读取 `content.get("data")` 的旧 `display_text_for_tool_result` 调用点（例如 `view_assembler/output.rs:527` 处的 Edit 的 `data.diff`）在迁移后将开始工作。**

**Side-effect note 已添加到 Task 6 顶部**（在 Tasks 7、8、9 中作为 callout 复制）：

> ⚠️ **副作用：** 将 `ToolResult::success(json!({...}).to_string())` → `ToolResult::success_json(json!({...}))` 会将 `ToolResult.output` 从字符串化的 JSON 改为通过 `display_text_from_content`（`agent/shared/src/tool.rs:97-107`）提取的 display/message 文本。这是**正确**的语义；旧的 `success(str)` 形式是一个潜在 bug，因为 `content` 变成 `{"text": <stringified_json>}`，且 `content.get("data")` 始终返回 `None`。迁移后，下游读取 `content.get("data").and_then(|d| d.get("X"))` 的 TUI 消费者（例如 `view_assembler/output.rs:527` 处的 Edit 的 `data.diff`）将开始返回真实值。
>
> **Task 6 完成后验证：** 运行 `cargo test -p cli --lib`（不仅仅是 tools crate），以确保没有 TUI 测试断言旧的 `output` 形状。

---

**Minor #1 修复 —— Task 5 Step 3 dummy_ctx() 澄清**

`dummy_ctx()` 在 `file_read.rs` 中未定义。执行者必须：

1. 阅读 `file_edit.rs:342-413`（`#[cfg(test)] mod tests`）中的测试 harness 模式
2. 复制 `test_ctx`（或等价的 `dummy_ctx`）辅助函数定义
3. 如果现有测试文件的辅助函数是私有的，则复制或抽取一个共享的测试工具到公共测试模块——但仅当 2+ 个工具文件需要时才这样做（否则内联）

将此指引作为注释添加到 Task 5 Step 3。

---

**Minor #2 修复 —— Task 9 Step 2 显式回退**

将 `let total_count = ...;` 替换为显式指引：

```rust
// 首先检查 ripgrep 的输出对象是否有 total count 字段
// （例如 `output.line_count`、`output.total` 等——通过阅读
//  本代码库使用的 ripgrep 封装来验证）。如果有，使用它。
// 否则，设置 `let total_count = raw_matches.len();`（截断
// 之前的数量）——渲染器随后将根据
// `shown_matches.len() < total_count` 显示 "X matches" 并
// 附带 `truncated: true`。
```

---

**Minor #3 修复 —— Task 11 Step 1 措辞**

将"Migrate each `error_json` call to include `data` field"改为"**Add `data` field to each existing `error_json` call**（无传输变更；bash.rs:70/78/87 等已使用 `error_json`）"。

---

**Nit #1 修复 —— `<tools-crate>` 占位符**

实际的 tools crate 是 **`agent-features-tools`**（路径：`agent/features/tools/`）。Task 5-13 提交示例中的所有 `<tools-crate>` 都应替换为 `agent-features-tools`。

---

**Task 24 Step 4（手动 smoke）新增验证：**

验证先前失败的情况现在可以正常工作：

| 工具 | 迁移前（TUI 显示） | 迁移后（TUI 应显示） |
|---|---|---|
| Edit | 结果子块：仅 `Replaced 1 occurrence(s) in /path` | `Replaced 1 occurrence(s) in /path\n---DIFF:LINE:N---\n...\n---DIFF:LINE:N---\n...` |
| Bash `false` | 子块：stdout 文本 + `Command failed: ...` | 子块：stdout 文本 + `Command failed: ...\nexit 1`（若被 kill 则为 `signal 9`） |
| Glob | 子块：仅文件列表 | 子块：`Found 5 files`（保留 message）+ 文件列表 |

若任何"迁移后"列退化为"迁移前"形式，则视为失败。

---

## 关联 Issue 与 Follow-up

### 父 Issue

- **#273** — Tool call header 应当从结构化数据读取 `(N lines)` / `(N bytes)` 等统计（issue 发起时的根需求）

### Follow-up Issues（已知设计债，本 plan 不实施）

| Issue | 标题 | 顺序 | 备注 |
|---|---|---|---|
| **#321** | feat(tui): 为 18 个未注册工具补充 ToolDisplayEntry | #273 之后 | 复用本 plan 提取的 helpers（`build_header_line`、`data_field_*`） |
| **#322** | feat(tui): 激活 HeaderPolicy 死代码 | #321 之后 | 死代码激活，对 29 个工具统一生效 |
| **#323** | feat(tui): tool call header 路径优化 | #321 之后 | 抽取 `relative_path_for_display` helper |
| **#324** | refactor(tui): 抽取 ResultRender::Diff 与 max_lines 参数化 | #321 之后 | 纯重构，不改行为 |
| **#325** | refactor(shared): 统一 ToolResult 为扁平 { ok, message, data } | **决策：#273 之后** | 见下方"执行顺序决策" |

### 执行顺序决策（2026-06-18）

**问题**：`ToolResult::success_json` / `success(str)` / `error` / `error_json` 4 个 method 设计不对称——`success(str)` 把字符串包成 `content = {"text": str}`，导致下游 `content.get("data")` 永远 None。Issue #325 提议统一为 `ToolResult::ok(message, data)` 扁平结构（消除该 latent bug）。

**决策**：先实施本 plan（#273），再做 #325。

**理由**：
1. **风险隔离**：#273 范围（11 个 tool 的 `data` 字段填充 + 11 个 `*Display`）< #325 范围（100 处 `success_json/error_json` 调用 + 6 个 crate + 持久化 schema）
2. **业务价值优先**：#273 验证了"结构化数据驱动 TUI header"的产品价值，再做 #325 重构时方向更明确
3. **可回滚性**：#273 在 #325 之前失败，回滚成本低；#325 在 #273 之前失败，影响 #273 落地
4. **latent bug 部分修复**：#273 实施后，`success_json` 形式的 tool 都能正确读 `data.*`；`success(str)` 形式的 latent bug 仍存在，由 #325 统一消除

**跨 issue 影响**：
- #321 / #322 / #323 / #324 在 #325 实施后实现更简洁（无需考虑 4-method 适配）
- #325 实施时，本 plan 的 Side-effect note 中描述的 latent bug 彻底消除
- 本 plan 的 24 个 task 全部使用 `success_json` 形式（**显式记录在每个 task 的 commit 模板中**），不调用 `success(str)`/`error(str)`/`error_json` 的旧形式
