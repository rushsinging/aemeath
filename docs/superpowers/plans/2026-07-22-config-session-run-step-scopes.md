# 配置 Session / Run / Run Step 作用域 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将配置应用语义明确拆分为 Session、Run、Run Step 三个固定边界，使普通配置变更仅对后续 Run 生效，单次模型调用始终使用不可变请求快照，并将需要重启 session 的配置显式投影给 SDK/TUI。

**Architecture:** `ConfigAppService` 继续是唯一的来源加载、合并、校验和 committed revision 发布者；它不直接重建 Runtime 资源。Runtime 在新 Run 创建前捕获一个 `RunConfigSnapshot`，由该快照构造 provider binding、权限、hooks、并发和 agent runner；每个 Run Step 再从同一 Run 快照冻结 `ContextRequest`。Session bootstrap/resume 只处理项目身份和不可热切换资源，变更时发出 restart-required 状态而不修改活跃 Session。

**Tech Stack:** Rust、Tokio `watch`/`RwLock`、Config BC、Context MainSessionWiring、Runtime shared run loop、ProviderFactory、SDK DTO、Ratatui TUI。

---

## 已确认的作用域规则

### Session 级：创建/恢复时固定

在启动、session resume、跨项目切换或显式 session restart 时捕获。普通 JSON reload 不在活跃 session 自动应用。

| 分类 | 字段 / 资源 | 行为 |
|---|---|---|
| UI 进程模式 | `ui.tui`、输出模式 | 变更标记 `session_restart_required`；不在运行中切 TUI/no-TUI。 |
| 日志基础设施 | logs dir、sink、rotation 后端 | 标记 restart-required；动态 filter 不是本计划范围。 |
| 外部拓扑 | MCP server 清单、skills 搜索目录 | 标记 restart-required，避免中途新增/销毁连接与目录语义。 |
| 持久化根 | storage/session persistence 路径、memory backing identity | 仅 Session bootstrap/resume 应用。 |
| 项目身份 | config 路径、workspace/project identity | 仅 `MainSessionWiring::resume_prepared` 原子切换。 |

### Run 级：每个 Main Run / 新 Subagent Run 固定

在真实用户 turn 被接纳、创建 `Run::new(RunSpec::main(), ...)` 前捕获；Subagent 在 `AgentRunner::run_agent` 入口捕获。配置文件 reload 成功后只影响下一个 Run。

| 分类 | 字段 | 行为 |
|---|---|---|
| Provider | provider、model、API key、base URL、timeout | 从 run snapshot 构造 binding；候选失败不污染既有 Run。 |
| 模型参数 | max tokens、reasoning、context size | 每 Run 固定；Step 只消费已解析的值。 |
| 权限 | `permissions.mode` / `allow_all` | Run 内固定；不得在 tool round 中改变授权语义。 |
| 执行编排 | hooks、tool/agent concurrency、tool-result policy | 新 Run 构造/选择资源；已运行的 work 不变。 |
| Agent | roles、models、language | 新 Main Run 与新 Subagent Run 使用同一 revision 规则。 |
| Memory | injection 参数、reflection 开关 | 新 Run 固定；Memory backing 仍由 Session 级项目身份拥有。 |

### Run Step 级：每次 LLM invocation 前冻结

Step 不读取 ConfigReader、不 refresh 文件；只接收其所属 Run 的 snapshot 和派生 binding。

| 字段 | 来源 |
|---|---|
| `ConfigSnapshot` revision | Run snapshot |
| provider binding / model id / output token 上限 | Run 级已构造对象 |
| messages / pending input | Context backing 与本 Step 投影 |
| tool schemas | 当前 Catalog snapshot |
| task reminder | 当前 TaskAccess snapshot |
| current date、memory materialization、token budget | 当前 Step 构造 |

### Prompt 资产：不属于 JSON Config 生命周期

`guidance`、`AGENTS.md`、`CLAUDE.md` 用内容 hash 检测。发生实质变化后，仅在下一 Run 前追加带路径的 `<system-reminder>`，要求 LLM 用 Read 重读；永不重建 cacheable system prompt，永不影响当前 Run / Step。

---

## 文件结构

| 路径 | 职责 |
|---|---|
| `agent/shared/src/config/domain/scope.rs`（新建） | 定义 `ConfigApplicationScope`、分类结果和 restart-required 字段集合。 |
| `agent/shared/src/config/domain/scope_tests.rs`（新建） | 纯领域测试：字段变更分类、Run/Session 边界、稳定顺序。 |
| `agent/shared/src/config/domain/snapshot.rs` | 提供构造 `RunConfigSnapshot` 所需的细粒度 accessor；不泄露裸 Config。 |
| `agent/features/config/src/contract.rs` | `ConfigRefreshOutcome` 带分类后的 changed scopes；Reader 仍是唯一 committed snapshot 入口。 |
| `agent/features/config/src/application.rs` | reload 时对旧/新有效配置分类；仅发布 committed state，不创建 Runtime 对象。 |
| `agent/features/context/src/application/main_session.rs` | Session bootstrap/resume 应用 Session 级 snapshot，维持 Session/Memory/Project 原子性。 |
| `agent/features/runtime/src/application/run_config.rs`（新建） | RunConfigSnapshot、run binding factory、候选构造和失败回退。 |
| `agent/features/runtime/src/application/run_config_tests.rs`（新建） | Run 级 Main/Sub snapshot 与 binding 的 TDD 测试。 |
| `agent/features/runtime/src/application/chat/looping/loop_runner.rs` | 在接纳真实用户 turn 后、`Run::new` 前捕获 RunConfigSnapshot。 |
| `agent/features/runtime/src/application/chat/looping/main_run_port.rs` | 每 Step 仅从 frozen RunConfigSnapshot 构造 ContextRequest。 |
| `agent/features/runtime/src/application/agent/runner.rs`、`setup.rs` | 新 Subagent Run 捕获当前 RunConfigSnapshot；已启动 subagent 不变化。 |
| `agent/features/runtime/src/application/client/mapping.rs` | 映射 restart-required / pending scope 变化到 SDK DTO。 |
| `packages/sdk/src/**` | Config scope change DTO / Chat event，只暴露展示安全字段。 |
| `apps/cli/src/tui/**` | 展示“下次 Run 生效”或“重启 Session 后生效”，不直接读取 Config。 |
| `specs/config-compat.md`、`specs/runtime.md`、`specs/tui-cli.md` | 固化三级作用域与消费者边界。 |

---

### Task 1: 定义配置应用作用域领域模型

**Files:**
- Create: `agent/shared/src/config/domain/scope.rs`
- Create: `agent/shared/src/config/domain/scope_tests.rs`
- Modify: `agent/shared/src/config/domain.rs` 或对应 module declaration
- Modify: `agent/shared/src/config/domain/snapshot.rs`

- [ ] **Step 1: 写失败的领域分类测试**

建立以下语义断言：

1. 仅 `permissions.mode` 改变分类为 `Run`；
2. provider/model/API key/base URL/timeout、roles、hooks、concurrency、memory injection 参数分类为 `Run`；
3. `ui.tui`、日志 sink/目录、MCP 拓扑、skills 目录、storage path 分类为 `SessionRestartRequired`；
4. 空 effective-config diff 分类为空；
5. 多字段变化按稳定枚举顺序去重；
6. `allow_all` 永不分类为 Step，也不分类为 Session。

- [ ] **Step 2: 运行领域测试，确认类型和分类器尚不存在**

Run: `cargo test -p share config_application_scope --lib`

Expected: FAIL，提示 `ConfigApplicationScope` / classifier 未定义。

- [ ] **Step 3: 实现纯领域分类器**

定义：

```rust
pub enum ConfigApplicationScope {
    SessionRestartRequired,
    Run,
}
```

分类器只比较两个有效 `Config` 的相关字段，返回稳定、去重的 scope/field 描述；不做 I/O、不读取 env、不引用 Runtime/SDK。为 `ConfigSnapshot` 增加仅供 Runtime 构造 RunConfigSnapshot 的 accessor，不能返回可变或裸 `Config`。

- [ ] **Step 4: 验证领域分类器**

Run: `cargo test -p share config_application_scope --lib`

Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add agent/shared/src/config/domain/scope.rs agent/shared/src/config/domain/scope_tests.rs agent/shared/src/config/domain.rs agent/shared/src/config/domain/snapshot.rs
git commit -m "feat(config): classify configuration application scopes"
```

### Task 2: 将作用域分类纳入 Config reload Published Language

**Files:**
- Modify: `agent/features/config/src/contract.rs`
- Modify: `agent/features/config/src/application.rs`
- Modify: `agent/features/config/src/application_tests.rs` 或现有同职责外置测试文件
- Test: `agent/features/config/tests/config_scope_reload.rs`（新建）

- [ ] **Step 1: 写失败的 Config reload scope 测试**

用临时 global/project/Claude 文件和 fake env 覆盖：

1. `allow_all` 文件变化发布 `Reloaded`，结果含 `Run` scope；
2. `ui.tui` 变化发布 `Reloaded`，结果含 `SessionRestartRequired`；
3. Env 覆盖使最终值未变时仍为 `Unchanged`，无 scope；
4. 多项变化同时存在时 scope 稳定去重；
5. parse/validate 失败仍为 `Rejected`，不产生 scope、不改变 committed revision。

- [ ] **Step 2: 运行测试，确认 RefreshOutcome 尚不携带 scope 分类**

Run: `cargo test -p config --test config_scope_reload`

Expected: FAIL，提示 `ConfigRefreshOutcome::Reloaded` 缺少 scope 字段。

- [ ] **Step 3: 扩展 refresh outcome 而不扩大 Reader I/O**

将 `Reloaded` 改为包含：

```rust
Reloaded {
    snapshot: ConfigSnapshot,
    scopes: Vec<ConfigApplicationScope>,
}
```

ConfigAppService 在 effective-config fingerprint 确认不同后调用 Shared 分类器；之后原子 commit/watch 发布。`ConfigReader::refresh_if_sources_changed()` 仍是唯一文件 reload 入口，普通 getter 不做 I/O。

- [ ] **Step 4: 验证 Config crate**

Run: `cargo test -p config`

Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add agent/features/config/src/contract.rs agent/features/config/src/application.rs agent/features/config/tests/config_scope_reload.rs
git commit -m "feat(config): publish reload application scopes"
```

### Task 3: 明确 Session 级应用与 restart-required 投影

**Files:**
- Modify: `agent/features/context/src/application/main_session.rs`
- Modify: `agent/features/runtime/src/application/client/mapping.rs`
- Modify: `packages/sdk/src/**`（按现有 Config/Chat DTO 位置）
- Test: `agent/features/context/tests/main_session_config_scope.rs`（新建）
- Test: `agent/features/runtime/tests/config_scope_projection.rs`（新建）

- [ ] **Step 1: 写失败的 Session 级行为测试**

覆盖：

1. `ui.tui`/logs dir/MCP/skills/storage 变更不会替换活跃 Session 的运行资源；
2. Config refresh 结果被映射为 SDK 可观察的 `session_restart_required` 标志及字段列表；
3. session resume / 跨项目恢复仍通过 `prepare_for_project → memory open → commit_project` 原子替换 Session/Memory/Config；
4. Run 级字段变更不要求重启 Session。

- [ ] **Step 2: 运行测试，确认 restart-required 状态尚无 DTO 投影**

Run: `cargo test -p context --test main_session_config_scope && cargo test -p runtime --test config_scope_projection`

Expected: FAIL，提示缺少 scope projection 或 pending session restart 状态。

- [ ] **Step 3: 实现 Session 级状态投影**

- 保持 `MainSessionWiring::bind_main_run()` 的 Session/Memory/Config gate 语义；
- 添加只读 pending restart state，记录最新 committed revision 与 SessionRestartRequired 字段；
- 通过 Runtime/SDK event 映射给 TUI；
- Session restart/resume 成功后清除已应用的 pending state；
- 不在此路径重建 TUI、logger sink、MCP 或 storage；它们仍由下一 Session bootstrap 使用新 snapshot。

- [ ] **Step 4: 验证 Context 与 Runtime 投影测试**

Run: `cargo test -p context --test main_session_config_scope && cargo test -p runtime --test config_scope_projection`

Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add agent/features/context/src/application/main_session.rs agent/features/context/tests/main_session_config_scope.rs agent/features/runtime/src/application/client/mapping.rs agent/features/runtime/tests/config_scope_projection.rs packages/sdk/src
git commit -m "feat(session): project restart-required config changes"
```

### Task 4: 在 Main Run 创建边界捕获 RunConfigSnapshot

**Files:**
- Create: `agent/features/runtime/src/application/run_config.rs`
- Create: `agent/features/runtime/src/application/run_config_tests.rs`
- Modify: `agent/features/runtime/src/application/chat/looping/loop_runner.rs`
- Modify: `agent/features/runtime/src/application/chat/looping/loop_phases.rs`
- Modify: `agent/features/runtime/src/application/client/trait_model.rs`
- Test: `agent/features/runtime/tests/main_run_config_scope.rs`（新建）

- [ ] **Step 1: 写失败的 Main Run 配置边界测试**

使用 fake ConfigReader/ProviderFactory：

1. 在真实用户输入被接纳后、`Run::new` 前捕获一个 RunConfigSnapshot；
2. Run 内连续多个 Step 的 `ContextRequest.config_snapshot.revision()` 相同；
3. Run 运行中变更 `allow_all` 或 provider 配置不改变该 Run 的 binding/权限；
4. 下一真实用户 turn 捕获新 revision，并从新 snapshot 构造新 binding；
5. binding 候选构造失败时当前 Run 不启动，向调用方返回明确错误；既有已运行 Run 不受影响；
6. SessionRestartRequired scope 不触发 Run resource 重建。

- [ ] **Step 2: 运行测试，确认 loop 当前在每 turn 后仍混用启动期 RuntimeResources 与 committed config**

Run: `cargo test -p runtime --test main_run_config_scope`

Expected: FAIL，显示 RunConfigSnapshot / factory 未定义。

- [ ] **Step 3: 实现 RunConfigSnapshot 与 Main binding factory**

`RunConfigSnapshot` 必须包含：

- committed `ConfigSnapshot`；
- 解析后的 runtime model、provider binding、context size、reasoning；
- permission mode / `allow_all`；
- hooks、concurrency、tool-result policy、memory config、language；
- applied revision。

构造规则：先从 ConfigReader 捕获 committed snapshot，再构造所有候选派生资源；只有完整成功才能创建 Main Run。不得复用启动时固定的 binding 作为新 Run fallback，不得在 Run 中调用 refresh。

- [ ] **Step 4: 在 loop runner 固定顺序接入**

顺序必须是：

```text
1. 接纳真实 user input
2. ConfigReader.refresh_if_sources_changed()
3. prompt asset detector/reminder
4. 读取 pending Session restart 状态
5. 捕获并构造 RunConfigSnapshot
6. Run::new(RunSpec::main(), ...)
7. bind_main_run() 取得 Session/Memory 原子组合
8. 进入 shared run loop
```

不得在 Run 的第二个 Step 或工具 round 重抓 ConfigReader。

- [ ] **Step 5: 验证 Main Run 作用域**

Run: `cargo test -p runtime --test main_run_config_scope && cargo test -p runtime --lib`

Expected: PASS。

- [ ] **Step 6: Commit**

```bash
git add agent/features/runtime/src/application/run_config.rs agent/features/runtime/src/application/run_config_tests.rs agent/features/runtime/src/application/chat/looping/loop_runner.rs agent/features/runtime/src/application/chat/looping/loop_phases.rs agent/features/runtime/src/application/client/trait_model.rs agent/features/runtime/tests/main_run_config_scope.rs
git commit -m "feat(runtime): freeze config at main run boundary"
```

### Task 5: 让新 Subagent Run 使用同一 RunConfigSnapshot 规则

**Files:**
- Modify: `agent/features/runtime/src/application/agent/runner.rs`
- Modify: `agent/features/runtime/src/application/agent/runner/setup.rs`
- Modify: `agent/features/runtime/src/application/agent/runner/loop_run.rs`
- Modify: `agent/features/runtime/src/application/startup/runtime_support.rs`
- Test: `agent/features/runtime/src/application/agent/runner_tests.rs`
- Test: `agent/features/runtime/tests/subagent_run_config_scope.rs`（新建）

- [ ] **Step 1: 写失败的 Subagent Run 作用域测试**

覆盖：

1. 新建 subagent 从当前 ConfigReader revision 构造自己的 RunConfigSnapshot；
2. 新 Main Run 与其新建 subagent 解析相同 provider/model/API key/timeout/roles 规则；
3. 已启动 subagent 在配置 reload 后保持自身 binding、permission、snapshot revision；
4. 随后创建的 subagent 捕获新 revision；
5. unknown role、unknown model 或 binding 构造失败 fail-closed，且不污染其他 Run。

- [ ] **Step 2: 运行测试，确认 CliAgentRunner 当前永久冻结 bootstrap snapshot**

Run: `cargo test -p runtime --test subagent_run_config_scope`

Expected: FAIL，显示 bootstrap snapshot 被长期存入 CliAgentRunner。

- [ ] **Step 3: 移除 bootstrap 冻结配置**

- `CliAgentRunner` 持有 narrow RunConfig factory / ConfigReader，而不是 `models_config`、`agents_config`、`config_snapshot`、language、timeout 的启动期副本；
- 每个 `run_agent` 入口捕获一个完整 RunConfigSnapshot；
- `SubAgentRun` 与其 `ContextRequest` 只保存该捕获值；
- 不让已运行 subagent 订阅 watch，也不在 Step 内读取 Reader。

- [ ] **Step 4: 验证 Subagent 作用域**

Run: `cargo test -p runtime --test subagent_run_config_scope && cargo test -p runtime --lib`

Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add agent/features/runtime/src/application/agent/runner.rs agent/features/runtime/src/application/agent/runner/setup.rs agent/features/runtime/src/application/agent/runner/loop_run.rs agent/features/runtime/src/application/startup/runtime_support.rs agent/features/runtime/src/application/agent/runner_tests.rs agent/features/runtime/tests/subagent_run_config_scope.rs
git commit -m "feat(runtime): freeze config at subagent run boundary"
```

### Task 6: 让 Run Step 只能消费 frozen RunConfigSnapshot

**Files:**
- Modify: `agent/features/runtime/src/application/chat/looping/main_run_port.rs`
- Modify: `agent/features/runtime/src/application/agent/runner/loop_run.rs`
- Modify: `agent/features/context/src/domain.rs`
- Modify: `agent/features/context/src/application/service.rs`
- Test: `agent/features/runtime/tests/run_step_config_scope.rs`（新建）
- Test: `agent/features/context/tests/context_request_config_snapshot.rs`（新建）

- [ ] **Step 1: 写失败的 Step 冻结测试**

覆盖：

1. 同一 Run 内两个 Step 的 `ContextRequest.config_snapshot.revision` 与 permission mode 一致；
2. ConfigReader 在两个 Step 之间发布新 revision 后，当前 Run 的第二 Step 仍使用旧 revision；
3. 新 Run 的第一 Step 使用新 revision；
4. Step 构造读取当前 Catalog/Task/Memory 输入，但不调用 ConfigReader refresh 或直接读取 Config 文件；
5. Context service 不替换 request 的 config snapshot。

- [ ] **Step 2: 运行测试，确认 Step freeze 约束尚未由类型和守卫覆盖**

Run: `cargo test -p runtime --test run_step_config_scope && cargo test -p context --test context_request_config_snapshot`

Expected: FAIL，提示测试 seam 或断言缺失。

- [ ] **Step 3: 收紧 Step 输入与 Context 透传**

- `MainRunPort::freeze_request` 和 Subagent `freeze_request` 都只使用所属 RunConfigSnapshot；
- `ContextRequest` 保持 owned `ConfigSnapshot`，Context service 原样透传；
- 删除 Step 中重新调用 `committed_config()` / `ConfigQuery::snapshot()` 的路径；
- 增加 architecture guard，禁止 `application/chat/looping/main_run_port.rs` 与 `application/agent/runner/loop_run.rs` 生产区域引用 `ConfigReader` / `ConfigQuery`。

- [ ] **Step 4: 验证 Run Step 与 Context 合同**

Run: `cargo test -p runtime --test run_step_config_scope && cargo test -p context --test context_request_config_snapshot`

Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add agent/features/runtime/src/application/chat/looping/main_run_port.rs agent/features/runtime/src/application/agent/runner/loop_run.rs agent/features/context/src/domain.rs agent/features/context/src/application/service.rs agent/features/runtime/tests/run_step_config_scope.rs agent/features/context/tests/context_request_config_snapshot.rs
git commit -m "refactor(runtime): freeze config per run step"
```

### Task 7: 更新 SDK/TUI 与架构规范，完成验证

**Files:**
- Modify: `packages/sdk/src/**`
- Modify: `apps/cli/src/tui/**`
- Modify: `specs/config-compat.md`
- Modify: `specs/runtime.md`
- Modify: `specs/tui-cli.md`
- Modify: `.agents/hooks/check-config-reader-injection.sh`
- Test: matching SDK/TUI/guard tests

- [ ] **Step 1: 写失败的 SDK/TUI 投影与 guard 测试**

验证：

1. SDK 只暴露 scope、pending restart 字段和安全展示值，不暴露 ConfigReader / ConfigSnapshot；
2. TUI 对 Run scope 显示“下次 Run 生效”，对 Session scope 显示“重启 Session 后生效”；
3. Run Step 生产路径出现 ConfigReader / ConfigQuery 时 guard 失败；
4. Runtime/Context 以外不得直接读 Config 文件或 env。

- [ ] **Step 2: 更新 SDK/TUI 投影**

- 增加 scope-safe Config change event / view；
- TUI 只消费 SDK DTO，并在状态栏或 system message 展示 pending state；
- 不在 TUI 创建 ConfigReader、订阅 watch 或决定重建资源。

- [ ] **Step 3: 更新规范与守卫**

在规格中写明：

- Session / Run / Run Step 的字段映射与固定不变式；
- `allow_all` 是 Run 级；
- prompt assets 不属于 JSON Config；
- ConfigReader refresh 仅可在新 Run 创建前调用；
- Step 级禁止 refresh/reader；
- SessionRestartRequired 的 UX 和 session resume 清除规则。

- [ ] **Step 4: 执行完整验证**

Run:

```bash
cargo test -p share
cargo test -p config
cargo test -p context
cargo test -p runtime
cargo test -p cli --bin aemeath
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
bash .agents/hooks/check-agent-stop.sh
```

Expected: 全部 PASS。

- [ ] **Step 5: 需求覆盖与废弃路径检查**

确认并记录：

- 旧“每 turn 全局立即应用”的描述和实现已删除；
- 长生命周期 RuntimeResources 不再错误标注为 session 期间不变配置；
- Main/Sub bootstrap snapshot 永久冻结路径已删除；
- Step 级无 ConfigReader 读取；
- #1326 / #858 的状态与新承接 Issue 已更新，但不自行关闭新 Issue。

- [ ] **Step 6: Commit**

```bash
git add packages/sdk/src apps/cli/src/tui specs/config-compat.md specs/runtime.md specs/tui-cli.md .agents/hooks/check-config-reader-injection.sh
git commit -m "docs(config): define session run step scopes"
```

## 最终交付检查

- [ ] 所有 scope 分类都有 Shared domain 单元测试。
- [ ] 每个 Config reload outcome 都含明确应用 scope。
- [ ] Session 级变更不会改变活跃 session 的基础设施。
- [ ] Main 与 Subagent 均在 Run 边界捕获配置。
- [ ] 同一 Run 的每个 Step 使用相同 ConfigSnapshot revision。
- [ ] `allow_all` 在 Run 内稳定，仅对后续 Run 生效。
- [ ] guidance/AGENTS/CLAUDE 仅走下一 Run 的 Read reminder，不改变 system prompt cache。
- [ ] 完整 workspace 验证与全架构守卫通过。
