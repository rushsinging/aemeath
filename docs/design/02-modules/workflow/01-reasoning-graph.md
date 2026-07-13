# Workflow · ReasoningGraph 战术设计

> 层级：02-modules / workflow（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#792（S2）
> 本文定义 ReasoningGraph 的节点状态机、effort 调节、ReasoningPort OHS、provider clamp 统一策略，以及 Workflow 远期方向与暂缓条件。ReasoningGraph 是 Runtime 内部的 effort 调节模块，不是独立 BC。

## 1. 定位

ReasoningGraph 是 **Runtime 内部的 effort 调节模块**：

- 根据对话阶段（Explore / Plan / Execute）动态调节 reasoning effort
- 不独立成 BC——它是 Runtime 的内部策略，无独立聚合/不变量/生命周期
- 通过 `ReasoningPort` 读写 reasoning level，与 Provider BC 解耦

**不在本文范围**：Provider 侧的 `ReasoningLevel` 枚举定义和 per-driver wire format（见 [../provider/02-ports-stream-and-client-scope.md](../provider/02-ports-stream-and-client-scope.md)）、Config 侧的 `ReasoningGraphConfig` 静态阈值（见 [../config/01-config-layer.md](../config/01-config-layer.md)）。

## 2. ReasoningNode 状态机

### 2.1 节点定义

```rust
enum ReasoningNode {
    Idle,       // 初始态，无活跃任务
    Explore,    // 探索阶段：读文件、搜索、理解代码
    Plan,       // 规划阶段：制定方案、决策
    Execute,    // 执行阶段：编辑、运行命令
}
```

### 2.2 转移信号

```rust
enum ReasoningSignal {
    UserMessage { turn_count: usize, complex_intent: bool },
    ToolCompleted { declared_phase: Option<String>, is_error: bool, tool_name: String },
    TextOnly,           // assistant 纯文本回复（无 tool call）
    TurnBoundary,       // 轮次边界（不改变节点）
}
```

### 2.3 转移规则

| 当前节点 | 信号 | 目标节点 | 条件 |
|---|---|---|---|
| * | `UserMessage` | `Plan` | `turn_count ≤ 1 && complex_intent` |
| * | `UserMessage` | `Explore` | `turn_count ≤ 1`（非复杂意图） |
| * | `UserMessage` | `Plan` | `complex_intent`（非首轮） |
| * | `UserMessage` | `Explore` | 默认 |
| * | `ToolCompleted` | `Plan` | `is_error == true`（**强制，覆盖一切**） |
| * | `ToolCompleted` | `parse_declared_phase()` | `declared_phase` 有效（**ground truth**） |
| * | `ToolCompleted` | `infer_node_from_tool()` | `declared_phase` 缺失（**heuristic fallback**） |
| * | `TextOnly` | `Idle` | 总是 |
| * | `TurnBoundary` | 不变 | 总是 |

### 2.4 优先级链

```
is_error → Plan (强制覆盖)
  ↓ 否则
declared_phase (LLM 声明，ground truth)
  ↓ 缺失
classify heuristic (~15% 误判率)
```

> **doc-vs-code 分歧**：LLM 声明的 `phase` 是 ground truth——但 `is_error` 覆盖它（错误总需重新规划）。这是设计决策，不是 bug。declared phase 不直接控制 effort——effort 由节点决定。

## 3. effort 映射

### 3.1 节点 → effort

每个节点有 `default_effort`，可被 config 中的 `override_effort` 覆盖：

```rust
struct NodeConfig {
    default_effort: ReasoningLevel,
    override_effort: Option<ReasoningLevel>,
}

// 默认映射
Idle    → Off
Explore → Low
Plan    → High
Execute → Medium
```

### 3.2 effort 解析

```rust
fn current_effort(&self) -> ReasoningLevel {
    let node_config = self.config.nodes.get(&self.current_node);
    node_config
        .and_then(|c| c.override_effort)
        .or(node_config.map(|c| c.default_effort))
        .unwrap_or(ReasoningLevel::Medium)
}
```

### 3.3 ReasoningLevel 枚举

```rust
enum ReasoningLevel {
    Off,        // 不启用 reasoning
    Low,        // 低 effort（快速探索）
    Medium,     // 中等 effort（默认执行）
    High,       // 高 effort（复杂规划）
    Xhigh,      // 超高 effort（深度推理）
    Max,        // 最大 effort（极限场景）
}
```

- 实现 `Ord` / `PartialOrd` / `clamp`——支持 `min()` 比较
- per-provider 可能不支持全部级别（如 Ollama 只有 on/off）

## 4. ReasoningPort OHS

```rust
trait ReasoningPort: Send + Sync {
    /// 当前 reasoning level（有效值，已 clamp）
    fn current_level(&self) -> ReasoningLevel;
    /// 模型支持的最大 reasoning level（provider ceiling）
    fn max_level(&self) -> ReasoningLevel;
    /// 设置 reasoning level（内部 clamp 到 max_level 和 user_max）
    fn set_level(&self, level: ReasoningLevel);
    /// 模型是否支持 reasoning
    fn is_reasoning(&self) -> bool;
}
```

### 4.1 替代关系

| 现状 | 目标 |
|---|---|
| `client.current_reasoning_level()` | `ReasoningPort.current_level()` |
| `client.max_reasoning_level()` | `ReasoningPort.max_level()` |
| `client.set_reasoning_level(level)` | `ReasoningPort.set_level(level)` |
| `client.is_reasoning()` | `ReasoningPort.is_reasoning()` |
| `ProviderInfoPort` 无 reasoning accessor | `ProviderInfoPort` 补充 `max_reasoning_level()` |

### 4.2 NoOpReasoningPort

```rust
struct NoOpReasoningPort;

impl ReasoningPort for NoOpReasoningPort {
    fn current_level(&self) -> ReasoningLevel { ReasoningLevel::Off }
    fn max_level(&self) -> ReasoningLevel { ReasoningLevel::Off }
    fn set_level(&self, _: ReasoningLevel) {}
    fn is_reasoning(&self) -> bool { false }
}
```

## 5. clamp 策略统一

### 5.1 当前问题

clamp 发生在 3 个时机，无统一策略：

| 时机 | 位置 | 行为 |
|---|---|---|
| bootstrap | `provider_client.rs:80-82` | `desired.min(user_cap).min(provider.max)` |
| per-API-call | `loop_runner.rs:856` | `graph.effort().clamped_to(client.max)` |
| sub-agent setup | `runner/setup.rs:61` | save/restore `previous_reasoning_level` |

**问题**：
- `max_reasoning` config 字段已解析但**从未生效**（bootstrap 拿了 user_cap，per-call 没拿）
- declared vs effective drift：`current_reasoning_level()` 可能报告 `Xhigh`，但 driver 实际发送 `high`

### 5.2 目标：唯一 clamp 点

```rust
impl ReasoningPort for ReasoningPortImpl {
    fn set_level(&self, desired: ReasoningLevel) {
        let clamped = desired
            .min(self.user_max_reasoning)      // 用户配置上限（修 max_reasoning 未生效）
            .min(self.provider_max);            // provider 模型上限
        self.inner.set_reasoning_level(clamped);
    }

    fn current_level(&self) -> ReasoningLevel {
        // 返回已 clamp 的有效值，不是 declared 值
        self.inner.current_reasoning_level()
    }
}
```

- **唯一 clamp 点**：`ReasoningPort.set_level` 内部
- bootstrap / per-call / sub-agent 只调 `set_level`，不自行 clamp
- `current_level()` 返回已 clamp 的有效值，消除 declared vs effective drift

### 5.3 clamp 链

```
desired = graph.current_effort()             // 图决定期望值
  OR
desired = config.default_reasoning           // 无图时从 config 继承

clamped = desired
    .min(user_max_reasoning)                 // 用户配置上限
    .min(provider.max_reasoning_level())      // provider 模型上限

ReasoningPort.set_level(desired)             // set_level 内部 clamp
```

## 6. 无 graph 时继承父

### 6.1 Sub Run 行为

- Sub Run **不创建 ReasoningGraph 实例**
- 从父 Run 的 `ReasoningPort.current_level()` 获取初始值
- setup.rs 已有 save/restore `previous_reasoning_level`——保留

### 6.2 目标

```rust
// runner/setup.rs
fn setup_sub_agent(parent: &ReasoningPort) -> Box<dyn ReasoningPort> {
    let inherited_level = parent.current_level();
    // 子 agent 用继承的 level，无图调节
    let port = ReasoningPortImpl::new(...)
        .with_initial_level(inherited_level);
    port
}
```

- 子 agent 从 `ReasoningPort.current_level()` 继承，而非独立图推断
- 子 agent 的 `set_level` 仍受 clamp 保护

## 7. Loop 集成点

ReasoningGraph 在 loop_runner 中有 4 处集成点：

| 时机 | 信号 | 作用 |
|---|---|---|
| 用户消息进入 | `UserMessage` | transition → 可能改变节点 |
| tool 执行完成 | `ToolCompleted` | transition → 可能改变节点（用 declared_phase） |
| LLM 调用前 | — | `ReasoningPort.set_level(graph.current_effort())` |
| 轮次边界 | `TurnBoundary` | 不改变节点（占位，预留） |

### 7.1 目标集成

```rust
// 1. 用户消息进入
graph.transition(ReasoningSignal::UserMessage { turn_count, complex_intent });

// 2. tool 执行完成
graph.transition(ReasoningSignal::ToolCompleted {
    declared_phase: llm_declared_phase,
    is_error: tool_result.is_error,
    tool_name: tool_call.name,
});

// 3. LLM 调用前
if reasoning_port.is_reasoning() {
    reasoning_port.set_level(graph.current_effort());
}

// 4. 轮次边界
graph.transition(ReasoningSignal::TurnBoundary);
```

## 8. `/think` 命令

### 8.1 现状

`/think` 命令只支持 binary 切换（Medium / Off），不支持完整 6 级。

### 8.2 目标态

```rust
// /think              → 切换 Medium/Off（当前行为）
// /think high         → 设为 High
// /think max          → 设为 Max
// /think off          → 关闭 reasoning
```

- 支持完整 6 级：`off` / `low` / `medium` / `high` / `xhigh` / `max`
- 通过 `ReasoningPort.set_level()` 设置（受 clamp 保护）
- 无参数时保持 binary 切换行为（向后兼容）

### 8.3 暂缓实现

v0.1.0 只设计目标态，不实现。原因：
1. 当前 `/think` 在 `idle_lifecycle.rs` 中，改动涉及 idle 状态命令解析
2. 优先级低于 ReasoningPort 抽象和 clamp 统一
3. 放入后续 issue 实施

## 9. Workflow 远期方向

### 9.1 定位

Phase 3 Workflow Engine 是**远期规划**，与 ReasoningGraph 并存，不替代：

| 维度 | ReasoningGraph | Workflow Engine |
|---|---|---|
| 关注点 | effort 调节（每个节点的 reasoning level） | 控制流编排（步骤顺序、条件分支、循环） |
| 作用层 | 参数调节（影响 LLM 调用参数） | 流程编排（影响执行路径） |
| 状态 | 4 节点状态机（Idle/Explore/Plan/Execute） | DAG / 状态图（步骤节点 + 边） |
| 复杂度 | 低（当前已实现） | 高（远期） |

### 9.2 暂缓条件

Workflow Engine **暂缓实现**，原因：

1. **ReasoningGraph 已满足当前需求**——effort 调节是高频价值，控制流编排是低频需求
2. **控制流编排的复杂度远高于 effort 调节**——需要定义步骤 DSL、条件分支、循环、异常处理
3. **当前 Loop Engine 已是事实上的控制流**——用户消息 → LLM → tool → LLM 循环，Workflow Engine 需要在此基础上再抽象一层，收益不明确
4. **缺乏真实场景驱动**——没有足够的"Loop Engine 无法满足"的场景来验证 Workflow Engine 的设计

### 9.3 远期规划

当以下条件**全部满足**时，启动 Workflow Engine 设计：

- ReasoningGraph 稳定运行，effort 调节的边界清晰
- 出现 Loop Engine 无法表达的控制流需求（如多步骤条件编排、跨 turn 状态机）
- 有至少 2 个真实场景验证 Workflow Engine 的必要性

### 9.4 设计方向

如果启动，Workflow Engine 将作为 `business/workflow/` 独立模块：

- 与 ReasoningGraph 并存——ReasoningGraph 调 effort，Workflow Engine 调控制流
- Workflow Engine 不修改 ChatChain——它是读模型层变换（与 compact 管线的 L2-L4 同理）
- 通过 `WorkflowPort` trait 暴露给 Runtime

## 10. 现状缺口与迁移动作

| 目标 | 现状 | 迁移动作 |
|---|---|---|
| `ReasoningPort` trait | ❌ 无，runtime 直接调 client 方法 | 抽 trait，实现移到 adapter |
| `max_reasoning` 接入 clamp | ⚠️ config 已解析但未生效 | `ReasoningPort.set_level` 内部 clamp `user_max_reasoning` |
| clamp 策略统一 | ⚠️ 3 处分散 clamp | 收口到 `ReasoningPort.set_level` 唯一 clamp 点 |
| declared vs effective drift | ⚠️ `current_reasoning_level()` 可能报告未 clamp 值 | `current_level()` 返回已 clamp 值 |
| `ProviderInfoPort` 缺 accessor | ⚠️ 无 `max_reasoning_level()` | 补充 accessor |
| `/think` 完整 6 级 | ⚠️ 只支持 binary | 目标态设计完成，暂缓实现 |
| Sub-agent 无独立 graph | ⚠️ 继承父 level 但未通过 port | 通过 `ReasoningPort.current_level()` 继承 |
| Workflow Engine | ❌ 未实现 | 远期规划，暂缓条件见 §9.3 |
| `user_max_level()` 未使用 | ⚠️ `graph.user_max_level()` 定义但从未调用 | 接入 clamp 链或删除 |

## 11. 相关文档

- Runtime 端口（ReasoningPort = Runtime 出站端口）：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Provider 端口（ReasoningLevel 枚举 + provider clamp）：[../provider/02-ports-stream-and-client-scope.md](../provider/02-ports-stream-and-client-scope.md)
- Config 分层（ReasoningGraphConfig 静态阈值）：[../config/01-config-layer.md](../config/01-config-layer.md)
- Run 状态机（Loop 集成点）：[../runtime/03-loop-and-state-machine.md](../runtime/03-loop-and-state-machine.md)
- 上下文地图（Workflow = Runtime 内部模块）：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：节点状态机、effort 映射、ReasoningPort OHS、clamp 统一、Workflow 远期方向 | #792 |
