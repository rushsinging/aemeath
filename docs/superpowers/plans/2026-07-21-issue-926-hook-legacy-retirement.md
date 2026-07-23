# Issue #926 Hook Legacy Retirement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 删除 legacy `HookRunner` / `HookUi` / `hook::api`，使 Runtime 仅经 Hook crate-root 的 `HookPort` 与结构化 `HookOutcome` 执行、投影和展示 Hook。

**Architecture:** 在 Hook adapter 增加旧 `HooksConfig` 到 Hook-owned `HookSubscription` 的唯一转换与 `Dispatcher` 装配；Runtime 持有注入的 `Arc<dyn HookPort>`，经现有 `project_hook_outcome` 处理结构化 directive、execution 与 display messages。删除 legacy DTO、运行器和手工 JSON/exit-code 推断，并以零白名单 Guard 固化 crate-root façade 与 Runtime 消费边界。

**Tech Stack:** Rust、Tokio、async_trait、Cargo workspace、Shell/Python architecture guards、GitHub Issue #926。

---

## 前置约束与范围

- 开发基线、文档—代码差异和 Guard 预算已回填至 #926。
- 不在本计划处理：env_clear/环境白名单（#1216）；Stop 15 次上限、typed `StopHookRetryExhausted`、out-of-band cancel/terminate（#878）；Main/Sub 单 Loop（PHA9）；ConfigSnapshot 的 Hook/Stop 上限配置来源（需独立 tracking Issue）。
- 本计划不得新增 migration exception、路径豁免、行级 allow 或隐式 exclude；Hook 预算维持 0，repository migration debt 维持 6。
- 每个核心行为改动遵循 TDD：先添加失败测试、确认因目标能力未接入而失败、再做最小实现、再运行定向与受影响测试。

## 文件结构

| 路径 | 职责 / 改动 |
|---|---|
| `agent/features/hook/src/adapters/config.rs`（新建） | 旧 `HooksConfig` 到 Hook-owned subscriptions 的唯一兼容入站转换与 Dispatcher 工厂。 |
| `agent/features/hook/src/adapters.rs` | 声明 config adapter，移除 legacy module。 |
| `agent/features/hook/src/lib.rs` | 仅发布稳定 Hook PL、`HookPort` 与生产 Dispatcher 工厂；删除 `api`。 |
| `agent/features/hook/src/adapters/legacy/**`（删除） | 删除旧 DTO、Runner、JSON 解析、事件便捷方法及其测试。 |
| `agent/features/runtime/src/application/hook_adapter.rs` | 将 `hook::api::*` 类型引用改为 crate-root PL，不改变纯投影职责。 |
| `agent/features/runtime/src/application/**` | 把 `HookRunner` / `HookUi` 持有与手工推断切为 `Arc<dyn HookPort>` + `RuntimeHookDispatch`。 |
| `agent/features/runtime/src/application/main_loop/looping/hook_ui.rs`（删除或以纯 glue 替代） | 删除运行器直调与 `HookResult` / JSON 推断。 |
| `agent/features/runtime/src/adapters/runtime.rs`、`agent/shared/src/adapter/hook.rs` | 删除或将 Notification compatibility adapter 切到 HookPort。 |
| `.agents/hooks/check-hook-target-facade.sh`（新建） | 零白名单 Guard：禁止 legacy façade/re-export/Runtime `hook::api` 消费。 |
| `.agents/hooks/check-hook-target-facade-tests.sh`（新建） | 在临时副本注入反例，验证单 Guard exit 2。 |
| `.agents/hooks/check-architecture-guards.sh` | 注册新 Guard 至 fast/full 编排。 |
| `.agents/architecture-guard-registry.json` | 登记新 Target policy；仅在脚本确有测试排除时登记 scope exclusion。 |
| `docs/design/02-modules/hook/01-run-loop-integration.md` | 回写切换结果、验证证据与 OOS。 |
| `docs/design/03-engineering/01-architecture-guards.md` | 记录新 Guard 的范围、零白名单和故意违规证据。 |
| `docs/design/03-engineering/03-migration-governance.md` | 将 PHA4–PHA7 更新为最终 Current → Target 结果和仍存责任。 |

### Task 1: 建立 HooksConfig 入站转换与 Dispatcher 装配

**Files:**
- Create: `agent/features/hook/src/adapters/config.rs`
- Create: `agent/features/hook/src/adapters/config_tests.rs`
- Modify: `agent/features/hook/src/adapters.rs`
- Modify: `agent/features/hook/src/lib.rs`

- [ ] **Step 1: 写失败的转换测试**

覆盖 `HooksConfig` 的事件 key、空 matcher、工具 matcher、声明顺序、timeout 与 command 到 `HookSubscription` / `HookInvocation` 所需语义的映射；非法配置必须返回结构化 `SubscriptionError`，不静默丢弃。

- [ ] **Step 2: 运行 Hook crate 测试确认失败原因正确**

Run: `cargo test -p hook config_tests -- --nocapture`
Expected: 测试因转换/生产工厂尚不存在而失败，不得因测试编译错误失败。

- [ ] **Step 3: 实现唯一转换与生产 Dispatcher 工厂**

在 Hook adapter 内实现旧 Config 到 `Vec<HookSubscription>` 的映射，统一填充 `point`、`enabled`、`order`、matcher、command、timeout 与默认 failure policy；在同一 adapter 内创建 `Dispatcher::try_new` 所需 cwd/env 输入。不得将 Config DTO 或转换语义泄漏给 Runtime。

- [ ] **Step 4: 运行定向测试确认通过**

Run: `cargo test -p hook config_tests -- --nocapture`
Expected: 全部转换与非法配置测试通过。

- [ ] **Step 5: 核查 adapter 边界**

Run: `bash .agents/hooks/check-cola-layer-purity.sh && bash .agents/hooks/check-crate-api-boundary.sh`
Expected: exit 0；Hook ports/domain 不依赖 adapters。

### Task 2: 将 Runtime 注入模型切到 HookPort

**Files:**
- Modify: `agent/features/runtime/src/application/startup/runtime_support.rs`
- Modify: `agent/features/runtime/src/application/resources.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/loop_context.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/loop_run.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/setup.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/finalize.rs`
- Test: `agent/features/runtime/src/application/subagent/runner/tests.rs`

- [ ] **Step 1: 写 Runtime 装配失败测试**

断言生产启动由 Hook-owned 工厂产生 `Arc<dyn HookPort>`；Main 与 Sub 的 Runtime resources 传递同一注入 port，不构造 `HookRunner`。

- [ ] **Step 2: 运行定向 Runtime 测试确认失败**

Run: `cargo test -p runtime startup::runtime_support -- --nocapture`
Expected: 因仍返回/持有 `HookRunner` 而失败。

- [ ] **Step 3: 最小化替换持有字段和构造函数**

把生产结构中的 `HookRunner` / `hook_runner` 改为 `Arc<dyn HookPort>` / `hooks`，由 Hook adapter 的唯一工厂创建。测试使用 Hook crate 的 test-only scripted Dispatcher 或 runtime-local port fake；不得引入生产 test accessor。

- [ ] **Step 4: 运行受影响 Runtime 测试**

Run: `cargo test -p runtime startup::runtime_support -- --nocapture && cargo test -p runtime loop_context -- --nocapture`
Expected: 通过，并且不再依赖 legacy 类型。

### Task 3: 以 RuntimeHookDispatch 替代 HookUi 与手工推断

**Files:**
- Modify: `agent/features/runtime/src/application/hook_adapter.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/finalize.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/tools.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/post_batch.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/non_agent.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/agent_calls.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/ask_user.rs`
- Delete: `agent/features/runtime/src/application/main_loop/looping/hook_ui.rs`
- Modify/Create: 对应 `*_tests.rs`

- [ ] **Step 1: 为 Stop、PreToolUse、Permission、PostTool、PostToolBatch 写失败的邻接契约测试**

每项至少覆盖 `Continue`、`Block` 与有 messages 的结果；PreToolUse 另覆盖 `UpdatedInput` 必须走既有 schema/Policy 重验。断言 Runtime 不读 `HookResult.blocked/error`，不解析 stdout/JSON。

- [ ] **Step 2: 运行各测试确认失败**

Run: `cargo test -p runtime finalize -- --nocapture && cargo test -p runtime tools -- --nocapture && cargo test -p runtime non_agent -- --nocapture`
Expected: 测试因调用链仍使用 `HookUi`/legacy DTO 而失败。

- [ ] **Step 3: 实现统一 Runtime dispatch glue**

创建或放置一个 Runtime 私有 helper：调用 `HookPort::dispatch`、调用 `project_hook_outcome`、按 `RuntimeHookDirective` 编排，并将所有 messages 交由统一发送函数处理。该 helper 不得解析 JSON、exit code 或 stdout/stderr。

- [ ] **Step 4: 切换各调用点**

将 Stop、PreToolUse、PermissionRequest、PermissionDenied、PostToolUse、PostToolUseFailure、PostToolBatch、TaskCreated、TaskCompleted 的调用点迁至统一 glue。删除 `HookUi`、`HookResult`、`HookJsonOutput`、`is_blocking` 和 `emit_json_hook_context` 的生产依赖。

- [ ] **Step 5: 验证 UpdatedInput 重验**

Run: `cargo test -p runtime tools -- --nocapture`
Expected: 更新输入在进入工具执行前经过既有 Tools schema validator 与 Policy 复验；失败时不执行工具。

### Task 4: 切换 InstructionsLoaded、Notification 与 Subagent 生命周期

**Files:**
- Modify: `agent/features/runtime/src/application/startup.rs`
- Modify: `agent/features/runtime/src/application/prompt/build/prompt_build.rs`
- Modify: `agent/features/runtime/src/application/prompt/prompt_build_ext.rs`
- Modify: `agent/features/runtime/src/application/prompt/build/prompt_build_tests.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/setup.rs`
- Modify: `agent/features/runtime/src/application/subagent/runner/finalize.rs`
- Modify: `agent/features/runtime/src/adapters/runtime.rs`
- Modify/Delete: `agent/shared/src/adapter/hook.rs`
- Test: `agent/features/runtime/src/application/subagent/runner/tests.rs`

- [ ] **Step 1: 写失败测试**

断言 InstructionsLoaded、Notification、SubagentStart 和 SubagentStop 都通过 `HookPort::dispatch`；Subagent display message 仍按原有 `ProgressSink` 可观察路径输出；不再存在 `HookRunnerAdapter<HookRunner>`。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p runtime instructions_loaded -- --nocapture && cargo test -p runtime subagent -- --nocapture`
Expected: 失败原因为旧 Runner 入口仍存在。

- [ ] **Step 3: 切换跨模块桥接**

将 Context 的 InstructionsLoaded bridge、legacy notification port adapter 和 Subagent 生命周期调用改为 `Arc<dyn HookPort>`。若某 legacy port 无其他消费者，删除而非保留泛型 wrapper。

- [ ] **Step 4: 运行定向测试**

Run: `cargo test -p runtime --lib -- instructions_loaded && cargo test -p runtime --lib -- subagent`
Expected: 通过。

### Task 5: 结构化 HookMessage 事件与展示投影

**Files:**
- Modify: `agent/features/runtime/src/application/main_loop/looping/finalize.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/tools.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/post_batch.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/non_agent.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/agent_calls.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/ask_user.rs`
- Modify: `agent/features/runtime/src/adapters/event_projection.rs`
- Test: `agent/features/runtime/src/application/main_loop/looping/finalize_tests.rs`
- Test: `agent/features/runtime/src/application/main_loop/looping/loop_runner_tests.rs`
- Test: `agent/features/runtime/src/application/main_loop/looping/pre_compact_trigger_tests.rs`
- Test: `agent/features/runtime/src/adapters/event_projection_tests.rs`
- Test: `apps/cli/src/tui/adapter/event_adapter_tests.rs`（若该现有事件适配测试承载 HookMessage 消费）

- [ ] **Step 1: 写失败事件测试**

断言 `HookDisplayMessage` 的 point、source、execution ordinal、attempt、kind、text 完整变成 `RuntimeStreamEvent::HookMessage` 与 SDK `ChatEvent::HookMessage`；空文本仍由展示层忽略，不由 Runtime 丢弃。

- [ ] **Step 2: 运行事件投影测试确认失败**

Run: `cargo test -p runtime event_projection -- --nocapture`
Expected: 当前 legacy SystemMessage 路径不满足 HookMessage 断言。

- [ ] **Step 3: 最小实现与删除通用 SystemMessage 路径**

统一由 Runtime dispatch glue 逐条发送 HookMessage；删除 Hook 输出专用的 SystemMessage 构造和 JSON context 解析。保留 Hook running/finished 观察事件时，完整保留 retry executions，不得重新推断业务 directive。

- [ ] **Step 4: 运行 Runtime、SDK、TUI 的相邻测试**

Run: `cargo test -p runtime event_projection -- --nocapture && cargo test -p sdk -- --nocapture && cargo test -p cli -- tui --nocapture`
Expected: 三层事件链路通过。

### Task 6: 删除 legacy Hook 实现并修正文档语义

**Files:**
- Delete: `agent/features/hook/src/adapters/legacy.rs`
- Delete: `agent/features/hook/src/adapters/legacy/**`
- Modify: `agent/features/hook/src/adapters.rs`
- Modify: `agent/features/hook/src/lib.rs`
- Modify: `agent/shared/src/config/domain/hooks.rs`
- Modify: `agent/features/runtime/src/application/hook_adapter.rs`
- Modify: 所有受影响测试

- [ ] **Step 1: 写编译级退役断言 / 搜索清单**

定义零引用检查目标：`HookRunner`、`HookResult`、`HookData`、`HookInput`、`HookJsonOutput`、`run_hooks`、`run_hooks_with_json`、`HookUi`、`hook::api`、`exit_code == 2`。测试中也不得留下已退役 DTO。

- [ ] **Step 2: 删除 `hook::api` 和 legacy modules**

删除 `lib.rs` 的兼容 module 与 legacy adapter tree；将 `hook_adapter.rs` 的类型路径改为 Hook crate-root PL。更新 shared config 注释为任意非零 exit 均为 Block，避免将兼容语义留作第二真相。

- [ ] **Step 3: 运行零引用和编译验证**

Run: `grep -RInE 'HookRunner|HookResult|HookData|HookInput|HookJsonOutput|run_hooks(_with_json)?|HookUi|hook::api|exit_code[[:space:]]*==[[:space:]]*2' agent/features/hook agent/features/runtime agent/shared --include='*.rs'`
Expected: 无生产或测试命中；如仍有文档历史引用，必须逐项判断并删除/迁移。

- [ ] **Step 4: 运行 Hook 与 Runtime 全量测试**

Run: `cargo test -p hook && cargo test -p runtime`
Expected: 通过。

### Task 7: 增加零白名单 Hook façade Guard

**Files:**
- Create: `.agents/hooks/check-hook-target-facade.sh`
- Create: `.agents/hooks/check-hook-target-facade-tests.sh`
- Modify: `.agents/hooks/check-architecture-guards.sh`
- Modify: `.agents/architecture-guard-registry.json`
- Modify: `docs/design/03-engineering/01-architecture-guards.md`

- [ ] **Step 1: 写 Guard 反例脚本**

在临时副本分别注入：(a) `pub mod api`，(b) legacy crate-root re-export，(c) Runtime production `use hook::api::...`。每一反例断言单 Guard exit 2 且诊断明确；恢复后 exit 0。

- [ ] **Step 2: 运行反例脚本确认先失败**

Run: `bash .agents/hooks/check-hook-target-facade-tests.sh`
Expected: 因 Guard 尚未创建而失败。

- [ ] **Step 3: 实现 Guard 与注册**

Guard 扫描 Hook lib 与 Runtime production Rust 源；测试文件的范围排除仅在确有必要时登记到 registry，并保持零 migration exception。将 Guard 加入 fast/full 编排。

- [ ] **Step 4: 验证单 Guard、反例和总编排**

Run: `bash .agents/hooks/check-hook-target-facade-tests.sh && bash .agents/hooks/check-hook-target-facade.sh && bash .agents/hooks/check-architecture-guards.sh --fast`
Expected: 反例测试证明 exit 2；干净树单 Guard 与 fast 编排 exit 0。

### Task 8: L5 真实进程 / worktree / cancel 回归与最终文档回写

**Files:**
- Modify: 现有 Hook process、Runtime loop/worktree/cancel 相邻测试
- Modify: `docs/design/02-modules/hook/01-run-loop-integration.md`
- Modify: `docs/design/03-engineering/03-migration-governance.md`
- Modify: GitHub Issue #926

- [ ] **Step 1: 为真实受管进程路径建立或迁移 L5 回归测试**

覆盖 cwd、`AEMEATH_PROJECT_DIR`、`CLAUDE_PROJECT_DIR`、worktree 切换后的路径、取消时的子进程回收；测试必须经 Dispatcher 路径，不得复活 legacy Runner。

- [ ] **Step 2: 运行 L5 定向测试**

Run: `cargo test -p runtime process_chat_loop_uses_workspace_workspace_root_for_stop_hook_env -- --nocapture`
Expected: 通过；若测试名随切换重命名，计划与 Issue 同步记录新的精确命令。

- [ ] **Step 3: 跑完整验证门禁**

Run: `cargo fmt --check && cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && bash .agents/hooks/check-architecture-guards.sh --full`
Expected: 所有命令 exit 0。

- [ ] **Step 4: 回填文档与 Issue 完成前门禁**

将 PHA4–PHA7 更新为已对齐；在 Guard 文档写入零白名单、扫描范围、反例 exit 2 证据；在 Issue 差异表将每项更新为已对齐或 OOS 并附承接 Issue。仅在所有 checklist 均有证据后才准备 PR。

- [ ] **Step 5: 提交**

Run: `git add agent/features/hook agent/features/runtime agent/shared .agents docs && git commit -m "refactor(hook): 退役 HookRunner 兼容入口"`
Expected: 仅包含 #926 相关路径；提交前先按仓库历史确认最终 message 风格。
