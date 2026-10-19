# Memory 评测报告

> 评估日期：2026-10-17
> 评估范围：Memory 的读写、检索、注入、持久化、事件流、日志和既有评测机制。
> 证据边界：本报告使用已有事件和日志，未重新执行测试。

## 1. 结论

Memory 的读写、检索、注入、事件记录和持久化链路已经具备较完整的工程实现，但当前缺少真正的端到端质量评测。

现有数据能够证明：

- 系统确实在读写记忆；
- 记忆确实被召回和注入；
- CAS、反思、归档、过期标记等操作持续发生；
- 事件流中的存储操作均成功。

现有数据不能证明：

- 召回内容是否真正相关；
- 注入是否帮助了后续任务；
- 是否存在大量误注入；
- 反思生成的记忆是否比原记忆更好；
- 过期和 supersede 判断是否正确。

## 2. 运行事件统计

事件目录：`~/.agents/memory/*/events/*.jsonl`。

当前共有 17 个事件分片、2001 条事件，事件 outcome 全部为 `succeeded`。

| 操作 | 次数 |
|---|---:|
| commit_cas | 408 |
| search | 398 |
| per_message_recall | 237 |
| assembly_fingerprint | 153 |
| open_load | 153 |
| retrieve_for_inject | 107 |
| write_add | 106 |
| mark_outdated | 98 |
| supersede_synthesis | 84 |
| reflection_triggered | 53 |
| reflection_applied | 53 |
| reflection_cost | 53 |
| eviction_watermark | 6 |

这些数据说明生产链路覆盖了读、写、召回、注入、合并和过期处理，而不是只有单向写入。

## 3. 普通日志证据

文件：`~/.agents/logs/agent-memory.log`。

- 日志行数：2226；时间范围：2026-07-22 至 2026-10-15。
- `memory_injection`：1084 次。
- `memory_write`：9 次。
- `memory_read_fallback`：7 次。
- `memory_rerank_fallback`：1 次。
- `memory_reflection_above_target`：17 次。
- `Reflection apply failed`：6 次。

常见注入日志会记录候选数、命中数和限制，例如 `candidates=9 hits=5`，能证明注入流程执行，但不能证明命中的 5 条内容正确。

## 4. 当前评测覆盖

已有覆盖：

- Memory domain 单元测试；
- 应用层读写和反思契约测试；
- 序列化和持久化测试；
- TUI memory display 场景测试；
- System One 的合成和真实 rerank 评测。

主要不足：

1. 没有独立的 memory recall gold 集。线上已有 237 条 `per_message_recall`，但没有人工相关性标注。
2. 没有 memory injection 质量指标，当前只有 candidates、hits、limit 等数量。
3. 没有端到端收益指标，无法判断注入后任务是否更准确、是否减少重复询问或工具调用。
4. 事件 `succeeded` 主要表示操作或持久化成功，不表示记忆内容正确、相关或有用。

## 5. 风险判断

### 容量风险

日志中出现 17 次 `memory_reflection_above_target`，典型状态为：

```text
active=198/199 target=150 hard_cap=200
```

这说明 Project memory 曾多次接近硬上限，明显高于目标水位。当前策略要求依靠 reflection、supersede 和 outdated 收敛，不能简单机械裁剪，因此需要持续观测能否稳定回到 active≤150。

### 降级风险

已有 7 次 Memory read fallback 和 1 次 rerank fallback。数量不大，但还没有按会话、provider、daemon 状态做关联分析，无法判断是否集中于某类环境。

## 6. 建议

- 新增 `memory_recall.jsonl`，从线上 recall 事件抽样并补人工 gold。
- 新增 memory injection 数据集，标注 query、候选记忆、gold 相关记忆、是否应注入和后续任务结果。
- 指标至少包括 Recall@k、MRR、precision@k、false injection rate、重复率、注入 token 成本和 downstream task gain。
- 将 fallback、容量超目标和内容质量结果纳入统一回归报告。
- 将“事件执行成功”和“内容质量成功”明确分成两套指标。

## 7. 评估结论

Memory 的工程完整性较好，已有真实运行证据；但质量可证明性不足。当前最重要的工作是建立独立的 recall/injection 标注集，不能只用 System One 的 rerank 结果推断整个 Memory 系统质量。
