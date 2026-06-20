# ToolResult API 彻底收敛（#392 S2：A+B+C）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把三层工具结果类型（`TypedToolResult<T>` / `ToolResult<R>` / `ToolOutcome`）收敛为统一语义 `text→LLM / data→TUI`，字段名对齐、删冗余构造与死字段、消除从 `content` 猜测显示文本的 hack。

**Architecture:** S1（commit `8eeeda0b`，已合入本分支）已删 `TypedToolResult::success_msg` 并迁移 28 处调用点，修复 Read/Grep 内容丢失。本 S2 做字段层收敛：
- **A（字段改名）**：`TypedToolResult.output` → `text`，与 `ToolOutcome.text` 对齐。
- **B（删冗余构造）**：删 `ToolResult::success_json` / `error_json`（零生产调用）+ `text()`/`json()` 内部构造器。
- **C（中间层塌缩）**：删死字段 `ToolResult.data: Option<R>`（`with_data` 0 调用）；`ToolResult.content` → `data` 改名（与 `ToolOutcome.data` 对齐）；删 `display_text_from_content` 兜底 hack（typed 路径显示文本一律走 `output/text` 字段，不从 `content` 反推）。

收敛后链路（唯一方向）：

```
① TypedToolResult<T>{ text, data, is_error, images }   （工具作者）
   └ adapter: data = serialize(typed)
② ToolResult{ text, data, is_error, images }           （类型擦除中间态）
   └ from_tool_result: text=r.text, data=r.data
③ ToolOutcome{ text, data, is_error, images }           （runtime 执行态）
     text → to_llm_view → LLM
     data → TUI / message content
```

**Tech Stack:** Rust workspace（tools / shared / runtime / cli crates）；验证靠 `cargo check` / `cargo clippy` / 各 crate 单测 + 非交互 CLI 实测。

---

## 范围与前置事实（已核实）

| 字段 / 方法 | 现状 | 处置 |
|---|---|---|
| `TypedToolResult.output` | LLM 文本 | 改名 `text` |
| `TypedToolResult.data: Option<T>` | typed 数据（bash 等用） | 保留 |
| `ToolResult.output` | LLM 文本 | 改名 `text` |
| `ToolResult.content` | typed JSON / MCP `{text}` / offload 后 `{text}` | 改名 `data` |
| `ToolResult.data: Option<R>` | **死字段**（`with_data` 0 调用，构造器设 None） | **删除** |
| `ToolResult::success_json` / `error_json` | 0 生产调用，仅 4 单测 | 删定义 + 删测试 |
| `display_text_from_content` | 从 content 猜 display/message/text（typed 的 hack 兜底） | 删除；显示文本统一走 `text` 字段 |
| `ToolOutcome` | `{ text, data, is_error, images }` | 不变（已是目标形态） |
| `loop_run.rs:140` `outcome.data = {text:...}` | offload 改写 | 保留语义 |
| MCP 工具（read_mcp/mcp_manager/list_mcp/mcp_tool） | 非 typed，用 `ToolResult::success(text)` | 不变 |

**关键风险**：`ToolResult` **不 derive Serialize/Deserialize**（仅 `Debug, Clone`），不参与持久化/wire。真正序列化的是 `share::message::ContentBlock::ToolResult`（独立结构）。字段改名**不破坏 session 文件**。

**架构守卫**：改动涉及 `agent/shared/**`、`agent/features/tools/**`、`apps/cli/**`、`agent/features/runtime/**`。加载分片：`specs/rust-coding.md`（横切）、`specs/tools.md`、`specs/tui-cli.md`、`specs/runtime.md`。

---

## File Structure

修改文件清单（按依赖顺序，下游先改会被上游破坏，故自底向上：定义层 → 消费层）：

| 文件 | 责任 | 改动 |
|---|---|---|
| `agent/features/tools/src/contract/tool.rs` | `TypedToolResult` 定义 + adapter | A: 字段改名；C: adapter 写 `data` |
| `agent/shared/src/tool.rs` | `ToolResult` / `ToolOutcome` / `display_text_from_content` | B+C: 删冗余、改名、删 hack |
| `agent/features/tools/src/business/*.rs` | 工具实现 + 测试的 `.output` | A: 改名引用 |
| `agent/features/runtime/src/business/chat/looping/*.rs` | ToolOutcome 消费、message 组装 | 适配字段名 |
| `apps/cli/src/tui/view_model/conversation/tool_result_payload.rs` | TUI view_model payload | C: `.content` → `.data` |
| `apps/cli/src/tui/view_assembler/output.rs` | model→view_model 组装 | C: 字段名 |
| `apps/cli/src/tui/render/display/render.rs` | `tool_result_content_to_string` 等 3 helper | C: 入参改名 / 简化 |
| `apps/cli/src/tui/render/output/tool_display*.rs` | TUI typed 显示 | C: `payload.content` → `.data` |

---

## Task 1: `TypedToolResult.output` → `text`（A，定义层 + tools crate）

**Files:**
- Modify: `agent/features/tools/src/contract/tool.rs:64-93`（struct + success/error）
- Modify: `agent/features/tools/src/contract/tool.rs:195-205`（adapter）
- Modify: `agent/features/tools/src/business/*.rs`（测试 `.output` 引用，~15 处）

- [ ] **Step 1: 改 `TypedToolResult` struct + 构造器**

`agent/features/tools/src/contract/tool.rs`，把：

```rust
pub struct TypedToolResult<T: Serialize + Send + 'static> {
    /// 文本输出（TUI 显示 + 发给 LLM）
    pub output: String,
    /// 结构化数据（adapter 自动序列化为 JSON）
    pub data: Option<T>,
    pub is_error: bool,
    pub images: Vec<ImageData>,
}

impl<T: Serialize + Send + 'static> TypedToolResult<T> {
    pub fn success(output: impl Into<String>, data: T) -> Self {
        Self {
            output: output.into(),
            data: Some(data),
            is_error: false,
            images: vec![],
        }
    }

    pub fn error(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            data: None,
            is_error: true,
            images: vec![],
        }
    }
```

改为：

```rust
pub struct TypedToolResult<T: Serialize + Send + 'static> {
    /// 给 LLM 的文本（经 `to_llm_view` text-first 投影）。
    pub text: String,
    /// 结构化数据（adapter 自动序列化为 JSON，给 TUI）。
    pub data: Option<T>,
    pub is_error: bool,
    pub images: Vec<ImageData>,
}

impl<T: Serialize + Send + 'static> TypedToolResult<T> {
    pub fn success(text: impl Into<String>, data: T) -> Self {
        Self {
            text: text.into(),
            data: Some(data),
            is_error: false,
            images: vec![],
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            data: None,
            is_error: true,
            images: vec![],
        }
    }
```

- [ ] **Step 2: 改 adapter `output: result.output` → `output: result.text`**

同一文件 `tool.rs:202`，adapter 把 `TypedToolResult` 映射到 `ToolResult`：

```rust
        ToolResult {
            output: result.output,
            content,
            is_error: result.is_error,
            images: result.images,
            data: None,
        }
```

改为（此处先只改字段源，`ToolResult` 字段名在 Task 3 再改）：

```rust
        ToolResult {
            output: result.text,
            content,
            is_error: result.is_error,
            images: result.images,
            data: None,
        }
```

- [ ] **Step 3: 批量改 tools crate 内测试 `.output` → `.text`**

用脚本（见 Step 4 命令）定位并替换 `agent/features/tools/src/business/*.rs` 与 `*_tests.rs` 中 `result.output` / `first.output` 等 `TypedToolResult` 实例的 `.output` 访问为 `.text`。注意排除 `.output()`（方法调用，如 rg 的 `.output().await`）和 `stdout`/`process` 等无关 `.output`。

需改的已知点（来自 S1 调查）：
- `task_list_complete.rs:107` `result.output.contains(...)`
- `task_list_create.rs:107,124,167,184`
- `file_edit.rs:367` / `file_edit_tests.rs:64`
- `bash.rs:524,526,531,652,654,796,798,801,803`
- `memory_tool/tests.rs`（若涉及 TypedToolResult）

- [ ] **Step 4: 编译验证（tools crate）**

Run: `cargo check --manifest-path agent/features/tools/Cargo.toml --all-targets 2>&1 | grep -E "^error" | head -20`
Expected: 空（无 error）。若有遗漏的 `.output`，按报错逐一改为 `.text`，重跑直到无 error。

- [ ] **Step 5: tools crate 测试**

Run: `cargo test --manifest-path agent/features/tools/Cargo.toml 2>&1 | grep "test result:"`
Expected: `ok. 110 passed`（与 S1 基线一致）。

- [ ] **Step 6: 提交**

```bash
git add -A
git commit -m "refactor(tools): TypedToolResult.output → text 字段重命名 (#392 A)"
```

---

## Task 2: 删 `ToolResult` 冗余构造（B）

**Files:**
- Modify: `agent/shared/src/tool.rs:176-214`（删 success_json/error_json/text/json 定义）
- Modify: `agent/shared/src/tool.rs:260-290`（删对应 4 个单测）

- [ ] **Step 1: 确认零生产调用**

Run:
```bash
grep -rn "success_json\|error_json" agent/ apps/ packages/ --include="*.rs" \
  | grep -v "target/\|fn success_json\|fn error_json\|//\|test\|#\[" | grep -v "tool.rs"
```
Expected: 空（无生产调用）。若有，先迁移为 `success(output)` / `error(output)` 再继续。

- [ ] **Step 2: 删 `success_json` / `error_json` / `json()` 方法**

`agent/shared/src/tool.rs`，删除：

```rust
    pub fn success_json(content: serde_json::Value) -> Self {
        Self::json(content, false)
    }

    pub fn error_json(content: serde_json::Value) -> Self {
        Self::json(content, true)
    }

    pub fn json(content: serde_json::Value, is_error: bool) -> Self {
        let output = display_text_from_content(&content);
        Self {
            output,
            content,
            is_error,
            images: Vec::new(),
            data: None,
        }
    }
```

`text()` 构造器暂保留（MCP 工具的 `success`/`error` 依赖它，Task 3 再随字段改名一起处理）。

- [ ] **Step 3: 删对应单测**

删除 `tool.rs` 测试模块中引用 `success_json`/`error_json` 的测试：
- `test_tool_result_json_prefers_display_text`
- `test_tool_result_json_falls_back_to_message_text_or_serialized_json`（该测含 `success_json`/`error_json`）

保留 `test_tool_result_success_wraps_text_payload`（它测 `success`，但断言 `result.content == json!({"text":"ok"})` —— Task 3 改名后需更新，此处先不动）。

- [ ] **Step 4: 编译 + 测试**

Run: `cargo check --manifest-path agent/shared/Cargo.toml --all-targets 2>&1 | grep -E "^error" | head`
Expected: 空。

Run: `cargo test --manifest-path agent/shared/Cargo.toml 2>&1 | grep "test result:"`
Expected: `ok`（删了 2 测试，从 214 降到 212 左右）。

- [ ] **Step 5: 提交**

```bash
git add -A
git commit -m "refactor(shared): 删 ToolResult::success_json/error_json 冗余构造 (#392 B)"
```

---

## Task 3: `ToolResult` 字段塌缩（C 核心：删死字段 + content→data）

**Files:**
- Modify: `agent/shared/src/tool.rs:160-230`（struct 字段 + 构造器 + display_text_from_content）
- Modify: `agent/features/tools/src/contract/tool.rs:195-205`（adapter）
- Modify: `agent/features/runtime/src/business/chat/looping/tools.rs:247,271`（消费 .data）

- [ ] **Step 1: 重构 `ToolResult` struct + 构造器**

`agent/shared/src/tool.rs`，把：

```rust
#[derive(Debug, Clone)]
pub struct ToolResult<R = serde_json::Value> {
    pub output: String,
    pub content: serde_json::Value,
    pub is_error: bool,
    pub images: Vec<ImageData>,
    pub data: Option<R>,
}
```

改为（删死字段 `data: Option<R>`，`output`→`text`，`content`→`data`；泛型参数 `R` 不再需要，回归非泛型）：

```rust
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// 给 LLM 的文本。
    pub text: String,
    /// 给 TUI / 持久化的结构化数据。
    pub data: serde_json::Value,
    pub is_error: bool,
    pub images: Vec<ImageData>,
}
```

> **设计决策（adapter 填充语义）**：typed 路径下 adapter 把 `serialize(typed)` 写入 `data`；MCP 路径 `success(text)` 下 `data = {text}`（沿用旧行为，保持 wire 兼容）。

- [ ] **Step 2: 重写构造器**

删除 `text()` / `with_data()` / `display_text()` / `display_text_from_content()`，替换为：

```rust
impl ToolResult {
    pub fn success(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            data: serde_json::json!({ "text": text.clone() }),
            text,
            is_error: false,
            images: Vec::new(),
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            data: serde_json::json!({ "text": text.clone() }),
            text,
            is_error: true,
            images: Vec::new(),
        }
    }

    pub fn with_image(mut self, base64: String, media_type: String) -> Self {
        self.images.push(ImageData { base64, media_type });
        self
    }
}
```

并删除文件底部 `fn display_text_from_content(...)` 整个函数。

> MCP 工具此前用 `ToolResult::success(json_string)`，新 `success(text)` 把整个 JSON 字符串塞进 `text` 与 `data.text` —— 与旧行为等价（旧 `text()` 构造器 content=`{text:json_string}`，output=同串）。验证点见 Step 6。

- [ ] **Step 3: 更新 `from_tool_result`**

```rust
    pub fn from_tool_result(r: ToolResult) -> Self {
        Self {
            text: r.text,
            data: r.data,
            is_error: r.is_error,
            images: r.images,
        }
    }
```

（字段名一一对应，最简。）

- [ ] **Step 4: 更新 adapter**

`agent/features/tools/src/contract/tool.rs:195-205`：

```rust
        let content = match &result.data {
            Some(data) => serde_json::to_value(data)
                .expect("TypedToolAdapter: data serialization should not fail"),
            None => Value::Null,
        };
        ToolResult {
            output: result.text,
            content,
            is_error: result.is_error,
            images: result.images,
            data: None,
        }
```

改为：

```rust
        let data = match &result.data {
            Some(d) => serde_json::to_value(d)
                .expect("TypedToolAdapter: data serialization should not fail"),
            None => serde_json::json!({ "text": &result.text }),
        };
        ToolResult {
            text: result.text,
            data,
            is_error: result.is_error,
            images: result.images,
        }
```

> typed 成功路径 `data` = 序列化的 typed struct（如 `{content, file_path, line_count, ...}`）；typed 错误路径（bash error 带 data 时 data 非 None，同前）。`None` 兜底用 `{text}` 保 MCP 一致形态。

- [ ] **Step 5: 更新 runtime 消费点**

`agent/features/runtime/src/business/chat/looping/tools.rs`，`ToolResult` 被 `from_tool_result` 消费后是 `ToolOutcome`（字段已叫 `text`/`data`，无需改）。但 `tool_results_for_api` 直接读 `ToolResult` 字段处需检查。搜索：

```bash
grep -rn "\.output\b\|\.content\b" agent/features/runtime/src/ --include="*.rs" \
  | grep -i "tool_result\|result\.\|outcome" | grep -v "tokens\|stdout\|//\|test"
```

把 `ToolResult` 实例的 `.output`→`.text`、`.content`→`.data`。已知 `tools.rs:247`（`content: execution.outcome.data.clone()`）已是 outcome，无需改。

- [ ] **Step 6: 修 `from_tool_result` 关联测试**

`tool.rs` 测试模块，更新 `test_tool_result_success_wraps_text_payload`：

```rust
    #[test]
    fn test_tool_result_success_wraps_text_payload() {
        let result = ToolResult::success("ok");
        assert_eq!(result.text, "ok");
        assert!(!result.is_error);
        assert_eq!(result.data, serde_json::json!({ "text": "ok" }));
    }
```

补充 `from_tool_result` 映射测试（若现有 `test_tool_outcome_from_tool_result_maps_fields` 引用旧字段名，更新为 `text`/`data`）。

- [ ] **Step 7: 编译（workspace，预期下游 TUI 报错）**

Run: `cargo check --workspace --all-targets 2>&1 | grep -E "^error" | head -30`
Expected: cli crate 报错（`ToolResultPayload.content` / `result.content` 等旧字段引用）—— Task 4 修复。tools/runtime/shared 应已通过。

- [ ] **Step 8: 提交（shared + runtime 层）**

```bash
git add -A
git commit -m "refactor(shared+runtime): ToolResult 字段塌缩 content→data + 删死字段 (#392 C)"
```

---

## Task 4: TUI 层适配（C 下游：view_model + assembler + render）

**Files:**
- Modify: `apps/cli/src/tui/view_model/conversation/tool_result_payload.rs`（`.content`→`.data`）
- Modify: `apps/cli/src/tui/view_assembler/output.rs:100-120,353,430`
- Modify: `apps/cli/src/tui/render/display/render.rs:118-121,315-360`
- Modify: `apps/cli/src/tui/render/output/tool_display.rs:113,586-620`
- Modify: `apps/cli/src/tui/render/output/tool_display/tool_impls.rs:14-22`（`payload.content`→`.data`）

- [ ] **Step 1: view_model `ToolResultPayload.content` → `.data`**

`apps/cli/src/tui/view_model/conversation/tool_result_payload.rs`：

```rust
pub struct ToolResultPayload {
    pub output: String,
    pub content: Value,
    pub is_error: bool,
    pub image_count: usize,
}
```

改为：

```rust
pub struct ToolResultPayload {
    /// 给 LLM 的文本（显示用）。
    pub text: String,
    /// 结构化数据（TUI typed 显示用）。
    pub data: Value,
    pub is_error: bool,
    pub image_count: usize,
}
```

同步更新 `new()` 签名、`Eq`/`Hash` impl 中的 `self.output` → `self.text`。

- [ ] **Step 2: view_assembler 填充点**

`apps/cli/src/tui/view_assembler/output.rs`，`ToolResultPayload::new(output, content, is_error, image_count)`（约 353 行）改参数名与字段对应；`find_tool_result_block` 返回的元组 `(output, content, ...)` 内部变量名改 `text`/`data`（逻辑不变，仅改名）。`display_text_for_tool_result` 入参若叫 `content`，改 `data`。

- [ ] **Step 3: render.rs 3 个 helper**

`apps/cli/src/tui/render/display/render.rs:118-121`：

```rust
                                output: tool_result_content_to_string(result.content),
                                content: normalize_tool_result_content(result.content),
                                ...
                                image_count: tool_result_image_count(result.content),
```

`result.content` → `result.data`。3 个 helper 函数体（315-360）入参名 `content` → `data`（逻辑不变：仍从 Value 提取 text/image）。`normalize_tool_result_content` 对 `Value::String`/`Array` 归一为 `{text}` —— 保留（MCP/offload 路径仍可能产生这些形态）。

- [ ] **Step 4: tool_display `payload.content` → `payload.data`**

```bash
grep -rn "payload\.content\|result_payload\.content" apps/cli/src/tui/render/output/
```

逐处改 `.content` → `.data`（约 6 处，含 `tool_impls.rs:19,22` 的 `payload.content.is_null()` / `from_value(payload.content.clone())`，`tool_display.rs:113,586,593,620`）。

- [ ] **Step 5: 编译（cli crate）**

Run: `cargo check --manifest-path apps/cli/Cargo.toml --all-targets 2>&1 | grep -E "^error" | head -20`
Expected: 空。按报错补改遗漏点。

- [ ] **Step 6: cli 测试**

Run: `cargo test --manifest-path apps/cli/Cargo.toml 2>&1 | grep "test result:" | tail -3`
Expected: 全 `ok`。

- [ ] **Step 7: 提交**

```bash
git add -A
git commit -m "refactor(tui): ToolResultPayload.content → data 适配 (#392 C)"
```

---

## Task 5: 全量验证门禁 + 实测

**Files:** 无（验证 only）

- [ ] **Step 1: workspace check**

Run: `cargo check --workspace --all-targets 2>&1 | tail -3`
Expected: `Finished` 无 error/warning。

- [ ] **Step 2: workspace clippy**

Run: `cargo clippy --workspace --all-targets 2>&1 | grep -E "^(error|warning)" | head`
Expected: 空。

- [ ] **Step 3: 全 crate 测试**

Run:
```bash
for c in agent/features/tools agent/shared agent/features/runtime apps/cli; do
  echo "=== $c ==="
  cargo test --manifest-path $c/Cargo.toml 2>&1 | grep "test result:" | tail -1
done
```
Expected: 全 `ok`，tools=110、shared≈212、runtime/cli 与 S1 基线持平。

- [ ] **Step 4: 架构守卫**

Run: `.agents/hooks/check-architecture-guards.sh 2>&1 | tail -5`
Expected: 全 guard OK。

- [ ] **Step 5: 残留扫描**

Run:
```bash
grep -rn "success_msg\|success_json\|error_json\|\.with_data(\|display_text_from_content" \
  agent/ apps/ packages/ --include="*.rs" | grep -v "target/\|\.git/"
```
Expected: 空（全部清除）。

- [ ] **Step 6: 实测 LLM 读取（Read bug 回归）**

Run:
```bash
printf '%s' '调用 Read 读 Cargo.toml，逐字复述前 3 行。' \
  | AEMEATH_VERSION= RUST_LOG= cargo run -- -qv 2>/dev/null | tail -5
```
Expected: LLM 复述出 `[workspace]` / `resolver = "2"` / `members = [`。

- [ ] **Step 7: 实测 Grep 文件级路径（Grep bug 回归）**

Run:
```bash
printf '%s' '用 Grep 工具：pattern="resolver", path="Cargo.toml"。报匹配数与首条。' \
  | AEMEATH_VERSION= RUST_LOG= cargo run -- -qv 2>/dev/null | tail -5
```
Expected: 1 条匹配，`Cargo.toml:2:resolver = "2"`。

- [ ] **Step 8: 提交 + PR**

```bash
git push -u origin feature/issue-392-tool-result-api-converge-s1
gh pr create --repo rushsinging/aemeath \
  --base main \
  --title "refactor(tools): ToolResult API 彻底收敛 (#392 S2)" \
  --body-file .github/pr_templates/... # 或 heredoc
```
PR body 引用 `Closes #392`（若 S1+S2 完整覆盖）、说明 A+B+C 范围、附验证清单。

---

## Self-Review

**1. Spec coverage（#392 验收项）**
- 「工具作者面 API 只剩 success/error/with_image」→ Task 1（TypedToolResult）+ S1 已删 success_msg ✓；with_image 保留 ✓
- 「全仓无 success_msg/success_json/error_json」→ S1 处理 success_msg；Task 2 处理 json 系；Task 5 Step 5 扫描兜底 ✓
- 「ToolOutcome 的 text（LLM）/data（TUI）单测覆盖」→ Task 3 Step 6 更新 from_tool_result 测试 ✓
- 「text→LLM, data→TUI 单一语义」→ 链路图中 text/data 贯穿三层 ✓

**2. Placeholder scan**：无 TBD/TODO；每步含具体代码或精确命令。

**3. Type consistency**：
- `TypedToolResult` 字段全程 `text`（Task 1 定义，Task 3 Step 4 adapter 消费 `result.text`）✓
- `ToolResult` 字段 `text`/`data`（Task 3 定义，Task 4 TUI 消费 `payload.text`/`payload.data`）✓
- `from_tool_result`：Task 3 Step 3 用 `r.text`/`r.data`，与 Task 3 Step 1 定义一致 ✓

**已知未覆盖 / 风险**：
- `loop_run.rs:140` offload 改写 `outcome.data` 语义保留（Task 3 Step 5 验证点），但未显式加测；若 clippy/测试发现回归需补。
- MCP 工具 `success(json_string)` 走新 `success(text)`：`data.text` = 整串 JSON（与旧 `content={text:json_string}` 等价），Task 3 Step 6 + Step 7 实测覆盖。
- 非交互 CLI 实测依赖 provider key 可用；若不可用，以单测为准并在 PR 注明。
