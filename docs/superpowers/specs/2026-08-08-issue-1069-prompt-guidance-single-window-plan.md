# #1069 Prompt / Guidance 与唯一 Provider 窗口实施计划

> 对应 Issue：[#1069](https://github.com/rushsinging/aemeath/issues/1069)  
> 父 Issue：[#863](https://github.com/rushsinging/aemeath/issues/863)  
> Milestone：v0.1.0 — Context Engineering + 架构重构  
> 状态：已实施，待 PR 审查  
> 日期：2026-08-08

## 1. 已确认决策

1. Prompt assets 在 **Session bootstrap** 时一次物化并冻结；Session 内所有 Main Run 复用相同 system prefix。
2. Main Run、model switch 和 Subagent **NEVER** 因 Run 创建重新读取或重建 Session Prompt assets。
3. model 或 guidance 来源变化通过 reminder 表达，**NEVER** 重建 Session-frozen system prompt。
4. reminder 的触发条件与 once-per-Run 生命周期属于 Runtime；typed intent 的渲染、排序、预算和 Provider-visible 注入属于 Context。
5. `ContextPort::build_window` 是 Main 与 Sub Provider-visible 内容的唯一生产 owner。
6. Runtime 在 `ContextWindow` 生成后只做机械映射；**NEVER** 再追加、插入、删除或重写 Provider-visible system block、message 或 tool schema。
7. 每层 user guidance 使用 `AGENTS.md` 首选、`CLAUDE.md` fallback；跨层全部收集，按 global → 远祖先 → 近祖先 → 当前目录排序。
8. user guidance 仅按 canonical source identity 去重，**NEVER** 以内容相同静默删除不同来源。
9. 所有外部 model guidance 文件统一执行安全扫描；builtin guidance 不作为外部文件扫描。
10. “唯一 cache breakpoint”仅指 Context-owned stable system prefix 的唯一逻辑断点；Anthropic adapter 可另外拥有 tool 与 message-history 协议断点。

## 2. 当前问题与根因

### 2.1 Provider 窗口存在后置装饰路径

主链已经是：

```text
ContextRequest
  → ContextPort::build_window
  → ContextWindow
  → extract_invocation_context
  → InvocationRequest
  → Provider adapter
```

但 `application/model/invocation.rs` 在 `extract_invocation_context` 后直接修改 `messages_for_api`，注入 Task reminder。该行为让 Context 产出的窗口不再是最终 Provider 请求事实，形成第二条请求侧窗口装饰路径。任何新增 guidance/model reminder 若继续放在 Runtime 后置注入，都会扩大双轨。

### 2.2 Reload reminder 位于失配的 Run message state

`handle_turn_boundary_config` 当前把 `Inject` / `Remind` / `Confirm` 文本加入 `RunExecutionState.messages` 的启动快照。共享 Loop 构建 `ContextRequest.pending_messages` 时并不直接消费这份初始化消息，因此它不是稳定、明确的 Provider 注入 seam；它同时可能影响 SDK/TUI 投影和 reflection 输入。触发、通知、Provider reminder 与对话状态职责混在同一个 `Vec<Message>` 中。

### 2.3 Prompt 生命周期术语漂移

现有 Composition/Runtime 实现把 Prompt assets 保存为 Session-life 数据，这是保护 cacheable prefix 的正确边界；#1069、#863 与部分设计文字仍使用 Run-frozen、下一 Main Run重读等语义，容易诱导每 Run 重建 system prompt。

### 2.4 Guidance 文件读取策略不统一

model guidance 的 config-file 路径会执行安全扫描，但 `_default.md`、`_reasoning.md`、model-prefix 和语言目录文件绕过扫描。user guidance 当前每层可同时加载 `AGENTS.md` 与 `CLAUDE.md`，且存在按内容去重，均不符合已确认规则。

## 3. 目标架构

### 3.1 唯一窗口生产链

```text
Session-frozen PromptAssembly
         │
Run creation / source-change detection
         │ Runtime：只生成 typed ReminderIntent 与生命周期状态
         ▼
ContextRequest
  ├─ canonical + pending messages
  ├─ session-frozen base system prompt
  ├─ run/model/config facts
  └─ invocation-only ReminderIntent list
         │
         ▼
ContextPort::build_window
  ├─ Prompt materialization
  ├─ reminder render/order/budget
  ├─ memory/summary/compact
  ├─ tool schemas
  └─ unique stable-system cache break
         │
         ▼
ContextWindow（最终 Provider-visible 内容）
         │ Runtime：纯机械映射
         ▼
InvocationRequest
         │ Provider：仅 wire serialization
         ▼
Provider request
```

Main 与 Sub 只允许输入事实不同，不允许窗口组装算法不同。Subagent 的固定 base system prompt仍可由 Agent Tool/Runtime提供，但进入 `ContextRequest` 后必须与 Main 共用同一 `build_window → ContextWindow → InvocationRequest` 路径。

### 3.2 Reminder 所有权

#### Runtime 拥有

- source/config/model change detection；
- Session prompt model 与当前 binding 的比较；
- Main/Sub Run 类型与 model identity；
- typed `ReminderIntent` 的产生；
- once-per-Run / once-per-purpose 生命周期；
- UI/SDK diagnostic event；
- reminder 不进入 canonical append 的约束。

#### Context 拥有

- typed intent 到本地化文本的渲染；
- Provider-visible message 位置与稳定排序；
- invocation-only message 的领域身份；
- token estimation 与 compact 边界；
- 与 canonical、pending、memory、summary 的最终合并；
- 输出最终 `ContextWindow`。

#### Provider 拥有

- `ContextWindow` 经 Runtime 机械映射后的 wire serialization；
- Provider 私有 cache-control 字段；
- **NEVER** 理解 reminder 类型或读取 guidance/config 文件。

### 3.3 Reminder 数据模型

在 Context Published Language 中新增 typed intent，名称在实现前按职责复核。候选结构：

- `InvocationReminderKind`
  - `TaskProgress`
  - `GuidanceSourcesChanged`
  - `ModelGuidanceMismatch`
- `InvocationReminder`
  - `kind`
  - renderer 所需的结构化、安全字段

`ContextRequest` 携带 reminder intents；**NEVER** 携带 Runtime 预渲染的任意字符串或物理 guidance 路径。Context 渲染为 invocation-only user message，并确保：

- 不进入 CanonicalSession；
- 不进入 finalized Step append；
- 不进入 Resume backing；
- 不进入 SDK/TUI conversation history；
- 纳入 Provider request token estimation；
- 同一 Run 的 tool round/retry 不重复。

若 reminder 必须建议 Read，只允许使用公开、稳定、经 Context 定义的抽象描述；是否暴露物理路径需单独安全审查，不由 Runtime 文案硬编码。

### 3.4 Session-frozen Prompt assets

`PromptAssembly.system_blocks/system_prompt_text/user_context` 继续在 Composition bootstrap 物化并保存在 SessionRuntime。实现和文档统一为：

- Session 内 Main Run 不重建 system prefix；
- model switch 只改变后续 Run 的 binding，不改变 Session-frozen prefix；
- model mismatch 通过 invocation-only reminder 表达；
- Subagent 不读取或重建 Session Prompt assets；
- 新 Session 才重新物化磁盘 Prompt assets。

### 3.5 User guidance 选择与去重

每个逻辑层只选择一个来源：

```text
if AGENTS.md exists and is readable:
    select AGENTS.md
else if CLAUDE.md exists and is readable:
    select CLAUDE.md
else:
    no source for this layer
```

随后：

1. 收集 global 与全部项目目录层；
2. 按 global → 远祖先 → 近祖先 → 当前目录排序；
3. 对选中的真实文件做 canonical identity 去重；
4. 不按内容去重；
5. 对每个最终来源执行安全扫描与 `InstructionsLoaded` hook；
6. 保持 source path XML escaping。

“存在但读取失败”采用 fail-closed 还是 fallback 必须在首个 TDD 用例前明确。推荐：不可读 `AGENTS.md` 记录诊断后 fallback 到同层 `CLAUDE.md`，避免整层静默丢失；读取错误不得改变其他层顺序。

### 3.6 Cache breakpoint 精确定义

- Context：stable system prefix 只有一个 `cache_break=true` 块；这是 Context 领域唯一逻辑断点。
- Runtime：一对一映射该标记为 `RequestSystemBlock::Cacheable`，不得新建断点。
- Anthropic：可在最后一个 tool schema 与历史 message 上设置额外协议断点。
- OpenAI Chat/Responses/Ollama：消费文本，不接收 Anthropic 私有 `cache_control`。

文档、测试名和注释统一使用“唯一 Context system-prefix breakpoint”，不再宣称整个 Provider request 只有一个断点。

## 4. 实施任务

所有核心逻辑按 TDD 执行：先写失败测试，确认失败原因，再修改生产代码。跨层改动必须为每个相邻边界补测试。

### 任务 1：锁定现状与更新目标文档

**文件**

- `docs/design/02-modules/context-management/04-prompt-guidance.md`
- `docs/design/03-engineering/03-migration-governance.md`
- `docs/design/03-engineering/04-testing-and-coverage.md`
- `specs/3.4-runtime.md`
- `specs/3.7-prompt.md`
- `specs/3.9-config-compat.md`

**步骤**

1. 将 Prompt assets 生命周期改为 Session-frozen。
2. 记录 Runtime intent / Context render / Provider serialization 的所有权。
3. 定义 Main/Sub 唯一 `build_window` 生产链。
4. 修正 user guidance 首选/fallback 与 canonical 去重规则。
5. 修正 cache breakpoint 术语。
6. 在迁移治理中记录 Runtime 后置 Task reminder、reload message state 和 guidance 扫描差距。

**验证**

- `rg -n "Run-frozen|下一 Main Run|unique cache breakpoint|唯一.*cache" docs specs`
- 人工逐项核对本文第 1 节决策无冲突。

### 任务 2：为 invocation-only reminder 建立 Context Published Language

**文件**

- `agent/features/context/src/domain.rs`
- `agent/features/context/src/ports.rs`
- `agent/features/runtime/src/ports/context_port.rs`
- `agent/features/runtime/src/application/loop_engine/context_request.rs`
- 对应 `*_tests.rs` / `tests/` 契约测试

**测试先行**

1. 无 reminder 时 `ContextRequest` 与窗口行为保持不变。
2. typed reminder 被 Context 渲染到最终窗口一次。
3. 多 reminder 按稳定业务顺序输出，不依赖 `HashMap/HashSet` 遍历。
4. reminder 不出现在 canonical append DTO。
5. reminder 纳入 token estimation。

**实现**

- 新增 typed reminder PL；
- 将 intents 加入 `ContextRequest`；
- 在 Context application service 的唯一窗口组装阶段渲染 invocation-only messages；
- 不把 intent 或渲染文本写入 Session backing。

**验证**

- `cargo test -p context --lib`
- `cargo test -p context --test application_service_contract`
- `cargo test -p runtime context_request`

### 任务 3：迁移 Task reminder，删除 Runtime 后置窗口装饰

**文件**

- `agent/features/runtime/src/application/model/invocation.rs`
- `agent/features/runtime/src/application/run/execution_state.rs`
- `agent/features/runtime/src/application/loop_engine/llm_strategy.rs`
- `agent/features/runtime/src/application/loop_engine/llm_strategy_tests.rs`
- Task reminder renderer 的现有 owner 与相关测试

**测试先行**

1. Runtime 的 `ContextWindow → InvocationRequest` 映射字符级保持 system/messages/tools，不新增文本。
2. Task reminder 由请求中的 typed intent 经 Context进入首个窗口。
3. 同 Run tool round/retry 不重复。
4. 下一 Run 可再次产生 intent。
5. SDK/TUI message snapshot 与 canonical Session 不包含 Task reminder。

**实现**

- 将 Task snapshot 转为 typed intent；
- 删除 `build_task_reminder_for_invocation` 和 `messages_for_api.push(...)`；
- 删除 `task_reminder_injected` 后置注入状态，或将通用 once 状态收敛到 Run 创建时 intent 冻结；
- 保持 `extract_invocation_context` 为纯映射。

**验证**

- `cargo test -p runtime llm_strategy`
- `cargo test -p runtime model::invocation`
- `cargo test -p runtime task_reminder`

### 任务 4：收敛 guidance reload 与 model mismatch intent

**文件**

- `agent/features/runtime/src/application/loop_engine/chat/loop_phases.rs`
- `agent/features/runtime/src/application/loop_engine/chat/session_driver/run_launch.rs`
- `agent/features/runtime/src/application/loop_engine/chat/config_reload.rs`
- `agent/shared/src/config/domain/config.rs`
- `agent/shared/src/i18n/prompt/**`
- Context reminder renderer 与相关测试

**测试先行**

1. source change 只产生 diagnostic event 与 typed intent，不写 `RunExecutionState.messages`。
2. `Inject` / `Remind` / `Confirm` 的可观察差异具有明确契约；任何策略都不重建 system prompt。
3. model switch 后下一 Main Run 产生一次 `ModelGuidanceMismatch` intent。
4. 同 Run retry/tool round不重复。
5. reminder 不进入 canonical、SDK/TUI conversation、Resume。
6. reflection 输入不包含 reload/model reminder，除非 Context 明确将 reflection 定义为 Provider invocation 并复用同一窗口；禁止从 `RunExecutionState.messages` 泄漏。

**实现**

- 从 `handle_turn_boundary_config` 删除普通 `messages.push(...)`；
- 将检测结果转成结构化 Run launch facts；
- 在创建 `ContextRequest` 前冻结当前 Run 的 reminder intents；
- model switch 只更新 binding，下一 Main Run 比较 Session prompt model 与 Run model生成 intent；
- Config policy 保留、收窄或退役的最终决定必须以测试表达，不允许三种策略只剩不同文案但无领域差异。

**验证**

- `cargo test -p runtime config_reload`
- `cargo test -p runtime session_driver`
- `cargo test -p runtime model_switch`
- `cargo test -p context reminder`

### 任务 5：让 Subagent 复用相同 reminder/window 管线

**文件**

- `agent/features/runtime/src/application/run/derived/setup.rs`
- `agent/features/tools/src/adapters/agent_tool.rs`
- `agent/features/runtime/src/application/run/derived/tests/**`
- 共享 ContextRequest 构建代码与契约测试

**测试先行**

1. Subagent 不读取或重建 Session Prompt assets。
2. Subagent 固定 base system prompt作为 `ContextRequest.system_prompt` 输入。
3. Subagent 使用不同模型时产生一次 typed model reminder。
4. Main/Sub 都只通过 `ContextPort::build_window` 生成 Provider-visible 内容。
5. Sub reminder 不污染父 Session、Sub canonical backing、SDK/TUI 或 retry。

**实现**

- 复用 Main 的 typed reminder 输入和 ContextRequest coordinator；
- 禁止在 Agent Tool/Derived invocation 层拼接 reminder 文案；
- 不改变 Subagent 固定 system template 的所有权。

**验证**

- `cargo test -p runtime run::derived`
- `cargo test -p tools agent_tool`
- Main/Sub 相邻契约测试。

### 任务 6：统一 model guidance 外部文件安全扫描

**文件**

- `agent/features/context/src/adapters/prompt/guidance/resolver.rs`
- `agent/features/context/src/adapters/prompt/guidance/resolver_tests.rs`
- `agent/features/context/src/adapters/prompt/security.rs`

**测试先行**

分别为以下来源构造风险内容并断言扫描发生且组合顺序不变：

1. `_default.md`；
2. language-specific `_default.md`；
3. `{prefix}.md`；
4. language-specific `{prefix}.md`；
5. `_reasoning.md`；
6. config-map file path；
7. builtin fallback 不被误判为外部文件。

**实现**

- 建立单一外部 guidance 文件读取/扫描 helper；
- 所有外部文件统一经过该 helper；
- builtin 内容走独立显式入口；
- 扫描只产生安全诊断还是装饰 prompt，按目标文档统一，禁止不同来源行为不一致。

**验证**

- `cargo test -p context guidance`
- `cargo test -p context security`

### 任务 7：修正 user guidance 每层首选/fallback 与去重

**文件**

- `agent/features/runtime/src/application/prompt/build/prompt_build.rs`
- `agent/features/runtime/src/application/prompt/build/prompt_build_tests.rs`
- 必要时将文件选择 adapter 迁入 Context-owned guidance source

**测试先行**

1. 同层两文件同时存在时只加载 `AGENTS.md`。
2. `AGENTS.md` 缺失时加载 `CLAUDE.md`。
3. 每个祖先层独立执行首选/fallback，并全部组合。
4. global 层同样首选/fallback。
5. symlink/相对路径 alias 按 canonical identity 只加载一次。
6. 不同 canonical 文件内容相同仍保留两份来源。
7. 选择后的每个文件均执行安全扫描、hook 和 XML path escaping。
8. 不可读首选文件的 fallback 语义有明确测试。

**实现**

- 用单一 `select_instruction_for_layer` helper 表达每层规则；
- 删除同层双文件同时加载；
- 删除内容 HashSet 去重，仅保留 canonical identity；
- 保持跨层确定顺序。

**验证**

- `cargo test -p runtime prompt_build`
- 若迁入 Context，再运行对应 Context adapter tests。

### 任务 8：精确化 cache breakpoint 契约

**文件**

- `agent/features/context/src/domain.rs`
- `agent/features/context/src/application/service.rs`
- `agent/features/runtime/src/application/loop_engine/llm_strategy.rs`
- `agent/composition/src/provider.rs`
- `agent/features/provider/src/adapters/anthropic.rs`
- `agent/features/provider/src/adapters/anthropic/message_conversion.rs`
- OpenAI Chat/Responses/Ollama 契约测试

**测试先行/补强**

1. Context stable system blocks 恰有一个逻辑 breakpoint，且位于最后一个 cacheable block。
2. Runtime 一对一映射，不增加 breakpoint。
3. Anthropic wire 可同时存在 system、tool、message-history 三类断点。
4. OpenAI Chat/Responses/Ollama wire 无 Anthropic 私有字段。
5. reminder 位于 dynamic messages，不改变 system-prefix bytes 或 breakpoint 位置。

**实现**

- 修正注释、测试名和文档术语；
- 只有发现实现与契约不符时才修改生产逻辑。

**验证**

- `cargo test -p context application_service_contract`
- `cargo test -p runtime llm_strategy`
- `cargo test -p composition provider`
- `cargo test -p provider cache`

### 任务 9：新增唯一窗口 L0 架构 Guard

**文件**

- `.agents/hooks/check-provider-window-single-owner.sh`（建议新建）
- `.agents/hooks/check-architecture-guards.sh`
- `.agents/aemeath.json`
- `docs/design/03-engineering/01-architecture-guards.md`
- Guard 测试夹具/自测入口

**规则**

1. Runtime 生产代码不得在 `extract_invocation_context` 后对 `messages_for_api` 做 `push/insert/extend/remove/retain`。
2. Runtime 生产代码不得构造 `<system-reminder>`、`<task-reminder>` 等 Provider-visible reminder 文案。
3. Main/Sub Provider invocation 必须消费 `ContextWindow` 的同一机械 mapper。
4. Provider adapter不得读取 Context/Config/guidance 文件。
5. Context 以外不得定义第二个 Provider-visible window assembler。

Guard 必须使用 Rust AST 或足够窄的结构扫描；**NEVER** 以大范围关键词 grep 制造误报白名单。新增 Guard 前加载并遵循架构守卫设计文档。

**验证**

- Guard 正例运行成功；
- 每条规则以最小故意违规 fixture 证明失败；
- 总编排脚本对失败返回约定退出码。

### 任务 10：建立 L0–L4 场景证据矩阵

**L1**

- guidance 排序、语言 fallback、安全扫描；
- user guidance 每层选择、canonical 去重；
- reminder renderer 与稳定排序；
- cache-break 标记纯函数。

**L2**

- Runtime intent → ContextRequest；
- Context prompt/reminder/memory/window 协作；
- 同 Run 重复 build/invoke 的 system-prefix bytes 稳定；
- reminder once 生命周期。

**L3**

- ContextWindow → InvocationRequest 无装饰契约；
- InvocationRequest → Anthropic/OpenAI/Responses/Ollama wire；
- canonical append / Resume codec 不包含 invocation-only reminder。

**L4**

1. Session 中途修改 guidance：system prefix bytes 不变，下一 Main Run 只收到一次 reminder。
2. model switch：system prefix bytes 不变，下一 Main Run 使用新 binding 并收到一次 model reminder。
3. Subagent 使用不同模型：不重物化 Session assets，首个 invocation 仅收到一次 reminder。
4. Task reminder：只出现在 Context-produced Provider window，不进入 SDK/TUI/canonical/Resume。
5. Main/Sub 均无法绕过唯一 window mapper。

**验证**

- 将每条 #1069 验收条件映射到测试名和执行命令；
- 未覆盖项必须记录承接 Issue，不得以低层测试替代。

### 任务 11：执行完整验证并回写 Issue

**定向验证**

- `cargo fmt --all -- --check`
- `cargo test -p context`
- `cargo test -p runtime`
- `cargo test -p provider`
- `cargo test -p composition`
- 相关 Guard 脚本。

**门禁验证**

- `cargo build`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --workspace`
- `bash .agents/hooks/check-architecture-guards.sh`
- 按 `specs/3.2-rust-coding.md` 运行生产可达性与依赖方向检查。

**Issue 回写**

- #1069：附“需求 → 实现 → 测试 → 命令”矩阵；
- #863：回写 Session-frozen Prompt、reminder 所有权与唯一窗口结论；
- 不自动关闭 Issue，等待用户确认与 PR 合并。

## 5. 实施顺序与依赖

```text
任务 1 文档语义
  → 任务 2 Reminder PL
  → 任务 3 Task reminder 迁移
  → 任务 4 Reload/model reminder
  → 任务 5 Subagent 对齐

任务 6 Guidance 安全扫描 ─┐
任务 7 User guidance 选择 ├→ 任务 10 L0–L4 矩阵
任务 8 Cache 契约 ────────┤
任务 9 唯一窗口 Guard ────┘

任务 10 → 任务 11 完整验证与 Issue 回写
```

任务 6、7、8 在任务 1 完成后可并行；任务 9 应在任务 2–5 的目标代码形态稳定后实施，避免 Guard 固化过渡结构。

## 6. 最小补丁与根因方案对比

### 最小补丁

- 只修 model guidance 安全扫描；
- 只修每层 `AGENTS.md`/`CLAUDE.md` 选择；
- 只改 cache-break 文案；
- guidance/model reminder继续在 Runtime 的局部 message 或 Provider request 上拼接。

**优点**：改动小、短期风险低。  
**缺点**：Main Provider 窗口仍双轨；Task/guidance/model reminder 会继续绕过 Context；新增 reminder 时重复发生所有权漂移。  
**结论**：仅适合临时止血，不能满足 #1069 的架构完成定义。

### 根因方案（推荐并按本计划实施）

- Runtime 只产 typed intent；
- Context 独占最终窗口；
- Main/Sub 共用单链；
- Guard 阻止后置装饰复发；
- 同步修复扫描、fallback 与 cache 术语。

**优点**：消除第二窗口 owner，所有 reminder 自动纳入预算、顺序与持久化边界测试。  
**成本**：跨 Context/Runtime/Provider/Composition/Tools 与设计文档，需逐层 TDD。  
**风险**：若一次性迁移过大，可能影响 Task reminder、model switch 和 Subagent；通过任务 2–5 的分步兼容迁移和 L2/L3 相邻测试控制。

## 7. 完成定义

- Session-frozen Prompt assets 的实现、文档和测试一致。
- `ContextPort::build_window` 是 Main/Sub Provider-visible 内容的唯一生产 owner。
- Runtime `ContextWindow → InvocationRequest` 为纯机械映射，无后置文本装饰。
- Task、guidance change、model mismatch reminder 均以 typed intent进入 Context，且满足 once、预算和非持久化语义。
- 所有外部 model guidance 文件统一安全扫描。
- 每层 user guidance 只选择 `AGENTS.md` 或 fallback `CLAUDE.md`，只按 canonical identity 去重。
- Context system-prefix 唯一逻辑 breakpoint 与 Anthropic wire 多协议断点术语清晰。
- L0–L4 矩阵无未解释空白，定向与 workspace 门禁全部通过。
- #1069/#863 已回写验证证据；Issue 关闭仍等待用户确认。
