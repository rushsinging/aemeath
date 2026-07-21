# 每 Turn 配置热重载 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Config 服务在每个 turn 边界无感检测并原子发布有效 JSON 配置变化；新 provider invocation（Main/Sub）使用新 binding，同时 guidance/指令文件仅向 LLM 注入精确重读提醒而不重建 system prompt。

**Architecture:** `ConfigAppService` 成为 JSON 配置源 fingerprint、加载、合并、校验与 committed snapshot 发布的唯一所有者；Runtime 只在 turn 边界调用注入的 refresh port。运行资源通过 revision 驱动的协调器在下一次 invocation 前构造候选资源并原子替换。Guidance/AGENTS/CLAUDE 是独立 prompt 资产，检测内容 hash 后只追加携带路径的 reminder message，避免改变 cacheable system block。

**Tech Stack:** Rust、Tokio `watch`/`RwLock`/`Mutex`、SHA-256、serde、现有 Config BC、Runtime Chat loop、provider factory。

---

## 边界与生效语义

- **正确合并顺序**：`Default → Global → Claude compatibility → Project Aemeath → native override → Env → CLI`。后层仅覆盖自身显式字段。
- **配置文件 touch / 格式化但有效配置未变**：不发布 snapshot、不增加 `ConfigRevision`、不发 `ConfigReloaded`。
- **JSON 读取、解析或校验失败**：保留旧 committed snapshot；记录可诊断失败；不向消费者发布半成品或默认值。
- **JSON reload 成功**：本 turn 进入 invocation 前读取新 snapshot；正在运行的 HTTP stream、工具调用、subagent 不被中断。
- **provider 相关配置**：在下一次 Main invocation、以及每次新建 Subagent 时从同一最新 snapshot 构造 binding；候选 binding 构造失败时继续使用旧 binding，并报告失败。
- **guidance / `AGENTS.md` / `CLAUDE.md`**：不重建 `system_blocks`、不改变 cacheable system prompt；内容 hash 实质变化时，在下一次请求前追加含变更文件绝对/可读路径的 `<system-reminder>`，要求 LLM 用 Read 重读。
- **运行资源**：logging filter 可立即按新值更新；provider binding、hooks、并发限制、memory、tool-result policy 在 turn/invocation 安全边界更新；`ui.tui`、输出模式、日志输出目录等启动模型/进程资源标记为 restart-required。

## 文件结构

| 路径 | 职责 |
|---|---|
| `agent/features/config/src/contract.rs` | 发布 reload port、结果与错误契约；保留 Reader 的只读语义。 |
| `agent/features/config/src/application.rs` | Config source fingerprint、reload candidate、有效值比较、原子 commit。 |
| `agent/features/config/src/application_tests.rs`（新建） | Config reload、优先级、失败回退、revision 与 watch 契约测试。 |
| `agent/features/runtime/src/application/chat/looping/config_reload.rs` | 缩减为独立 prompt asset 内容检测与提醒构造；移除 JSON 直接读取。 |
| `agent/features/runtime/src/application/chat/looping/config_reload_tests.rs`（新建） | prompt asset hash / reminder 路径、无 touch 噪声测试。 |
| `agent/features/runtime/src/application/client/from_args.rs` | 装配动态 runtime reconfiguration coordinator 与 refresh port。 |
| `agent/features/runtime/src/application/client/trait_chat.rs` | 将 refresh/reconfiguration 能力送入 chat loop。 |
| `agent/features/runtime/src/application/chat/looping/loop_runner.rs` | 每 turn 先 refresh、再协调资源、再 bind run；使用当前 binding。 |
| `agent/features/runtime/src/application/chat/looping/loop_phases.rs` | 接收 Config refresh 结果与 prompt asset change，发布 SDK event / LLM reminder。 |
| `agent/features/runtime/src/application/reconfiguration.rs`（新建） | revision 驱动的 Main binding、hooks、并发、agent runner 等候选构造和原子替换。 |
| `agent/features/runtime/src/application/reconfiguration_tests.rs`（新建） | Main/Sub binding 生效边界与失败回退测试。 |
| `agent/features/runtime/src/application/agent/runner.rs`、`setup.rs` | 新建 Subagent 时经动态配置/binding factory 获取当前 snapshot，不永久冻结启动时配置。 |
| `agent/shared/src/config/domain/config.rs`、`snapshot.rs`、`merge.rs` | 为规范化有效配置 fingerprint、reload policy accessor 与优先级补齐领域能力。 |
| `specs/config-compat.md` | 更新精确优先级、热重载边界和 restart-required 规则。 |

### Task 1: 建立 Config reload 契约与有效配置等价性

**Files:**
- Modify: `agent/features/config/src/contract.rs`
- Modify: `agent/shared/src/config/domain/config.rs`
- Modify: `agent/shared/src/config/domain/snapshot.rs`
- Test: `agent/features/config/src/contract_tests.rs`
- Test: `agent/shared/src/config/domain/snapshot_tests.rs`

- [ ] **Step 1: 写失败的 Config reload 契约测试**

覆盖 `ConfigRefreshPort` 的三类结果：无来源内容变化、来源变但有效配置等价、成功发布新有效配置；断言只有最后一种有新 revision / 新 snapshot。补充 snapshot 的 revision 保持不可变且可比较。

- [ ] **Step 2: 运行定向测试，确认因 port / 结果类型不存在而失败**

Run: `cargo test -p config config_refresh --lib`

Expected: 编译失败，提示 `ConfigRefreshPort`、`ConfigRefreshOutcome` 或对应测试 helper 尚未定义。

- [ ] **Step 3: 在 Config Published Language 中定义最小 reload seam**

在 `contract.rs` 定义：

- `ConfigRefreshPort: Send + Sync`，仅暴露 async `refresh_if_sources_changed()`；
- `ConfigRefreshOutcome::{Unchanged, Reloaded { snapshot, changed_fields }, Rejected { error }}`；
- `ConfigRefreshError` 使用中文显示信息并区分 I/O、解析、校验失败；
- `ConfigReader` 保持无 I/O 的 `committed_snapshot` / `subscribe_committed`。

为 `Config` 提供稳定的规范化有效配置 fingerprint：使用 `serde_json::to_value` 后递归排序所有 object key，再序列化并计算 SHA-256；不得将 `ConfigSnapshot.revision` 纳入 fingerprint。该方式避免为全部嵌套配置补 `PartialEq/Eq`，并保证 HashMap 序列化顺序不导致伪变更。

- [ ] **Step 4: 运行定向测试，确认契约和领域比较通过**

Run: `cargo test -p config config_refresh --lib && cargo test -p share config_snapshot --lib`

Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add agent/features/config/src/contract.rs agent/features/config/src/contract_tests.rs agent/shared/src/config/domain/config.rs agent/shared/src/config/domain/snapshot.rs agent/shared/src/config/domain/snapshot_tests.rs
git commit -m "feat(config): add reload contract"
```

### Task 2: 将 JSON source fingerprint 与 reload 原子提交收敛到 ConfigAppService

**Files:**
- Modify: `agent/features/config/src/application.rs`
- Modify: `agent/features/config/src/adapters.rs`
- Modify: `agent/features/config/src/contract.rs`
- Test: `agent/features/config/src/application_tests.rs` (create)
- Test: `agent/features/config/tests/config_reload.rs` (create)

- [ ] **Step 1: 写失败的 reload 行为测试**

使用唯一临时目录、可替换 env source 和测试 native store，写以下场景：

1. Project Aemeath 与 Claude compatibility 对同字段冲突时，Aemeath 值胜出；
2. 仅 touch 或原子替换为相同 bytes 时，`refresh_if_sources_changed()` 返回 `Unchanged`，revision 和 watch 版本不变；
3. 格式不同但 merge 后有效 `Config` 相等时，不发布；
4. 有效字段变化时 revision 恰好加一，订阅者获得新 snapshot；
5. 无效 JSON 或 validator 拒绝时返回 `Rejected`，旧 snapshot 与 revision 不变；
6. Env / CLI 覆盖文件变化时，有效配置不变，不发布。

- [ ] **Step 2: 运行测试，确认 reload 尚未实现且优先级测试失败**

Run: `cargo test -p config --test config_reload`

Expected: FAIL，显示 Claude compatibility 当前覆盖了 Project Aemeath，且没有 reload 行为。

- [ ] **Step 3: 实现内容 fingerprint 与候选 reload**

在 Config BC（不得放进 Runtime）实现 source registry：

- 初始 `load()` 成功后，对 global、Claude compatibility、project Aemeath 三个 JSON 源保存 SHA-256 内容 fingerprint；不存在文件以稳定 absent 状态表示；
- `refresh_if_sources_changed()` 首先比较 fingerprint；无内容变化立即返回 `Unchanged`；
- 有变化时复用完整 `load_config()` 管道，读取全部来源并重新应用 native、Env、CLI；
- 调整 `load_config()` push 顺序为 global → Claude compatibility → Project Aemeath → native → Env → CLI；
- merge/validate 成功后比较规范化有效配置 fingerprint；相等时更新 source fingerprint 但不发布；
- 不相等时在 mutation lock 内再次确认 active state，revision `next()`，更新 active config 与 fingerprint，`watch::Sender::send_replace`；
- 任何 load/parse/validate 失败均不得更新 fingerprint 基线，确保修复文件后下次 turn 仍会重试；旧 committed state 不变。

- [ ] **Step 4: 运行 Config crate 的单元与集成测试**

Run: `cargo test -p config`

Expected: PASS，包含 reload、优先级、旧配置保留与订阅者测试。

- [ ] **Step 5: 请求 spec compliance review**

检查：reload I/O 是否完全收敛在 Config BC；Project Aemeath 是否覆盖 Claude compatibility；无效候选是否永远不能发布。

- [ ] **Step 6: Commit**

```bash
git add agent/features/config/src/application.rs agent/features/config/src/adapters.rs agent/features/config/src/contract.rs agent/features/config/src/application_tests.rs agent/features/config/tests/config_reload.rs
git commit -m "feat(config): reload effective configuration per turn"
```

### Task 3: 将 Runtime 文件轮询拆为 Config refresh 与 prompt asset detector

**Files:**
- Modify: `agent/features/runtime/src/application/chat/looping/config_reload.rs`
- Modify: `agent/features/runtime/src/application/chat/looping/loop_phases.rs`
- Modify: `agent/features/runtime/src/application/chat/looping/loop_runner.rs`
- Modify: `agent/features/runtime/src/application/client/trait_chat.rs`
- Test: `agent/features/runtime/src/application/chat/looping/config_reload_tests.rs` (create)
- Test: `agent/features/runtime/src/application/chat/looping/loop_phases_tests.rs`

- [ ] **Step 1: 写失败的 prompt asset 检测与 reminder 测试**

覆盖：

1. guidance、global/project instruction 只有内容 hash 改变才产生 change；首次 touch 相同内容不产生 change；
2. guidance 变更生成的 LLM reminder 含实际变动路径、变更类型及 Read 指令；
3. instruction 文件变更也产生对应路径的 Read reminder；
4. reminder 不修改 cacheable `system_blocks` 或 `system_prompt_text`；
5. JSON config 文件变化由 `ConfigRefreshPort` 处理，Runtime 不再直接读取 JSON 或解析 `reload_policy`；
6. refresh `Reloaded` 才发 `ConfigReloaded` SDK 事件，`Rejected` 发失败事件/系统消息但仍以旧配置继续。

- [ ] **Step 2: 运行测试，确认旧 registry 的 mtime 首次假阳性和直接读取 JSON 行为失败**

Run: `cargo test -p runtime config_reload --lib`

Expected: FAIL，显示首次 touch 被视为 change，且旧路径仍含 `std::fs::read_to_string` 配置旁路。

- [ ] **Step 3: 实现独立 prompt asset 内容 detector**

保留 Runtime 对 prompt 资产的监听，但只注册 guidance 与指令文件：

- baseline 即计算 SHA-256，新增、删除、内容变化均以内容身份变化判断；
- JSON config 路径和 `resolve_guidance_reload_policy()` 从 Runtime 删除；
- `handle_turn_boundary_config` 接收注入的 `Arc<dyn ConfigRefreshPort>` 与当前 `ConfigSnapshot`，先 refresh JSON，再检查 prompt asset；
- 使用 snapshot 的 `guidance.reload_policy` accessor，不重新读文件；
- `inject` / `remind` 统一产生带路径列表的双语 `<system-reminder>`，明确要求 `Read`；`confirm` 保留用户确认语义但仍列出路径；
- 任何 reminder 只追加到 messages，并按现有事件同步机制发送；不得调用 prompt builder 或改变 `system_prompt_text`。

- [ ] **Step 4: 运行 Runtime 定向测试**

Run: `cargo test -p runtime config_reload --lib && cargo test -p runtime loop_phases --lib`

Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add agent/features/runtime/src/application/chat/looping/config_reload.rs agent/features/runtime/src/application/chat/looping/config_reload_tests.rs agent/features/runtime/src/application/chat/looping/loop_phases.rs agent/features/runtime/src/application/chat/looping/loop_phases_tests.rs agent/features/runtime/src/application/chat/looping/loop_runner.rs agent/features/runtime/src/application/client/trait_chat.rs
git commit -m "feat(runtime): refresh config and notify changed guidance"
```

### Task 4: 在 invocation 边界协调 Main provider binding 与运行资源

**Files:**
- Create: `agent/features/runtime/src/application/reconfiguration.rs`
- Create: `agent/features/runtime/src/application/reconfiguration_tests.rs`
- Modify: `agent/features/runtime/src/application/resources.rs`
- Modify: `agent/features/runtime/src/application/client/from_args.rs`
- Modify: `agent/features/runtime/src/application/chat/looping/loop_runner.rs`
- Modify: `agent/features/runtime/src/application/client/accessors.rs`
- Test: `agent/features/runtime/tests/config_reconfiguration.rs` (create)

- [ ] **Step 1: 写失败的资源协调测试**

构造 fake `ConfigReader` / `ProviderFactory`，覆盖：

1. revision 未变化时不重建 binding；
2. provider、model、api key、base URL、timeout、max tokens 或 reasoning 变化时，在下一 Main invocation 前构造并替换 binding；
3. binding 候选构造失败时保留旧 binding，报告失败但不停止 chat；
4. `allow_all`、language、context size 与 memory config 由下一 turn 的当前 snapshot 生效；
5. 已启动 invocation 保存旧 binding，新 invocation 才见新 binding；
6. logging level 字段变化调用动态 filter 更新 seam；`ui.tui` 被标识 restart-required、不能在运行中改写 loop 模式。

- [ ] **Step 2: 运行测试，确认 RuntimeResources 当前永久冻结启动配置**

Run: `cargo test -p runtime --test config_reconfiguration`

Expected: FAIL，显示 `RuntimeResources.binding`、hooks、semaphore 等只在 `from_args` 初始化。

- [ ] **Step 3: 实现 revision 驱动 reconfiguration coordinator**

新增 Runtime application 层的 `RuntimeReconfigurationCoordinator`：

- 持有 injected `ConfigReader`、`ProviderFactory`、当前 applied revision 与可替换 runtime handles；
- 每 turn 在 Main invocation 前读取 `committed_snapshot()`；revision 相同直接复用；
- revision 不同时，从新 snapshot 构造候选 provider binding、HookRunner、并发控制、tool-result policy 等；所有候选可用后一次替换 runtime handles；
- provider binding 构建沿用现有 model resolve / `ProviderBuildSpec` 逻辑，禁止复制模型解析规则；
- 将 `RuntimeResources` 中需要热切换的冻结值搬到 coordinator 管理的当前 view，loop 只获取该 view；
- logging level 通过 Logging 暴露的动态 update port 更新；若该 port 尚未具备，只定义 Runtime 调用 seam 与明确 issue dependency，不在 Runtime 直接操作 logger internals；
- 将 `ui.tui`、输出模式、日志目录等变更汇总为 restart-required changed fields，发出可观察事件，不尝试切换当前 UI。

- [ ] **Step 4: 将 loop 接到协调器**

在每 turn 的顺序固定为：

1. `ConfigRefreshPort::refresh_if_sources_changed()`；
2. prompt asset detector / reminder；
3. `RuntimeReconfigurationCoordinator::reconcile()`；
4. `bind_main_run()` 获取 Session/Memory 的原子组合；
5. 从 coordinator 当前 view 取得 binding 与本轮资源并发起请求。

`bind_main_run()` 必须保留为 Session / Memory / 项目切换 gate，不再被误用为文件 reload 机制。

- [ ] **Step 5: 运行 Runtime 测试与编译检查**

Run: `cargo test -p runtime && cargo check -p runtime`

Expected: PASS。

- [ ] **Step 6: 请求 spec compliance 与代码质量 review**

确认没有 Runtime 直接读 config/env；确认 binding 切换只影响下一 invocation；确认 resource candidate 失败不会破坏旧资源。

- [ ] **Step 7: Commit**

```bash
git add agent/features/runtime/src/application/reconfiguration.rs agent/features/runtime/src/application/reconfiguration_tests.rs agent/features/runtime/src/application/resources.rs agent/features/runtime/src/application/client/from_args.rs agent/features/runtime/src/application/chat/looping/loop_runner.rs agent/features/runtime/src/application/client/accessors.rs agent/features/runtime/tests/config_reconfiguration.rs
git commit -m "feat(runtime): reconcile config at invocation boundary"
```

### Task 5: 让新建 Subagent 使用与 Main 相同的动态 binding factory

**Files:**
- Modify: `agent/features/runtime/src/application/agent/runner.rs`
- Modify: `agent/features/runtime/src/application/agent/runner/setup.rs`
- Modify: `agent/features/runtime/src/application/agent/runner/loop_run.rs`
- Modify: `agent/features/runtime/src/application/startup/runtime_support.rs`
- Test: `agent/features/runtime/src/application/agent/runner_tests.rs`
- Test: `agent/features/runtime/tests/subagent_config_reconfiguration.rs` (create)

- [ ] **Step 1: 写失败的 Main/Sub 一致性测试**

使用 fake ConfigReader 与 provider factory，断言：

1. reload 后新建 Subagent 与下一 Main invocation 使用相同 revision 的模型、provider、base URL、API key、timeout 和 role 解析；
2. 已启动 Subagent 保存创建时 binding，即使随后 reload 也不被中断/替换；
3. subagent 候选 binding 构造失败不影响已运行 subagent，也不污染 Main binding；
4. role / models 配置更新只影响后续创建的 subagent。

- [ ] **Step 2: 运行测试，确认 CliAgentRunner 永久冻结启动 snapshot**

Run: `cargo test -p runtime --test subagent_config_reconfiguration`

Expected: FAIL，显示 `CliAgentRunner` 在 bootstrap 保存 `models_config`、`agents_config`、`config_snapshot`。

- [ ] **Step 3: 复用动态 binding factory，而非复制解析逻辑**

将 `CliAgentRunner` 改为持有由 coordinator 提供的窄 `CurrentRunConfig` / binding factory：

- 每次启动 subagent 时读取当前 committed snapshot 并捕获局部副本；
- 复用 Main 使用的模型解析与 `ProviderBuildSpec` 构建函数；
- subagent 的 `ContextRequest.config_snapshot` 使用创建时的局部 snapshot；
- 已启动的 subagent 不订阅或重新读取 ConfigReader。

- [ ] **Step 4: 运行 Subagent 与全部 Runtime 测试**

Run: `cargo test -p runtime --test subagent_config_reconfiguration && cargo test -p runtime`

Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add agent/features/runtime/src/application/agent/runner.rs agent/features/runtime/src/application/agent/runner/setup.rs agent/features/runtime/src/application/agent/runner/loop_run.rs agent/features/runtime/src/application/startup/runtime_support.rs agent/features/runtime/src/application/agent/runner_tests.rs agent/features/runtime/tests/subagent_config_reconfiguration.rs
git commit -m "feat(runtime): use current config for new subagents"
```

### Task 6: 更新规格、添加架构守卫并完成跨层验证

**Files:**
- Modify: `specs/config-compat.md`
- Modify: `specs/runtime.md`
- Modify: `specs/prompt.md`
- Modify: `.agents/hooks/check-config-reader-injection.sh`
- Test: `.agents/hooks/check-config-reader-injection.sh` fixture coverage

- [ ] **Step 1: 写失败的守卫/规格验证测试**

为新增或扩展的 architecture guard 建立 fixture，证明 Runtime 直接使用 `FileAdapter`、`std::fs::read_to_string` 读取 JSON 配置，或直接读取业务 env 时被拒绝；同时允许 Runtime prompt asset detector 读取 guidance / instruction 文本。

- [ ] **Step 2: 更新规范**

在 `specs/config-compat.md` 明确：

- 完整合并顺序和 Aemeath 覆盖 Claude compatibility；
- ConfigReader 为单一 committed snapshot / refresh 服务，Reader getter 不做 I/O；
- JSON reload 的 hash、effective-config diff、失败回退与 revision 语义；
- invocation、subagent、resource、restart-required 的应用边界。

在 `specs/runtime.md` 写明每 turn refresh / reconcile 顺序及 in-flight 不变式；在 `specs/prompt.md` 写明 guidance 与 instructions 改变只发 path-aware Read reminder、不能重建 system prompt。

- [ ] **Step 3: 实现或更新 guard**

扩展 `check-config-reader-injection.sh`，使其检查生产 Runtime 路径没有 JSON config 直读，但允许 prompt asset 文件读取；不得以整文件白名单绕过检查。

- [ ] **Step 4: 运行最小到完整验证**

Run:

```bash
cargo test -p config
cargo test -p runtime
cargo test -p context
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
bash .agents/hooks/check-config-reader-injection.sh
```

Expected: 全部 PASS；若 logging dynamic port 属于未完成依赖，必须在 #1326 记录阻断及可验证范围，不能静默跳过。

- [ ] **Step 5: 检查废弃路径与完整需求覆盖**

确认以下旧路径已删除或退役：

- Runtime `resolve_guidance_reload_policy()` 直接读取 global JSON；
- Runtime JSON source registry 与 config key 分类；
- 启动时永久冻结、却被动态配置替代的 Main/Sub binding 配置副本。

逐项对照本计划“边界与生效语义”与 #1326 验收，记录任何明确 out-of-scope / 外部依赖。

- [ ] **Step 6: Commit**

```bash
git add specs/config-compat.md specs/runtime.md specs/prompt.md .agents/hooks/check-config-reader-injection.sh
git commit -m "docs(config): define hot reload boundaries"
```

## 最终验证与交付

- [ ] 执行 `git diff origin/main...HEAD --check`，确认无空白错误。
- [ ] 执行 `cargo test --workspace`。
- [ ] 执行 `cargo clippy --workspace --all-targets -- -D warnings`。
- [ ] 检查 #1326 全部验收项：优先级、无效变更去重、失败回退、Main/Sub binding、安全的 guidance reminder。
- [ ] 在 #1326 更新实现状态、测试证据、任何 logging port 依赖或 restart-required 项；不关闭 issue，等待用户确认。
