# Agent Workflow Graph

> 本文档是 **架构设计** 类设计文档：将 Reasoning Graph 从纯观察式 effort 调节器升级为 LLM 驱动的显式工作流引擎，通过 `PlanWorkflow` tool 建图、线性序列执行、准入准出守卫，实现 effort 精确联动与流程控制。
>
> 本文档 **MUST** 作为实现依据，**SHOULD** 配套 umbrella issue + 子 issue 使用。
>
> 关联文档：
> - `docs/design/06-agent-reasoning-graph.md`（前置设计，本文是其 Phase 3 的具体落地）
> - `docs/design/05-agent-orchestration.md`（Agent 编排范式知识地图，本文是其 Workflow / Graph 维度的具体落地）
> - `docs/design/03-runtime-design.md`（Runtime 核心域，本文涉及 agent loop 控制流改动）

## 1. 动机

### 1.1 问题

aemeath 当前 Reasoning Graph（设计文档 `06-agent-reasoning-graph.md`）是**纯观察式**状态机：

```
runtime 观察 tool 类型/结果 → 推断阶段 → 调 effort
LLM 完全不知道 graph 的存在
```

Phase 2 已实现 LLM 在 tool call input 中声明 `phase` 字段作为 ground truth，但存在三个结构性缺陷：

**缺陷 1：effort 滞后一轮**

effort 是请求参数（调 LLM **之前**设），phase 声明在响应里（LLM **之后**才有）：

```
Turn N:  设 effort=Medium（基于上一轮推断）
         LLM 响应，声明 phase=plan
         tool 执行
         graph 更新为 Plan
Turn N+1: 设 effort=Max  ← 终于用上了 plan 的 effort
```

LLM 声明需要深度推理的那一轮本身，并没有得到深度推理。

**缺陷 2：Execute = Off 过于激进**

当前 Execute 节点默认 effort = Off。Edit/Write 并非纯机械执行——LLM 需要精确匹配 `old_string`、理解上下文位置、处理边界情况。完全关闭 thinking 会导致低级错误。

**缺陷 3：无流程控制**

graph 纯观察，不阻塞 tool、不强制流程。LLM 可以跳过 Explore 直接 Edit、改完不 Verify。数据画像显示 LLM 天然在 turn 级别做阶段分离（0% 混合率），但"跳过必要步骤"仍是真实痛点。

### 1.2 解决方案

将 graph 从**runtime 隐式推断**升级为**LLM 显式规划 + runtime 执行**：

```
LLM 调用 PlanWorkflow tool 声明工作步骤序列
  → runtime 持久化图到 session
  → 按图执行：准入检查 → 设 effort → 调 LLM → 准出检查 → 推进游标
  → 需要调整时再次调用 PlanWorkflow（replan）
```

核心原则转变：

| 维度 | Graph 1.0（当前） | Graph 2.0（本文） |
|---|---|---|
| 谁控制流程 | runtime 隐式推断 | LLM 显式规划 |
| effort 时机 | 滞后一轮 | 精确——节点已知，effort 提前设好 |
| 准入准出 | 无 | 有——图定义了顺序，守卫检查前置条件 |
| LLM 感知 | 无（模式 A 隐藏） | 有——LLM 主动建图，tool_result 闭环反馈 |
| 灵活性 | 高（随时跳转） | 高（可 replan） |
| 持久化 | 无（每 session 新建） | 有（session 级持久化，resume 恢复） |

### 1.3 与 Graph 1.0 的关系

Graph 2.0 **不删除** Graph 1.0 的推断式机制，而是将其降级为 **fallback**：

- LLM 调用了 `PlanWorkflow` → 使用显式图
- LLM 未调用 `PlanWorkflow` → 退化为推断式 phase（当前行为，`classify.rs` 保留）

这保证向后兼容：不使用 `PlanWorkflow` 的 session 行为与当前完全一致。

## 2. 设计

### 2.1 核心原则

> **Graph 2.0 是 LLM 驱动的工作流引擎，拥有有限控制权。**

graph 做四件事：
1. **承载 LLM 规划的工作步骤序列**（线性，允许重复和循环）
2. **准入检查**——进入节点前验证前置条件
3. **准出检查**——离开节点前验证完成条件
4. **effort 联动**——节点已知，effort 提前设置，无滞后

graph **不阻塞** tool 执行。阶段不匹配时，graph 通过提升 effort + 注入提示引导 LLM 回到正确阶段，但最终尊重 LLM 的判断。

### 2.2 节点定义

#### 控制节点

| 节点 | 默认 effort | 用途 |
|---|---|---|
| `start` | Medium | 图入口：session 创建图时 cursor 指向 start，LLM 在此节点完成 `PlanWorkflow` 建图后准出 |

**`start` 节点生命周期**：

- session 首次创建 workflow graph 时，cursor 自动指向 `start`（隐式节点，由 runtime 注入到 steps 头部）
- LLM 在 start 节点调用 `PlanWorkflow` 声明后续工作步骤
- start 的**准出条件** = session.workflow 的 steps 中已有 ≥1 个工作节点（即 PlanWorkflow 已执行）
- start 准出后，cursor 推进到第一个工作节点（explore/plan/execute/verify）

> **start 不是 LLM 声明的节点**——它是 runtime 自动注入的图入口。LLM 在 `PlanWorkflow` 的 `steps` 中只声明 explore/plan/execute/verify 四种工作节点。runtime 在构造 `WorkflowGraph` 时自动在 `steps[0]` 插入 `start`。

#### 工作节点

| 节点 | 默认 effort | 用途 |
|---|---|---|
| `explore` | Medium | 只读 tool（Read/Grep/Glob/只读 Bash），收集信息，理解现状 |
| `plan` | Max | 深度推理，定方案，根因分析，可调只读 tool 辅助 |
| `execute` | **Low** | 写入类 tool（Edit/Write/Bash 写入），执行改动 |
| `verify` | Medium | 构建/测试/lint（cargo test/clippy/build/tsc），验证结果 |

> **Execute 从 Off → Low**：保留基础推理能力。Edit/Write 需要精确匹配上下文，完全关闭 thinking 会导致低级错误。Low 比 Medium 省 token，但保留基础推理。

每个节点的 effort 可通过 `aemeath.json` 的 `reasoning_graph.nodes.<node>.effort` 覆盖（沿用 Graph 1.0 配置）。

#### DAG 控制节点（待做，本文档不实现）

| 节点 | 用途 | 状态 |
|---|---|---|
| `branch` | 条件分支：评估后决定走哪条路径 | 待做 |
| `switch` | 多路分发：并行推进独立子任务 | 待做 |
| `join` | 汇合点：等待多个并行分支完成 | 待做 |

Graph 2.0 首期只实现线性序列。branch / switch / join 列为后续演进方向（§6.2）。

#### 不需要的节点

- **deliver**：图走完后 LLM 的纯文本回复即为最终交付，不需要独立节点。cursor 走完最后一个节点后回到 Idle。
- **replan**：replan 不是节点，是"在当前位置重新规划剩余路径"的操作。LLM 再次调用 `PlanWorkflow` 即触发 replan。
- **debug**：本质是 explore + plan 的组合，不需要独立。

### 2.3 数据结构

```rust
/// 工作流步骤（LLM 在 PlanWorkflow tool 中声明的单个节点）。
///
/// 注意：`start` 节点由 runtime 自动注入（steps[0]），LLM 不声明此节点。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    /// 节点类型：start（runtime 注入）/ explore / plan / execute / verify
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
    /// 按执行顺序排列的步骤序列
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

- **线性 `Vec<WorkflowStep>`**：4 节点不值得用 DAG。序列允许重复和循环（`[explore, plan, execute, verify, plan, execute, verify]`）。
- **`goal: Option<String>`**：每个节点可带目标描述。准出检查时 runtime 可参考 goal 判断是否达成（粗粒度——有 goal 存在即作为提示注入，不做语义判断）。
- **`cursor: usize`**：指向 `steps` 中的当前位置。准入/准出都围绕 cursor 操作。
- **`original_steps`**：保留首次规划用于审计。replan 只替换 `steps`，`original_steps` 不变。

### 2.4 PlanWorkflow Tool

#### Schema

```json
{
  "name": "PlanWorkflow",
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
              "enum": ["explore", "plan", "execute", "verify"],
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

#### 命名

沿用项目 PascalCase 命名约定（`TaskCreate`、`EnterPlanMode`、`AskUserQuestion`），tool name 为 **`PlanWorkflow`**。

#### Tool 行为

**首次建图**（session 中无图）：

```rust
fn execute(&self, input: Value, ctx: &ToolExecutionContext) -> ToolResult {
    let user_steps: Vec<WorkflowStep> = parse_and_validate(input)?;

    // runtime 自动在 steps[0] 注入 start 节点
    let mut steps = vec![WorkflowStep {
        node: ReasoningNode::Start,
        goal: None,
    }];
    steps.extend(user_steps);

    let graph = WorkflowGraph::new(steps);
    ctx.session.set_workflow(graph);

    ToolResult::success(json!({
        "registered": true,
        "steps": graph.steps_summary(),
        "current_node": graph.current_node(),         // "start"
        "current_goal": graph.current_goal(),
    }))
}
```

**Replan**（session 中已有图）：

```rust
fn execute(&self, input: Value, ctx: &ToolExecutionContext) -> ToolResult {
    let user_steps: Vec<WorkflowStep> = parse_and_validate(input)?;

    // runtime 自动注入 start 节点
    let mut steps = vec![WorkflowStep {
        node: ReasoningNode::Start,
        goal: None,
    }];
    steps.extend(user_steps);

    if let Some(graph) = ctx.session.workflow_mut() {
        graph.replan(steps);
        ToolResult::success(json!({
            "replanned": true,
            "preserved_completed": graph.completed_count(),
            "new_steps": graph.steps_summary(),
            "current_node": graph.current_node(),         // "start"（快速准出）
        }))
    } else {
        // 首次建图
        let graph = WorkflowGraph::new(steps);
        ctx.session.set_workflow(graph);
        ToolResult::success(json!({
            "registered": true,
            "steps": graph.steps_summary(),
            "current_node": graph.current_node(),         // "start"
        }))
    }
}
```

#### Replan 语义

replan **不保留** completed 状态——新图的步骤是全新的路径。即使新步骤中有 explore，也需要重新走准入流程。但 explore 的准入条件是"无"，所以总是可以进入。

replan 时 runtime 同样自动注入 `start` 节点到新 steps[0]，cursor 重置为 0（指向 start）。但 start 的准出条件 `steps.len() > 1` 立即满足（因为 replan 的图已包含工作节点），所以 start 会在下一轮自动准出，cursor 快速推进到第一个工作节点。

replan 只递增 `replan_count`，保留 `original_steps` 不变（审计用途）。

#### Tool 属性

```rust
impl TypedTool for PlanWorkflowTool {
    fn name(&self) -> &str { "PlanWorkflow" }
    fn is_read_only(&self) -> bool { true }        // 不修改文件系统
    fn is_concurrency_safe(&self) -> bool { true } // 可并发调用
    fn timeout_secs(&self) -> u64 { 5 }             // 快速返回
    fn is_input_safe(&self) -> bool { true }        // 无需用户确认
}
```

### 2.5 执行循环

```
Turn 开始:
  ① 检查 session.workflow
     ├─ 有图 → 按 cursor 节点的 effort 设置 + 准入检查
     │   └─ cursor 指向 start → 等待 LLM 调用 PlanWorkflow 建图
     └─ 无图 → fallback 到推断式 phase 机制（当前行为）

  ② 调 LLM

  ③ LLM 返回响应
     ├─ 含 PlanWorkflow tool call → 执行（注册/replan 图）
     │   └─ cursor 在 start → PlanWorkflow 执行后，start 准出条件满足 → cursor++
     ├─ 含其他 tool call → 执行 → 记录到当前 cursor 节点
     │   ├─ tool call input 含 phase 声明 → 检查是否匹配 cursor 节点
     │   │   ├─ 匹配 → 正常推进
     │   │   └─ 不匹配 → 记录偏离，注入提示，提升 effort
     │   └─ 无 phase 声明 → 用 classify.rs 推断
     └─ 纯文本回复（无 tool call）
         ├─ cursor 节点准出检查通过 → cursor++
         │   └─ cursor 走完所有步骤 → 回到 Idle（图生命周期结束）
         └─ cursor 节点准出未通过 → 保持 cursor

  ④ 准出检查 cursor 节点
     ├─ 满足 → cursor++ → 回到 ①
     └─ 不满足 → 保持 cursor → 回到 ①
```

#### PlanWorkflow 优先执行

同一轮内如果 LLM 同时调用 `PlanWorkflow` + 其他 tool（如 `Read`），runtime 先执行 `PlanWorkflow`（注册图），再执行其他 tool（记入对应节点）。

实现方式：在 `execute_tool_round` 中对 tool calls 按 name 排序，`PlanWorkflow` 排在最前。

```rust
// execute_tool_round 中
let mut sorted_calls = tool_calls.clone();
sorted_calls.sort_by_key(|tc| if tc.name == "PlanWorkflow" { 0 } else { 1 });
```

#### 图生命周期

```
Idle → Start → Explore → Plan → Execute → Verify → Idle
       ↑                ↑
       (cursor 初始)    (replan 可插入新步骤)
```

- **图的创建**：LLM 首次调用 `PlanWorkflow`
- **图的结束**：cursor 走完最后一个步骤 + 该步骤准出通过 → 回到 Idle
- **图的销毁**：session 结束。图持久化到 session，resume 时恢复。

### 2.6 准入 / 准出

#### 准入（Entry Guard）

进入节点前检查前置条件。

| 目标节点 | 检查条件 | 不满足时 |
|---|---|---|
| `start` | 无（图的入口，自动进入） | — |
| `explore` | 无 | — |
| `plan` | 前序步骤中 explore 已 completed | 注入提示 + effort 提升到 Plan(Max) |
| `execute` | 前序步骤中 plan 已 completed | 注入提示 + effort 提升到 Plan(Max) |
| `verify` | 前序步骤中 execute 已 completed | 注入提示 + effort 提升到 Plan(Max) |

**准入检查由 runtime 做**（粗粒度，基于 cursor 之前步骤的 completed 状态）。

**注意**：准入检查的是**图中前序步骤**的 completed 状态。因为图是 LLM 规划的，可能第一条就是 `plan`（用户消息含复杂意图时）——此时 plan 前面没有 explore 步骤，准入无门槛。

**不满足准入时的行为**：

runtime **不阻塞** tool 执行。但做两件事：
1. 记录偏离（记入 `NodeRecord`）
2. 提升 effort 到 Plan(Max)——强制 LLM 在更高推理深度下重新评估

如果 Max effort 下 LLM 仍然坚持 → 尊重 LLM 判断，推进 cursor 到 LLM 声明的位置（隐式 replan）。

#### 准出（Exit Guard）

离开节点前检查完成条件。

| 节点 | runtime 可检查 | LLM 自评 |
|---|---|---|
| `start` | session.workflow.steps.len() > 1（PlanWorkflow 已执行，已有工作节点） | — |
| `explore` | session 内有 ≥1 次只读 tool 执行 | — |
| `plan` | — | LLM 输出纯文本（无 tool call）= 方案就绪 |
| `execute` | 当前轮所有 tool 无 error | — |
| `verify` | 验证命令执行完毕 | — |

**准出检查混合模式**：

- runtime 可检查的：tool error 计数、是否执行过特定 tool 类型
- runtime 不可检查的（如"方案是否就绪"）：信任 LLM 自评——plan 节点的纯文本回复视为方案就绪

**不满足准出时**：保持 cursor 不动，下一轮继续在当前节点。

#### 阶段不匹配处理

LLM 声明的 phase ≠ cursor 节点时：

```
cursor 指向 explore，LLM 声明 phase=execute
  → runtime 不阻塞 tool 执行
  → 记录偏离
  → 检查 execute 的准入条件：
     ├─ 满足 → 推进 cursor 到 execute 对应位置
     └─ 不满足 → 提升 effort 到 Plan(Max)，注入提示
  → 下一轮根据情况决定
```

**尊重 LLM 判断**：如果 LLM 在高 effort 下仍然坚持当前行为，runtime 推进 cursor 到 LLM 声明的位置。这是隐式 replan——LLM 用行动而非 `PlanWorkflow` tool 表达了流程调整。

### 2.7 Replan 机制

#### 触发方式

| 触发方式 | 机制 | 说明 |
|---|---|---|
| **LLM 主动 replan** | 再次调用 `PlanWorkflow` tool | LLM 输出新的步骤序列，runtime 替换图 |
| **Verify 失败 → 自动回退** | runtime 检测 verify 节点 tool error | cursor 回退到最近的 plan 位置 + 注入提示 |
| **连续 error ≥ 2** | runtime 计数器 | 注入提示"当前路径可能不正确，建议 replan" |

**LLM 主动 replan** 是主要机制。后两种是 runtime 侧的建议性干预——不强制 replan，只注入提示让 LLM 自己决定。

#### replan 示例

```
原始图: [start, explore, plan, execute, verify]
                      ↑ cursor=3, 在 execute 节点

LLM 发现方案不对，调用 PlanWorkflow:
  steps: [
    { node: "plan", goal: "重新评估 auth 模块的 token 刷新策略" },
    { node: "execute", goal: "实现新的 refresh 逻辑" },
    { node: "verify", goal: "运行 auth 测试" }
  ]

replan 后（runtime 自动注入 start）:
  steps: [start, plan, execute, verify]
  cursor: 0  ← 重置到 start（准出条件立即满足 → 快速推进到 plan）
  replan_count: 1
  original_steps: [start, explore, plan, execute, verify]  ← 保留
```

### 2.8 Effort 联动

因为图是提前规划的，**effort 滞后问题自然消除**：

```
cursor 指向 explore → LLM 执行 explore → 声明完成 → cursor 推进到 plan
  → 下一轮调 LLM 前直接设 Max effort
  → plan 节点天然在 Max 下执行
```

与 Graph 1.0 的关键区别：Graph 1.0 中 LLM 声明 phase=plan 后下一轮才用 Max；Graph 2.0 中 cursor 已经指向 plan，LLM 进入 plan 节点时 effort 就是对的。

#### 三层 clamp（沿用 Graph 1.0）

```
final_level = min(graph.desired, provider.max_level, user.max_level)
```

graph 节点的 desired effort 经 provider driver 的 `clamp_effort()` 自适应降级，再经用户 `max_reasoning` 上限约束。机制完全沿用 Graph 1.0（设计文档 `06-agent-reasoning-graph.md` §2.4）。

### 2.9 持久化

#### Session 序列化

```json
{
  "workflow_graph": {
    "steps": [
      { "node": "start", "goal": null },
      { "node": "explore", "goal": "理解 auth 模块的 token 验证逻辑" },
      { "node": "plan", "goal": "设计 JWT 过期自动刷新方案" },
      { "node": "execute", "goal": "实现 token refresh 逻辑" },
      { "node": "verify", "goal": "运行 auth 相关测试" }
    ],
    "cursor": 3,
    "completed": [true, true, true, false, false],
    "node_records": [
      {
        "tools_executed": ["PlanWorkflow"],
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
      {
        "tools_executed": [],
        "has_error": false,
        "entered_at": 0,
        "completed_at": null
      }
    ],
    "replan_count": 0,
    "original_steps": [
      { "node": "start", "goal": null },
      { "node": "explore", "goal": "理解 auth 模块的 token 验证逻辑" },
      { "node": "plan", "goal": "设计 JWT 过期自动刷新方案" },
      { "node": "execute", "goal": "实现 token refresh 逻辑" },
      { "node": "verify", "goal": "运行 auth 相关测试" }
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

如果旧 session 没有 `workflow_graph` 字段（Graph 1.0 时代的 session），`#[serde(default)]` 返回 `None`，退化为 fallback 推断式。

## 3. 架构

### 3.1 模块位置

```
agent/features/runtime/src/
├── business/
│   ├── reasoning_graph/               ← 改造（Graph 1.0 → 2.0）
│   │   ├── mod.rs                     ← WorkflowGraph + WorkflowStep + NodeRecord
│   │   ├── classify.rs                ← 保留（fallback 推断式）
│   │   ├── config.rs                  ← 保留（effort 配置）
│   │   ├── guard.rs                   ← 新增：准入/准出检查
│   │   └── reasoning_graph_tests.rs   ← 更新测试
│   ├── chat/
│   │   └── looping/
│   │       ├── loop_runner.rs         ← 改造执行循环
│   │       └── events.rs              ← GraphPhaseChanged 事件扩展
│   └── session/
│       └── types.rs                   ← Session 新增 workflow_graph 字段

agent/features/tools/src/
└── business/
    └── plan_workflow.rs               ← 新增 PlanWorkflow tool

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
| `agent/features/tools/src/business/plan_workflow.rs` | `PlanWorkflowTool` 实现 |
| `agent/features/runtime/src/business/reasoning_graph/guard.rs` | 准入/准出检查逻辑 |
| `agent/features/prompt/src/business/guidance/_workflow.md` | LLM workflow 规划指导（双语） |

#### 改造

| 文件 | 改动 |
|---|---|
| `agent/features/runtime/src/business/reasoning_graph.rs` | `ReasoningGraph` → `WorkflowGraph`；新增 `WorkflowStep`、`NodeRecord`；保留 `ReasoningNode` 枚举和 `classify.rs` |
| `agent/features/runtime/src/business/chat/looping/loop_runner.rs` | 执行循环改造：cursor 推进 + 准入准出 + PlanWorkflow 优先执行 |
| `agent/features/runtime/src/business/chat/looping/events.rs` | `GraphPhaseChanged` 事件扩展（含 cursor/completed 信息） |
| `agent/features/runtime/src/business/session/types.rs` | `Session` 新增 `workflow_graph: Option<WorkflowGraph>` 字段 |
| `agent/features/tools/src/business.rs` | 注册 `PlanWorkflowTool` |
| `agent/features/runtime/src/business/reasoning_graph.rs` | `ReasoningNode::Execute` 的 `default_effort()` 从 Off → Low |
| `agent/shared/src/i18n/prompt/discipline.rs` | phase 声明指导更新（与 PlanWorkflow 配合） |
| `apps/cli/src/tui/model/runtime/model.rs` | `graph_phase` 扩展显示 cursor + completed |
| `docs/design/README.md` | 索引表新增 08 行 |
| `docs/design/06-agent-reasoning-graph.md` | 顶部标注"已被 08-agent-workflow-graph.md 升级" |

#### 保留（无改动）

| 文件 | 说明 |
|---|---|
| `agent/features/runtime/src/business/reasoning_graph/classify.rs` | 作为 fallback 推断式分类器保留 |
| `agent/features/runtime/src/business/reasoning_graph/config.rs` | effort 配置解析保留 |
| `agent/shared/src/config/reasoning_graph.rs` | 配置结构保留（nodes effort 覆盖沿用） |

### 3.3 与现有架构的关系

#### 不改变的部分

- agent loop 主结构（`loop_runner.rs`）——在已有 turn 循环中插入 cursor 推进逻辑
- tool 执行流程（`execute_tool_round`）——仅增加 PlanWorkflow 排序优先
- compact 逻辑——完全不变
- provider API——复用现有 `set_reasoning_level`
- fallback 路径——LLM 不调用 PlanWorkflow 时行为与 Graph 1.0 完全一致

#### 新增的部分

- `PlanWorkflowTool`——LLM 建图入口
- `WorkflowGraph`——线性序列 + cursor 状态机
- 准入/准出 guard——有限控制权
- session 持久化——workflow_graph 字段
- Guidance `_workflow.md`——LLM 规划指导

#### 与 Graph 1.0 的关系

设计文档 `06-agent-reasoning-graph.md` §6.3 预留了 "Workflow 扩展空间"：

> 如果后续需要更强的流程控制（如可恢复的 sub-workflow），再独立设计。

本文档就是该预留方向的具体落地。与 06 的关系：

| 06 的设计 | 08 的升级 |
|---|---|
| Graph 是 effort 调节器，不是流程约束器 | Graph 是 LLM 驱动的工作流引擎，有有限控制权 |
| runtime 推断阶段 | LLM 显式规划阶段 |
| 不阻塞 tool | 不阻塞 tool（保持），但加准入准出引导 |
| 不持久化 | 持久化到 session |
| classify.rs 是主路径 | classify.rs 降级为 fallback |

## 4. Guidance 改动

### 4.1 新增 _workflow.md（双语）

**英文版 `_workflow.md`**：

```markdown
# Workflow Planning

For non-trivial tasks, call the `PlanWorkflow` tool first to declare your work steps:

PlanWorkflow({
  "steps": [
    { "node": "explore", "goal": "Understand the auth module's token validation logic" },
    { "node": "plan", "goal": "Design JWT auto-refresh strategy" },
    { "node": "execute", "goal": "Implement token refresh logic" },
    { "node": "verify", "goal": "Run auth-related tests" }
  ]
})

Node types:
- explore: Gather information (Read/Grep/Glob/read-only Bash), effort=medium
- plan: Deep reasoning, design solution, effort=max
- execute: Make changes (Edit/Write/Bash write), effort=low
- verify: Validate results (cargo test/clippy/build), effort=medium

Choose steps based on task complexity:
- Simple query: [explore]
- Standard fix: [explore, plan, execute, verify]
- Iterative fix: [explore, plan, execute, verify, plan, execute, verify]

Call PlanWorkflow again to replan when the current approach isn't working.
PlanWorkflow can be called in the same turn as your first tool call.
```

**中文版**（追加到现有中文 guidance）：

```markdown
# 工作流规划

收到非简单任务后，首先调用 PlanWorkflow 工具规划工作步骤。

节点说明：
- explore: 收集信息（Read/Grep/Glob/只读 Bash），effort=medium
- plan: 深度推理，定方案，effort=max
- execute: 执行改动（Edit/Write/Bash 写入），effort=low
- verify: 验证结果（cargo test/clippy/build），effort=medium

根据任务复杂度选择合适的节点序列。
发现方案不对时，再次调用 PlanWorkflow 重新规划剩余路径。
```

### 4.2 discipline.rs 更新

现有 phase 声明指导（`discipline.rs`）保留不变。phase 声明与 PlanWorkflow 配合：

- **PlanWorkflow**：声明**整张图**（每 session 1-3 次）
- **phase 字段**：声明**当前轮**在哪个节点（每轮，fallback 时用）

## 5. 风险与缓解

### 5.1 LLM 不调用 PlanWorkflow

**风险**：LLM 可能忘记调用 `PlanWorkflow`，直接开始用 tool。

**缓解**：fallback 到 Graph 1.0 推断式机制。行为与当前完全一致，不会更差。Guidance 中引导但不强制。

### 5.2 过度规划

**风险**：LLM 对简单任务也规划 4 步图，增加延迟和 token。

**缓解**：Guidance 明确引导"简单查询只需 `[explore]`"。PlanWorkflow tool 的 description 中说明"根据任务复杂度选择合适的节点序列"。

### 5.3 Replan 震荡

**风险**：LLM 频繁 replan，导致流程反复跳变。

**缓解**：
- `replan_count` 记录在 session 中，可用于审计和未来限速
- replan 后 cursor 重置，LLM 需要从新图起点重新走——这本身就是"重新思考"的成本，自然抑制频繁 replan

### 5.4 准入准出误判

**风险**：runtime 的准入/准出检查可能误判（如把非只读 tool 误认为只读）。

**缓解**：
- 准入不满足时不阻塞 tool，只提升 effort + 注入提示
- 准出不满足时保持 cursor，下一轮继续
- 最坏情况是 effort 调错一档或 cursor 推进延迟一轮，不阻塞执行

### 5.5 与 compact 的交互

Graph 2.0 降低 effort 后（尤其 execute=low），单轮 reasoning token 减少，延迟 compact 触发——这是正效应。

compact 时不重置 workflow graph——图代表 LLM 的工作计划，compact 只压缩历史消息，不影响计划本身。

### 5.6 Provider 兼容性

完全沿用 Graph 1.0 的三层 clamp 机制（§2.8）。对不支持 reasoning 的模型，graph 退化为纯阶段跟踪 + 流程控制（准入准出仍工作，只是 effort 调节被 provider 静默忽略）。

## 6. 演进路线

### 6.1 落地计划

本文档是 Graph 2.0 的完整设计。落地时 **SHOULD** 拆分为 umbrella issue + 子 issue：

| 子 issue | 范围 | 依赖 |
|---|---|---|
| WorkflowGraph 核心类型 + 持久化 | `reasoning_graph.rs` + `session/types.rs` | 无 |
| PlanWorkflow tool 实现 + 注册 | `plan_workflow.rs` + `business.rs` | 核心类型 |
| 准入/准出 guard | `guard.rs` | 核心类型 |
| loop_runner 集成 | `loop_runner.rs` 改造 | 核心类型 + tool + guard |
| Guidance + fallback 兼容 | `_workflow.md` + `discipline.rs` | tool |
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

### 6.3 对 06 文档的标注

`06-agent-reasoning-graph.md` 顶部 **MUST** 标注：

> ⚠️ 本文档的 Phase 3 已被 `08-agent-workflow-graph.md` 升级为完整设计。Graph 1.0 的推断式机制保留为 fallback。

## 7. 开放问题

| 问题 | 当前倾向 | 待验证 |
|---|---|---|
| PlanWorkflow 是否需要用户确认？ | 不需要（is_input_safe=true） | 实际使用中是否有误规划风险 |
| 准入不满足时是否注入 system message？ | 注入提示（非 system message，是 tool_result 附加） | 提示频率是否干扰 LLM |
| graph 走完后是否自动销毁？ | 是（cursor 走完 → Idle → workflow=None） | 用户是否需要查看已完成的图 |
| 多模型 pool 故障转移时 graph 如何处理？ | 保留图，cursor 不变 | 故障转移后 effort 可能需要重设 |
| Sub-agent 是否使用 PlanWorkflow？ | 否（sub-agent 任务范围由父 agent 限定） | 后续评估 |