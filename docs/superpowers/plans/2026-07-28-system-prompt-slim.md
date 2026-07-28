# v0.1.0 System Prompt 精简实施计划

> 对应 Issue：[#1448](https://github.com/rushsinging/aemeath/issues/1448)
> 分支：`feat/1448-system-prompt-slim`
> 基线：`origin/main` @ `7940df0f`
> Milestone：`v0.1.0 — Context Engineering + 架构重构`

## 1. 目标与完成定义

精简 Aemeath 内置 System Prompt，使现代 LLM 获得更短、更稳定、无重复的行为契约，同时不削弱以下能力：

- 工具调用的真实性与专用工具优先原则；
- 文件修改、外部副作用和权限模式的安全边界；
- 修改后的验证要求；
- Main/Sub 隔离及 Agent prompt 自包含语义；
- `path_base`、`workspace_root` 和 worktree 上下文切换；
- Skills、Memory、user guidance、active summary 的现有顺序、预算、缓存和安全扫描语义；
- Task reminder 仅注入 Provider 对话副本，不污染 System Block、canonical session、SDK、TUI 或 JSON 用户消息。

完成条件：

1. 每个稳定 System Block 只有一个生产 owner；主会话不再重复注入 `execution_discipline`。
2. 核心静态 Prompt 只保留 Aemeath 特有、不能由 Tool Schema 或 Runtime 强制保证的契约。
3. 工具级细节由工具 description/schema 承载；模型差异由 model guidance 承载；不在核心 Prompt 重复描述。
4. Main 与 Sub 的 system block 组合遵循同一 Context 物化契约，role suffix 仅作为显式扩展。
5. `task_reminder` 不再成为 System Block；仅在 Provider invocation 的消息副本中，追加到本轮最后一条真实 user message 的 text，并使用 `<task-reminder>...</task-reminder>` 标志。
6. canonical session、持久化 JSON、SDK `ChatMessage`、`UserMessagesAdopted` 与 TUI 回显始终保留用户原文，不包含 reminder。
7. System blocks 不再包含每轮动态 reminder，全部归入可缓存前缀，并保持唯一 cache breakpoint 和已有功能行为兼容。
8. 有可追溯的 L0、L1/L2、L3、L4 验证证据，并记录精简前后 token/byte 基线。

## 2. 当前根因与约束

### 2.1 已确认的重复注入

当前 Main 启动流程在 `agent/features/runtime/src/application/prompt/prompt_build_ext.rs` 中将：

```text
static_part + execution_discipline + agent_roles + model_guidance
```

拼成 `system_prompt_text`；随后 Context 的 `BaselinePromptSource` 又从 `ContextRequest` 生成：

```text
system_prompt + execution_discipline
```

因此 Runtime 传入的 `system_prompt` 已可能包含执行纪律，Context 又追加独立的 `execution_discipline` block，导致重复。

### 2.2 已确认的 Task reminder 缓存污染

当前 Main Runtime 在 `main_run_port.rs::freeze_request()` 中读取 Task 快照并写入 `ContextRequest.task_reminder`；随后 `ContextApplicationService::build_candidate()` 将其渲染为 `kind = "task_reminder"`、`cacheable = false` 的 `SystemBlock`。`extract_invocation_context()` 再把所有 `ContextWindow.system_blocks` 映射为 Provider system 输入。

这使每轮可能变化的 Task 状态进入 system 消息，破坏“System Prompt 全静态、可缓存”的目标。与此同时，canonical session、SDK/TUI 回显和 JSON 持久化都应继续保存用户原文，不能把 reminder 直接写回 `ContextWindow.messages` 或 accepted input。

目标边界：Context 仍接收结构化 `TaskReminderSnapshot` 并负责双语渲染，但产出独立的 invocation-only message decoration；Runtime 在 `ContextWindow → InvocationRequest` 映射时，只克隆 LLM 可见消息并将 `<task-reminder>...</task-reminder>` 追加到本轮最后一条真实 user message 的 text block。原始 Context message 不变。

### 2.3 已确认的下沉机会

- 文件工具 description 已包含 `Read first`、精确替换和 `old_string`/`new_string` 语义。
- Worktree 工具已包含 `path_base` / `workspace_root` 和上下文切换后的路径规则。
- Agent 工具已包含隔离会话、自包含 prompt、角色模型和子代理限制说明。
- Task 工具已包含复杂任务阈值、单一可验证步骤和生命周期操作说明。
- Tools BC 已将 `phase` 作为运行时元数据剥离，不属于业务 schema。

因此核心 System Prompt 不应继续逐项复制这些工具级说明。

### 2.4 设计基准

沿用 `docs/design/02-modules/context-management/04-prompt-guidance.md` 的 owner 约束，但将其中 task reminder 作为 System Block 的旧目标同步修正。Context Window assembler 负责最终静态 System Block 编排，稳定前缀顺序为：

```text
system_prompt
→ execution_discipline
→ model_guidance
→ skills
→ agent_roles
→ user_guidance
→ memory_context
→ active_summary
→ 唯一 cache breakpoint
```

`task_reminder` 不属于 System Prompt。它由 Context 根据 `TaskReminderSnapshot` 生成 invocation-only decoration，并在 Runtime 的共享 Main/Sub invocation 映射 seam 中应用到 Provider 消息副本：

```text
最后一条真实 user message 原文

<task-reminder>
双语 reminder 文本
</task-reminder>
```

这里“真实 user message”指 `Role::User` 且来源为用户的消息；`SystemGenerated` 与 `StopHook` 不作为注入目标。若本轮没有可用的真实 user message，则不伪造新 user message，保持 reminder 待下一次真实用户输入再投影。每次都从 canonical 原文重新构造 Provider 副本，禁止在工具续轮或 retry 中重复追加。

Git 首次快照继续作为普通系统生成消息；日期、工作区变化和 commit guidance 不进入 System Prompt。

## 3. 不在本期范围

- 不改变 Tool 的业务 schema、权限策略、workspace ACL 或执行逻辑。
- 不重写 Skills 的 source discovery、解析、预算或安全扫描算法。
- 不改变 Memory 检索、Reflection、compact 和 Task 状态机；只调整 Task reminder 的 LLM 出站投影位置。
- 不改变 canonical session / accepted input 的落盘 schema，不在 SDK、TUI 或 JSON DTO 中新增 reminder 字段或修改用户消息正文。
- 不新增模型能力探测或 A/B Prompt profile；本期先建立精简后的单一现代默认 Prompt，模型专属差异继续使用既有 guidance。
- 不为了迁移而批量移动无关测试或文档。
- 不修改用户自定义 `~/.agents/guidance/` 内容及其优先级语义。

## 4. 分阶段实施

### 阶段 A：建立 Prompt 资产清单与基线

**目标**：在任何行为改动前锁定现状和每条规则的归属。

**步骤**：

1. 列出所有面向 LLM 的内置文案来源：
   - `agent/shared/src/i18n/prompt/system.rs`
   - `agent/shared/src/i18n/prompt/discipline.rs`
   - `agent/shared/src/i18n/prompt/sections.rs`
   - `agent/shared/src/i18n/prompt/commit.rs`
   - `agent/shared/src/i18n/tools/**`
   - `agent/features/runtime/src/application/prompt/**`
   - `agent/features/context/src/adapters/prompt/**`
   - 子代理启动文案。
2. 为每个片段记录：当前 owner、注入路径、语言版本、cacheable 状态、是否重复、是否已有 Tool/Runtime 强制、目标去向。
3. 增加或复用测试辅助函数，构造代表性 Main Prompt 和 Context Window，输出 UTF-8 byte 数与 token estimate；保存精简前基线，不把完整 Prompt 内容写入生产日志。
4. 明确静态 Prompt 的保留清单与删除清单，作为后续测试断言的依据。

**交付物**：Prompt 资产矩阵、精简前 baseline、保留/下沉决策表。

**验证**：L1 纯函数/组装测试；L0 确认基线仅使用测试入口，不扩大生产 API。

### 阶段 B：收口唯一基础 Prompt owner

**目标**：消除主会话重复执行纪律，并让 `ContextPromptSource` 成为最终基础 System Block 的唯一 owner。

**步骤**：

1. 将 `static_system_prompt_for()` 的职责限定为核心 `system_prompt`：身份、工具真实性、必要安全边界、验证原则、环境/worktree 语义和子代理隔离摘要。
2. 从 Runtime 的 `build_static_prompt()` 移除 `universal_execution_discipline` 的拼接；Runtime 不再把执行纪律嵌入 `SystemPromptSpec`。
3. 保留 `BaselinePromptSource` 对 `system_prompt` 与 `execution_discipline` 的单次 materialization。
4. 确认 `SkillPromptSource` 复用同一 baseline，不产生第二套基础 block。
5. 若当前 Main 启动仍需传递 model guidance、roles 或 user guidance，改为通过 Context-owned materialization 的明确输入传递；禁止继续在 Runtime 先拼完整字符串再交给 Context。
6. 统一 Main/Sub 的 system block 入口；role-specific suffix 只在 Sub Run 的显式 role 扩展点追加一次。

**交付物**：无重复的基础 Prompt 管线；Main/Sub 对应的 owner 和输入边界。

**验证**：

- L1：`system_prompt` 不包含 `execution_discipline`，语言和权限模式替换仍正确。
- L2：baseline materialization 恰好产生预期 kind 和顺序。
- L3：Main/Sub invocation 均能完整映射 Context blocks；Provider 未收到重复基础块。

### 阶段 C：精简核心静态 Prompt

**目标**：把百余行流程规约压缩为短的产品契约。

**保留**：

- Agent 身份和“以完成且验证用户目标为准”；
- 涉及文件、命令或计算时必须使用真实工具，不得虚构结果；
- 有专用工具时优先使用专用工具；
- 修改或有副作用操作遵循当前权限/确认机制；
- 修改后按范围执行验证，未验证不得声称完成；
- 子代理是隔离会话，prompt 必须自包含；
- 当前 `path_base`、`workspace_root`、git/worktree 上下文必须以最新工具结果为准；
- 基本安全要求：不得引入常见注入、越权、凭据泄露风险；
- 用户未要求时保持简洁。

**下沉/删除**：

- Read/Edit/Write/Glob/Grep 的逐项替代表格：由工具 description 承载；
- Agent 两阶段浏览细则和 BAD/GOOD 示例：由 Agent 工具/相关 skill 承载；
- TaskList 全生命周期、依赖和示例：由 Task 工具与 Runtime 状态维护承载；
- `phase` 完整教程：保留 Tools 元数据契约，核心 Prompt 只保留必要的一句或移除；
- 12,000 字符、SSE、provider 输出预算等实现细节：移至 Write/Prompt 工具 description 或开发文档；
- Bug 最小补丁/根因方案、TDD、架构审查等长流程：由对应 skill 和仓库 guidance 承载；
- 模型 reasoning 语言/长度要求：保留在模型 guidance，不放入通用核心 Prompt；
- 重复的 `old_string`/`new_string`、AskUserQuestion 选项细则和 commit workflow。

**注意**：只删除重复指导，不删除工具本身的 description/schema，也不删除 Runtime 的安全校验。

**验证**：

- L1：EN/ZH 选择、占位符、权限模式和关键保留契约断言；
- L2：精简后 baseline 文本不存在已下沉长段落；
- L4：文件探索、编辑、验证、worktree 切换、Agent 派发代表性场景不退化。

### 阶段 D：将 Task reminder 迁出 System Prompt

**目标**：System blocks 只包含静态、可缓存内容；Task reminder 只影响 Provider invocation 的消息副本。

**步骤**：

1. 先修改 Context 契约测试，要求存在 unfinished Task 时 `ContextWindow.system_blocks` 中也不存在 `task_reminder`。
2. 将 Context 当前的双语 reminder 格式提取为 typed invocation decoration（仍由 `TaskReminderSnapshot` 纯值生成），使用结构标签 `<task-reminder>...</task-reminder>`，避免 Runtime 重新拥有业务文案。
3. 扩展 `ContextWindow` 的 invocation-only 输出契约，携带可选 reminder decoration；它不属于 canonical messages，不参与 accepted input、session envelope、SDK DTO 或 TUI event。
4. 在 Main/Sub 共用的 `extract_invocation_context()` seam 中，从 canonical message 的 `to_llm_view()` 副本开始，定位最后一条 `Role::User` 且 `MessageSource::User` 的消息，将 reminder 追加到其最后一个 text block；无 text block 时追加新的 text block。
5. 若窗口中没有真实 user message，不伪造消息、不注入到 `SystemGenerated`/`StopHook`；保留结构化 decoration，待后续包含真实用户输入的 window 再投影。
6. retry、工具续轮与 compact 后重建 invocation 时必须每次从 canonical 原文生成副本，禁止对已装饰消息再次装饰；同一请求中 `<task-reminder>` 最多出现一次。
7. token estimation 与 compaction decision 按最终 Provider 可见消息计入 reminder token，不再计入 `system_tokens`；Provider request logging 只记录既有安全摘要，不新增 reminder 正文泄露。
8. 保持 `AcceptedInputAppend`、canonical session JSON、SDK `ChatMessage`、`UserMessagesAdopted`、TUI 回显/复制/resume 的用户原文逐字不变。

**交付物**：全静态 System Block 管线；typed invocation-only reminder；无持久化/UI 污染的 Provider 消息投影。

**验证**：

- L1：双语 tag 渲染、目标 user message 选择、纯副本装饰、空 reminder/无真实 user/no text block、重复调用幂等。
- L2：Context window 不含 `task_reminder` system kind；reminder token 归入 message budget；canonical messages 与 JSON 序列化不变。
- L3：Runtime → Provider 的 user text 含且仅含一个 tag；Main 空 Task 与 Sub 默认路径不改变消息；retry/工具续轮不重复。
- L4：TUI、SDK event、session resume/JSON 继续显示用户原文，同时 Provider harness 观察到 reminder。

### 阶段 E：清理动态组装与过期路径

**目标**：删除迁移遗留，避免后续再次把规则拼回核心 Prompt。

**步骤**：

1. 检查 `prompt_build_ext.rs` 中 model guidance、agent roles、skills header 的实际 owner；将仍由 Runtime 拼接的稳定 System Block 迁入 Context Prompt materialization，或明确保留 Runtime 输入为独立 typed fragment，不允许裸字符串混合。
2. 删除旧的 `task_reminder` SystemBlock 组装、kind 断言和 uncached system 路径，禁止保留双轨兼容。
3. 清理不再使用的拼接 helper、旧常量、re-export、测试专用生产入口和过期注释。
4. 检查 `SystemPromptSpec`、`PromptMaterialization`、`SystemBlock` 的字段是否仍能表达唯一 owner、kind、revision、cacheable/cache_break；不为兼容而保留第二套拼装模型。
5. 检查 Main/Sub role suffix 与 Hook 输入，确保 Hook 看到的 system 文本与实际 provider invocation 一致。
6. 同步更新 `specs/prompt.md`、`specs/runtime.md` 和 Context Prompt 设计文档：System blocks 全静态且 task reminder 属于 invocation-only user-message decoration。

**验证**：L0 架构守卫、L2 组装顺序、L3 Runtime/Context/Provider 契约和死代码检查。

### 阶段 F：行为回归与基线对比

**目标**：证明精简减少了 Prompt，但未牺牲关键行为。

**步骤**：

1. 对同一模型、语言、权限模式和工具集合分别生成 before/after baseline。
2. 比较：总 bytes、估算 tokens、全静态 cacheable prefix bytes、各 block kind、重复文本 hash，以及 reminder 从 system tokens 迁移到 message tokens 后的预算守恒。
3. 验证 Main/Sub 发送的 system block 序列完全符合契约：不出现重复 `execution_discipline`，也不存在 `task_reminder` 或其他动态 system block。
4. 验证存在未完成 Task 时，Provider 最后一条真实 user message 的 text 副本包含且仅包含一个 `<task-reminder>`；canonical session、持久化 JSON、SDK/TUI 消息仍为用户原文。
5. 执行代表性场景：
   - 只读探索；
   - 精确 Edit/Write；
   - 修改后运行验证；
   - allow/ask 权限模式；
   - EnterWorktree/ExitWorktree 后路径切换；
   - Main 派发 Sub Agent；
   - Skill、Memory、active summary、task reminder 同时存在；
   - 工具续轮、retry 与 compact 重建 invocation 时 reminder 不重复；
   - session resume、SDK JSON 与 TUI 回显不出现 `<task-reminder>`；
   - guidance / skill injection warning 不被精简逻辑绕过。
6. 将所有未完成或不适用的 Issue checklist 项记录在 Issue 或 PR 中，说明理由、影响和后续处理。

**验证命令**：

```bash
cargo test -p shared
cargo test -p context
cargo test -p runtime
cargo test -p tools
cargo test -p provider
cargo test -p composition
cargo check
cargo clippy --all-targets -- -D warnings
bash .agents/hooks/check-architecture-guards.sh
```

实际 crate 名称以 workspace `Cargo.toml` 为准；命令执行前先核对 package 名称，失败时按失败层级定向修复，不用重跑掩盖首次失败。

## 5. 测试策略与门禁

### L0

- production-only build / check；
- architecture guards；
- clippy；
- 检查被删除的拼接函数和常量没有生产引用；
- 检查测试专属 API 未泄漏。

### L1

- 静态 Prompt 语言 fallback；
- cwd/git/permission mode 替换；
- Prompt 分类和重复 hash；
- 预算/byte/token 计算辅助函数；
- 保留关键契约的语义断言，而不是锁定完整长字符串快照。

### L2

- `BaselinePromptSource`、`SkillPromptSource` 与 Context assembler 的静态 block kind、顺序、缓存断点；
- guidance / roles / skills / memory / summary 组合；
- Context 的 typed reminder decoration、消息副本装饰与 token budget 分类；
- empty、disabled、supplier failure 和 revision 变化。

### L3

- Runtime → Context Port → Provider 的静态 `RequestSystemBlock` 映射；
- Context invocation decoration → Provider user message text 的副本投影；
- Main/Sub 使用同一 block 与消息投影契约；
- canonical session → SDK/TUI/JSON 的原始用户消息字段完整性；
- 工具 metadata `phase` 剥离与业务 schema 兼容；
- Hook 看到的 system 文本与实际 invocation 一致。

### L4

- 主会话和子代理真实用户旅程；
- 权限拒绝、worktree 切换、文件修改和验证；
- Skill 安全扫描、Memory、active summary 与 task reminder 同时存在时，System blocks 仍全静态且 Provider 可见 reminder；
- TUI 回显、复制、session resume 与 JSON 导出只包含用户原文。

测试必须先建立失败/回归证据，再修改生产逻辑；不以弱化断言换取通过。新增测试按 owning layer 放置，避免跨层万能 fixture。

## 6. 风险与回滚

| 风险 | 控制措施 |
|---|---|
| 精简后模型不再遵循某个 Aemeath 特有协议 | 只删除已有 Tool/Runtime 强制或重复条款；L4 保留场景回归；必要时恢复单条契约，不恢复整段流程说明 |
| Runtime/Context 双重组装导致顺序变化 | 先锁定 kind/order/cache breakpoint 契约，再迁移 owner |
| Main/Sub 行为不一致 | 共享 Context materialization 与 L3 双端契约测试 |
| 用户 Guidance 与内置 Prompt 冲突 | 保留既有 revision、语言、来源和注入优先级；不改用户文件内容 |
| cache 前缀失效或成本回退 | System blocks 禁止动态 reminder；对 before/after 的静态 cacheable prefix 和唯一 breakpoint 做基线比较 |
| reminder 污染用户原文、TUI 或 JSON | 仅装饰 `to_llm_view()` 后的 Provider 副本；逐层断言 accepted input、canonical session、SDK event 和 TUI 文本不变 |
| 工具续轮或 retry 重复追加 reminder | 每次从 canonical 原文重建 invocation；单次投影最多一个 tag，并覆盖 continuation/retry 测试 |
| 无真实 user message 时伪造协议消息 | 不创建消息、不注入 system-generated/stop-hook message；等待下一条真实用户输入 |
| reminder 移出 system 后 token 低估 | token estimation 与 compaction decision 使用最终 Provider 可见消息，将 reminder 计入 message tokens |
| 删除旧路径后出现死代码/隐式依赖 | 每阶段运行 production check、架构守卫和定向 grep；完成后清理废弃路径 |

若 L4 发现关键行为退化，优先恢复缺失的最小产品契约；不得把完整执行手册重新塞回核心 Prompt。若连续三次单点修复仍无法稳定，暂停实现并重新评估 owner/Context 架构。

## 7. 交付顺序

1. Prompt 资产矩阵和 before baseline；
2. 基础 Prompt 唯一 owner 与重复注入修复；
3. 核心 Prompt 精简；
4. Task reminder 迁出 System Block 并建立 Provider-only user message decoration；
5. 动态组装 owner 收口和废弃路径清理；
6. L0–L4 验证、after baseline 和 Issue checklist 更新；
7. 在 worktree 分支上 `git pull origin main` 后更新 PR，等待用户 review；agent 不自行合并或关闭 Issue。
