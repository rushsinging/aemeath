# Workflow · ReasoningGraph 战术设计

> 层级：02-modules / workflow（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#792（S2）/ [#972](https://github.com/rushsinging/aemeath/issues/972)
> 本文定义 Workflow 支撑域 BC 的 ReasoningGraph 状态机、effort 调节、ReasoningPort OHS 与 provider clamp 策略。**只描述目标态**；实现差距见 [迁移治理](../../03-engineering/03-migration-governance.md)。

## 1. 定位

ReasoningGraph 是 **独立 Workflow 支撑域 BC 的聚合 / 策略核心**：

- 根据对话阶段（Explore / Plan / Execute / Verify）动态调节 reasoning effort
- Workflow 独占节点迁移、desired effort 与**用户静态上限** clamp；Runtime 只负责在确定的 loop 时机发送信号
- 通过 Workflow-owned `ReasoningPort` OHS 读写 requested reasoning level，与 Provider 的 model-capability clamp 解耦

**版本边界**：v0.1.0 仅交付 Reasoning Graph / effort 调节；完整 Workflow Engine、DAG、恢复和持久化属于 v0.2.0。Workflow 始终是独立 BC，不因当前能力较窄而归入 Runtime。

**不在本文范围**：Shared Kernel 的 `ReasoningLevel` 稳定枚举定义、Provider 的 per-driver wire format（见 [../provider/02-ports-stream-and-client-scope.md](../provider/02-ports-stream-and-client-scope.md)）、Config 侧的 `ReasoningGraphConfig` 静态阈值（见 [../config/01-config-layer.md](../config/01-config-layer.md)）。

## 2. ReasoningNode 状态机

### 2.1 节点定义

```rust
enum ReasoningNode {
    Idle,       // 初始态，无活跃任务
    Explore,    // 探索阶段：读文件、搜索、理解代码
    Plan,       // 规划阶段：制定方案、决策
    Execute,    // 执行阶段：编辑、运行命令
    Verify,     // 验证阶段：测试、检查门禁、解读验证结果
}
```

### 2.2 转移信号

```rust
enum ReasoningSignal {
    UserMessage { text: String, turn_count: usize },
    ToolCompleted {
        declared_phase: Option<String>,
        is_error: bool,
        tool_name: String,
        bash_command: Option<String>,
    },
    TextOnly,           // assistant 纯文本回复（无 tool call）
    TurnBoundary,       // 轮次边界（不改变节点）
}
```

### 2.3 转移规则

| 当前节点 | 信号 | 目标节点 | 条件 |
|---|---|---|---|
| * | `UserMessage` | `Plan` | `turn_count ≤ 1 && text` 命中复杂意图 |
| * | `UserMessage` | `Explore` | `turn_count ≤ 1`（非复杂意图） |
| * | `UserMessage` | `Plan` | `text` 命中复杂意图（非首轮） |
| * | `UserMessage` | `Explore` | 默认 |
| * | `ToolCompleted` | `Plan` | `is_error == true`（**强制，覆盖一切**） |
| * | `ToolCompleted` | `parse_declared_phase()` | `declared_phase` 有效（**ground truth**） |
| * | `ToolCompleted` | `infer_node_from_tool()` | `declared_phase` 缺失（**heuristic fallback**；已知 test / check / lint 类 tool 映射为 `Verify`） |
| * | `TextOnly` | `Idle` | 总是 |
| * | `TurnBoundary` | 不变 | 总是 |

### 2.4 优先级链

```
is_error → Plan (强制覆盖)
  ↓ 否则
declared_phase（trim 后大小写不敏感；支持 explore/exploring 等四组形式）
  ↓ 缺失或无效
classify heuristic（tool_name + Bash command，约 15% 误判率）
```

> `UserMessage.text` 与 `ToolCompleted.bash_command` 是 Runtime 已观察到的事实，不是 Runtime 预计算策略；复杂意图与 fallback 分类均由 Workflow 拥有。`declared_phase` 是 ground truth，但 `is_error` 覆盖它（错误总需重新规划）；无效声明与缺失声明同样回退 heuristic。

Fallback 规则冻结如下：Read/Grep/Glob/LSP/ToolSearch → Explore；Edit/Write → Execute；Bash 按 command 中的 test/check/build/lint 或只读关键词映射 Verify/Explore，其余 Execute；Agent 保持当前节点；未知 tool 保守映射 Explore。

`ReasoningGraph::observe(signal)` **MUST** 在一次同步 mutation 中应用上表转移并返回新节点的 desired effort；graph 与 `current_effort` 都保持 Workflow-private，只有 `ReasoningPort` 实现可以调用。

```rust
impl ReasoningGraph {
    /// Workflow-private：应用 §2.3 转移表并返回新节点的 desired effort。
    /// Workflow-private；只有本模块内的 AdaptiveReasoningPort 可调用。
    /// §4 的公开 `ReasoningPort::observe` 额外返回纯投影 observation。
    fn observe(&mut self, signal: ReasoningSignal) -> ReasoningLevel;
}
```

## 3. effort 映射

### 3.1 节点 → effort

每个节点的默认 effort 由 Workflow 定义；除 Idle 外，可被 config 中的 `override_effort` 覆盖：

```rust
struct NodeConfig {
    override_effort: Option<ReasoningLevel>,
}

// 默认映射（Workflow 唯一拥有；Config 只可覆盖非 Idle 节点）
Idle    → Off
Explore → Medium
Plan    → Max
Execute → Off
Verify  → Medium
```

### 3.2 effort 解析

```rust
fn current_effort(&self) -> ReasoningLevel {
    self.config
        .override_effort(self.current_node)
        .unwrap_or_else(|| self.current_node.default_effort())
}
```

### 3.3 Shared Kernel ReasoningLevel

```rust
use share::ReasoningLevel; // Off | Low | Medium | High | Xhigh | Max
```

- 枚举由 Shared Kernel 唯一定义，Workflow / Config / Runtime / Context Management / Provider 直接消费，**NEVER** 各自复制同名类型
- 实现 `Ord` / `PartialOrd` / `clamp`——支持 `min()` 比较
- per-provider 可能不支持全部级别（如 Ollama 只有 on/off）

### 3.4 运行态 ReasoningGraph vs 配置态 ReasoningGraphConfig

| 维度 | ReasoningGraph（运行态） | ReasoningGraphConfig（配置态） |
|---|---|---|
| 归属 | Workflow BC 私有（仅 `AdaptiveReasoningPort` 持有，Runtime 不可见） | Config BC（见 [../config/01-config-layer.md](../config/01-config-layer.md)） |
| 生命周期 | 与 Main Run 绑定，崩溃从头开始 | 静态，由 `ConfigSnapshot` 在 Run 启动时一次性提供；resume 时可能随 Config prepare 刷新 |
| 内容 | 当前节点（`ReasoningNode`）+ 转移逻辑 + 私有 `observe()` 方法 | 静态开关、每节点 `override_effort`、`max_reasoning` |
| 可变性 | 可变（状态机随 `ReasoningSignal` 转移） | 不可变（Run 内不变） |
| 构造 | `ReasoningGraph::new(config: &ReasoningGraphConfig)` | 由 `ConfigSnapshot` 读取，Composition Root 在 `reasoning_for()` 中组装并注入 Workflow-owned port 实现 |

- `ReasoningGraphConfig` 是纯数据（开关、节点 override 与用户上限），不含行为。由 Config BC 定义，Workflow 直接消费。
- 节点默认 effort 与复杂意图/fallback 策略由 Workflow 唯一拥有；Config **NEVER** 复制默认映射。
- `ReasoningGraph` 持有配置快照，驱动状态转移和 `current_effort` 计算。
- 两者 **MUST NOT** 合并为一个类型——运行态包含可变状态（当前节点），配置态是静态快照。
- Idle 固定为 Off，不接受 Config override；其他节点缺失 override 时使用 Workflow 默认值。无效字符串必须由 Config 校验边界拒绝，迁移完成前的静默回退属于 Current 差距。

## 4. ReasoningPort OHS

```rust
trait ReasoningPort: Send + Sync {
    /// 输入 Runtime 已观察到的领域事实；Main 实现内部推进私有 graph。
    /// 返回 Workflow-owned observation，只供 Runtime 保持 phase event 纯投影；
    /// desired effort 与 graph 本体均不透出。
    fn observe(&self, signal: ReasoningSignal) -> ReasoningObservation;
    /// 当前 requested reasoning（已受 user maximum 限制，尚未按 model capability 裁剪）。
    fn current_requested_level(&self) -> ReasoningLevel;
    /// 设置本会话 thinking gate；Off 作为硬门，非 Off 恢复 graph 自适应，并返回 clamp 后值。
    fn set_level(&self, level: ReasoningLevel) -> ReasoningLevel;
    /// 模型切换时刷新默认 requested，不制造永久 override。
    fn reset_default_level(&self, level: ReasoningLevel) -> ReasoningLevel;
}

struct ReasoningObservation {
    previous: ReasoningNode,
    current: ReasoningNode,
    requested: ReasoningLevel,
}
```

### 4.1 Sub 实现延期

#920 只交付 Main adaptive 实现。Fixed / Inherit / NoOp 不提前进入生产 artifact，随 #875 model invocation 与 #878 shared Loop / RuntimeContext 切换按真实生产调用落地；#879 删除 legacy Sub/MainRunPort 入口。

## 5. clamp 策略统一

### 5.1 两个所有者、两个不同 clamp

Workflow 与 Provider 处理的是不同约束，**NEVER** 在 `ReasoningPort` 中读取 model capability：

1. Workflow 把 graph / explicit override 的 desired value clamp 到 Config user maximum，发布 `requested reasoning`；
2. Provider-owned option resolver 再把 requested value clamp 到目标 model 的 supported levels，发布 `effective reasoning`；
3. Runtime 在 `build_window` 前取得一次 resolved options，把其中同一个 effective value 同时放入 `ContextRequest` 与 `InvocationRequest`。Provider `invoke` 只校验 resolved capability fingerprint，**NEVER** 静默生成第三个值。

```rust
impl AdaptiveReasoningPort {
    fn apply_desired(&self, desired: ReasoningLevel) {
        let requested = desired.min(self.user_max_reasoning);
        *self.requested.write().unwrap() = requested;
    }
}

impl ReasoningPort for AdaptiveReasoningPort {
    fn observe(&self, signal: ReasoningSignal) -> ReasoningObservation {
        let mut state = self.state.lock().unwrap();
        let previous = state.graph.current_node();
        if state.graph.enabled() {
            state.graph.transition(signal);
            if state.manual_override.is_none() {
                state.requested = state.graph.current_effort().min(self.user_max_reasoning);
            }
        }
        ReasoningObservation {
            previous,
            current: state.graph.current_node(),
            requested: state.requested,
        }
    }

    fn set_level(&self, desired: ReasoningLevel) -> ReasoningLevel {
        let mut state = self.state.lock().unwrap();
        state.manual_override = Some(desired);
        state.requested = desired.min(self.user_max_reasoning);
        state.requested
    }

    fn current_requested_level(&self) -> ReasoningLevel {
        *self.requested.read().unwrap()
    }
}
```

- Workflow 内 graph observation、`/think` gate 与模型默认刷新共用 user maximum clamp，因此上限只有一个实现点
- Runtime **NEVER** 取得 `ReasoningGraph`；只经 `observe` 输入事实、经 `current_requested_level` 读取 requested value
- `set_level(Off)` 表达本会话硬关闭；设置为非 Off 时恢复 graph 自适应。模型切换经 `reset_default_level` 刷新默认值，避免把旧模型 requested 或永久 override 带入新模型
- Workflow 只保存 requested 领域值，**NEVER** 保存 provider ceiling、capability snapshot 或 mutate Provider client
- Provider option resolver 是 model-capability clamp 的唯一所有者；它返回的 effective value 才能进入 Prompt 与最终 Invocation Scope

### 5.2 clamp 链

```
desired = graph.current_effort()             // 图决定期望值
  OR
desired = config.default_reasoning           // 无图时从 config 继承

requested = desired.min(user_max_reasoning)  // Workflow-owned

resolved = provider.resolve_invocation_options(
    model,
    RequestedInvocationOptions { reasoning: requested, .. },
)                                            // Provider-owned model clamp

effective = resolved.effective_reasoning     // Runtime 冻结；Context / Invocation 共用
```

## 6. 无 graph 时继承父

### 6.1 Sub Run 行为

- Sub Run **不创建 ReasoningGraph 实例**；使用 fixed-effort 实现，`observe` 为 no-op
- 从父 Run 的 `ReasoningPort.current_requested_level()` 获取初始值
- 子 Run 获得独立 `ReasoningPort` instance；**NEVER** 临时 mutate / restore 父 Run 或共享 provider client

### 6.2 装配

```rust
// runner/setup.rs
fn setup_sub_agent(parent: &ReasoningPort) -> Box<dyn ReasoningPort> {
    let inherited_requested = parent.current_requested_level();
    // 子 agent 用继承的 requested level，无图调节；仍受子 ConfigSnapshot 的 user maximum 限制。
    Box::new(FixedReasoningPort::new(inherited_requested, ...))
}
```

- 子 agent 从 `ReasoningPort.current_requested_level()` 继承，而非独立图推断
- 子 agent 的 `set_level` 仍受 clamp 保护

## 7. Loop 集成点

Runtime 只经 `ReasoningPort` OHS 集成；`ReasoningGraph` 是 Main 实现的私有状态：

| 时机 | 信号 | 作用 |
|---|---|---|
| 用户消息进入 | `observe(UserMessage)` | Main 实现内部 transition → 可能改变节点 |
| tool 执行完成 | `observe(ToolCompleted)` | Main 实现内部 transition（用 declared_phase） |
| LLM 调用准备 | `current_requested_level()` | 取得 Workflow requested value；Provider resolver 随后产生 effective value |
| 轮次边界 | `observe(TurnBoundary)` | 不改变节点（占位，预留） |

### 7.1 集成

```rust
// 1. 用户消息进入
reasoning_port.observe(ReasoningSignal::UserMessage { text, turn_count });

// 2. tool 执行完成
reasoning_port.observe(ReasoningSignal::ToolCompleted {
    declared_phase: llm_declared_phase,
    is_error: tool_result.is_error,
    tool_name: tool_call.name,
    bash_command: observed_bash_command,
});

// 3. LLM 调用准备；Main adaptive / Sub fixed / unsupported NoOp 使用同一读取面
let requested = reasoning_port.current_requested_level();
let resolved = provider.resolve_invocation_options(model, requested.into())?;
let effective = resolved.effective_reasoning; // build_window 前冻结

// 4. 轮次边界
reasoning_port.observe(ReasoningSignal::TurnBoundary);
```

## 8. `/think` 命令

### 8.1 命令语义

```rust
// /think              → 切换 Medium/Off
// /think high         → 设为 High
// /think max          → 设为 Max
// /think off          → 关闭 reasoning
```

- 支持完整 6 级：`off` / `low` / `medium` / `high` / `xhigh` / `max`
- 通过 `ReasoningPort.set_level()` 设置（受 clamp 保护）
- 无参数时采用 Medium / Off binary toggle；显式参数总是优先
- `set_level` 与命令确认反馈（CLI/TUI 回显）只暴露 `current_requested_level()`——即 §5.1 clamp 链中的 `requested` 值；**NEVER** 展示 provider `resolve_invocation_options` 产生的 model `effective` 值。同一 Run 尚未触发下一次 LLM 调用前不存在新的 effective reasoning，命令层没有能力提前得知它。

## 9. Workflow 远期方向

### 9.1 定位

Phase 3 Workflow Engine 是**远期规划**，与 ReasoningGraph 并存，不替代：

| 维度 | ReasoningGraph | Workflow Engine |
|---|---|---|
| 关注点 | effort 调节（每个节点的 reasoning level） | 控制流编排（步骤顺序、条件分支、循环） |
| 作用层 | 参数调节（影响 LLM 调用参数） | 流程编排（影响执行路径） |
| 状态 | 5 节点状态机（Idle/Explore/Plan/Execute/Verify） | DAG / 状态图（步骤节点 + 边） |
| 复杂度 | 低 | 高（Future） |

### 9.2 暂缓条件

Workflow Engine **暂缓实现**，原因：

1. **ReasoningGraph 覆盖 v0.1.0 需求**——effort 调节是高频价值，控制流编排是低频需求
2. **控制流编排的复杂度远高于 effort 调节**——需要定义步骤 DSL、条件分支、循环、异常处理
3. **Loop Engine 已承担执行控制流**——用户消息 → LLM → tool → LLM 循环，Workflow Engine 需要在此基础上再抽象一层，收益尚无证据
4. **缺乏真实场景驱动**——没有足够的"Loop Engine 无法满足"的场景来验证 Workflow Engine 的设计

### 9.3 远期规划

当以下条件**全部满足**时，启动 Workflow Engine 设计：

- ReasoningGraph 稳定运行，effort 调节的边界清晰
- 出现 Loop Engine 无法表达的控制流需求（如多步骤条件编排、跨 turn 状态机）
- 有至少 2 个真实场景验证 Workflow Engine 的必要性

### 9.4 设计方向

如果启动，Workflow Engine 将作为 Workflow BC 内由独立 leaf issue 证明的新 capability：

- 与 ReasoningGraph 并存——ReasoningGraph 调 effort，Workflow Engine 调控制流
- Workflow Engine 不修改 ChatChain——它是读模型层变换（与 compact 管线的 L2-L4 同理）
- 通过 `WorkflowPort` trait 暴露给 Runtime

## 10. 相关文档

- Runtime 装配（消费 Workflow-owned ReasoningPort）：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Provider 端口（消费 Shared Kernel ReasoningLevel + provider clamp）：[../provider/02-ports-stream-and-client-scope.md](../provider/02-ports-stream-and-client-scope.md)
- Config 分层（ReasoningGraphConfig 静态阈值）：[../config/01-config-layer.md](../config/01-config-layer.md)
- Run 状态机（Loop 集成点）：[../runtime/03-loop-and-state-machine.md](../runtime/03-loop-and-state-machine.md)
- 上下文地图（Workflow = 支撑域 BC）：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- Current → Target 迁移责任：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-17 | #919 冻结当前生产语义：节点默认 effort、事实型 ReasoningSignal、declared phase/fallback 优先级与 Config-only override | [#919](https://github.com/rushsinging/aemeath/issues/919) |
| 2026-07-12 | 初稿：节点状态机、effort 映射、ReasoningPort OHS、clamp 统一、Workflow 远期方向 | #792 |
| 2026-07-14 | 对齐 Context Map：Workflow 作为支撑域 BC 独占 ReasoningPort 与 clamp 不变量；将 Verify 收入五节点统一语言与状态机 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | 拆分 Workflow user-maximum clamp 与 Provider model-capability clamp；Runtime 在 Context build 前冻结唯一 effective value | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-17 | #920 建立 Main adaptive ReasoningPort：graph 私有化、user-max clamp、manual override、observation 纯投影；Sub Fixed/Inherit/NoOp 延期 #875/#878 | [#920](https://github.com/rushsinging/aemeath/issues/920) |
| 2026-07-14 | 明确私有 `ReasoningGraph` 只计算 desired effort，公开 `ReasoningPort::observe` 返回纯投影 observation；`/think` 命令反馈只暴露 requested 值，NEVER 暴露 model effective 值 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | 新增 §3.4 运行态 ReasoningGraph vs 配置态 ReasoningGraphConfig 区分；两者 MUST NOT 合并为一个类型 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
