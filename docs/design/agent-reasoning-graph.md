# Agent Reasoning Graph

> 对应 Issue: 待创建
>
> 本文档是 **架构设计** 类设计文档：基于真实 session 数据画像，设计一套以阶段节点驱动 reasoning effort 的推断式状态机。文档 **MUST** 作为实现依据，**SHOULD** 配套 umbrella issue + 子 issue 使用。
>
> 关联文档：`docs/design/agent-orchestration.md`（Agent 编排范式知识地图，本文是其 Workflow / Graph 维度的具体落地之一）

## 1. 动机

### 1.1 问题

aemeath 当前 agent loop 中，reasoning effort 在整个 session 内是**静态**的：

```
bootstrap 时 resolve（CLI flag > model config > env > 默认）
    → 整个 session 不变
    → 每次 LLM 调用都按同一 effort 跑
```

但 100 个真实 session 的数据画像表明，agent 工作天然分阶段，且各阶段对推理深度的需求差异极大。静态 effort 导致：

- **Explore 阶段（61% 的 tool call）** 用满血 effort 浪费 token——多数只是 Read/Grep 机械收集信息
- **Execute 阶段（22%）** 用满血 effort 不必要——LLM 已经知道该改什么，只需机械执行
- **复杂规划时刻** effort 不够——当检测到偏差需要重新规划时，effort 没有自动升高

### 1.2 数据画像（100 个 session，92 个有效，6035 次 tool call）

| 阶段 | 占比 | 含义 |
|---|---|---|
| EXPLORE | 61% | Read / Grep / Glob / 只读 Bash |
| EXECUTE | 22% | Edit / Write / 执行类 Bash |
| VERIFY | 6% | cargo test / clippy / build |
| AGENT | 4% | 子代理编排 |
| TASK | 2% | 任务管理 |
| OTHER | 2% | Skill / AskUserQuestion 等 |

**关键发现**：

1. **混合率 0%**：同一 user turn 内同时出现 EXPLORE + EXECUTE 的仅 3/4149 turns（0.07%）。LLM 天然在 turn 级别做阶段分离。
2. **高频回环**：`E→X→E`（214 次）、`X→E→X`（176 次）、`E→V→E`（67 次）。阶段不是线性推进，而是密集小循环。
3. **Explore 连续长度大**：avg 3.8 calls，max 50。大块探索是省 token 的最佳目标。
4. **Edit 后续行为**：51% 继续 Execute、32% 回 Explore、12% 去 Verify、2% 去 Agent。强制验证不符合自然行为。

### 1.3 为什么选 graph 而非 turn-aware 或纯 LLM tool

| 方案 | 机制 | 问题 |
|---|---|---|
| turn-aware（按轮次） | 第 1 轮 high，第 2+ 轮 low | 粗糙——数据显示阶段是密集交替的（E→X→E→X），不是线性递减 |
| LLM 显式 tool | LLM 调 `enter_plan_mode` / `exit_plan_mode` | LLM 可能忘记调用；且阻塞 tool 执行改变现有 loop 结构 |
| **Reasoning Graph（本文）** | runtime 根据上一个 tool **推断**当前阶段，自动调 effort | 不阻塞任何 tool、不改 loop 结构、不依赖 LLM 配合 |

## 2. 设计

### 2.1 核心原则

> **Graph 是 effort 调节器，不是流程约束器。**

graph 只做两件事：
1. **跟踪当前阶段**（推断式，基于上一个 tool 的类型和结果）
2. **映射到 reasoning effort**（每个节点有默认 effort 值）

graph **NEVER** 阻塞 tool 执行、**NEVER** 强制流程顺序、**NEVER** 改变现有 agent loop 结构。LLM 想从 EXPLORE 直接 Edit 没问题，runtime 只是把 effort 从 medium 调到 low。

### 2.2 节点定义

```
┌─────────────────────────────────────────────────────────┐
│                    IDLE                                  │
│              (effort: inherit / 不调 LLM)                │
└────────────┬────────────────────────────────────────────┘
             │ user message
             ▼
┌─────────────────────────────────────────────────────────┐
│                   EXPLORE                                │
│           effort: medium (可配置)                        │
│  Read / Grep / Glob / 只读 Bash                          │
│  含义：收集信息，理解现状                                  │
└────────────┬──────────────────────────┬─────────────────┘
             │ Edit/Write/执行Bash      │ tool_error 或检测到偏差
             │                          │ 或 user 追加信息
             ▼                          ▼
┌──────────────────────────────────┐  ┌─────────────────────┐
│           EXECUTE                │  │      PLAN            │
│       effort: low (可配置)       │  │  effort: high        │
│  Edit / Write / 执行类 Bash      │  │  (可配置)            │
│  含义：机械执行已确定的改动       │  │  纯思考，重新评估     │
└──────────┬───────────────────────┘  │  深度推理             │
           │ Bash(cargo test/clippy)   └─────────┬───────────┘
           ▼                                      │
┌──────────────────────────────────────────────────┐
│                   VERIFY                           │
│         effort: medium (可配置)                    │
│  cargo test / clippy / build / tsc                │
│  含义：验证执行结果                                │
└──────────┬──────────────────────────┬─────────────┘
           │ 验证通过                  │ 验证失败
           ▼                          ▼
        → DONE                    → EXPLORE（回探索找原因）
                                   或 → PLAN（重新规划）
```

| 节点 | effort | 允许的 tools | 含义 |
|---|---|---|---|
| IDLE | inherit | — | 等待用户输入，不主动调 LLM |
| EXPLORE | medium | 全部（推断为只读） | 收集信息，理解现状 |
| PLAN | high | 全部 | 深度推理，定方案，处理异常 |
| EXECUTE | low | 全部（推断为写入） | 机械执行计划 |
| VERIFY | medium | 全部（推断为测试） | 验证执行结果 |

**注意**：每个节点允许**全部** tools——节点的划分仅用于调 effort，不限制 tool 选择。这保证 LLM 的自由度不被破坏。

### 2.3 推断式转移规则

节点转换由 runtime 根据**上一个 tool 的类型 + 结果**自动推断。这是单向被动观察，不干预 LLM。

#### 转移信号

| 信号 | 条件 | 目标节点 |
|---|---|---|
| `UserMessage` | 新 user message 到达 | → EXPLORE（小任务）或 PLAN（复杂任务） |
| `ToolExplored` | tool = Read / Grep / Glob / 只读 Bash | → EXPLORE（stay） |
| `ToolExecuted` | tool = Edit / Write / 执行类 Bash（git add / gh pr / cd） | → EXECUTE |
| `ToolVerified` | tool = Bash 且命令含 test/clippy/build/check 关键词 | → VERIFY |
| `ToolError` | tool_result.is_error == true | → PLAN（任何节点都可能触发） |
| `ToolDone` | LLM 无 tool_call（纯文本回复） | → IDLE |
| `TurnBoundary` | 新 turn 开始（agent loop 新轮次） | 保持上一轮节点（除非有新 user message） |

#### Bash 分类规则

Bash 是万能工具，穿越所有阶段。分类逻辑：

```rust
fn classify_bash(command: &str) -> BashCategory {
    let cmd = command.to_lowercase();
    // 验证类：构建 / 测试 / lint
    let verify_keywords = ["cargo test", "cargo clippy", "cargo check",
                           "cargo build", "npm test", "pytest", "go test",
                           "tsc", "make test", "yarn test", "rustc"];
    for kw in &verify_keywords {
        if cmd.contains(kw) { return BashCategory::Verify; }
    }
    // 探索类：只读命令
    let explore_keywords = ["git log", "git diff", "git show", "git status",
                            "git branch", "ls ", "cat ", "head ", "tail ",
                            "wc ", "find ", "grep ", "rg ", "fd "];
    for kw in &explore_keywords {
        if cmd.contains(kw) { return BashCategory::Explore; }
    }
    // 默认：执行类
    BashCategory::Execute
}
```

> **已知局限**：`gh pr create` / `echo` / `cd` + 复合命令会被归为 Execute。数据画像显示这类误分类率约 15%，但影响仅是 effort 调错一档（medium vs low），不阻塞执行。**MAY** 在后续迭代中用 LLM 输出的 `<phase>` 标记覆盖。

#### Turn 开始时的初始节点

| 条件 | 初始节点 |
|---|---|
| 首个 turn（turn_count == 1） | EXPLORE |
| resume 会话首 turn | EXPLORE（保守，先理解现状） |
| user message 含复杂意图关键词（"设计"/"重构"/"架构"/"排查"/"为什么"） | PLAN |
| user message 是简单指令（"修复"/"运行"/"提交"） | EXPLORE 或 EXECUTE |

### 2.4 Effort 映射

```rust
impl ReasoningNode {
    fn default_effort(&self) -> ReasoningEffort {
        match self {
            ReasoningNode::Idle   => ReasoningEffort::Inherit,
            ReasoningNode::Explore => ReasoningEffort::Medium,
            ReasoningNode::Plan    => ReasoningEffort::High,
            ReasoningNode::Execute => ReasoningEffort::Low,
            ReasoningNode::Verify  => ReasoningEffort::Medium,
        }
    }
}
```

#### Effort 到 provider 参数的映射

不同 provider 的 effort 语义不同：

| Provider | low | medium | high |
|---|---|---|---|
| OpenAI 兼容（reasoning_effort） | `"low"` | `"medium"` | `"high"` |
| Anthropic（thinking_max_tokens） | 1024 | 4096 | 16384 |
| Ollama | thinking 关闭 | thinking 开启（默认 budget） | thinking 开启（大 budget） |

映射通过现有 `LlmClient::set_reasoning_effort()` 实现，**MUST NOT** 新增 provider API。

#### 可配置性

effort 映射 **MAY** 通过 `aemeath.json` 配置覆盖：

```json
{
  "reasoning_graph": {
    "enabled": true,
    "nodes": {
      "explore": { "effort": "medium" },
      "plan":    { "effort": "high" },
      "execute": { "effort": "low" },
      "verify":  { "effort": "medium" }
    }
  }
}
```

`enabled: false` 时回退到当前的静态 effort 行为（零行为变更保证）。

### 2.5 LLM 对状态机的感知策略

graph 的状态机和 effort 调节是否应该让 LLM 知晓？这是一个关键设计抉择。三种模式对比：

| 模式 | 机制 | 优点 | 风险 |
|---|---|---|---|
| **A. 完全隐藏**（Phase 1 默认） | runtime 后台静默调 effort，LLM 无感知 | 零行为干扰，LLM 保持自然模式 | effort 突降时 LLM 可能困惑；异常时无法主动升档 |
| **B. 告知状态** | 每轮注入 `current_phase: EXPLORE` 到 context | LLM 理解为什么 effort 变了 | 消耗 token；LLM 可能"迎合"状态而改变自然行为 |
| **C. LLM 驱动状态** | LLM 每轮显式声明 `<phase>PLAN</phase>` | 分类准确率 100% | 增加输出 token；LLM 可能忘记声明；与 runtime 推断冲突 |

#### 决策：A + 最小覆盖通道

**决定采用模式 A 为默认，加最小覆盖通道作为异常逃生口。**

理由：

1. **数据证明 LLM 自然行为已经正确**——0% 混合率意味着 LLM 已经在 turn 级别做对了阶段分离。告知状态是多余的——LLM 不需要被告知它已经在做的事。
2. **告知反而可能有害**——如果 LLM 知道当前是 EXECUTE，可能"表演"阶段（跳过必要的思考）；知道是 EXPLORE 可能人为限制探索深度。effort 调节是**工程优化**，不该成为 LLM 的认知负担。
3. **但 Bash 误分类是真实痛点**——15% 误分类率下，LLM 能自己感知到"我需要更深的推理"，而 runtime 推断错了节点。需要一条逃生通道。

因此采用 **A + 最小覆盖通道**：

```
默认行为（Phase 1）：
  runtime: 观察 tool → 更新 node → 调 effort
  LLM:    完全无感知

异常覆盖（Phase 2）：
  runtime: 观察 tool → 更新 node → 调 effort
  LLM:    正常输出；如需升档，输出 <reasoning_effort boost="high" />
  runtime: 检测到标记 → 当前轮覆盖为 high → 下一轮回归 graph 默认
```

#### 覆盖通道的 provider 兼容性问题

覆盖通道**放弃文本标记方案**（`<reasoning_effort boost="high" />`），原因：

不同 provider 的 thinking 表达方式完全不兼容：

| Provider | thinking 表达方式 | 文本标记是否可用 |
|---|---|---|
| Anthropic | 独立 `content_block`（`type: thinking`），thinking 已经表达了深度 | ❌ 多余——thinking block 本身就是"需要深度推理"的信号 |
| OpenAI 兼容（DeepSeek/GLM/Mimo） | 独立 `reasoning_content` 字段 | ❌ 侵入——标记混入正常文本输出，且 reasoning_content 是 provider 私有字段 |
| Ollama | 只有开/关，无格式概念 | ❌ 无意义——没有 reasoning 输出可供标记 |

让 LLM 输出文本标记有三个根本问题：
1. **对 Anthropic 多余**——thinking block 已经表达了"我需要深度推理"，再加标记是冗余
2. **对 OpenAI 兼容侵入**——标记混入正常文本，需要从输出中解析剥离，增加 stream_handler 复杂度
3. **对 Ollama 无意义**——没有 reasoning 能力，标记无的放矢

#### 覆盖通道改为 runtime 侧信号检测（provider 无关）

放弃 LLM 主动输出标记，改为 runtime **被动观察可检测信号**自动升级 effort。这些信号全部来自 runtime 已有的数据，不依赖任何 provider 的 thinking 格式：

| 信号 | 检测方式 | 触发动作 | 适用 provider |
|---|---|---|---|
| 连续 tool_error ≥ 2 | 计数器（`tool_result.is_error`） | → PLAN（effort: high） | 全部 |
| Bash exit code != 0（VERIFY 节点） | tool_result 输出解析 | → EXPLORE（effort: medium） | 全部 |
| 连续 3+ 轮无 Edit / 无新文件读 | 状态比较 | → PLAN（effort: high） | 全部 |
| reasoning_tokens 超过阈值但 effort 已是 high | usage 统计 | 保持 high（不降） | 支持 reasoning 的 |

**关键区别**：这些信号由 runtime 从 tool 结果和 usage 统计中推断，**NEVER** 依赖 LLM 主动声明，**NEVER** 依赖 provider 的 thinking 格式。对不支持 reasoning 的 provider，信号检测仍然工作——只是升级到 high 后 provider 不响应（静默降级）。

#### Phase 分期

> **Phase 1**：纯模式 A（完全隐藏 + 推断式）。先验证推断式 graph 的效果。数据画像显示 0% 混合率，推断式已经覆盖绝大多数场景。
>
> **Phase 2**：runtime 侧偏差检测信号（上表前两条）。纯 runtime 实现，provider 无关。
>
> **Phase 3**：更复杂的偏差检测（上表后两条），根据 Phase 2 数据评估是否需要。

## 3. 架构

### 3.1 模块位置

```
agent/features/runtime/src/
├── business/
│   ├── agent/
│   │   ├── runner/
│   │   │   ├── loop_run.rs          ← 主 chat loop，调用 graph 的位置
│   │   │   └── ...
│   │   └── ...
│   ├── chat/
│   │   └── looping/
│   │       ├── loop_runner.rs       ← stream_message 调用点（line ~473）
│   │       └── ...
│   └── compact/
│       └── token_estimation.rs      ← 已有的 effort 无关逻辑
├── core/
│   └── ...
└── reasoning_graph/                  ← 新增模块
    ├── mod.rs                        ← ReasoningGraph, ReasoningNode
    ├── classify.rs                   ← Bash 分类、tool→node 推断
    ├── config.rs                     ← 配置反序列化与覆盖
    └── reasoning_graph_tests.rs      ← 单元测试
```

### 3.2 核心类型

```rust
/// 推理阶段节点
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningNode {
    /// 空闲，等待用户输入
    Idle,
    /// 探索：收集信息，理解现状
    Explore,
    /// 规划：深度推理，定方案
    Plan,
    /// 执行：机械执行已确定的改动
    Execute,
    /// 验证：检查执行结果
    Verify,
}

/// 推理深度级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReasoningEffort {
    /// 继承 model config 的默认值（不调用 set_reasoning_effort）
    Inherit,
    Low,
    Medium,
    High,
}

/// 转移信号
#[derive(Debug, Clone)]
pub enum GraphSignal {
    /// 新 user message（含消息文本，用于判断初始节点）
    UserMessage { text: String, turn_count: usize },
    /// tool 执行完成
    ToolCompleted {
        tool_name: String,
        bash_command: Option<String>,
        is_error: bool,
    },
    /// LLM 回复无 tool call（纯文本）
    TextOnly,
    /// agent loop 新轮次
    TurnBoundary,
}

/// Reasoning Graph 状态机
pub struct ReasoningGraph {
    current: ReasoningNode,
    config: ReasoningGraphConfig,
}

impl ReasoningGraph {
    pub fn new(config: ReasoningGraphConfig) -> Self;

    /// 当前节点
    pub fn current_node(&self) -> ReasoningNode;

    /// 当前节点对应的 effort
    pub fn current_effort(&self) -> ReasoningEffort;

    /// 消费信号，更新当前节点，返回是否发生变化
    pub fn transition(&mut self, signal: GraphSignal) -> bool;
}
```

### 3.3 集成点

在 `loop_runner.rs` 的 `stream_message` 调用前后插入 graph 交互：

```rust
// loop_runner.rs，line ~448 附近（现有代码）
logging::context::set_current_model(client.model_name().to_string());
// ...

// === 新增：调 LLM 前根据 graph 设置 effort ===
if reasoning_graph.enabled() {
    let effort = graph.current_effort();
    if effort != ReasoningEffort::Inherit {
        client.set_reasoning_effort(Some(effort.as_str().to_string()));
    }
}

let api_start = std::time::Instant::now();
let response = client.stream_message(...).await; // 现有调用

// === 新增：tool 执行完成后，根据结果更新 graph ===
for tool_result in &tool_results {
    graph.transition(GraphSignal::ToolCompleted {
        tool_name: tool_result.name.clone(),
        bash_command: tool_result.bash_command.clone(),
        is_error: tool_result.is_error,
    });
}

// 如果 LLM 无 tool call（纯文本回复）
if tool_results.is_empty() {
    graph.transition(GraphSignal::TextOnly);
}
```

在 user message 入口处：

```rust
// user message 到达时
graph.transition(GraphSignal::UserMessage {
    text: user_text.clone(),
    turn_count,
});
```

### 3.4 Sub-agent 处理

Sub-agent（`SubAgentRun`）**SHOULD** 有独立的 graph 实例。原因：

- Sub-agent 通常是纯执行任务（数据画像：sub-agent session 中 Explore 占比更高，但整体寿命短）
- Sub-agent 的 `compact_if_needed`（`loop_helpers.rs:72`）已经独立处理 effort 阈值
- Sub-agent 不需要 PLAN 节点（它的任务范围由父 agent 限定）

Sub-agent graph 简化为三节点：`Explore → Execute → Verify`，无 PLAN。

### 3.5 日志

每次节点转换 **MUST** 记录到 `aemeath:agent:runtime` target：

```rust
log::info!(
    target: LOG_TARGET,
    "reasoning_graph transition: {} → {} (effort: {:?}, signal: {:?})",
    old_node, new_node, new_effort, signal
);
```

日志 schema 参见 `specs/logging.md`。

## 4. 落地计划

### 4.1 Phase 1：纯推断式 graph（MVP）

**目标**：验证推断式 graph 能否有效减少 reasoning token，不改变任何现有行为。

| 子任务 | 范围 | 依赖 |
|---|---|---|
| 实现 `ReasoningGraph` 核心类型 | `reasoning_graph/mod.rs` + `classify.rs` | 无 |
| Bash 分类器 + tool→node 推断 | `classify.rs` | 核心类型 |
| 配置反序列化 | `config.rs` + `aemeath.json` schema | 核心类型 |
| 主 chat loop 集成 | `loop_runner.rs` line ~448 | 核心类型 + 配置 |
| Sub-agent graph 独立实例 | `agent/runner/setup.rs` | 主 loop 集成 |
| 日志埋点 | `aemeath:agent:runtime` target | 集成完成 |
| 单元测试（分类器、转移矩阵） | `reasoning_graph_tests.rs` | 核心类型 |
| 配置开关 `enabled: false` 回退路径 | bootstrap.rs | 配置 |

**Phase 1 验证指标**：

- [ ] `enabled: false` 时所有现有测试无回归（零行为变更）
- [ ] `enabled: true` 时 agent loop 正常推进，graph 转换日志可见
- [ ] 长会话（≥50 tool call）的 reasoning token 占比对比静态 effort 下降 ≥20%
- [ ] Bash 分类准确率 ≥85%（与数据画像基线一致）

### 4.2 Phase 2：runtime 侧偏差检测信号（provider 无关）

仅在 Phase 1 数据证明推断式 graph 存在明显盲区时启动。**放弃 LLM 文本标记方案**（provider thinking 格式不兼容，见 §2.5），改用 runtime 可检测信号：

| 子任务 | 范围 | 信号 |
|---|---|---|
| 连续 tool_error 计数器 | `reasoning_graph/deviation.rs` | 连续 ≥2 次 error → PLAN |
| Bash exit code 解析 | `reasoning_graph/deviation.rs` | VERIFY 节点 exit != 0 → EXPLORE |

### 4.3 Phase 3：高级偏差检测（可选）

| 子任务 | 范围 | 信号 |
|---|---|---|
| 无进展检测 | `reasoning_graph/deviation.rs` | 连续 3+ 轮无 Edit/无新文件读 → PLAN |
| reasoning_tokens 阈值 | `reasoning_graph/deviation.rs` | usage 统计超阈值但 effort 已 high → 保持 |
| 上下文接近 compact 阈值 | `reasoning_graph/deviation.rs` + `token_estimation.rs` | 保持当前节点，但降低 effort |

## 5. 风险与缓解

### 5.1 误分类风险

Bash 分类器无法 100% 准确（复合命令、管道、自定义脚本）。

**缓解**：
- 误分类仅影响 effort 一档差异（low vs medium），不阻塞执行
- 错调 effort 比**完全不调**好——静态满血 effort 的浪费远大于偶尔调错一档
- `enabled: false` 配置项保证随时可关闭

### 5.2 阶段震荡风险

频繁切换（E→X→E→X→E）可能导致 effort 反复跳变，破坏 LLM 的连续推理。

**缓解**：
- 同一 turn 内不切换 effort（turn 是 agent loop 的一轮 LLM 调用 + tool 执行）
- effort 变化只在新 turn 开始时生效，turn 内保持稳定
- 引入 hysteresis：连续 2 次相同信号才真正切换

### 5.3 与 compact 的交互

现有 compact 逻辑（`compact.rs`）根据 token 使用率触发，与 reasoning effort 无直接耦合。但 graph 降低 effort 后，单轮 reasoning token 减少，会延迟 compact 触发——这是正效应。

**注意**：`loop_runner.rs:501` 处的日志 `total_tokens = input + output + reasoning` 将 reasoning 作为独立项相加，与 PR #445 修复的 `needs_compaction_actual` 逻辑不一致（后者已正确视为 `reasoning ⊂ completion`）。日志显示值偏大，但不影响 compact 判定。**MAY** 在后续单独修正此日志显示。

### 5.4 Provider 兼容性

不同 provider 对 effort 的支持差异：

| Provider | 支持 effort 动态调整 | 备注 |
|---|---|---|
| OpenAI / DeepSeek / GLM | ✅ `reasoning_effort` 参数 | 原生支持 low/medium/high |
| Anthropic | ⚠️ 通过 `thinking_max_tokens` 间接映射 | 需额外参数转换 |
| Ollama | ⚠️ 只有开/关 | medium 和 high 可能都映射为开 |
| 不支持 reasoning 的模型 | ❌ | graph 退化为纯阶段跟踪，不调 effort |

**保证**：对不支持 reasoning 的模型，graph **MUST** 静默降级——只跟踪阶段、记录日志，不调用 `set_reasoning_effort`。

## 6. 与现有架构的关系

### 6.1 不改变的部分

- agent loop 主结构（`loop_runner.rs`）——只在调用前后插入 graph 交互
- tool 执行流程——tool 调用不变，只是结果被 graph 观察
- compact 逻辑——完全不变，graph 通过减少 reasoning token 间接延迟 compact
- provider API——复用现有 `set_reasoning_effort`，**MUST NOT** 新增 API
- Guidance 系统——Phase 1 不修改 prompt

### 6.2 新增的部分

- `reasoning_graph/` 模块——核心类型 + 分类器 + 配置
- `aemeath.json` 的 `reasoning_graph` 配置段
- `aemeath:agent:runtime` 的 graph 转换日志

### 6.3 与 `agent-orchestration.md` 的关系

`agent-orchestration.md` 在 Workflow / Graph 维度标注 aemeath 当前为空，并列举了四个探索方向。本文档是其中 **"动态 reasoning 参数"** 方向的具体落地，属于最小侵入的 graph 引入方式：

- **不引入完整 workflow engine**——没有 DAG 调度、没有节点持久化、没有分支恢复
- **不改 agent loop 控制流**——边是推断式的观察，不是强制的转移
- **只解决一个具体问题**——reasoning effort 的动态调节

如果后续需要更强的流程控制（如可恢复的 sub-workflow），再独立设计。

## 7. 开放问题

| 问题 | 当前倾向 | 待验证 |
|---|---|---|
| user message 意图分类（EXPLORE vs PLAN）用关键词还是 LLM 分类？ | 关键词（Phase 1 简单） | Phase 1 数据验证准确率 |
| graph 状态是否持久化到 session？ | 不持久化（每 session 新建） | resume 场景的行为是否可接受 |
| 是否对 reasoning-disabled 模型启用 graph？ | 启用（纯阶段跟踪 + 日志） | 日志价值是否足够 |
| runtime 侧偏差检测信号的阈值（error 次数、无进展轮数） | 先用保守默认（error≥2、无进展≥3） | Phase 2 数据校准 |
| 多模型 pool 故障转移时 graph 如何处理？ | 重置为 EXPLORE | 实际故障转移频率 |