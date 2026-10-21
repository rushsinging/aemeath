# Harness · 证据反思与经验库

> 层级：02-modules / harness｜状态：Target｜Milestone：v0.2.0
> 本文定义无 ground-truth verifier 场景下的 RSI 效果反思：行为对比信号、证据绑定与经验库 corpus。

## 1. 设计立场

交互式会话没有 benchmark 评分。效果判断依赖**行为对比信号**：改进前后同类任务的行为差异。信号只作为 LLM 提案与采纳判断的**证据输入**，NEVER 用阈值规则机械自动采纳（对齐「语义合并判断交给 LLM，系统负责门禁」的反思职责边界先例）。

## 2. 信号集（daemon 从落盘投影读取，分块进材料）

| 信号 | 来源 | 说明 |
|---|---|---|
| token 效率 | run usage 汇总 | 每 run 输入/输出消耗与预算比 |
| 重试与纠错 | 工具事件流 | Bash 失败重跑、Edit 冲突重试、恢复性命令 |
| 用户反馈 | 交互事件 | 确认（accept）、纠正（reject 后改写重发）、打断 |
| 任务结局 | run 终态 | 完成/失败/超时/取消 |

信号为**既有落盘事件的汇总视图**（runtime 事件流与 usage 事实的落盘投影），NEVER 新建独立埋点通道，也 NEVER 依赖主进程内存态——会话全部关闭后 daemon 仍可离线读取。

## 3. 证据绑定（EvidenceBinding）

```rust
struct EvidenceBinding {
    run_ref: String,       // 来源 run 标识
    signal: SignalKind,    // 信号类别
    excerpt: String,       // 行为摘录（有界，预览上限对齐 memory 内容预览常量族）
}
```

**不变量**：经验库中每条被采纳的 lesson MUST 携带至少一条 EvidenceBinding；RSI 提案中每条 edit MUST 引用至少一条信号证据。无证据的提案在 daemon 自校验阶段拒绝。

## 4. 经验库 corpus

- **落盘**：独立文件（`~/.agents/harness/` 下，实施定名），JSONL 追加 + 周期整理；**NEVER** 与 memory corpus 混用路径、schema 或写锁。
- **条目**：

```rust
struct LessonEntry {
    id: LessonId,                 // UUIDv7
    component_ref: ComponentId,   // 指向哪类组件的经验
    lesson: String,               // 可复用结论（有界长度）
    evidence: Vec<EvidenceBinding>,
    adopted_at: u64,
    status: LessonStatus,         // Active | Retired（被后续经验取代）
}
```

- **复利路径**：daemon 每次改进循环读 Active lessons 作为提案上下文（改进级递归）；lesson 失效（组件删除、结论被反证）标 Retired 而非物理删除。
- **容量**：Active 上限 + 超限整理策略（实施时定数值；整理由 daemon 在改进循环内顺带完成，NEVER 独立定时任务）。

## 5. 与 memory reflect 的职责边界（MUST 遵守）

| | memory reflect | RSI 经验库 |
|---|---|---|
| 治理对象 | 记忆条目（corpus 去噪/归纳/容量收敛） | harness 组件改进经验 |
| 触发 | 独立三触发 + 写锁体系 | 本模块三触发，daemon 串行 |
| 写路径 | memory 模块独占 | harness 模块独占 |
| 进程 | 主进程 detached 派发（0.3.0 迁入 daemon，见 03 分片第 6 节） | 独立 daemon |

两者 NEVER 共享 corpus、写锁或触发器；唯一交汇点是都消费 runtime 事件流的只读投影。

## 6. 验收

- 证据不变量：无证据提案被拒（L2）；采纳 lesson 均含 EvidenceBinding（L2 + e2e）。
- 真模型 e2e：一次完整循环后经验库出现带证据的 Active lesson，且下一次改进周期的材料分块消费到该 lesson。
- JSON 兜底：空组件树/无信号时 daemon 仍输出合法 JSON 结构（对齐反思空 corpus 兜底教训，NEVER 自然语言解释）。
