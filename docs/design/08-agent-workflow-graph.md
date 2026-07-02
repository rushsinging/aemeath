# Agent Workflow Graph

> 本文档是 **架构设计** 类设计文档：设计 LLM 驱动的显式工作流引擎，通过 `WorkflowPlan` tool 建图、`WorkflowNext` tool 推进游标、线性序列执行、建议性守卫，实现 effort 精确联动与流程控制。
>
> 本文档 **MUST** 作为实现依据，**SHOULD** 配套 umbrella issue + 子 issue 使用。
>
> 关联文档：
> - `docs/design/05-agent-orchestration.md`（Agent 编排范式知识地图，本文是其 Workflow / Graph 维度的具体落地）
> - `docs/design/03-runtime-design.md`（Runtime 核心域，本文涉及 agent loop 控制流改动）

## 1. 动机

### 1.1 问题

aemeath 当前的 Reasoning Graph（设计文档 `06-agent-reasoning-graph.md`）是**纯观察式**状态机，存在三个结构性缺陷：

**缺陷 1：effort 滞后一轮**

runtime 观察 tool 类型/结果 → 推断阶段 → 下一轮调 effort。effort 是请求参数（调 LLM **之前**设），阶段推断在响应里（LLM **之后**才有）。LLM 需要深度推理的那一轮本身，并没有得到深度推理。

**缺陷 2：Execute = Off 过于激进**

Execute 节点默认 effort = Off。Edit/Write 并非纯机械执行——LLM 需要精确匹配 `old_string`、理解上下文位置、处理边界情况。完全关闭 thinking 会导致低级错误。

**缺陷 3：无流程控制**

graph 纯观察，不阻塞 tool、不强制流程。LLM 可以跳过 Explore 直接 Edit、改完不 Verify。

### 1.2 解决方案

用**LLM 显式规划 + 显式推进**的 workflow 引擎**替换**观察式 graph：

```
LLM 调用 WorkflowPlan tool 声明工作步骤序列
  → runtime 持久化图到 session
  → 按图执行：建议性准入 → 设 effort → 调 LLM → LLM 调 WorkflowNext 推进游标
  → 需要调整时再次调用 WorkflowPlan（replan）
```

核心设计：

| 维度 | 旧观察式 Graph | Workflow Graph（本文） |
|---|---|---|
| 谁控制流程 | runtime 隐式推断 | LLM 显式规划 + 显式推进 |
| effort 时机 | 滞后一轮 | 精确——节点已知，effort 提前设好 |
| 流程推进 | runtime 推断阶段变化 | LLM 调用 `WorkflowNext` tool |
| LLM 感知 | 无 | 有——LLM 主动建图/推进，tool_result 闭环反馈 |
| 灵活性 | 高（随时跳转） | 高（可 replan） |
| 持久化 | 无 | 有（session 级持久化，resume 恢复） |

### 1.3 与旧 Graph 的关系

**Workflow Graph 替换旧的观察式 Reasoning Graph，不是叠加。**

- `classify.rs`（阶段推断器）**删除**——不再有推断式阶段切换
- `phase` 字段声明机制**删除**——不再要求 LLM 在 tool call input 里声明 phase
- `reasoning_graph/config.rs` 的 effort 配置**沿用**——节点 effort 映射保留，结构不变
- LLM 不调 `WorkflowPlan` 时，没有图、没有 effort 调节，使用 provider 默认 effort

## 2. 设计

### 2.1 核心原则

> **Workflow Graph 是 LLM 驱动的工作流引擎，cursor 推进完全由 LLM 显式控制。**

graph 做四件事：
1. **承载 LLM 规划的工作步骤序列**（线性，允许重复和循环）
2. **effort 联动**——节点已知，effort 提前设置，无滞后
3. **建议性准入守卫**——进入节点时检查前置条件，不满足时注入提示但不阻塞
4. **建议性准出门槛**——`WorkflowNext` 调用时检查最低门槛，不满足时注入提示但仍然生效

graph **不阻塞** tool 执行，**不推断**阶段切换。所有 cursor 推进由 LLM 调用 `WorkflowNext` 显式触发。

### 2.2 节点定义

#### 控制节点

| 节点 | 默认 effort | 用途 |
|---|---|---|
| `start` | Medium | 图入口：runtime 自动注入到 steps[0]，cursor 初始指向 start，LLM 在此节点调用 `WorkflowPlan` 建图后推进 |
| `deliver` | Medium | 终止节点：cursor 进入 deliver = 图生命周期结束，回到 Idle |

**`start` 节点**：

- runtime 自动注入到 `steps[0]`，LLM 不声明此节点
- cursor 初始指向 start，等待 LLM 调用 `WorkflowPlan` 建图
- LLM 建图后调 `WorkflowNext` 推进到第一个工作节点

**`deliver` 节点**：

- LLM 在 `WorkflowPlan` 的 steps 中显式声明，表示"到此应该交付"
- cursor 进入 deliver 时图立即终止，回到 Idle
- 适用场景：
  - 简单问题：`[explore, deliver]`——探索完直接交付
  - 需要用户确认：`[explore, plan, deliver]`——方案出来后交付给用户确认
  - 遇到阻塞：`[explore, deliver]`——发现问题需要用户输入
- 如果 LLM 未在 steps 末尾声明 deliver，cursor 走完最后一个工作节点 + LLM 调 `WorkflowNext` 后自动回 Idle（隐式交付）

#### 工作节点

| 节点 | 默认 effort | 用途 |
|---|---|---|
| `explore` | Medium | 只读 tool（Read/Grep/Glob/只读 Bash），收集信息，理解现状 |
| `plan` | Max | 深度推理，定方案，根因分析，可调只读 tool 辅助 |
| `execute` | **Low** | 写入类 tool（Edit/Write/Bash 写入），执行改动 |
| `verify` | Medium | 构建/测试/lint（cargo test/clippy/build/tsc），验证结果 |

> **Execute 从 Off → Low**：保留基础推理能力。Edit/Write 需要精确匹配上下文，完全关闭 thinking 会导致低级错误。Low 比 Medium 省 token，但保留基础推理。

每个节点的 effort 可通过 `aemeath.json` 的 `reasoning_graph.nodes.<node>.effort` 覆盖。

#### DAG 控制节点（待做，本文档不实现）

| 节点 | 用途 | 状态 |
|---|---|---|
| `branch` | 条件分支：评估后决定走哪条路径 | 待做 |
| `switch` | 多路分发：并行推进独立子任务 | 待做 |
| `join` | 汇合点：等待多个并行分支完成 | 待做 |

首期只实现线性序列。branch / switch / join 列为后续演进方向（§6.2）。

#### 不需要的节点

- **replan**：replan 不是节点，是"重新规划"的操作。LLM 再次调用 `WorkflowPlan` 即触发。
- **debug**：本质是 explore + plan 的组合，不需要独立。

### 2.3 数据结构

```rust
/// 工作流步骤（LLM 在 WorkflowPlan tool 中声明的单个节点）。
///
/// 注意：`start` 节点由 runtime 自动注入（steps[0]），LLM 不声明此节点。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    /// 节点类型：start（runtime 注入）/ explore / plan / execute / verify / deliver
    pub node: ReasoningNode,
    /// 此节点要完成的目标（一句话，由 LLM 声明）
    pub goal: Option<String>,
}

/// 节点执行历史记录。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeRecord {
    /// 该节点内执行过的 tool 名称列表
    pub tools_executed: Vec<String>,
    /// 是否发生过 error
    pub has_error: bool,
    /// 进入时的 turn count
    pub entered_at: usize,
    /// 完成时的 turn count
    pub completed_at: Option<usize>,
}

/// LLM 驱动的工作流图（线性序列）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowGraph {
    /// 按执行顺序排列的步骤序列（steps[0] 始终是 runtime 注入的 start）
    steps: Vec<WorkflowStep>,
    /// 当前执行游标（指向 steps 中的索引）
    cursor: usize,
    /// 各步骤是否已完成
    completed: Vec<bool>,
    /// 每个步骤的执行记录
    node_records: Vec<NodeRecord>,
    /// replan 次数
    replan_count: usize,
    /// 原始工作流（首次规划，用于审计）
    original_steps: Vec<WorkflowStep>,
}
```

**设计决策**：

- **线性 `Vec<WorkflowStep>`**：序列允许重复和循环（`[explore, plan, execute, verify, plan, execute, verify]`）。
- **`goal: Option<String>`**：每个节点可带目标描述，作为上下文注入帮助 LLM 聚焦。
- **`cursor: usize`**：指向 `steps` 中的当前位置。**只有 `WorkflowNext` tool 能修改 cursor**。
- **`original_steps`**：保留首次规划用于审计。replan 只替换 `steps`，`original_steps` 不变。

### 2.4 Workflow Tool 系列

两个 workflow tool，均以 `Workflow` 前缀命名：

| Tool | 职责 | 频率 |
|---|---|---|
| `WorkflowPlan` | 建图 / replan（声明步骤序列） | 每 session 1-3 次 |
| `WorkflowNext` | 推进游标（当前节点完成，进入下一个） | 每节点 1 次 |

两者各司其职：`WorkflowPlan` 管**结构**，`WorkflowNext` 管**推进**。

#### 2.4.1 WorkflowPlan

##### Schema

```json
{
  "name": "WorkflowPlan",
  "description": "规划工作流图。收到任务后首先调用此工具声明工作步骤。发现需要调整时再次调用以 replan。",
  "input_schema": {
    "type": "object",
    "properties": {
      "steps": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "node": {
              "type": "string",
              "enum": ["explore", "plan", "execute", "verify", "deliver"],
              "description": "节点类型（start 节点由 runtime 自动注入，不要声明）"
            },
            "goal": {
              "type": "string",
              "description": "此节点要完成的目标（一句话）"
            }
          },
          "required": ["node"]
        },
        "min_items": 1,
        "description": "按执行顺序排列的节点序列"
      }
    },
    "required": ["steps"]
  }
}
```

##### Tool 行为

无论首次建图还是 replan，执行逻辑统一：

```rust
fn execute(&self, input: Value, ctx: &ToolExecutionContext) -> ToolResult {
    let user_steps: Vec<WorkflowStep> = parse_and_validate(input)?;

    // runtime 自动在 steps[0] 注入 start 节点
    let mut steps = vec![WorkflowStep {
        node: ReasoningNode::Start,
        goal: None,
    }];
    steps.extend(user_steps);

    if let Some(graph) = ctx.session.workflow_mut() {
        // replan
        graph.replan(steps);
        ToolResult::success(json!({
            "replanned": true,
            "replan_count": graph.replan_count,
            "new_steps": graph.steps_summary(),
            "current_node": graph.current_node(),
        }))
    } else {
        // 首次建图
        let graph = WorkflowGraph::new(steps);
        ctx.session.set_workflow(graph);
        ToolResult::success(json!({
            "registered": true,
            "steps": graph.steps_summary(),
            "current_node": graph.current_node(),
        }))
    }
}
```

##### Replan 语义

replan **不保留** completed 状态——新图的步骤是全新的路径。runtime 自动注入 `start` 到新 `steps[0]`，cursor 重置为 0。LLM 调 `WorkflowNext` 即可推进到第一个工作节点。

replan 只递增 `replan_count`，保留 `original_steps` 不变（审计用途）。

##### Tool 属性

```rust
impl TypedTool for WorkflowPlanTool {
    fn name(&self) -> &str { "WorkflowPlan" }
    fn is_read_only(&self) -> bool { true }        // 不修改文件系统
    fn is_concurrency_safe(&self) -> bool { true }
    fn timeout_secs(&self) -> u64 { 5 }
    fn is_input_safe(&self) -> bool { true }        // 无需用户确认
}
```

#### 2.4.2 WorkflowNext

##### Schema

```json
{
  "name": "WorkflowNext",
  "description": "推进工作流游标。当前节点的目标已达成时调用此工具，cursor 推进到下一个节点。",
  "input_schema": {
    "type": "object",
    "properties": {
      "summary": {
        "type": "string",
        "description": "当前节点完成摘要（一句话，说明达成了什么）"
      }
    },
    "required": ["summary"]
  }
}
```

##### Tool 行为

```rust
fn execute(&self, input: Value, ctx: &ToolExecutionContext) -> ToolResult {
    let summary: String = parse_summary(input)?;

    let Some(graph) = ctx.session.workflow_mut() else {
        return ToolResult::error("无活动的工作流图，请先调用 WorkflowPlan");
    };

    // 检查准出门槛（建议性，不阻塞）
    let gate_warning = check_exit_gate(graph);

    // 标记当前节点完成，推进 cursor
    let finished_node = graph.current_node().clone();
    graph.advance_current(summary);

    // 检查是否进入 deliver 或走完所有步骤
    if graph.current_node() == ReasoningNode::Deliver || graph.cursor >= graph.steps.len() {
        let next_node = graph.current_node();
        ctx.session.clear_workflow();
        return ToolResult::success(json!({
            "completed": true,
            "finished_node": finished_node,
            "next_node": next_node,
            "message": "工作流已完成，图生命周期结束",
        }));
    }

    ToolResult::success(json!({
        "advanced": true,
        "finished_node": finished_node,
        "current_node": graph.current_node(),
        "current_goal": graph.current_goal(),
        "gate_warning": gate_warning,  // Option<String>，有值时为建议性警告
    }))
}
```

##### Tool 属性

```rust
impl TypedTool for WorkflowNextTool {
    fn name(&self) -> &str { "WorkflowNext" }
    fn is_read_only(&self) -> bool { true }
    fn is_concurrency_safe(&self) -> bool { true }
    fn timeout_secs(&self) -> u64 { 5 }
    fn is_input_safe(&self) -> bool { true }
}
```

### 2.5 执行循环

```
Turn 开始:
  ① 检查 session.workflow
     ├─ 有图 → 按 cursor 节点的 effort 设置 + 建议性准入检查
     │   └─ cursor 指向 start → 等待 LLM 调用 WorkflowPlan 建图
     └─ 无图 → 使用 provider 默认 effort（无阶段调节）

  ② 调 LLM（effort 已按 cursor 节点设置好）

  ③ LLM 返回响应，执行 tool calls
     ├─ 含 WorkflowPlan tool call → 执行（注册/replan 图）
     ├─ 含 WorkflowNext tool call → 执行（推进 cursor）
     │   ├─ 推进到 deliver 或走完所有步骤 → 图终止，session.workflow = None
     │   └─ 推进到其他节点 → cursor++，下一轮使用新节点的 effort
     ├─ 含其他 tool call → 执行 → 记录到当前 cursor 节点的 NodeRecord
     └─ 纯文本回复（无 tool call）→ 图状态不变，cursor 不动

  ④ 回到 ①（下一轮）
```

**关键**：cursor 推进**只**由 `WorkflowNext` 触发。runtime 不推断阶段切换，不因纯文本回复推进 cursor。

#### Workflow tool 优先执行

同一轮内如果 LLM 同时调用 `WorkflowPlan` + 其他 tool，runtime 先执行 `WorkflowPlan`（注册图），再执行其他 tool。

实现方式：在 `execute_tool_round` 中对 tool calls 按 name 排序，`Workflow` 前缀的 tool 排在最前。

```rust
// execute_tool_round 中
let mut sorted_calls = tool_calls.clone();
sorted_calls.sort_by_key(|tc| {
    if tc.name.starts_with("Workflow") { 0 } else { 1 }
});
```

#### 图生命周期

```
Idle → Start → Explore → Plan → Execute → Verify → Deliver → Idle
       ↑                ↑                       ↑
       (cursor 初始)    (replan 可插入)         (出现即终止)
                                                或不声明 deliver，走完末节点即终止
```

- **图的创建**：LLM 首次调用 `WorkflowPlan`
- **图的推进**：LLM 调用 `WorkflowNext`，cursor++ 到下一个节点
- **图的终止**：cursor 进入 `deliver` 节点，或 cursor 走完最后一个工作节点（未声明 deliver）
- **图的销毁**：session 结束。图持久化到 session，resume 时恢复。

### 2.6 建议性准入守卫

#### 设计理念

准入守卫是**建议性的**，不是阻塞性的。LLM 调 `WorkflowNext` 推进到新节点后，runtime 检查该节点的准入条件：

- **满足** → 正常执行
- **不满足** → 在 tool_result 中附加 `gate_warning` 提示 + 提升 effort 到 Plan(Max)，但 **不回退 cursor**

准入检查的是**图中前序步骤**的 completed 状态。因为图是 LLM 规划的，可能第一条就是 `plan`（用户消息已足够明确时）——此时 plan 前面没有 explore 步骤，准入无门槛。

#### 准入条件

| 目标节点 | 检查条件（顺序推进时） | 检查条件（跳转时） |
|---|---|---|
| `start` | 无（图的入口） | — |
| `explore` | 无 | 无 |
| `plan` | 无 | 无（plan 可以是图的第一步） |
| `execute` | 无 | 前序有 `plan` 或 `explore` 已 completed |
| `verify` | 无 | 前序有 `execute` 已 completed |
| `deliver` | 无 | 无（随时可以交付） |

**注意**：正常顺序推进（cursor → cursor+1）时不检查准入——线性图本身就是 LLM 规划的顺序，自然推进不需要准入。准入只在**跳转**（LLM 调 `WorkflowNext` 但意图跳到非相邻节点）时有意义，但由于 `WorkflowNext` 只推进到 cursor+1，跳转实际通过 replan 实现。

> **结论**：在当前的线性 + `WorkflowNext` 逐节点推进的设计下，准入守卫实际上不会触发。准入条件表保留作为语义文档和未来 DAG 扩展的基础，但首期实现中所有 `WorkflowNext` 调用都自然满足准入。

#### 准入不满足时的行为（未来扩展保留）

runtime **不阻塞** tool 执行。但做两件事：
1. 在 `WorkflowNext` 的 tool_result 中附加 `gate_warning`
2. 提升 effort 到 Plan(Max)——强制 LLM 在更高推理深度下重新评估

### 2.7 建议性准出门槛

#### 设计理念

准出门槛也是**建议性的**。LLM 调 `WorkflowNext` 时，runtime 检查当前节点的最低完成门槛：

- **满足** → 正常推进
- **不满足** → 在 tool_result 中附加 `gate_warning` 提示，但 **`WorkflowNext` 仍然生效**（cursor 仍然推进）

这保证了 LLM 对流程有完全的控制权，runtime 只提供反馈信号。

#### 准出门槛

| 节点 | 最低门槛（该节点内的客观信号） | 不满足时的 warning |
|---|---|---|
| `start` | `WorkflowPlan` 已执行（steps.len() > 1） | "尚未规划工作步骤" |
| `explore` | 该节点内 ≥1 次只读 tool 执行 | "尚未执行任何探索操作" |
| `plan` | 无强制门槛 | — |
| `execute` | 该节点内所有写入 tool 无 error | "存在执行错误未修复" |
| `verify` | 验证 tool（cargo/test/lint）已执行 | "尚未执行验证命令" |
| `deliver` | 无（终止节点，不检查） | — |

**门槛作用域是"该节点内"**——基于 `NodeRecord` 中当前节点的 `tools_executed` 和 `has_error`，不是 session 级别的。这解决了多轮 execute / 循环 explore 的作用域问题。

### 2.8 Replan 机制

#### 触发方式

| 触发方式 | 机制 | 说明 |
|---|---|---|
| **LLM 主动 replan** | 再次调用 `WorkflowPlan` tool | LLM 输出新的步骤序列，runtime 替换图 |
| **Verify 失败 → 建议回退** | runtime 检测 verify 节点 tool error | `WorkflowNext` 的 gate_warning 提示"验证失败，建议 replan 回到 plan" |
| **连续 error ≥ 2** | runtime 计数器 | 注入提示"当前路径可能不正确，建议 replan" |

**LLM 主动 replan** 是唯一能实际改变图结构的机制。后两种是 runtime 侧的建议性干预——不强制 replan，只通过 gate_warning 让 LLM 自己决定。

#### replan 示例

```
原始图: [start, explore, plan, execute, verify]
                        ↑ cursor=3, 在 execute 节点

LLM 发现方案不对，调用 WorkflowPlan:
  steps: [
    { node: "plan", goal: "重新评估 auth 模块的 token 刷新策略" },
    { node: "execute", goal: "实现新的 refresh 逻辑" },
    { node: "verify", goal: "运行 auth 测试" }
  ]

replan 后（runtime 自动注入 start）:
  steps: [start, plan, execute, verify]
  cursor: 0  ← 重置到 start
  replan_count: 1
  original_steps: [start, explore, plan, execute, verify]  ← 保留
```

### 2.9 Effort 联动

因为图是提前规划的，**effort 滞后问题自然消除**：

```
cursor 指向 explore → LLM 执行 explore → 调 WorkflowNext → cursor 推进到 plan
  → 下一轮调 LLM 前直接设 Max effort
  → plan 节点天然在 Max 下执行
```

#### Effort 设置时机

```
Turn N:   cursor=explore, effort=Medium → 调 LLM → LLM 执行 explore
          LLM 调 WorkflowNext → cursor 变为 plan

Turn N+1: cursor=plan, effort=Max → 调 LLM → LLM 在 Max 下做深度推理
```

与旧 Graph 的关键区别：旧 Graph 中 LLM 声明 phase=plan 后下一轮才用 Max；Workflow Graph 中 cursor 已经指向 plan（由 WorkflowNext 推进），LLM 进入 plan 节点时 effort 就是对的。

#### Effort clamp

```
final_level = min(graph.desired, provider.max_level, user.max_level)
```

graph 节点的 desired effort 经 provider driver 的 `clamp_effort()` 自适应降级（不支持 reasoning 的模型静默忽略），再经用户 `max_reasoning` 上限约束。

### 2.10 持久化

#### Session 序列化

```json
{
  "workflow_graph": {
    "steps": [
      { "node": "start", "goal": null },
      { "node": "explore", "goal": "理解 auth 模块的 token 验证逻辑" },
      { "node": "plan", "goal": "设计 JWT 过期自动刷新方案" },
      { "node": "execute", "goal": "实现 token refresh 逻辑" },
      { "node": "verify", "goal": "运行 auth 相关测试" },
      { "node": "deliver", "goal": "交付改动摘要给用户" }
    ],
    "cursor": 3,
    "completed": [true, true, true, false, false, false],
    "node_records": [
      {
        "tools_executed": ["WorkflowPlan"],
        "has_error": false,
        "entered_at": 0,
        "completed_at": 1
      },
      {
        "tools_executed": ["Read", "Grep", "Grep"],
        "has_error": false,
        "entered_at": 1,
        "completed_at": 4
      },
      {
        "tools_executed": [],
        "has_error": false,
        "entered_at": 5,
        "completed_at": 6
      },
      {
        "tools_executed": ["Edit"],
        "has_error": false,
        "entered_at": 7,
        "completed_at": null
      },
      { "tools_executed": [], "has_error": false, "entered_at": 0, "completed_at": null },
      { "tools_executed": [], "has_error": false, "entered_at": 0, "completed_at": null }
    ],
    "replan_count": 0,
    "original_steps": [
      { "node": "start", "goal": null },
      { "node": "explore", "goal": "理解 auth 模块的 token 验证逻辑" },
      { "node": "plan", "goal": "设计 JWT 过期自动刷新方案" },
      { "node": "execute", "goal": "实现 token refresh 逻辑" },
      { "node": "verify", "goal": "运行 auth 相关测试" },
      { "node": "deliver", "goal": "交付改动摘要给用户" }
    ]
  }
}
```

存储位置：`Session.workflow_graph: Option<WorkflowGraph>`（新增字段，`#[serde(default)]` 兼容旧 session）。

#### Resume 恢复

resume session 时：
1. 反序列化 `workflow_graph`
2. 恢复 cursor 位置、completed 状态、node_records
3. 从 cursor 指向的节点继续执行

如果旧 session 没有 `workflow_graph` 字段，`#[serde(default)]` 返回 `None`，使用 provider 默认 effort。

## 3. 架构

### 3.1 模块位置

```
agent/features/runtime/src/
├── business/
│   ├── reasoning_graph/               ← 改造（替换旧观察式 Graph）
│   │   ├── mod.rs                     ← WorkflowGraph + WorkflowStep + NodeRecord + ReasoningNode
│   │   ├── guard.rs                   ← 新增：建议性准入/准出检查
│   │   ├── config.rs                  ← 沿用（effort 配置）
│   │   └── reasoning_graph_tests.rs   ← 更新测试
│   ├── chat/
│   │   └── looping/
│   │       ├── loop_runner.rs         ← 改造执行循环
│   │       └── events.rs              ← GraphPhaseChanged 事件扩展
│   └── session/
│       └── types.rs                   ← Session 新增 workflow_graph 字段

agent/features/tools/src/
└── business/
    ├── workflow_plan.rs               ← 新增 WorkflowPlan tool
    └── workflow_next.rs               ← 新增 WorkflowNext tool

agent/features/prompt/src/
└── business/guidance/
    └── _workflow.md                   ← 新增：workflow 规划指导

apps/cli/src/
└── tui/
    ├── model/runtime/model.rs         ← graph_phase 显示扩展
    └── render/output_area/spinner.rs  ← 显示当前 cursor 位置
```

### 3.2 改造范围

#### 新增

| 文件 | 内容 |
|---|---|
| `agent/features/tools/src/business/workflow_plan.rs` | `WorkflowPlanTool` 实现 |
| `agent/features/tools/src/business/workflow_next.rs` | `WorkflowNextTool` 实现 |
| `agent/features/runtime/src/business/reasoning_graph/guard.rs` | 建议性准入/准出检查 |
| `agent/features/prompt/src/business/guidance/_workflow.md` | LLM workflow 规划指导（双语） |

#### 改造

| 文件 | 改动 |
|---|---|
| `agent/features/runtime/src/business/reasoning_graph.rs`（或 `mod.rs`） | `ReasoningGraph` → `WorkflowGraph`；新增 `WorkflowStep`、`NodeRecord`；`ReasoningNode` 枚举增加 `Start` / `Deliver` 变体；`Execute` 默认 effort Off → Low |
| `agent/features/runtime/src/business/chat/looping/loop_runner.rs` | 执行循环改造：cursor effort 联动 + Workflow tool 优先执行 |
| `agent/features/runtime/src/business/chat/looping/events.rs` | `GraphPhaseChanged` 事件扩展（含 cursor/completed 信息） |
| `agent/features/runtime/src/business/session/types.rs` | `Session` 新增 `workflow_graph: Option<WorkflowGraph>` 字段 |
| `agent/features/tools/src/business.rs` | 注册 `WorkflowPlanTool` + `WorkflowNextTool` |
| `apps/cli/src/tui/model/runtime/model.rs` | `graph_phase` 扩展显示 cursor + completed |
| `docs/design/README.md` | 索引表新增 08 行 |
| `docs/design/06-agent-reasoning-graph.md` | 顶部标注"已被 08-agent-workflow-graph.md 替换" |

#### 删除

| 文件 | 说明 |
|---|---|
| `agent/features/runtime/src/business/reasoning_graph/classify.rs` | 阶段推断器——不再需要 |

#### 沿用（无改动）

| 文件 | 说明 |
|---|---|
| `agent/features/runtime/src/business/reasoning_graph/config.rs` | effort 配置解析保留 |
| `agent/shared/src/config/reasoning_graph.rs` | 配置结构保留（nodes effort 覆盖沿用） |

### 3.3 与现有架构的关系

#### 不改变的部分

- agent loop 主结构（`loop_runner.rs`）——在已有 turn 循环中插入 cursor effort 联动
- tool 执行流程（`execute_tool_round`）——仅增加 Workflow tool 排序优先
- compact 逻辑——完全不变
- provider API——复用现有 `set_reasoning_level`

#### 新增的部分

- `WorkflowPlanTool`——LLM 建图入口
- `WorkflowNextTool`——LLM 推进游标入口
- `WorkflowGraph`——线性序列 + cursor 状态机
- 建议性准入/准出 guard——反馈信号
- session 持久化——workflow_graph 字段
- Guidance `_workflow.md`——LLM 规划指导

#### 删除的部分

- `classify.rs` 阶段推断器
- tool call input 中的 `phase` 字段声明机制
- `discipline.rs` 中 phase 声明相关的指导

## 4. Guidance 改动

### 4.1 新增 _workflow.md（双语）

**英文版 `_workflow.md`**：

```markdown
# Workflow Planning

For non-trivial tasks, call the `WorkflowPlan` tool first to declare your work steps:

WorkflowPlan({
  "steps": [
    { "node": "explore", "goal": "Understand the auth module's token validation logic" },
    { "node": "plan", "goal": "Design JWT auto-refresh strategy" },
    { "node": "execute", "goal": "Implement token refresh logic" },
    { "node": "verify", "goal": "Run auth-related tests" },
    { "node": "deliver", "goal": "Summarize changes for the user" }
  ]
})

Node types:
- explore: Gather information (Read/Grep/Glob/read-only Bash), effort=medium
- plan: Deep reasoning, design solution, effort=max
- execute: Make changes (Edit/Write/Bash write), effort=low
- verify: Validate results (cargo test/clippy/build), effort=medium
- deliver: Terminate workflow and deliver to user (optional — omit for implicit delivery)

When a node's goal is achieved, call `WorkflowNext` to advance the cursor:
WorkflowNext({ "summary": "Explored auth module, found token validation in auth.rs" })

Choose steps based on task complexity:
- Simple query: [explore, deliver] or just [explore]
- Standard fix: [explore, plan, execute, verify]
- Need user confirmation: [explore, plan, deliver]
- Iterative fix: [explore, plan, execute, verify, plan, execute, verify, deliver]

Call WorkflowPlan again to replan when the current approach isn't working.
```

**中文版**（追加到现有中文 guidance）：

```markdown
# 工作流规划

收到非简单任务后，首先调用 WorkflowPlan 工具规划工作步骤。

节点说明：
- explore: 收集信息（Read/Grep/Glob/只读 Bash），effort=medium
- plan: 深度推理，定方案，effort=max
- execute: 执行改动（Edit/Write/Bash 写入），effort=low
- verify: 验证结果（cargo test/clippy/build），effort=medium
- deliver: 终止工作流并交付给用户（可选——省略则走完末节点隐式交付）

当前节点目标达成后，调用 WorkflowNext 推进游标到下一个节点：
WorkflowNext({ "summary": "探索完成，理解了 auth 模块的 token 验证逻辑" })

根据任务复杂度选择合适的节点序列。
发现方案不对时，再次调用 WorkflowPlan 重新规划剩余路径。
```

### 4.2 discipline.rs 改动

**删除** phase 声明相关指导（tool call input 中的 `phase` 字段不再使用）。

## 5. 风险与缓解

### 5.1 LLM 不调用 WorkflowPlan

**风险**：LLM 可能忘记调用 `WorkflowPlan`，直接开始用 tool。

**缓解**：没有图时使用 provider 默认 effort，行为等同于无 graph 的标准 agent loop。Guidance 中引导但不强制。LLM 对简单任务可以不建图，直接调 tool 回答——这是合理行为。

### 5.2 LLM 忘记调用 WorkflowNext

**风险**：LLM 完成了一个节点的目标但忘记调 `WorkflowNext`，cursor 卡在旧节点，effort 不切换。

**缓解**：
- Guidance 中明确强调"节点目标达成后必须调 WorkflowNext"
- tool_result 中持续显示 `current_node` + `current_goal`，提醒 LLM 当前所在节点
- 不阻塞——LLM 可以继续用 tool，只是 effort 还在旧节点（最坏情况是 effort 调错一档）

### 5.3 过度规划

**风险**：LLM 对简单任务也规划完整图，增加延迟和 token。

**缓解**：Guidance 明确引导"简单查询只需 `[explore]` 或 `[explore, deliver]`"。`WorkflowPlan` 的 description 中说明"根据任务复杂度选择合适的节点序列"。

### 5.4 Replan 震荡

**风险**：LLM 频繁 replan，导致流程反复跳变。

**缓解**：
- `replan_count` 记录在 session 中，可用于审计和未来限速
- replan 后 cursor 重置到 start，LLM 需要重新走——这本身就是"重新思考"的成本，自然抑制频繁 replan

### 5.5 与 compact 的交互

Workflow Graph 降低 effort 后（尤其 execute=low），单轮 reasoning token 减少，延迟 compact 触发——这是正效应。

compact 时不重置 workflow graph——图代表 LLM 的工作计划，compact 只压缩历史消息，不影响计划本身。

### 5.6 Provider 兼容性

effort clamp 机制对不支持 reasoning 的模型静默忽略。Workflow Graph 退化为纯阶段跟踪 + 流程控制（准入准出仍工作，只是 effort 调节无效）。

## 6. 演进路线

### 6.1 落地计划

本文档是 Workflow Graph 的完整设计。落地时 **SHOULD** 拆分为 umbrella issue + 子 issue：

| 子 issue | 范围 | 依赖 |
|---|---|---|
| WorkflowGraph 核心类型 + 持久化 | `reasoning_graph.rs` + `session/types.rs` | 无 |
| 删除 classify.rs + phase 机制 | `classify.rs` 删除 + `loop_runner.rs` 清理 | 无（可并行先行） |
| WorkflowPlan tool 实现 + 注册 | `workflow_plan.rs` + `business.rs` | 核心类型 |
| WorkflowNext tool 实现 + 注册 | `workflow_next.rs` + `business.rs` | 核心类型 |
| 建议性准入/准出 guard | `guard.rs` | 核心类型 |
| loop_runner 集成 | `loop_runner.rs` 改造 | 核心类型 + tool + guard |
| Guidance | `_workflow.md` + `discipline.rs` 清理 | tool |
| TUI 事件 + 渲染 | `model.rs` + `spinner.rs` | loop_runner |
| Execute effort Off→Low | `reasoning_graph.rs` + config | 无（可独立先行） |
| 测试 + 验证 | 全模块 | 全部 |

### 6.2 后续演进：DAG 节点

本文档只实现线性序列。后续如果数据证明需要，再增加控制节点：

| 节点 | 用途 | 触发条件 |
|---|---|---|
| `branch` | 条件分支 | explore 后需根据结果选择不同路径 |
| `switch` | 多路分发 | 多个独立子任务可并行 |
| `join` | 汇合点 | 等待并行分支完成 |

branch 的替代方案（当前可用）：LLM 在 plan 节点做深度推理决定路径，然后 replan 出精确的线性序列。不需要图层面的分支结构。

### 6.3 对 06 文档的处理

`06-agent-reasoning-graph.md` 顶部 **MUST** 标注：

> ⚠️ 本文档已被 `08-agent-workflow-graph.md` 替换。观察式 Reasoning Graph 已废弃，`classify.rs` 和 `phase` 声明机制已删除。

## 7. 开放问题

| 问题 | 当前倾向 | 待验证 |
|---|---|---|
| WorkflowPlan 是否需要用户确认？ | 不需要（is_input_safe=true） | 实际使用中是否有误规划风险 |
| 准出门槛 warning 是否足够引导 LLM？ | 注入 tool_result 即可 | 提示频率是否干扰 LLM |
| graph 走完后是否自动销毁？ | 是（cursor 进入 deliver / 走完末节点 → workflow=None） | 用户是否需要查看已完成的图 |
| 多模型 pool 故障转移时 graph 如何处理？ | 保留图，cursor 不变 | 故障转移后 effort 可能需要重设 |
| Sub-agent 是否使用 Workflow Graph？ | 否（sub-agent 任务范围由父 agent 限定） | 后续评估 |