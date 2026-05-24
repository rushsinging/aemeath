# Feature 47 Review — DDD 架构重设计 Spec & Phase 1-4 实施计划审查

> Reviewer: Aemeath (DeepSeek/deepseek-v4-pro)  
> Review Date: 2026-05-24  
> 审查范围:
> - `docs/feature/specs/047-ddd-redesign.md`（全局架构 spec）
> - `docs/superpowers/plans/2026-05-24-feature-47-*-phase*.md`（Phase 1-4 实施计划）

---

## 一、Spec 文档审查

### 整体评价

高质量架构设计。DDD 核心域判断准确（Agent Runtime），统一语言定义清晰，Bounded Context 与 Context Map 划分合理，COLA 分层定位对路。

### 发现的问题

#### 问题 1：第 6.2 节与 6.5 节职责描述矛盾（中等）

| 节 | 描述 |
|---|---|
| 6.2 "统一应用服务" | Application service 负责"把入口命令**编排**到 Agent Runtime、Tool Execution、Security / Policy、Audit 等上下文" |
| 6.5 设计约束 #4 | "ChatApplicationService 继续只负责**校验和分发**，不直接调用 repl、tui::App 或任何入口实现" |

"编排"(orchestrate) 暗示多上下文协调，"分发"(dispatch) 暗示薄路由转发，两种参与深度不同。按 COLA 规范，Application 层应做编排。建议统一为"编排"，或明确说明当前 Phase 限于分发、后续演进为编排。

#### 问题 2：第 6.5 节风格断裂（中等）

前 6.4 节是纯架构抽象描述（聚合、边界、原则），6.5 节突然引入具体类型名（`ChatApplicationService`、`ChatRuntimePort`、`ChatRuntimeContext`）并引用外部 Phase 1/2。作为全局架构 spec，建议：
- 要么把 Phase 1/2/3 上下文补入文档
- 要么把 6.5 节标注为"实施进展附注"并单独成节

#### 问题 3：ChatRuntimeContext 中未定义的概念（轻微）

`agent_semaphore`、`json_logger`、`system_blocks`、`context_size` 出现在 6.5 节但未在第 3 节（统一语言）或第 4 节（Bounded Context）中定义归属。`json_logger` 属 infrastructure 层，放在 Application 层边界对象中需在 COLA 约束中说明合理原因（"已初始化的 infrastructure 依赖可安全传入 context"）。

#### 问题 4：Agent Runtime 职责列表遗漏（轻微）

第 4.1 节列出了 Agent Runtime 调用的上下文（Model Gateway、Tool Execution、Memory、Skill / Guidance），但遗漏了 Configuration。第 5 节 Context Map 中 `Agent Runtime → Configuration` 这条边存在，应补入职责列表。

#### 问题 5：第 9 节聚合表缺少分层标注（轻微）

聚合表中 `SessionRecord` 列在 "Session History" 行下。第 4.3 节将 Session History 归为 ACL / Infrastructure。按 COLA 分层，持久化投影属于 infrastructure 而非 domain。spec 在聚合表中没有区分 domain aggregate 与 infrastructure model。建议在表头标注所属分层。

---

## 二、Phase 1-4 实施计划审查

### 整体评价

渐进式、行为保持型重构路径清楚，每步保持可验证。COLA adapter/application/infrastructure 边界落实得当。

### 发现的问题

#### 问题 6：Phase 1 — `available_permits()` 语义错误（高）

**位置**：Phase 1 — Task 2 Step 4，`run_no_tui` 体构造：

```rust
max_agent_concurrency: agent_semaphore.available_permits(),
```

`Semaphore::available_permits()` 返回**当前可用** permits，不是**最大**并发数。运行时其他 agent 正在工作时该值会变小，导致启动参数不稳定。应使用原始配置值 `config.max_agent_concurrency`。

> Phase 3 中已修正此问题（替换为 `config.max_tool_concurrency`），但 Phase 1/2 实施时需避免踩坑。

#### 问题 7：Phase 3 — TUI context 构造多余字段（中等）

**位置**：Phase 3 — Task 4 Step 5，TUI context 构造：

```rust
let context = ChatRuntimeContext {
    ...
    max_agent_concurrency,  // ChatRuntimeContext 中没有此字段
    agent_semaphore,
};
```

文档在 Step 5 末尾用文字指出需删除该行。建议在代码片段中直接用删除线或注释标记，避免实施者忽略。

#### 问题 8：Phase 4 — `_mcp_manager` 生命周期需验证（中等）

**位置**：Phase 4 — Task 3 Step 2。

若 MCP manager 搬入 `ChatBootstrap`，需确认 `setup::spawn_mcp_managers` 返回类型满足 `Send + 'static`（因为 Tokio 运行时迁移会要求这一点）。Phase 4 应补充编译期验证此约束。

#### 问题 9：Phase 4 — `ChatBootstrap` 包含 `args: Args` 冗余（轻微）

`Args` 包含大量 CLI-only 字段（如 `no_tui`、`tui`），这些字段在 `bootstrap_chat` 之后不应再被使用。同时 `ChatBootstrap` 持有 `mode_selection: ChatModeSelection`，意味着同一信息存了两份（`args.no_tui/args.tui` 与 `mode_selection`）。后续 Phase 应从 `ChatBootstrap` 中移除 `args` 字段，只保留需要透传的子集。

#### 问题 10：Phase 4 Task 7 — branch 命名不一致（轻微）

**位置**：Phase 4 — Task 7 Step 2/3。

引用的 branch 名为 `feature/47-ddd-redesign-plan`，但 Phase 1-3 使用的命名模式是 `feature/47-*-phaseN`。Phase 4 建议保持一致，如 `feature/47-chat-bootstrapping-phase4`。

---

## 三、全局一致性矩阵

| 检查项 | 状态 | 备注 |
|---|---|---|
| spec 内部矛盾（核心域定义 vs Bounded Context 划分） | ✅ 通过 | |
| spec 与 Phase 计划的一致性 | ✅ 通过 | spec 6.5 节与 Phase 3 目标对齐 |
| 各 Phase 之间 DTO 演进一致性 | ✅ 通过 | Phase 1→2 port 化，2→3 context/launch 化，3→4 bootstrap 化 |
| COLA 分层遵守情况 | ✅ 通过 | application 层不直接调用 adapter 实现 |
| 不重写 agent loop / 行为不变承诺 | ✅ 通过 | 每个 Phase 均明确声明 |
| `docs/feature/active.md` 登记 | ✅ 通过 | #47 已登记 |
| 测试覆盖 | ✅ 通过 | Phase 1-3 有单元测试，Phase 4 有检查点 |
| Stop hooks 验证 | ✅ 通过 | 每个 Phase 均覆盖 |

---

## 四、问题汇总

| 等级 | 数量 | 要点 |
|---|---|---|
| 🔴 高 | 1 | Phase 1 `available_permits()` 语义错误 |
| 🟡 中等 | 3 | 6.2 vs 6.5 职责矛盾、6.5 节风格断裂、Phase 3 多余字段清理方式、Phase 4 MCP 生命周期 |
| 🔵 轻微 | 5 | 统一语言遗漏、聚合表分层标注、Phase 4 `args` 冗余、branch 命名不一致 |

---

## 五、优先修正建议

1. **spec 6.2 vs 6.5**：统一 "编排" vs "分发" 语义，必要时区分当前状态与目标状态。
2. **Phase 1 实施时**：将 `available_permits()` 替换为 `config.max_agent_concurrency`，避免启动参数漂移。
3. **Phase 4 实施前**：确认 `_mcp_manager` 类型满足 `Send + 'static`。

其余问题不影响全局正确性，可在 Phase 推进过程中渐进消化。
