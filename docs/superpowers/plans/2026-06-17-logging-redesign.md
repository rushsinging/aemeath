# 日志系统重设计实施计划（#303）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 统一日志 target 命名（`aemeath:` 三级前缀）、引入 14 字段结构化 schema（含 `boot_ts`/`ver`/`request_id`/`provider`/`role`/`event_type`）、收敛版本号、清理死代码、加固架构守卫。

**Architecture:** logging crate 提供 target 常量定义规范（各 crate `lib.rs` 定义 `LOG_TARGET`），`UnifiedLogger` 按新路由表匹配 12 个合法 target。context 全局变量从 4 个扩展到 8 个。版本号走 `[workspace.package]` 统一继承 + `cargo-release` 自动化发版。

**Tech Stack:** Rust workspace、`log` crate、`cargo-release`、bash 守卫脚本

---

## PR 序列总览

| PR | 主题 | 依赖 | 可独立交付 |
|----|------|------|-----------|
| PR1 | workspace.package 版本统一 + cargo-release | 无 | ✅ |
| PR2 | logging crate 核心改造（context/format/router/死代码清理） | 无 | ✅ |
| PR3 | 各 crate LOG_TARGET 常量 + 批量改 target | PR2 | ✅ |
| PR4 | input/output 合并到 provider + event_type | PR2, PR3 | ✅ |
| PR5 | 架构守卫加固（Rust test + shell） | PR3 | ✅ |
| PR6 | specs/logging.md 规范文档 + 触发表更新 | PR1-PR5 | ✅ |

---

## 文件结构

### 新建文件
- `release.toml`（workspace 根）— cargo-release 配置
- `.agents/hooks/check-log-target-prefix.sh` — shell 守卫
- `specs/logging.md` — 日志规范分片

### 删除文件
- `packages/global/logging/src/text.rs` — 死代码（`append_*`/`LogFile` 枚举）

### 核心改造文件（`packages/global/logging/src/`）
- `context.rs` — 新增 `boot_ts`/`app_version`/`provider`/`request_id`/`role` 全局变量
- `format.rs` — 行 schema 从 8 字段扩展到 14 字段；时间改本地
- `unified_logger.rs` — 路由表重写（12 个 `aemeath:` target）；删除 `audit()` 死代码
- `target_guard.rs` — 守卫加固（检查 `LOG_TARGET` 常量 + 白名单值）
- `lib.rs` — 删除 `text` 模块导出；新增常量/白名单定义

### 调用方改造文件（按 crate）
- 每个 feature crate 的 `lib.rs` — 新增 `pub const LOG_TARGET: &str`
- 全仓库 ~163 处 `log::xxx!(target: "...")` — 改为引用 `LOG_TARGET`

---

## PR1: workspace.package 版本统一 + cargo-release

### Task 1.1: 根 Cargo.toml 加 [workspace.package]

**Files:**
- Modify: `Cargo.toml`（workspace 根）

- [ ] **Step 1: 在 `[workspace]` 段下方新增 `[workspace.package]`**

在根 `Cargo.toml` 的 `[workspace]` 段（`members` 列表之后、`resolver` 之后）新增：

```toml
[workspace.package]
version = "0.8.2"
edition = "2021"
```

> `edition = "2021"` 也改为 workspace 继承，统一收口。

- [ ] **Step 2: 验证根 Cargo.toml 语法正确**

Run: `cargo metadata --no-deps --format-version 1 | head -5`
Expected: 正常输出 JSON，无错误

### Task 1.2: 所有 member crate 改为 workspace 继承

**Files:**
- Modify: 全部 14 个 member crate 的 `Cargo.toml`

需要修改的 crate 列表（`[package]` 段中 `version` 和 `edition` 改为 `.workspace = true`）：

```
apps/cli/Cargo.toml
agent/composition/Cargo.toml
agent/features/runtime/Cargo.toml
agent/features/audit/Cargo.toml
agent/features/hook/Cargo.toml
agent/features/policy/Cargo.toml
agent/features/project/Cargo.toml
agent/features/prompt/Cargo.toml
agent/features/provider/Cargo.toml
agent/features/storage/Cargo.toml
agent/features/tools/Cargo.toml
agent/shared/Cargo.toml
packages/sdk/Cargo.toml
packages/global/logging/Cargo.toml
packages/global/utils/Cargo.toml
```

- [ ] **Step 1: 每个 crate 的 `[package]` 段，将**

```toml
version = "0.1.0"
edition = "2021"
```

改为：

```toml
version.workspace = true
edition.workspace = true
```

> 注意：`apps/cli/Cargo.toml` 有两个 `[package]` 段（`cli` lib + `aemeath` bin），两个都要改。

- [ ] **Step 2: 验证 workspace 构建通过**

Run: `cargo build --workspace`
Expected: 编译成功，无错误

- [ ] **Step 3: 验证版本号统一生效**

Run: `cargo metadata --no-deps --format-version 1 | python3 -c "import sys,json; d=json.load(sys.stdin); [print(p['name'], p['version']) for p in d['packages'] if 'aemeath' in p['name'].lower() or p['name'] in ('cli','sdk','logging','utils')]" 2>/dev/null || cargo metadata --no-deps --format-version 1 | grep -o '"version":"[^"]*"' | sort -u`
Expected: 所有包版本为 `0.8.2`

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore(workspace): 统一版本号到 [workspace.package] 继承"
```

### Task 1.3: 添加 cargo-release 配置

**Files:**
- Create: `release.toml`（workspace 根）

- [ ] **Step 1: 创建 release.toml**

```toml
# cargo-release 配置
# 用法: cargo release <version> --execute
# 例如: cargo release 0.8.3 --execute

consolidate-commits = true
consolidate-pushes = true
allow-branch = ["main"]

pre-release-hook = ["cargo", "build", "--workspace"]

[[pre-release-replacements]]
# 更新引用了版本号的文件（如有需要在此添加）
```

- [ ] **Step 2: 验证 cargo-release 可用**

Run: `cargo release --version 2>/dev/null || echo "cargo-release 未安装，可选安装: cargo install cargo-release"`
Expected: 显示版本号或提示未安装（不影响 PR）

- [ ] **Step 3: Commit**

```bash
git add release.toml
git commit -m "chore(release): 添加 cargo-release 配置"
```

---

## PR2: logging crate 核心改造

### Task 2.1: context.rs 新增全局变量

**Files:**
- Modify: `packages/global/logging/src/context.rs`

- [ ] **Step 1: 新增 5 个全局变量 + getter/setter**

在现有 `SESSION_ID` / `CURRENT_CHAT_ID` / `CURRENT_TURN` / `CURRENT_MODEL` 之后，新增：

```rust
static BOOT_TS: OnceLock<String> = OnceLock::new();
static APP_VERSION: OnceLock<String> = OnceLock::new();
static CURRENT_PROVIDER: RwLock<String> = RwLock::new(String::new());
static CURRENT_REQUEST_ID: RwLock<String> = RwLock::new(String::new());
static CURRENT_ROLE: RwLock<String> = RwLock::new(String::new());
```

新增 setter/getter：

```rust
/// 设置进程启动时间戳（本地时间 RFC3339）。`init_logging` 时调用一次。
pub fn set_boot_ts(ts: String) {
    let _ = BOOT_TS.set(ts);
}

/// 设置 app 版本号。`init_logging` 时调用一次。
pub fn set_app_version(ver: String) {
    let _ = APP_VERSION.set(ver);
}

/// 设置当前 provider。
pub fn set_current_provider(provider: String) {
    if let Ok(mut current) = CURRENT_PROVIDER.write() {
        *current = provider;
    }
}

/// 设置当前 request_id（每次 LLM 调用前）。
pub fn set_current_request_id(id: String) {
    if let Ok(mut current) = CURRENT_REQUEST_ID.write() {
        *current = id;
    }
}

/// 设置当前 role（主 agent 为 "default"，sub-agent 为其 role 名）。
pub fn set_current_role(role: String) {
    if let Ok(mut current) = CURRENT_ROLE.write() {
        *current = role;
    }
}
```

新增 getter：

```rust
pub fn boot_ts() -> Option<&'static str> {
    BOOT_TS.get().map(|s| s.as_str())
}

pub fn app_version() -> Option<&'static str> {
    APP_VERSION.get().map(|s| s.as_str())
}

pub fn current_provider() -> Option<String> {
    CURRENT_PROVIDER
        .read()
        .ok()
        .and_then(|s| if s.is_empty() { None } else { Some(s.clone()) })
}

pub fn current_request_id() -> Option<String> {
    CURRENT_REQUEST_ID
        .read()
        .ok()
        .and_then(|s| if s.is_empty() { None } else { Some(s.clone()) })
}

pub fn current_role() -> Option<String> {
    CURRENT_ROLE
        .read()
        .ok()
        .and_then(|s| if s.is_empty() { None } else { Some(s.clone()) })
}
```

更新文件头注释表格，补充新增变量说明。

- [ ] **Step 2: 为每个新增变量补充单元测试**

参照现有 `model_empty_is_none` / `model_non_empty_some` 的模式，为 `provider` / `request_id` / `role` 各写 empty→None 和 non_empty→Some 两个测试。

- [ ] **Step 3: 运行测试验证**

Run: `cargo test -p logging --lib context`
Expected: 全部 PASS

- [ ] **Step 4: Commit**

```bash
git add packages/global/logging/src/context.rs
git commit -m "feat(logging): context 新增 boot_ts/app_version/provider/request_id/role 全局变量"
```

### Task 2.2: format.rs 扩展 schema 到 14 字段 + 本地时间

**Files:**
- Modify: `packages/global/logging/src/format.rs`

- [ ] **Step 1: 时间戳函数改为本地时间**

找到现有 `timestamp_rfc3339()` 函数（当前使用 UTC），改为本地时间：

```rust
/// 本地时间 RFC3339 格式（含时区偏移），毫秒精度。
pub fn timestamp_local_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as i64;
    let millis = now.subsec_millis();

    // 用 chrono 处理本地时区（如已依赖 chrono）
    // 或用简易偏移计算
    // ...
}
```

> **实现细节**：检查 logging crate 是否已依赖 `chrono`。如果未依赖，评估两个方案：
> - (a) 引入 `chrono` 依赖（`features = ["clock"]`）
> - (b) 读取 `TZ` 环境变量 + `/etc/localtime` 手动算偏移（不推荐，复杂且易错）
> 推荐方案 (a)。

- [ ] **Step 2: 检查 logging crate Cargo.toml 是否有 chrono 依赖**

Run: `grep chrono packages/global/logging/Cargo.toml`
Expected: 如无，添加 `chrono = { version = "0.4", default-features = false, features = ["clock"] }`

- [ ] **Step 3: 扩展诊断行 schema（diagnostic_line）**

找到 `format.rs` 中构建诊断行的函数（当前注入 `ts`/`session`/`chat`/`turn`/`model`），扩展为 14 字段：

```rust
/// 构建诊断日志行（JSON Value）。
/// 注入全部 14 个字段。
pub fn diagnostic_line(
    level: &str,
    target: &str,
    msg: &str,
) -> serde_json::Value {
    let mut line = serde_json::Map::new();
    line.insert("ts".into(), serde_json::Value::String(timestamp_local_rfc3339()));
    line.insert("boot_ts".into(), boot_ts().map(Value::String).unwrap_or(Value::Null));
    line.insert("ver".into(), app_version().map(Value::String).unwrap_or(Value::Null));
    line.insert("session".into(), session_id().map(Value::String).unwrap_or(Value::Null));
    line.insert("chat".into(), current_chat_id().map(Value::String).unwrap_or(Value::Null));
    line.insert("turn".into(), current_turn().map(Value::Number).unwrap_or(Value::Null));
    line.insert("request_id".into(), current_request_id().map(Value::String).unwrap_or(Value::Null));
    line.insert("model".into(), current_model().map(Value::String).unwrap_or(Value::Null));
    line.insert("provider".into(), current_provider().map(Value::String).unwrap_or(Value::Null));
    line.insert("role".into(), current_role().map(Value::String).unwrap_or(Value::Null));
    line.insert("level".into(), Value::String(level.to_string()));
    line.insert("target".into(), Value::String(target.to_string()));
    line.insert("event_type".into(), Value::Null); // 诊断行默认 null
    line.insert("msg".into(), Value::String(msg.to_string()));
    Value::Object(line)
}
```

- [ ] **Step 4: 扩展审计行 schema**

找到构建审计行的函数（`format_audit_line` 或类似），同样注入 14 字段，`event_type` 由调用方传入。

- [ ] **Step 5: 更新现有测试**

`format.rs` 的测试用例（`assert_eq!(value["role"], "default")` 等）需要适配新字段。更新测试断言覆盖 `boot_ts`/`ver`/`provider`/`request_id`/`role`。

- [ ] **Step 6: 运行测试验证**

Run: `cargo test -p logging --lib format`
Expected: 全部 PASS

- [ ] **Step 7: Commit**

```bash
git add packages/global/logging/src/format.rs packages/global/logging/Cargo.toml
git commit -m "feat(logging): schema 扩展到 14 字段 + 时间戳改本地时区"
```

### Task 2.3: unified_logger.rs 路由表重写

**Files:**
- Modify: `packages/global/logging/src/unified_logger.rs`

- [ ] **Step 1: 定义合法 target 白名单常量**

在 `unified_logger.rs`（或 `lib.rs`）顶部新增：

```rust
/// 合法日志 target 白名单（12 个）。
/// 所有 log::xxx! 调用的 target 值必须 ∈ 此列表或以此为前缀。
pub const ALLOWED_TARGETS: &[&str] = &[
    "aemeath:tui",
    "aemeath:shared",
    "aemeath:composition",
    "aemeath:agent:provider",
    "aemeath:agent:runtime",
    "aemeath:agent:tools",
    "aemeath:agent:prompt",
    "aemeath:agent:hook",
    "aemeath:agent:storage",
    "aemeath:agent:project",
    "aemeath:agent:policy",
    "aemeath:agent:audit",
];

/// target → 日志文件名映射。
fn target_to_file(target: &str) -> &str {
    // 最长前缀匹配
    for allowed in ALLOWED_TARGETS {
        if target == *allowed || target.starts_with(&format!("{}:", allowed)) {
            return match *allowed {
                "aemeath:tui" => "tui.log",
                "aemeath:shared" => "shared.log",
                "aemeath:composition" => "composition.log",
                "aemeath:agent:provider" => "agent-provider.log",
                "aemeath:agent:runtime" => "agent-runtime.log",
                "aemeath:agent:tools" => "agent-tools.log",
                "aemeath:agent:prompt" => "agent-prompt.log",
                "aemeath:agent:hook" => "agent-hook.log",
                "aemeath:agent:storage" => "agent-storage.log",
                "aemeath:agent:project" => "agent-project.log",
                "aemeath:agent:policy" => "agent-policy.log",
                "aemeath:agent:audit" => "agent-audit.log",
                _ => "aemeath.log",
            };
        }
    }
    "aemeath.log" // 硬兜底（守卫会拦截，不应到达）
}
```

- [ ] **Step 2: 重写 route() / log() 方法的路由逻辑**

将现有的按旧前缀（`cli::`/`runtime::`/`provider::` 等）路由的逻辑，替换为调用 `target_to_file(record.target())`。

- [ ] **Step 3: 删除 audit() 死代码方法**

删除 `UnifiedLogger::audit()` 方法（全仓库零调用，已确认）。

- [ ] **Step 4: 运行测试验证**

Run: `cargo test -p logging`
Expected: 全部 PASS

- [ ] **Step 5: Commit**

```bash
git add packages/global/logging/src/unified_logger.rs packages/global/logging/src/lib.rs
git commit -m "feat(logging): 路由表重写为 aemeath: 三级前缀 + 删除 audit() 死代码"
```

### Task 2.4: 删除 text.rs 死代码

**Files:**
- Delete: `packages/global/logging/src/text.rs`
- Modify: `packages/global/logging/src/lib.rs`

- [ ] **Step 1: 从 lib.rs 删除 text 模块导出**

找到 `pub mod text;` 或 `mod text;`，删除该行。

- [ ] **Step 2: 删除 text.rs 文件**

Run: `rm packages/global/logging/src/text.rs`

- [ ] **Step 3: 验证编译通过**

Run: `cargo build -p logging`
Expected: 编译成功（确认全仓库无 `text.rs` 的引用）

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "refactor(logging): 删除 text.rs 死代码（append_*/LogFile 枚举零调用）"
```

### Task 2.5: init_logging 注入 boot_ts 和 app_version

**Files:**
- Modify: `agent/features/runtime/src/utils/bootstrap/logging_setup.rs`

- [ ] **Step 1: init_logging 中注入 boot_ts 和 app_version**

在 `logging_setup.rs` 的 `init_logging` 函数中，初始化日志后调用：

```rust
logging::context::set_boot_ts(logging::format::timestamp_local_rfc3339());
logging::context::set_app_version(env!("CARGO_PKG_VERSION").to_string());
```

> 由于 PR1 已统一 workspace 版本，`env!("CARGO_PKG_VERSION")` 在 runtime crate 中也返回 `0.8.2`。

- [ ] **Step 2: 验证编译通过**

Run: `cargo build --workspace`
Expected: 编译成功

- [ ] **Step 3: Commit**

```bash
git add agent/features/runtime/src/utils/bootstrap/logging_setup.rs
git commit -m "feat(logging): init_logging 注入 boot_ts 和 app_version"
```

---

## PR3: 各 crate LOG_TARGET 常量 + 批量改 target

### Task 3.1: 各 crate lib.rs 定义 LOG_TARGET 常量

**Files:**
- Modify: 12 个 crate 的 `lib.rs`

| crate | lib.rs 路径 | LOG_TARGET 值 |
|-------|------------|---------------|
| apps/cli | `apps/cli/src/tui.rs`（已有 `LOG_TARGET`，改值） | `"aemeath:tui"` |
| runtime | `agent/features/runtime/src/lib.rs` | `"aemeath:agent:runtime"` |
| provider | `agent/features/provider/src/lib.rs` | `"aemeath:agent:provider"` |
| tools | `agent/features/tools/src/lib.rs` | `"aemeath:agent:tools"` |
| prompt | `agent/features/prompt/src/lib.rs` | `"aemeath:agent:prompt"` |
| hook | `agent/features/hook/src/lib.rs` | `"aemeath:agent:hook"` |
| storage | `agent/features/storage/src/lib.rs` | `"aemeath:agent:storage"` |
| project | `agent/features/project/src/lib.rs` | `"aemeath:agent:project"` |
| policy | `agent/features/policy/src/lib.rs` | `"aemeath:agent:policy"` |
| audit | `agent/features/audit/src/lib.rs` | `"aemeath:agent:audit"` |
| shared | `agent/shared/src/lib.rs` | `"aemeath:shared"` |
| composition | `agent/composition/src/lib.rs` | `"aemeath:composition"` |

- [ ] **Step 1: 每个 crate 的 lib.rs 新增常量**

在每个 crate 的 `lib.rs`（顶层）添加：

```rust
/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:agent:runtime"; // 各 crate 替换为对应值
```

> `apps/cli/src/tui.rs` 已有 `pub(crate) const LOG_TARGET: &str = "cli::tui";`，改为 `pub const LOG_TARGET: &str = "aemeath:tui";`（去掉 `pub(crate)` 改为 `pub`，值改为新前缀）。

- [ ] **Step 2: 验证编译通过**

Run: `cargo build --workspace`
Expected: 编译成功

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(logging): 各 crate lib.rs 定义 LOG_TARGET 常量"
```

### Task 3.2: 批量改 runtime crate 的 target 引用

**Files:**
- Modify: `agent/features/runtime/src/` 下 ~32 个文件

runtime crate 当前 target 值分布（全部要改为 `LOG_TARGET` 常量引用）：

```
"runtime::agent"          → LOG_TARGET
"runtime::finalize"       → LOG_TARGET
"runtime::setup"          → LOG_TARGET
"runtime::compact"        → LOG_TARGET
"runtime::config_reload"  → LOG_TARGET
"runtime::hook_ui"        → LOG_TARGET
"runtime::input_gate"     → LOG_TARGET
"runtime::loop_runner"    → LOG_TARGET
"runtime::reflection"     → LOG_TARGET
"runtime::stall"          → LOG_TARGET
"runtime::state"          → LOG_TARGET
"runtime::stream_handler" → LOG_TARGET
"runtime::autocompact"    → LOG_TARGET
"runtime::tracker"        → LOG_TARGET
"runtime::prompt_build"   → LOG_TARGET
"runtime::scheduler"      → LOG_TARGET
"runtime::from_args"      → LOG_TARGET
"runtime::commands"       → LOG_TARGET
"runtime::config_manager" → LOG_TARGET
"runtime::logging_setup"  → LOG_TARGET
"runtime::mcp_loader"     → LOG_TARGET
"runtime::runtime_support"→ LOG_TARGET
"sub_agent"               → LOG_TARGET  （违规修复）
"cli::tui::tool_flow"     → LOG_TARGET  （违规修复，runtime 不该用 cli target）
"tools::audit"            → LOG_TARGET  （4 处跨 crate 误写修复）
```

- [ ] **Step 1: 每个文件顶部添加 `use crate::LOG_TARGET;`（如未有）**

- [ ] **Step 2: 将所有 `target: "runtime::xxx"` 替换为 `target: LOG_TARGET`**

对每个文件，执行替换。示例：

```rust
// 旧
log::info!(target: "runtime::loop_runner", "compact triggered");
// 新
log::info!(target: LOG_TARGET, "compact triggered");
```

- [ ] **Step 3: 修复违规 target（"sub_agent" / "cli::tui::tool_flow" / "tools::audit"）**

同一 crate 内全部改为 `LOG_TARGET`。

- [ ] **Step 4: 验证编译通过**

Run: `cargo build -p runtime`
Expected: 编译成功

- [ ] **Step 5: Commit**

```bash
git add agent/features/runtime/src/
git commit -m "refactor(runtime): 所有 log target 统一引用 LOG_TARGET 常量"
```

### Task 3.3: 批量改 provider crate 的 target 引用

**Files:**
- Modify: `agent/features/provider/src/` 下 ~8 个文件

provider crate 当前 target 值：
```
"provider::ollama"          → LOG_TARGET
"provider::ollama_non_stream" → LOG_TARGET
"provider::ollama_stream"   → LOG_TARGET
"provider::openai_helpers"  → LOG_TARGET
"provider::openai_request"  → LOG_TARGET
"provider::openai_stream"   → LOG_TARGET
"provider::client"          → LOG_TARGET
"provider::pool"            → LOG_TARGET
```

- [ ] **Step 1: 每个文件添加 `use crate::LOG_TARGET;`，替换所有 target 值**

- [ ] **Step 2: 验证编译 + Commit**

Run: `cargo build -p provider`

```bash
git add agent/features/provider/src/
git commit -m "refactor(provider): 所有 log target 统一引用 LOG_TARGET 常量"
```

### Task 3.4: 批量改其余 crate 的 target 引用

**Files:**
- Modify: tools（7 文件）、prompt（3 文件）、hook（1 文件）、storage（1 文件）、cli（2 文件）

按同样模式：每个文件添加 `use crate::LOG_TARGET;`（cli 为 `use crate::tui::LOG_TARGET;`），替换所有 target 字符串字面量为常量引用。

- [ ] **Step 1: tools crate（7 文件，~21 处）**

- [ ] **Step 2: prompt crate（3 文件，~13 处）**

- [ ] **Step 3: hook crate（1 文件，~13 处）**

- [ ] **Step 4: storage crate（1 文件，~3 处）**

- [ ] **Step 5: cli crate（2 文件，~3 处）**

  > `apps/cli/src/tui.rs` 已有宏 `tui_log_debug!` 等引用 `LOG_TARGET`，只需改常量值（Task 3.1 已完成）。检查是否有直接写 `target: "..."` 的遗漏。

- [ ] **Step 6: 验证全 workspace 编译通过**

Run: `cargo build --workspace`
Expected: 编译成功

- [ ] **Step 7: grep 验证无旧 target 残留**

Run: `rg 'target:\s*"(runtime|provider|tools|prompt|hook|storage|cli)::' --type rust`
Expected: 无输出（所有旧前缀 target 已清除）

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor: 全 crate log target 统一引用 LOG_TARGET 常量"
```

### Task 3.5: 主对话 loop 补全 set_current_model / set_current_provider

**Files:**
- Modify: `agent/features/runtime/src/business/chat/looping/loop_runner.rs`

- [ ] **Step 1: 在 process_chat_loop 的每次 LLM 调用前设置 model 和 provider**

找到 loop_runner 中发起 LLM 请求的位置，在请求前调用：

```rust
logging::context::set_current_model(model_name.to_string());
logging::context::set_current_provider(provider_name.to_string());
logging::context::set_current_role("default".to_string()); // 主 agent
```

> 当前只有 sub-agent 的 `setup.rs` 调用了 `set_current_model`，主 loop 缺失。这是 `model` 字段恒为 `-` 的根因。

- [ ] **Step 2: 在每次 LLM 调用前生成并设置 request_id**

```rust
let request_id = uuid::Uuid::now_v7().to_string();
logging::context::set_current_request_id(request_id.clone());
```

- [ ] **Step 3: 验证编译通过**

Run: `cargo build -p runtime`
Expected: 编译成功

- [ ] **Step 4: Commit**

```bash
git add agent/features/runtime/src/business/chat/looping/loop_runner.rs
git commit -m "fix(logging): 主对话 loop 补全 set_current_model/provider/request_id/role"
```

---

## PR4: input/output 合并到 provider + event_type

### Task 4.1: log_input / log_output 改为 provider target + event_type

**Files:**
- Modify: `packages/global/logging/src/unified_logger.rs`
- Modify: `agent/features/runtime/src/business/agent/runner/loop_run.rs`

- [ ] **Step 1: UnifiedLogger::log_input 改用 event_type**

找到 `log_input` 方法，将写入逻辑改为：
- target 设为 `aemeath:agent:provider`（而非独立 input.log）
- event_type 设为 `"llm_input"`（区分 user_input）
- 行 schema 使用 14 字段格式

- [ ] **Step 2: UnifiedLogger::log_user_input 改用 event_type**

同上，event_type 设为 `"user_input"`。

- [ ] **Step 3: UnifiedLogger::log_output 改用 event_type**

target 设为 `aemeath:agent:provider`，event_type 设为 `"llm_output"`。

- [ ] **Step 4: loop_run.rs 中 role 注入**

`loop_run.rs:206/226` 的 `log_input`/`log_output` 调用已传 `role_name_for_log`，确保新的 context `set_current_role` 与此一致。

- [ ] **Step 5: 验证编译通过**

Run: `cargo build --workspace`
Expected: 编译成功

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor(logging): input/output 合并到 agent-provider.log，用 event_type 区分"
```

### Task 4.2: provider [LLM REQUEST/RESPONSE] 改 preview 策略

**Files:**
- Modify: `agent/features/provider/src/core/client.rs`（line 321/368）

- [ ] **Step 1: 将 DEBUG 级 pretty-print 完整 messages 改为 preview**

找到 `client.rs` 中 `[LLM REQUEST]` / `[LLM RESPONSE]` 的 `log::debug!` 调用（约 line 321/368），将 `serde_json::to_string_pretty(&messages)` 改为摘要：

```rust
// 旧
log::debug!(target: LOG_TARGET, "[LLM REQUEST] {}", serde_json::to_string_pretty(&request_body).unwrap());

// 新
log::debug!(
    target: LOG_TARGET,
    event_type = "llm_request_start";
    "[LLM REQUEST] model={} messages={} tools={} preview={}",
    model_name,
    messages.len(),
    tools_count,
    preview_messages(&messages), // 只记 role + 前 100 字符
);
```

> `preview_messages` 是一个辅助函数，遍历 messages 只记 `role` + content 前 100 字符 + 总长度。

- [ ] **Step 2: 实现 preview_messages 辅助函数**

在 `client.rs` 或 `llm_log.rs` 中新增：

```rust
fn preview_messages(messages: &[Value]) -> String {
    let previews: Vec<String> = messages.iter().map(|m| {
        let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("?");
        let content = m.get("content").and_then(|c| c.as_str()).unwrap_or("");
        let preview = if content.len() > 100 { &content[..100] } else { content };
        format!("{}({}chars):{}", role, content.len(), preview)
    }).collect();
    previews.join(" | ")
}
```

- [ ] **Step 3: 验证编译通过**

Run: `cargo build -p provider`
Expected: 编译成功

- [ ] **Step 4: Commit**

```bash
git add agent/features/provider/src/core/client.rs
git commit -m "perf(logging): provider LLM REQUEST/RESPONSE 改 preview 策略避免 85MB 日志"
```

---

## PR5: 架构守卫加固

### Task 5.1: target_guard.rs 加固（Rust test 层）

**Files:**
- Modify: `packages/global/logging/src/target_guard.rs`

- [ ] **Step 1: 新增 target 值白名单校验函数**

```rust
/// 检查源码中所有 target: "xxx" 的值是否 ∈ ALLOWED_TARGETS 或以其为前缀。
fn validate_target_values(source: &str) -> Vec<String> {
    let mut violations = Vec::new();
    for line in source.lines() {
        if let Some(target_val) = extract_target_value(line) {
            if !is_valid_target(&target_val) {
                violations.push(format!("invalid target: \"{}\"", target_val));
            }
        }
    }
    violations
}

fn extract_target_value(line: &str) -> Option<String> {
    // 解析 target: "xxx" 的字符串值
}

fn is_valid_target(target: &str) -> bool {
    // 必须以 aemeath: 开头
    // split(':') 最多 3 段
    // 前缀 ∈ ALLOWED_TARGETS
}
```

- [ ] **Step 2: 每个 crate 的测试函数改为检查 target 值合法性**

将现有 `check_layer(dir, prefix)` 升级为同时检查：
1. 裸 `log::xxx!`（无 target）→ 违规
2. `target:` 值不是 `aemeath:` 前缀 → 违规
3. `target:` 值超过 3 级 → 违规

- [ ] **Step 3: 运行守卫测试**

Run: `cargo test -p logging --lib target_guard`
Expected: 全部 PASS

- [ ] **Step 4: Commit**

```bash
git add packages/global/logging/src/target_guard.rs
git commit -m "feat(logging): target_guard 加固为校验 aemeath: 白名单 + 深度限制"
```

### Task 5.2: 新增 shell 守卫脚本

**Files:**
- Create: `.agents/hooks/check-log-target-prefix.sh`
- Modify: `.agents/hooks/check-architecture-guards.sh`

- [ ] **Step 1: 创建 check-log-target-prefix.sh**

```bash
#!/usr/bin/env bash
# 检查所有 .rs 生产代码的 log::xxx! target 值是否以 aemeath: 开头
set -euo pipefail

PROJECT_DIR="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "$0")/../.." && pwd)}"

violations=$(rg 'target:\s*"[^"]*"' \
  --type rust \
  "$PROJECT_DIR" \
  -g '!packages/global/logging/src/**' \
  -g '!**/tests/**' \
  -g '!**/*test*.rs' \
  -g '!target/**' \
  | grep -v 'aemeath:' \
  || true)

if [ -n "$violations" ]; then
  echo "✗ log target must start with 'aemeath:'" >&2
  echo "$violations" >&2
  exit 1
fi

echo "✓ log target prefix check passed"
```

- [ ] **Step 2: 赋予执行权限**

Run: `chmod +x .agents/hooks/check-log-target-prefix.sh`

- [ ] **Step 3: 注册到 check-architecture-guards.sh**

在 `check-architecture-guards.sh` 的编排列表中新增一行调用 `check-log-target-prefix.sh`。

- [ ] **Step 4: 运行守卫验证**

Run: `./.agents/hooks/check-log-target-prefix.sh`
Expected: `✓ log target prefix check passed`

- [ ] **Step 5: Commit**

```bash
git add .agents/hooks/check-log-target-prefix.sh .agents/hooks/check-architecture-guards.sh
git commit -m "feat(guard): 新增 check-log-target-prefix.sh shell 守卫"
```

### Task 5.3: 更新架构守卫注册表文档

**Files:**
- Modify: `docs/design/02-architecture-guards.md`

- [ ] **Step 1: 在守卫注册表中补一行**

记录新增的 `check-log-target-prefix.sh` 守卫：作用、触发时机、失败排查指引。

- [ ] **Step 2: Commit**

```bash
git add docs/design/02-architecture-guards.md
git commit -m "docs(guard): 注册表补充 check-log-target-prefix 守卫"
```

---

## PR6: specs/logging.md 规范文档 + 触发表更新

### Task 6.1: 创建 specs/logging.md 日志规范

**Files:**
- Create: `specs/logging.md`

- [ ] **Step 1: 编写日志规范文档**

文档结构：

```markdown
# 日志规范

## target 命名规则
- 统一前缀 aemeath:，最长三级
- 12 个合法 target 白名单
- 各 crate LOG_TARGET 常量定义要求

## 日志文件职责
- 每个文件的来源 crate、记录什么、不记录什么

## 14 字段 schema
- 完整字段表（类型、格式、来源、示例）
- 本地时间 RFC3339

## event_type 枚举
- lifecycle 事件清单 + 各事件的语义和字段约束

## 日志级别策略
- ERROR/WARN/INFO/DEBUG/TRACE 各自该记什么
- INFO: 用户排障
- DEBUG: 开发调试
- TRACE: chunk/token 级

## preview/脱敏策略
- 默认只记长度+hash+有限 preview
- 原文仅 DEBUG/TRACE

## request_id 生命周期
- 生成时机、贯穿范围、清理时机

## 废弃文件说明
- tool.log / audit.log / input.log / output.log 的废弃原因
```

- [ ] **Step 2: Commit**

```bash
git add specs/logging.md
git commit -m "docs(logging): 新增日志规范分片 specs/logging.md"
```

### Task 6.2: 更新 AGENTS.md 触发表

**Files:**
- Modify: `AGENTS.md`

- [ ] **Step 1: 在触发表中新增 logging 分片行**

在架构地图与触发表中新增：

```markdown
| `specs/logging.md` | `packages/global/logging/**`、全仓库 `log::xxx!` 调用点 —— 日志 target 命名、14 字段 schema、event_type 枚举、级别策略 | 新增/修改 log 调用、改日志路由、改 schema 字段 |
```

- [ ] **Step 2: 更新运行时目录章节的日志文件列表**

将旧的 11 文件列表更新为新的 12 文件列表，标注废弃文件。

- [ ] **Step 3: Commit**

```bash
git add AGENTS.md
git commit -m "docs(agents): 触发表新增 logging 分片 + 更新日志文件列表"
```

### Task 6.3: 全量验证

- [ ] **Step 1: cargo build --workspace**

Run: `cargo build --workspace`
Expected: 编译成功

- [ ] **Step 2: cargo clippy --workspace**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: 无警告

- [ ] **Step 3: cargo test --workspace**

Run: `cargo test --workspace`
Expected: 全部 PASS

- [ ] **Step 4: 守卫脚本验证**

Run: `./.agents/hooks/check-architecture-guards.sh`
Expected: 全部通过

- [ ] **Step 5: grep 验证无旧前缀残留**

Run: `rg 'target:\s*"(runtime|provider|tools|prompt|hook|storage|cli)::' --type rust -g '!target/**'`
Expected: 无输出

Run: `rg 'target:\s*"[^a]' --type rust -g '!target/**' -g '!packages/global/logging/**'`
Expected: 无输出（所有 target 以 `aemeath:` 开头）

- [ ] **Step 6: 创建 PR**

```bash
git push origin feat/logging-redesign-303
gh pr create --title "feat(logging): 日志系统重设计 — target 统一 + 14 字段 schema + 守卫加固 (#303)" --body "..."
```

---

## Self-Review

### Spec coverage
- ✅ target 统一 aemeath: 前缀 → PR2 (router) + PR3 (批量改)
- ✅ runtime feature 不拆子路径，靠 role 区分 → PR3 (LOG_TARGET) + Task 3.5
- ✅ 时间戳改本地 → Task 2.2
- ✅ 版本号字段 ver → Task 2.1 (context) + PR1 (workspace.package)
- ✅ request_id → Task 2.1 (context) + Task 3.5 (注入)
- ✅ provider 字段 → Task 2.1 + Task 3.5
- ✅ role 字段（主 default）→ Task 2.1 + Task 3.5
- ✅ event_type 枚举 → Task 4.1
- ✅ input/output 合并到 provider → Task 4.1
- ✅ 死代码清理（tool.log/audit.log/text.rs）→ Task 2.3/2.4
- ✅ 守卫加固 → PR5
- ✅ 常量方案（非宏）→ PR3 (LOG_TARGET)
- ✅ cargo-release → Task 1.3
- ✅ 规范文档 → PR6

### Type consistency
- `LOG_TARGET` 常量名统一（非 `RT_LOG_TARGET` / `TUI_LOG_TARGET`）
- `set_current_provider` / `set_current_request_id` / `set_current_role` 命名与前缀一致
- `target_to_file` 返回 `&str`，与现有路由函数签名一致
