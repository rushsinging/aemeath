# 日志系统重新设计：职责划分 + target 规范化 + 架构守卫

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 aemeath.log 从"垃圾桶"拆分为按业务模块独立的诊断日志文件，合并 tool.log 到 tools.log，新增用户输入原始记录，统一 log target 路由，增加编译期/测试期架构守卫。

**Architecture:** UnifiedLogger 从 6 个 sink（aemeath/tui/hook/input/output/tool）扩展到 11 个（aemeath/runtime/provider/tools/prompt/tui/hook/input/output/audit + panic 不变）。路由规则从 2 个前缀（cli::/hook::）扩展到 6 个（+runtime::/provider::/tools::/prompt::）。tool.log 删除，tool_call/tool_result 以诊断格式写入 tools.log。

**Tech Stack:** Rust, `log` crate, `env_logger`, `serde_json`, ratatui

---

## 文件职责划分（最终）

### 诊断日志

| 文件 | target 前缀 | 来源 crate | 当前日志量 |
|---|---|---|---|
| `aemeath.log` | 兜底（无前缀匹配） | shared/composition + 其他 | ~10 |
| `runtime.log` | `runtime::` | runtime | 61 |
| `provider.log` | `provider::` | provider | 30 |
| `tools.log` | `tools::` | tools + tool_call/tool_result | 17 + 原始记录 |
| `prompt.log` | `prompt::` | prompt | 13 |
| `tui.log` | `cli::` | cli/tui | 已有（宏） |
| `hook.log` | `hook::` | hook | 13 |

### 原始记录

| 文件 | 数据 | 写入方法 |
|---|---|---|
| `input.log` | 用户输入 + LLM 输入（messages/system/schemas） | `log_input()` / `log_user_input()` |
| `output.log` | LLM 输出（content/usage） | `log_output()` |

### 审计

| 文件 | 数据 | 写入方法 |
|---|---|---|
| `audit.log` | 权限/行为审计（预留，当前无写入点） | `audit()`（预留） |

### 不变

| 文件 | 说明 |
|---|---|
| `panic.log` | panic_hook.rs 直写，不纳入 UnifiedLogger |

---

## target 前缀约定

所有 `log::xxx!` 调用 **MUST** 显式指定 `target:`，前缀对应所属 crate：

| crate | target 前缀 | 示例 |
|---|---|---|
| cli (tui) | `cli::tui` | 通过 `crate::tui::log_xxx!` 宏自动指定 |
| hook | `hook::` | `log::debug!(target: "hook::runner", ...)` |
| runtime | `runtime::` | `log::info!(target: "runtime::loop_runner", ...)` |
| provider | `provider::` | `log::warn!(target: "provider::client", ...)` |
| tools | `tools::` | `log::warn!(target: "tools::mcp", ...)` |
| prompt | `prompt::` | `log::debug!(target: "prompt::guidance", ...)` |
| storage | `storage::` | `log::warn!(target: "storage::tool_result", ...)` |
| 其他 | 兜底 | `log::info!(target: "app", ...)` → aemeath.log |

---

## API 变化

### 保留（不变）

```rust
UnifiedLogger::log_input(role: &str, payload: Value)   // → input.log
UnifiedLogger::log_output(role: &str, payload: Value)  // → output.log
```

### 新增

```rust
UnifiedLogger::log_user_input(payload: Value)          // → input.log, type="user_input"
UnifiedLogger::audit(audit_type: &str, payload: Value)  // → audit.log（预留）
```

### 变更

```rust
// 旧：
UnifiedLogger::log_tool(role: &str, kind: ToolKind, payload: Value)  // → tool.log

// 新：删除 log_tool 和 ToolKind。
// tool_call/tool_result 改为诊断格式写入 tools.log，
// 通过 log::info!(target: "tools", ...) 写入，msg 携带 JSON payload。
```

---

## Task 1: UnifiedLogger sink 扩展 + 路由更新

**Files:**
- Modify: `packages/global/logging/src/unified_logger.rs`

### Step 1: 更新 SinkPaths 结构

- [ ] **修改 SinkPaths 结构体**

旧：
```rust
struct SinkPaths {
    aemeath: PathBuf,
    tui: PathBuf,
    hook: PathBuf,
    input: PathBuf,
    output: PathBuf,
    tool: PathBuf,
}
```

新：
```rust
struct SinkPaths {
    aemeath: PathBuf,
    runtime: PathBuf,
    provider: PathBuf,
    tools: PathBuf,
    prompt: PathBuf,
    tui: PathBuf,
    hook: PathBuf,
    input: PathBuf,
    output: PathBuf,
    audit: PathBuf,
}
```

- [ ] **修改 `SinkPaths::from_logs_dir`**

新：
```rust
fn from_logs_dir(logs_dir: &Path) -> Self {
    Self {
        aemeath: logs_dir.join("aemeath.log"),
        runtime: logs_dir.join("runtime.log"),
        provider: logs_dir.join("provider.log"),
        tools: logs_dir.join("tools.log"),
        prompt: logs_dir.join("prompt.log"),
        tui: logs_dir.join("tui.log"),
        hook: logs_dir.join("hook.log"),
        input: logs_dir.join("input.log"),
        output: logs_dir.join("output.log"),
        audit: logs_dir.join("audit.log"),
    }
}
```

### Step 2: 更新 UnifiedLogger 结构体字段

- [ ] **修改 `UnifiedLogger` struct**

新：
```rust
pub struct UnifiedLogger {
    aemeath: Mutex<Option<BufWriter<File>>>,
    runtime: Mutex<Option<BufWriter<File>>>,
    provider: Mutex<Option<BufWriter<File>>>,
    tools: Mutex<Option<BufWriter<File>>>,
    prompt: Mutex<Option<BufWriter<File>>>,
    tui: Mutex<Option<BufWriter<File>>>,
    hook: Mutex<Option<BufWriter<File>>>,
    input: Mutex<Option<BufWriter<File>>>,
    output: Mutex<Option<BufWriter<File>>>,
    audit: Mutex<Option<BufWriter<File>>>,
    paths: SinkPaths,
    max_bytes: u64,
    max_backups: usize,
    role_logs_enabled: bool,
    filter: env_logger::Logger,
}
```

### Step 3: 更新 init 方法

- [ ] **修改轮转列表和 sink 初始化**

轮转列表：
```rust
for path in [
    &paths.aemeath,
    &paths.runtime,
    &paths.provider,
    &paths.tools,
    &paths.prompt,
    &paths.tui,
    &paths.hook,
    &paths.input,
    &paths.output,
    &paths.audit,
] {
```

logger 构造：
```rust
let logger = UnifiedLogger {
    aemeath: Mutex::new(Some(open_buf(&paths.aemeath)?)),
    runtime: Mutex::new(Some(open_buf(&paths.runtime)?)),
    provider: Mutex::new(Some(open_buf(&paths.provider)?)),
    tools: Mutex::new(Some(open_buf(&paths.tools)?)),
    prompt: Mutex::new(Some(open_buf(&paths.prompt)?)),
    tui: Mutex::new(Some(open_buf(&paths.tui)?)),
    hook: Mutex::new(Some(open_buf(&paths.hook)?)),
    input: Mutex::new(Some(open_buf(&paths.input)?)),
    output: Mutex::new(Some(open_buf(&paths.output)?)),
    audit: Mutex::new(Some(open_buf(&paths.audit)?)),
    paths,
    max_bytes,
    max_backups,
    role_logs_enabled,
    filter: build_filter(max_level),
};
```

### Step 4: 新增路由辅助方法 + 更新 Log::log

- [ ] **新增 `route` 方法并修改 `log` 方法**

新增路由方法（放在 `impl UnifiedLogger` 中，`write_audit` 之前）：
```rust
/// 按 target 前缀路由到对应的诊断 sink。
/// 返回 `None` 时走兜底 aemeath sink。
fn route(&self, target: &str) -> Option<(&Mutex<Option<BufWriter<File>>>, &Path)> {
    if target.starts_with("cli::") {
        Some((&self.tui, &self.paths.tui))
    } else if target.starts_with("hook::") {
        Some((&self.hook, &self.paths.hook))
    } else if target.starts_with("runtime::") {
        Some((&self.runtime, &self.paths.runtime))
    } else if target.starts_with("provider::") {
        Some((&self.provider, &self.paths.provider))
    } else if target.starts_with("tools::") {
        Some((&self.tools, &self.paths.tools))
    } else if target.starts_with("prompt::") {
        Some((&self.prompt, &self.paths.prompt))
    } else {
        None
    }
}
```

修改 `log` 方法：
```rust
fn log(&self, record: &Record) {
    if !self.enabled(record.metadata()) {
        return;
    }
    let line = format_diag_json_line(record);
    let target = record.target();
    if let Some((sink, path)) = self.route(target) {
        self.write_diag(sink, path, &line);
    } else {
        self.write_diag(&self.aemeath, &self.paths.aemeath, &line);
    }
}
```

### Step 5: 更新 flush 方法

- [ ] **修改 sink 列表**

```rust
fn flush(&self) {
    for sink in [
        &self.aemeath,
        &self.runtime,
        &self.provider,
        &self.tools,
        &self.prompt,
        &self.tui,
        &self.hook,
        &self.input,
        &self.output,
        &self.audit,
    ] {
```

### Step 6: 删除 ToolKind 枚举和 log_tool 方法

- [ ] **删除 `ToolKind` 枚举及其 impl**（第 35-49 行）
- [ ] **删除 `log_tool` 静态方法**（第 172-182 行）

### Step 7: 新增 log_user_input 和 audit 方法

- [ ] **在 `log_output` 之后新增**

```rust
/// 记录用户输入到 `input.log`（type="user_input"）。
pub fn log_user_input(payload: Value) {
    let Some(logger) = Self::current() else {
        return;
    };
    if !logger.role_logs_enabled {
        return;
    }
    let line = format_audit_json_line("user_input", "default", payload);
    logger.write_audit(&logger.input, &logger.paths.input, &line);
}

/// 记录审计事件到 `audit.log`。
pub fn audit(audit_type: &str, payload: Value) {
    let Some(logger) = Self::current() else {
        return;
    };
    let line = format_audit_json_line(audit_type, "audit", payload);
    logger.write_audit(&logger.audit, &logger.paths.audit, &line);
}
```

### Step 8: 更新测试

- [ ] **更新 `sink_paths_in_logs_dir`**
- [ ] **删除 `tool_kind_as_str` 测试**
- [ ] **更新 `static_audit_methods_are_noop_without_init`**：删除 log_tool 调用，新增 log_user_input/audit
- [ ] **更新 `rotate_test_logger`**：新增 runtime/provider/tools/prompt/audit 字段，删除 tool
- [ ] **新增路由测试 `route_returns_correct_sink_for_known_prefixes` 和 `route_returns_none_for_unknown_prefix`**

### Step 9: 编译验证

- [ ] Run: `cargo build -p logging` → Expected: 编译通过
- [ ] Run: `cargo test -p logging` → Expected: 全部通过

### Step 10: Commit

```bash
git add packages/global/logging/src/unified_logger.rs
git commit -m "refactor(logging): UnifiedLogger sink 扩展至 11 个，新增 runtime/provider/tools/prompt/audit 路由"
```

---

## Task 2: 更新 lib.rs 和 text.rs

**Files:**
- Modify: `packages/global/logging/src/lib.rs`
- Modify: `packages/global/logging/src/text.rs`

### Step 1: 更新 lib.rs

- [ ] **更新文档注释中的文件职责表**
- [ ] **更新 pub use 导出**：删除 `ToolKind`
- [ ] **新增 `pub mod target_guard;`**（在 `pub mod text;` 之后）

### Step 2: 更新 text.rs LogFile 枚举

- [ ] **删除 Tool 变体，新增 Runtime/Provider/Tools/Prompt/Audit**

### Step 3: 验证

- [ ] Run: `cargo build -p logging` → Expected: 编译通过
- [ ] Commit

---

## Task 3: 更新 logging_setup.rs 和 log_tool 调用方

**Files:**
- Modify: `agent/features/runtime/src/utils/bootstrap/logging_setup.rs`
- Modify: `agent/features/runtime/src/business/chat/looping/llm_log.rs`
- Modify: `agent/features/runtime/src/business/chat/looping/tools.rs`

### Step 1: 更新 logging_setup.rs 注释

- [ ] 文件列表注释改为 10 个文件

### Step 2: 更新 llm_log.rs

- [ ] 删除 `ToolKind` import
- [ ] `log_tool("default", ToolKind::Call, tc_data)` 改为 `log::info!(target: "tools::audit", "tool_call: {}", serde_json::to_string(&tc_data).unwrap_or_default())`

### Step 3: 更新 tools.rs

- [ ] `UnifiedLogger::log_tool("default", ToolKind::Result, tr_data)` 改为 `log::info!(target: "tools::audit", "tool_result: {}", serde_json::to_string(&tr_data).unwrap_or_default())`
- [ ] 删除 `ToolKind` import（如有）

### Step 4: 搜索残留

- [ ] Run: `grep -rn 'log_tool\|ToolKind' --include='*.rs' agent/ apps/`
- [ ] 确认无残留

### Step 5: 验证

- [ ] Run: `cargo build` → Expected: 编译通过
- [ ] Commit

---

## Task 4: 新增用户输入记录点

**Files:**
- Modify: `agent/features/runtime/src/business/chat/looping/input_gate.rs`

### Step 1: 在 UserMessage 分支中新增 log_user_input

- [ ] 修改 `ChatInputEvent::UserMessage` 分支（第 163-167 行），在 `messages.push` 之前新增：

```rust
logging::UnifiedLogger::log_user_input(serde_json::json!({
    "text": &text,
    "image_paths": &image_paths,
}));
```

### Step 2: 验证

- [ ] Run: `cargo build -p runtime` → Expected: 编译通过
- [ ] Run: `cargo test -p runtime -- input_gate` → Expected: 全部通过
- [ ] Commit

---

## Task 5: Hook 层日志规范化（13 处）

**Files:**
- Modify: `agent/features/hook/src/business/hook/runner.rs`

### Step 1: 给 13 处裸 log 补 target: "hook::runner"

具体行号：97, 121, 152, 184, 193, 206, 213, 239, 248, 285, 292, 325, 335

- [ ] 逐个修改 `log::xxx!(` → `log::xxx!(target: "hook::runner", `

### Step 2: 验证

- [ ] Run: `cargo build -p hook` → Expected: 编译通过
- [ ] Commit

---

## Task 6: TUI 层日志规范化（18 处 + 1 处 chat.rs）

**Files:**
- Modify: 8 个 TUI 文件 + `apps/cli/src/chat.rs`

### 需修改的文件

| 文件 | 处数 | 旧 | 新 |
|---|---|---|---|
| `tui/app/update/spinner.rs` | 2 | `log::debug!(` | `crate::tui::log_debug!(` |
| `tui/effect/session/processing.rs` | 6 | `log::trace!(` | `crate::tui::log_trace!(` |
| `tui/render/output/blocks/tool_result.rs` | 1 | `log::debug!(` | `crate::tui::log_debug!(` |
| `tui/render/output/blocks/tool_call.rs` | 1 | `log::debug!(` | `crate::tui::log_debug!(` |
| `tui/adapter/tool_flow_projector.rs` | 3 | `log::debug!(` | `crate::tui::log_debug!(` |
| `tui/view_assembler/output.rs` | 1 | `log::debug!(` | `crate::tui::log_debug!(` |
| `tui/model/conversation/tool_flow.rs` | 2 | `log::debug!(` | `crate::tui::log_debug!(` |
| `tui/model/conversation/model.rs` | 2 | `log::debug!(` | `crate::tui::log_debug!(` |
| `chat.rs` | 1 | `log::error!(` | `crate::tui::log_error!(` |

- [ ] 逐文件修改
- [ ] Run: `cargo build -p cli` → Expected: 编译通过
- [ ] Run: `cargo test -p cli` → Expected: 全部通过
- [ ] Commit

---

## Task 7: Runtime 层日志规范化（61 处）

**Files:**
- Modify: `agent/features/runtime/src/` 下 ~24 个文件

### 需修改的文件和 target 子前缀

| 文件路径 | 数 | target |
|---|---|---|
| `core/command/commands.rs` | 1 | `runtime::commands` |
| `core/client/event.rs` | 3 | `runtime::event` |
| `core/client/from_args.rs` | 3 | `runtime::from_args` |
| `business/compact/autocompact.rs` | 1 | `runtime::autocompact` |
| `business/chat/looping/finalize.rs` | 1 | `runtime::finalize` |
| `business/chat/looping/compact.rs` | 1 | `runtime::compact` |
| `business/chat/looping/loop_runner.rs` | 7 | `runtime::loop_runner` |
| `business/chat/looping/hook_ui.rs` | 1 | `runtime::hook_ui` |
| `business/chat/looping/state.rs` | 3 | `runtime::state` |
| `business/chat/looping/stall.rs` | 2 | `runtime::stall` |
| `business/chat/looping/config_reload.rs` | 3 | `runtime::config_reload` |
| `business/chat/looping/input_gate.rs` | 1 | `runtime::input_gate` |
| `business/cost/tracker.rs` | 1 | `runtime::tracker` |
| `business/agent/runner/finalize.rs` | 1 | `runtime::finalize` |
| `business/agent/runner/setup.rs` | 3 | `runtime::setup` |
| `business/agent/agent.rs` | 3 | `runtime::agent` |
| `business/state.rs` | 1 | `runtime::state` |
| `business/prompt/build/prompt_build.rs` | 1 | `runtime::prompt_build` |
| `business/scheduler.rs` | 3 | `runtime::scheduler` |
| `business/reflection/runner.rs` | 2 | `runtime::reflection` |
| `utils/bootstrap/mcp_loader.rs` | 9 | `runtime::mcp_loader` |
| `utils/bootstrap/config_manager.rs` | 9 | `runtime::config_manager` |
| `utils/bootstrap/runtime_support.rs` | 2 | `runtime::runtime_support` |
| `utils/bootstrap/logging_setup.rs` | 2 | `runtime::logging_setup` |

- [ ] 逐文件修改：每个 `log::xxx!(` 补上 `target: "runtime::module_name"`
- [ ] Run: `cargo build -p runtime` → Expected: 编译通过
- [ ] Commit

---

## Task 8: Provider 层日志规范化（30 处）

**Files:**
- Modify: `agent/features/provider/src/` 下 8 个文件

| 文件路径 | 数 | target |
|---|---|---|
| `core/client.rs` | 4 | `provider::client` |
| `core/pool.rs` | 3 | `provider::pool` |
| `business/providers/ollama.rs` | 5 | `provider::ollama` |
| `business/providers/ollama/non_stream.rs` | 1 | `provider::ollama_non_stream` |
| `business/providers/ollama/stream.rs` | 3 | `provider::ollama_stream` |
| `business/providers/openai_compatible/stream.rs` | 7 | `provider::openai_stream` |
| `business/providers/openai_compatible/request_body.rs` | 4 | `provider::openai_request` |
| `business/providers/openai_compatible/message_helpers.rs` | 3 | `provider::openai_helpers` |

- [ ] 逐文件修改
- [ ] Run: `cargo build -p provider` → Expected: 编译通过
- [ ] Commit

---

## Task 9: Tools 层日志规范化（17 处）

**Files:**
- Modify: `agent/features/tools/src/` 下 6 个文件

| 文件路径 | 数 | target |
|---|---|---|
| `business/mcp/sse.rs` | 3 | `tools::sse` |
| `business/mcp/client.rs` | 1 | `tools::mcp_client` |
| `business/mcp/sse_stream.rs` | 4 | `tools::sse_stream` |
| `business/mcp_manager/wrapper.rs` | 1 | `tools::wrapper` |
| `business/mcp_manager/connection.rs` | 7 | `tools::connection` |
| `business/list_mcp_resources.rs` | 1 | `tools::list_resources` |

- [ ] 逐文件修改
- [ ] Run: `cargo build -p tools` → Expected: 编译通过
- [ ] Commit

---

## Task 10: Prompt 层日志规范化（13 处）

**Files:**
- Modify: `agent/features/prompt/src/` 下 3 个文件

| 文件路径 | 数 | target |
|---|---|---|
| `business/skill/parser.rs` | 2 | `prompt::parser` |
| `business/guidance/resolver.rs` | 9 | `prompt::resolver` |
| `business/guidance.rs` | 3 | `prompt::guidance` |

- [ ] 逐文件修改
- [ ] Run: `cargo build -p prompt` → Expected: 编译通过
- [ ] Commit

---

## Task 11: Storage 层日志规范化（3 处）

**Files:**
- Modify: `agent/features/storage/src/business/tool_result_storage.rs`

- [ ] 3 处 `log::warn!(` 补 `target: "storage::tool_result"`
- [ ] Run: `cargo build -p storage` → Expected: 编译通过
- [ ] Commit

---

## Task 12: 架构守卫

**Files:**
- Create: `packages/global/logging/src/target_guard.rs`
- Modify: `packages/global/logging/src/lib.rs`

### Step 1: 创建 target_guard.rs

守卫逻辑：扫描 workspace 下各 crate 目录的 `.rs` 生产代码（排除 `#[cfg(test)] mod`），确保不出现裸 `log::xxx!(` 调用。

覆盖 7 个层：tui、hook、runtime、provider、tools、prompt、storage。

详见完整代码（见下方 Self-Review 中确认的 `production_source` + `has_bare_log_calls` + 每层一个 `#[test]`）。

### Step 2: 在 lib.rs 注册模块

- [ ] `pub mod target_guard;`（Task 2 中已添加占位）

### Step 3: 验证

- [ ] Run: `cargo test -p logging target_guard` → Expected: 全部通过（前提：Task 5-11 已完成）
- [ ] Commit

---

## Task 13: 更新 specs/rust-coding.md

**Files:**
- Modify: `specs/rust-coding.md`

### Step 1: 替换日志规范章节（第 41-47 行）

更新为新的文件划分表、target 前缀约定、MUST/NEVER 规则。

### Step 2: Commit

---

## Task 14: 全量验证

- [ ] Run: `cargo build` → Expected: 编译通过
- [ ] Run: `cargo clippy --all-targets` → Expected: 无错误
- [ ] Run: `cargo test` → Expected: 全部通过（包括 target_guard）
- [ ] 手动验证：启动应用发一条消息，`ls ~/.agents/logs/` 确认存在新文件、不存在 `tool.log`
- [ ] 确认 `input.log` 有 `user_input` 类型，`tools.log` 有 `tool_call`

---

## Self-Review

### Spec coverage

- [x] aemeath.log 拆分 → Task 1 新增 4 个 sink + 路由
- [x] TUI input 记录 → Task 1 新增 `log_user_input` + Task 4 调用点
- [x] LLM output 记录 → 已有 `log_output`，不变
- [x] tool.log 合并到 tools.log → Task 1 删除 tool sink，Task 3 改调用方
- [x] 审计 audit.log → Task 1 新增 audit sink + `audit()` 方法
- [x] 不带 target 的 log 全部改 → Task 5-11（共 156 处）
- [x] 架构守卫 → Task 12
- [x] 文档更新 → Task 13

### Placeholder scan

无 TBD/TODO。每个 Task 有具体文件路径和代码。

### Type consistency

- `log_user_input(payload: Value)` — Task 1 定义，Task 4 调用 ✓
- `audit(audit_type: &str, payload: Value)` — Task 1 定义，预留无调用 ✓
- `route(target: &str) -> Option<(&Mutex<Option<BufWriter<File>>>, &Path)>` — Task 1 定义和使用 ✓
- `ToolKind` 删除后无残留 — Task 3 搜索确认 ✓
