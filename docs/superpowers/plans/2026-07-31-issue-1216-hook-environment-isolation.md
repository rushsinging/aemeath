# Hook 子进程环境隔离 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Hook 子进程清空父环境，只接收固定基础白名单与当前 invocation 兼容变量，并删除 Runtime 任意注入环境的旁路。

**Architecture:** Hook adapter 是环境策略的唯一 owner：私有 environment adapter 捕获 `PATH/HOME/SHELL/LANG/LC_ALL/TERM`，Dispatcher 按当前 `HookInvocation + cwd` 生成兼容变量，Executor 合并后构造 `ProcessRequest`。ProcessDriver 无条件 `env_clear`，Runtime 继续只传 invocation、cwd 与 cancellation。

**Tech Stack:** Rust 2021、Tokio `Command`、async-trait、serde_json、现有 Hook Hexagonal adapter、Cargo workspace tests。

---

## 文件结构与职责

- Create: `agent/features/hook/src/adapters/environment.rs` — 固定基础白名单名称与生产环境捕获。
- Create: `agent/features/hook/src/adapters/environment_tests.rs` — 基础白名单纯映射测试。
- Modify: `agent/features/hook/src/adapters.rs` — 注册私有 environment adapter。
- Modify: `agent/features/hook/src/adapters/process.rs` — 子进程创建时无条件清空父环境。
- Modify: `agent/features/hook/src/adapters/process_tests.rs` — 证明父环境不可见、请求环境可见，并保留资源回收回归。
- Modify: `agent/features/hook/src/adapters/dispatcher/executor.rs` — 只在执行边界合并基础环境与 invocation 环境。
- Modify: `agent/features/hook/src/adapters/dispatcher/fake.rs` — 测试记录 cwd 与完整 invocation 环境。
- Modify: `agent/features/hook/src/adapters/dispatcher.rs` — Dispatcher 生产构造捕获基础白名单；每次调用重新生成 invocation 环境。
- Modify: `agent/features/hook/src/adapters/dispatcher/tests.rs` — 连续 workspace/HookPoint 与 StopFailure 隔离测试。
- Modify: `agent/features/hook/src/ports.rs` — `HookDispatchContext` 收窄为 cwd-only。
- Modify: `agent/features/hook/src/adapters/config.rs` — `build_dispatcher` 删除裸环境参数。
- Modify: `agent/features/hook/src/adapters/config_tests.rs` — 固化无环境旁路的生产 factory 契约。
- Modify: `agent/composition/src/runtime.rs`、`agent/composition/tests/main_session_wiring.rs` — 适配唯一生产接线。
- Modify: `agent/features/runtime/**` 中仅测试/测试辅助的 `build_dispatcher` 调用 — 适配新签名，不改变 Runtime 生产语义。
- Modify: `docs/design/02-modules/hook/README.md` — 回写固定白名单与不支持 Config 扩展的最终安全语义。
- Modify: `docs/design/02-modules/hook/01-run-loop-integration.md` — 删除环境隔离仍待承接的过期描述。
- Modify: `docs/design/03-engineering/03-migration-governance.md` — 将环境隔离从剩余差距更新为已落地事实。
- Modify: `specs/policy-hook-audit.md` — 修正已退役 legacy 路径并记录 adapter-owned 环境策略。

### Task 1: ProcessDriver 强制清空父环境

**Files:**
- Modify: `agent/features/hook/src/adapters/process_tests.rs:1-55`
- Modify: `agent/features/hook/src/adapters/process.rs:81-92`

- [ ] **Step 1: 写父环境隔离失败测试**

在 `process_tests.rs` 增加唯一变量名测试。变量名包含 UUID，避免与其他测试碰撞；Guard 在 Drop 时恢复父进程状态。

```rust
struct ParentEnvironmentGuard {
    key: String,
    previous: Option<std::ffi::OsString>,
}

impl ParentEnvironmentGuard {
    fn set(key: String, value: &str) -> Self {
        let previous = std::env::var_os(&key);
        unsafe { std::env::set_var(&key, value) };
        Self { key, previous }
    }
}

impl Drop for ParentEnvironmentGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(&self.key, value),
                None => std::env::remove_var(&self.key),
            }
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn child_sees_only_request_environment() {
    let inherited_key = format!(
        "AEMEATH_HOOK_UNAPPROVED_{}",
        uuid::Uuid::new_v4().simple()
    );
    let _guard = ParentEnvironmentGuard::set(inherited_key.clone(), "parent-secret");
    let approved_key = "AEMEATH_HOOK_TEST_APPROVED";
    let mut process_request = request(format!(
        "printf '%s|%s' \"${{{inherited_key}-missing}}\" \"${{{approved_key}-missing}}\""
    ));
    process_request
        .env
        .insert(approved_key.to_string(), "approved-value".to_string());

    let output = ProcessDriver
        .execute(process_request, &CancellationToken::new())
        .await
        .expect("隔离环境下命令应正常退出");

    assert_eq!(output.stdout, b"missing|approved-value");
}
```

- [ ] **Step 2: 运行定向测试并确认红灯**

Run:

```bash
cargo test -p hook child_sees_only_request_environment -- --exact
```

Expected: FAIL，stdout 为 `parent-secret|approved-value`，证明子进程仍继承父环境。

- [ ] **Step 3: 在 ProcessDriver 加入最小隔离实现**

在 `Command` builder 的 `.stderr(...)` 与 `.envs(...)` 之间加入：

```rust
.env_clear()
.envs(&request.env)
```

确保顺序是先清空、后注入请求环境。

- [ ] **Step 4: 运行 ProcessDriver 全部测试**

Run:

```bash
cargo test -p hook adapters::process::tests
```

Expected: 6 tests PASS；正常退出、并发 drain、截断、timeout、cancel 和 TERM→KILL→wait 均不回归。

- [ ] **Step 5: 提交 ProcessDriver 隔离补丁**

```bash
git add agent/features/hook/src/adapters/process.rs \
  agent/features/hook/src/adapters/process_tests.rs
git commit -m "fix(hook): #1216 清空子进程父环境" \
  -m "让 ProcessDriver 只注入 ProcessRequest 明确提供的变量，并保留进程组回收与输出截断契约。" \
  -m "Refs #1216" \
  -m "Co-Authored-By: Aemeath (OpenAI/gpt-5.6) <github:rushsinging/aemeath>"
```

### Task 2: 建立 Hook-owned 基础环境白名单

**Files:**
- Create: `agent/features/hook/src/adapters/environment.rs`
- Create: `agent/features/hook/src/adapters/environment_tests.rs`
- Modify: `agent/features/hook/src/adapters.rs:1-5`
- Modify: `agent/features/hook/src/adapters/dispatcher/executor.rs:95-141`
- Modify: `agent/features/hook/src/adapters/dispatcher.rs:61-110`

- [ ] **Step 1: 写基础白名单纯映射测试**

创建 `environment_tests.rs`：

```rust
use std::collections::HashMap;

use super::{basic_environment_from, BASIC_ENVIRONMENT_VARIABLES};

#[test]
fn basic_environment_keeps_only_present_allowed_variables() {
    let source = HashMap::from([
        ("PATH", "/usr/bin"),
        ("HOME", "/home/test"),
        ("GITHUB_TOKEN", "secret"),
    ]);

    let environment = basic_environment_from(|name| {
        source.get(name).map(|value| (*value).to_string())
    });

    assert_eq!(
        BASIC_ENVIRONMENT_VARIABLES,
        ["PATH", "HOME", "SHELL", "LANG", "LC_ALL", "TERM"]
    );
    assert_eq!(environment.get("PATH").map(String::as_str), Some("/usr/bin"));
    assert_eq!(environment.get("HOME").map(String::as_str), Some("/home/test"));
    assert!(!environment.contains_key("SHELL"));
    assert!(!environment.contains_key("GITHUB_TOKEN"));
}
```

- [ ] **Step 2: 运行测试并确认编译红灯**

Run:

```bash
cargo test -p hook basic_environment_keeps_only_present_allowed_variables -- --exact
```

Expected: FAIL to compile，因为 `adapters::environment` 与目标函数尚不存在。

- [ ] **Step 3: 实现私有 environment adapter**

创建 `environment.rs`：

```rust
use std::collections::HashMap;

pub(super) const BASIC_ENVIRONMENT_VARIABLES: [&str; 6] =
    ["PATH", "HOME", "SHELL", "LANG", "LC_ALL", "TERM"];

pub(super) fn capture_basic_environment() -> HashMap<String, String> {
    basic_environment_from(|name| std::env::var(name).ok())
}

fn basic_environment_from(
    mut read: impl FnMut(&str) -> Option<String>,
) -> HashMap<String, String> {
    BASIC_ENVIRONMENT_VARIABLES
        .into_iter()
        .filter_map(|name| read(name).map(|value| (name.to_string(), value)))
        .collect()
}

#[cfg(test)]
#[path = "environment_tests.rs"]
mod tests;
```

在 `adapters.rs` 注册私有模块：

```rust
pub mod config;
pub mod dispatcher;
mod environment;
pub(crate) mod process;
```

为 sibling test 可见性，将 `basic_environment_from` 设为 `pub(super)`；最终签名应为：

```rust
pub(super) fn basic_environment_from(
    mut read: impl FnMut(&str) -> Option<String>,
) -> HashMap<String, String>
```

- [ ] **Step 4: 让 Dispatcher 生产构造捕获固定白名单**

保持 `ProcessDriverExecutor` 持有基础环境，但将调用方传入改为 Hook adapter 自行捕获。

`Dispatcher::try_new` 改为：

```rust
pub fn try_new(
    subscriptions: Vec<HookSubscription>,
) -> Result<Self, Vec<SubscriptionError>> {
    Self::build(
        subscriptions,
        Box::new(ProcessDriverExecutor::new(
            crate::adapters::environment::capture_basic_environment(),
        )),
    )
}
```

保留 `ProcessDriverExecutor::new(environment)` 为 `pub(crate)` 技术入口；其合并顺序继续为基础环境在前、invocation 环境在后，因此当前调用变量覆盖同名基础键。

- [ ] **Step 5: 运行 Hook adapter 测试**

Run:

```bash
cargo test -p hook basic_environment_keeps_only_present_allowed_variables
cargo test -p hook adapters::dispatcher::tests
```

Expected: 两条命令均 PASS。

- [ ] **Step 6: 运行配置环境 Guard**

Run:

```bash
bash .agents/hooks/check-config-env-guard.sh
```

Expected: `Config env guard OK.`；基础系统变量读取不被误判为业务 Config env。

- [ ] **Step 7: 提交基础环境白名单**

```bash
git add agent/features/hook/src/adapters.rs \
  agent/features/hook/src/adapters/environment.rs \
  agent/features/hook/src/adapters/environment_tests.rs \
  agent/features/hook/src/adapters/dispatcher.rs \
  agent/features/hook/src/adapters/dispatcher/executor.rs
git commit -m "refactor(hook): #1216 收口基础环境白名单" \
  -m "由 Hook adapter 固定捕获 PATH、HOME、SHELL、LANG、LC_ALL 与 TERM，禁止调用方提供任意基础环境。" \
  -m "Refs #1216" \
  -m "Co-Authored-By: Aemeath (OpenAI/gpt-5.6) <github:rushsinging/aemeath>"
```

### Task 3: 收窄 HookDispatchContext 并验证 invocation 隔离

**Files:**
- Modify: `agent/features/hook/src/ports.rs:6-43`
- Modify: `agent/features/hook/src/adapters/dispatcher.rs:131-420`
- Modify: `agent/features/hook/src/adapters/dispatcher/fake.rs:37-145`
- Modify: `agent/features/hook/src/adapters/dispatcher/tests.rs`

- [ ] **Step 1: 扩展 Scripted fake 的相邻边界记录**

让 `ScriptedCall` 记录环境和 cwd：

```rust
#[derive(Debug, Clone)]
pub(super) struct ScriptedCall {
    pub command: String,
    pub stdin: serde_json::Value,
    pub cwd: std::path::PathBuf,
    pub env: std::collections::HashMap<String, String>,
}
```

在 `Executor for Scripted` 中使用真实参数名并记录：

```rust
async fn execute(
    &self,
    command: &HookCommand,
    stdin: &serde_json::Value,
    cwd: &std::path::Path,
    env: &std::collections::HashMap<String, String>,
    _timeout: Duration,
    _cancellation: &dyn CancellationSignal,
) -> Result<RawExecution, ExecutionFault> {
    self.calls
        .lock()
        .expect("scripted calls lock")
        .push(ScriptedCall {
            command: command.command.clone(),
            stdin: stdin.clone(),
            cwd: cwd.to_path_buf(),
            env: env.clone(),
        });
    // 保留现有回放逻辑
}
```

- [ ] **Step 2: 写连续 workspace 与 HookPoint 隔离测试**

在 `dispatcher/tests.rs` 导入 `HookDispatchContext`，新增：

```rust
#[tokio::test]
async fn consecutive_dispatches_use_only_current_workspace_and_payload_environment() {
    let subscriptions = vec![
        sub(HookPoint::PreToolUse, "tool"),
        sub(HookPoint::Stop, "stop"),
    ];
    let scripted = Scripted::from_steps([
        ScriptStep::ok_exit(0, ""),
        ScriptStep::ok_exit(0, ""),
    ]);
    let dispatcher = Dispatcher::with_scripted(subscriptions, scripted.clone());
    let first_workspace = std::path::PathBuf::from("/tmp/aemeath-workspace-a");
    let second_workspace = std::path::PathBuf::from("/tmp/aemeath-workspace-b");

    dispatcher
        .dispatch_at(
            pre_tool_use("Bash"),
            HookDispatchContext::new(&first_workspace),
            &CancellationToken::new(),
        )
        .await;
    dispatcher
        .dispatch_at(
            stop(7),
            HookDispatchContext::new(&second_workspace),
            &CancellationToken::new(),
        )
        .await;

    let calls = scripted.calls();
    assert_eq!(calls[0].cwd, first_workspace);
    assert_eq!(calls[0].env["AEMEATH_HOOK_EVENT"], "\"PreToolUse\"");
    assert_eq!(calls[0].env["AEMEATH_PROJECT_DIR"], "/tmp/aemeath-workspace-a");
    assert_eq!(calls[0].env["CLAUDE_PROJECT_DIR"], "/tmp/aemeath-workspace-a");
    assert_eq!(calls[0].env["AEMEATH_TOOL_NAME"], "Bash");
    assert!(!calls[0].env.contains_key("AEMEATH_STOP_TURNS"));

    assert_eq!(calls[1].cwd, second_workspace);
    assert_eq!(calls[1].env["AEMEATH_HOOK_EVENT"], "\"Stop\"");
    assert_eq!(calls[1].env["AEMEATH_PROJECT_DIR"], "/tmp/aemeath-workspace-b");
    assert_eq!(calls[1].env["AEMEATH_STOP_TURNS"], "7");
    assert!(!calls[1].env.contains_key("AEMEATH_TOOL_NAME"));
    assert!(!calls[1].env.contains_key("AEMEATH_TOOL_INPUT"));
}
```

- [ ] **Step 3: 写 StopFailure 不继承 Stop-only 变量测试**

复用现有 Stop exhaustion 流程，新增：

```rust
#[tokio::test]
async fn stop_failure_rebuilds_environment_without_stop_only_variables() {
    let subscriptions = vec![
        sub(HookPoint::Stop, "stop"),
        sub(HookPoint::StopFailure, "observe"),
    ];
    let scripted = Scripted::from_steps([
        ScriptStep::fault(ExecutionFault::Timeout),
        ScriptStep::fault(ExecutionFault::Timeout),
        ScriptStep::fault(ExecutionFault::Timeout),
        ScriptStep::ok_exit(0, ""),
    ]);
    let dispatcher = Dispatcher::with_scripted(subscriptions, scripted.clone());

    dispatcher
        .dispatch_at(
            stop(9),
            HookDispatchContext::new("/tmp/aemeath-stop-workspace"),
            &CancellationToken::new(),
        )
        .await;

    let calls = scripted.calls();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[3].env["AEMEATH_HOOK_EVENT"], "\"StopFailure\"");
    assert_eq!(calls[3].env["AEMEATH_PROJECT_DIR"], "/tmp/aemeath-stop-workspace");
    assert!(!calls[3].env.contains_key("AEMEATH_STOP_TURNS"));
    assert_eq!(calls[3].stdin["StopFailure"]["turns"], 9);
}
```

- [ ] **Step 4: 运行测试确认现有接口下至少一个红灯**

Run:

```bash
cargo test -p hook consecutive_dispatches_use_only_current_workspace_and_payload_environment
cargo test -p hook stop_failure_rebuilds_environment_without_stop_only_variables
```

Expected: 新测试在 ScriptedCall 尚未记录 env/cwd 或 StopFailure 仍使用旧 context env 路径时 FAIL/无法编译。

- [ ] **Step 5: 将 HookDispatchContext 收窄为 cwd-only**

`ports.rs` 删除 `HashMap` import、`env` 字段、`with_env` 和 `env` accessor，保留：

```rust
#[derive(Debug, Clone)]
pub struct HookDispatchContext {
    cwd: PathBuf,
}

impl HookDispatchContext {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self { cwd: cwd.into() }
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }
}
```

同步删除代码注释中的外部追踪号，改为稳定责任描述。

- [ ] **Step 6: 让 Dispatcher 每次从空 invocation map 重建变量**

将 `invocation_environment` 改为不接受 base：

```rust
fn invocation_environment(
    invocation: &HookInvocation,
    cwd: &std::path::Path,
) -> HashMap<String, String> {
    let mut env = HashMap::new();
    // 保留现有 AEMEATH_HOOK_EVENT、项目目录与 payload match 插入逻辑
    env
}
```

普通 dispatch 调用改为：

```rust
let invocation_env = invocation_environment(&current_invocation, context.cwd());
```

`dispatch_stop_failure` 删除 `env` 参数；内部必须根据新建的 `HookInvocation::StopFailure` 调用：

```rust
let invocation_env = invocation_environment(&invocation, cwd);
```

- [ ] **Step 7: 运行 Dispatcher 全部测试**

Run:

```bash
cargo test -p hook adapters::dispatcher::tests
```

Expected: 全部 PASS，包括连续 workspace/HookPoint 与 StopFailure 环境隔离测试。

- [ ] **Step 8: 验证任意环境入口零引用**

Run:

```bash
rg 'with_env\(|context\.env\(\)|HookDispatchContext.*env' agent/features/hook agent/features/runtime agent/composition
```

Expected: 无匹配，exit code 1。

- [ ] **Step 9: 提交 invocation 环境收口**

```bash
git add agent/features/hook/src/ports.rs \
  agent/features/hook/src/adapters/dispatcher.rs \
  agent/features/hook/src/adapters/dispatcher/fake.rs \
  agent/features/hook/src/adapters/dispatcher/tests.rs
git commit -m "refactor(hook): #1216 隔离每次调用环境" \
  -m "将 HookDispatchContext 收窄为 cwd-only，并按当前 invocation 重新生成事件、项目目录与 payload 兼容变量。" \
  -m "Refs #1216" \
  -m "Co-Authored-By: Aemeath (OpenAI/gpt-5.6) <github:rushsinging/aemeath>"
```

### Task 4: 删除生产 factory 的裸环境参数并贯通 Composition

**Files:**
- Modify: `agent/features/hook/src/adapters/config.rs:37-50`
- Modify: `agent/features/hook/src/adapters/config_tests.rs`
- Modify: `agent/composition/src/runtime.rs:93-100`
- Modify: `agent/composition/tests/main_session_wiring.rs:417-424`
- Modify: `agent/features/runtime/tests/bootstrap_dependencies.rs`
- Modify: `agent/features/runtime/src/application/prompt/build/prompt_build_tests.rs`
- Modify: `agent/features/runtime/src/application/prompt/prompt_build_ext.rs`
- Modify: `agent/features/runtime/src/application/hook/stop_coordination_tests.rs`
- Modify: `agent/features/runtime/src/application/client/from_args.rs`（仅 `#[cfg(test)]` 区域）
- Modify: `agent/features/runtime/src/application/loop_engine/chat/pre_compact_trigger_tests.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/loop_runner_tests.rs`

- [ ] **Step 1: 写 Hook factory 无环境参数的编译契约测试**

在 `config_tests.rs` 导入 `build_dispatcher`，新增：

```rust
#[test]
fn build_dispatcher_owns_process_environment_policy() {
    let dispatcher = build_dispatcher(&HooksConfig::default())
        .expect("默认 Hook 配置应构造生产 Dispatcher");
    let _hook_port: &dyn crate::HookPort = &dispatcher;
}
```

- [ ] **Step 2: 运行测试并确认签名红灯**

Run:

```bash
cargo test -p hook build_dispatcher_owns_process_environment_policy -- --exact
```

Expected: FAIL to compile，当前 `build_dispatcher` 仍要求第二个 `HashMap` 参数。

- [ ] **Step 3: 收窄 Hook production factory**

将 `build_dispatcher` 改为：

```rust
pub fn build_dispatcher(
    config: &HooksConfig,
) -> Result<Dispatcher, Vec<SubscriptionError>> {
    let subscriptions = subscriptions_from_config(config);
    log::debug!(
        target: crate::LOG_TARGET,
        "hook dispatcher built: configured_events={} subscriptions={}",
        config.events.len(),
        subscriptions.len(),
    );
    Dispatcher::try_new(subscriptions)
}
```

删除 `adapters/config.rs` 不再需要的 `HashMap` import。

- [ ] **Step 4: 更新 Composition 唯一生产接线**

`agent/composition/src/runtime.rs` 改为：

```rust
let hook_runner: Arc<dyn hook::HookPort> = Arc::new(
    hook::build_dispatcher(config.reader().committed_snapshot().hooks())
        .map_err(|errors| sdk::SdkError::Init(format!("Hook 配置初始化失败：{errors:?}")))?,
);
```

`agent/composition/tests/main_session_wiring.rs` 改为：

```rust
let hook_runner: Arc<dyn hook::HookPort> = Arc::new(
    hook::build_dispatcher(&share::config::hooks::HooksConfig::default())
        .expect("test hook dispatcher"),
);
```

- [ ] **Step 5: 机械更新所有测试 factory 调用**

对上述 Runtime 测试文件执行同一规则：

```rust
hook::build_dispatcher(&HooksConfig::default(), std::collections::HashMap::new())
```

替换为：

```rust
hook::build_dispatcher(&HooksConfig::default())
```

并将：

```rust
hook::build_dispatcher(&HooksConfig { events }, HashMap::new())
```

替换为：

```rust
hook::build_dispatcher(&HooksConfig { events })
```

删除因此变成未使用的 `HashMap` import；仍被测试数据使用的 import 必须保留。

- [ ] **Step 6: 验证旧双参数调用零引用**

Run:

```bash
rg -n -U 'build_dispatcher\([\s\S]{0,180}HashMap::new\(\)' agent
```

Expected: 无匹配，exit code 1。

- [ ] **Step 7: 运行 Hook、Composition、Runtime 相邻测试**

Run:

```bash
cargo test -p hook
cargo test -p composition main_session
cargo test -p runtime bootstrap_dependencies
```

Expected: 全部 PASS。

- [ ] **Step 8: 运行 Hook 装配所有权 Guard**

Run:

```bash
bash .agents/hooks/check-runtime-hook-assembly-ownership.sh
```

Expected: `Runtime Hook assembly ownership guard OK.`；Runtime 生产代码未构造 Dispatcher，Composition 仍是唯一 owner。

- [ ] **Step 9: 提交 factory 与装配接线**

```bash
git add agent/features/hook/src/adapters/config.rs \
  agent/features/hook/src/adapters/config_tests.rs \
  agent/composition/src/runtime.rs \
  agent/composition/tests/main_session_wiring.rs \
  agent/features/runtime/tests/bootstrap_dependencies.rs \
  agent/features/runtime/src/application/prompt/build/prompt_build_tests.rs \
  agent/features/runtime/src/application/prompt/prompt_build_ext.rs \
  agent/features/runtime/src/application/hook/stop_coordination_tests.rs \
  agent/features/runtime/src/application/client/from_args.rs \
  agent/features/runtime/src/application/loop_engine/chat/pre_compact_trigger_tests.rs \
  agent/features/runtime/src/application/loop_engine/chat/loop_runner_tests.rs
git commit -m "refactor(hook): #1216 删除环境注入旁路" \
  -m "让 Hook production factory 自主管理固定环境策略，并保持 Composition 是唯一生产装配 owner。" \
  -m "Refs #1216" \
  -m "Co-Authored-By: Aemeath (OpenAI/gpt-5.6) <github:rushsinging/aemeath>"
```

### Task 5: 回写 Target 文档与治理状态

**Files:**
- Modify: `docs/design/02-modules/hook/README.md:219-281`
- Modify: `docs/design/02-modules/hook/01-run-loop-integration.md:103`
- Modify: `docs/design/03-engineering/03-migration-governance.md:261-264,355`
- Modify: `specs/policy-hook-audit.md:16-21`

- [ ] **Step 1: 更新 Hook Target 安全不变量**

将 README §11 的 Config 扩展描述替换为本次确认的固定策略：

```markdown
- **env_clear**：Hook 子进程 **MUST** 清空父进程环境，只接收 Hook adapter 构造的环境；
- **基础白名单**：只从父进程复制 `PATH` / `HOME` / `SHELL` / `LANG` / `LC_ALL` / `TERM`，缺失项不注入；
- **按次变量**：`AEMEATH_PROJECT_DIR` / `CLAUDE_PROJECT_DIR`、`AEMEATH_HOOK_EVENT` 与已发布 payload 兼容变量 **MUST** 根据当前 invocation 重新生成；
- **无 Config 扩展**：当前不支持 Config 自定义 Hook 环境变量；未知父环境变量默认不可见；
- **NEVER 泄漏密钥**：API key、token、secret 等 **NEVER** 进入 Hook 子进程 env；
```

同步 §12 目录说明，把 environment adapter 纳入 `adapters/` 的技术职责，不在目标目录树中恢复 legacy 目录。

- [ ] **Step 2: 更新 Run Loop 集成与 Migration Governance**

在 `01-run-loop-integration.md` 删除“env_clear / 白名单仍由后续承接”的过期句子，改为当前事实：Runtime 只传 invocation/cwd，环境隔离由 Hook adapter 完成。

在 Migration Governance 的 PHA4/PHA7 剩余差距中移除环境隔离项，并在变更记录新增稳定领域描述：Hook 生产链使用固定基础白名单、按 invocation 环境和 ProcessDriver `env_clear`。`docs/design/**` 不写 Issue/PR 编号。

- [ ] **Step 3: 更新渐进式规范中的 Hook 路径事实**

将 `specs/policy-hook-audit.md` 的 legacy runner 描述改为：

```markdown
- Hook 进程执行 adapter：`agent/features/hook/src/adapters/process.rs`；环境白名单与 invocation 投影由 `agent/features/hook/src/adapters/**` 统一拥有。
- **Hook 执行环境 MUST** 清空父环境，仅注入固定基础白名单、当前 invocation 变量及 `AEMEATH_PROJECT_DIR` / `CLAUDE_PROJECT_DIR`；Runtime **NEVER** 拼装或扩展 Hook 环境。
```

- [ ] **Step 4: 检查文档无过期语义和外部编号**

Run:

```bash
rg -n 'Config 可扩展|env_clear / 白名单由|adapters/legacy|#1216' \
  docs/design/02-modules/hook \
  docs/design/03-engineering/03-migration-governance.md \
  specs/policy-hook-audit.md
```

Expected: 无过期匹配；`docs/design/**` 无新增外部 Issue 编号。

- [ ] **Step 5: 检查文档格式并提交**

Run:

```bash
git diff --check
```

Expected: PASS。

```bash
git add docs/design/02-modules/hook/README.md \
  docs/design/02-modules/hook/01-run-loop-integration.md \
  docs/design/03-engineering/03-migration-governance.md \
  specs/policy-hook-audit.md
git commit -m "docs(hook): 固化子进程环境安全边界" \
  -m "同步固定基础白名单、按次 invocation 投影、Runtime 禁止拼装环境与 ProcessDriver 清空父环境的最终事实。" \
  -m "Refs #1216" \
  -m "Co-Authored-By: Aemeath (OpenAI/gpt-5.6) <github:rushsinging/aemeath>"
```

### Task 6: 全量验证、死代码检查与交付记录

**Files:**
- Verify only；如验证发现真实缺陷，只修改对应 owner 文件并单独提交。

- [ ] **Step 1: 格式化代码**

Run:

```bash
cargo fmt --all
```

Expected: 命令成功；仅本计划涉及的 Rust 文件发生格式变化。

- [ ] **Step 2: 运行 Hook 完整测试**

Run:

```bash
cargo test -p hook
```

Expected: 全部 PASS，包含父环境隔离、基础白名单、连续 invocation、timeout/cancel 和输出截断。

- [ ] **Step 3: 运行跨层回归测试**

Run:

```bash
cargo test -p composition
cargo test -p runtime
```

Expected: 全部 PASS；#1397 合入后的统一 Loop 与 BoundaryOnly Hook adapter 无回归。

- [ ] **Step 4: 运行 Clippy**

Run:

```bash
cargo clippy -p hook -p composition -p runtime --all-targets -- -D warnings
```

Expected: PASS，无 warning。

- [ ] **Step 5: 运行完整架构守卫**

Run:

```bash
bash .agents/hooks/check-architecture-guards.sh
```

Expected: exit 0，所有 Guard PASS。

- [ ] **Step 6: 检查废弃入口与死代码**

Run:

```bash
rg -n 'with_env\(|context\.env\(\)|build_dispatcher\([^)]*,|ProcessDriverExecutor::new\([^)]*HashMap' \
  agent/features/hook agent/features/runtime agent/composition
cargo build -p hook -p composition -p runtime
```

Expected: `rg` 无旧环境旁路匹配；build PASS 且无仅测试托活生产代码 warning。

- [ ] **Step 7: 检查工作树与补充格式提交（仅在需要时）**

Run:

```bash
git diff --check
git status --short --branch
```

Expected: 无未预期文件。若 `cargo fmt` 产生尚未提交的目标文件格式变化：

```bash
git add agent/features/hook agent/features/runtime agent/composition
git commit -m "style(hook): 格式化环境隔离改动" \
  -m "Refs #1216" \
  -m "Co-Authored-By: Aemeath (OpenAI/gpt-5.6) <github:rushsinging/aemeath>"
```

- [ ] **Step 8: 更新 Issue 为待确认但不关闭**

使用 `gh issue comment 1216 --repo rushsinging/aemeath --body-file <临时文件>` 记录：

```markdown
## 实施结果

- Hook 子进程使用 `env_clear`，未知父环境变量默认不可见。
- 仅继承 `PATH/HOME/SHELL/LANG/LC_ALL/TERM`，并按当前 invocation 生成项目目录、事件与 payload 兼容变量。
- `HookDispatchContext` 已收窄为 cwd-only，Runtime 不再具有任意环境注入旁路。
- 按当前范围不支持 Config 自定义扩展环境变量。

## 验证证据

- `cargo test -p hook`
- `cargo test -p composition`
- `cargo test -p runtime`
- `cargo clippy -p hook -p composition -p runtime --all-targets -- -D warnings`
- `bash .agents/hooks/check-architecture-guards.sh`
- `git diff --check`

状态：待用户确认；Issue 不自动关闭。
```

Expected: 评论创建成功，Issue 保持 OPEN。

- [ ] **Step 9: 最终确认提交范围**

Run:

```bash
git log --oneline origin/main..HEAD
git status --short --branch
```

Expected: 设计提交加 4–5 个聚焦实施提交；工作树 clean。创建 PR 前必须按仓库流程先执行 `git pull origin main`、重新运行受影响门禁，并使用 PR 模板记录 Summary、Refs、Breaking change 与 Test plan。
