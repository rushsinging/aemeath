# Reflect 评测报告

> 评估日期：2026-10-17
> 评估范围：Reflect 的触发、执行、解析、写锁、容量收敛、并发、持久化、日志和事件流。
> 证据边界：本报告使用已有事件和日志，未重新执行测试。

## 1. 结论

Reflect 的运行链路已经接通，但当前只能证明“反思任务在运行”，不能证明“反思结果质量足够好”。

已有证据表明：

- reflection trigger 能产生；
- reflection apply 能完成；
- supersede、mark outdated、reflection cost 事件均有记录；
- Memory 写入和反思之间存在事件关联。

当前没有：

- 反思质量 gold 集；
- suggestion、supersede、outdated 的 precision/recall/F1；
- 反思输出稳定性；
- 反思对后续召回和任务成功率的提升数据。

## 2. 运行事件统计

Memory 事件流中：

| 事件 | 次数 |
|---|---:|
| reflection_triggered | 53 |
| reflection_applied | 53 |
| reflection_cost | 53 |
| supersede_synthesis | 84 |
| mark_outdated | 98 |

当前观测窗口内触发和应用数量一致，说明至少没有从事件计数上看到明显的触发后永久丢失。但这是流程证据，不是内容质量证据。

## 3. 日志异常

`~/.agents/logs/agent-memory.log` 中观察到：

- `Reflection apply failed`：6 次；
- `memory_reflection_above_target`：17 次。

已出现的失败原因包括记忆 ID 格式不符合领域约束，例如“记忆 ID 必须是 UUID”。这表明 LLM 输出解析与 Memory 领域约束之间仍存在输入兼容风险。

同时，事件流中的全量 succeeded 与普通日志中的 apply failed 尚未形成一份统一失败账本，不能只根据 `reflection_applied` 计数推导真实反思成功率。

## 4. 当前测试覆盖

主要测试路径：

- `agent/features/memory/src/domain/reflection_tests.rs`
- `agent/features/memory/src/application_tests.rs`
- `agent/features/memory/src/write_lock_port_tests.rs`
- `agent/features/runtime/src/application/reflection/execution_tests.rs`
- `agent/features/runtime/src/application/reflection/task_tests.rs`
- `agent/features/runtime/src/application/loop_engine/chat/reflection_*`

已有测试重点覆盖：

- prompt 构建；
- output 解析；
- apply 行为；
- 写锁；
- busy skip；
- trigger；
- cancellation；
- runtime task 生命周期；
- pre-compact、manual、interval 触发链路。

这些测试对协议和生命周期正确性有价值，但不是反思内容质量评测。

## 5. 尚未覆盖的质量指标

| 指标 | 目的 |
|---|---|
| JSON/schema parse success rate | 判断 LLM 输出是否可消费 |
| suggestion validity rate | 判断建议是否满足 Memory 约束 |
| supersede precision/recall/F1 | 判断新旧记忆关系是否正确 |
| outdated precision/recall/F1 | 判断过期判断是否正确 |
| synthesis quality | 判断合并后是否保留关键事实 |
| no-op rate | 判断无变化场景下是否避免无意义写入 |
| capacity target pass rate | 判断反思后是否回到 active≤150 |
| apply success rate | 判断解析成功后能否最终落盘 |
| output stability | 判断固定输入重复运行时结果是否稳定 |
| downstream retrieval gain | 判断反思后召回质量是否提升 |

## 6. 容量和并发评估

当前设计目标包括：

- Reflect 全程持 corpus 写锁；
- 同一 corpus 已有 Running reflect 时跳过；
- 成功后 active 数量应降到 150 以下；
- 未达目标时允许补问一次；
- 第二次仍失败则整个批次不写入；
- Interval 异步执行，PreCompact/Manual 同步执行。

现有日志已经出现 active=198/199 的 above-target 记录，因此容量收敛是当前最值得持续观测的运行指标。

需要区分：

- `reflection_applied`：流程应用事件；
- `active≤150`：容量目标是否达成；
- `supersede_synthesis` / `mark_outdated`：内容关系变更；
- 反思质量：这些变更是否正确。

四者不能互相替代。

## 7. 建议

- 新增离线 reflection golden fixtures：初始记忆、消息、期望新增记忆、supersede/outdated 集合和容量结果。
- 对固定输入重复运行，测量输出稳定性和 schema 成功率。
- 对建议、合并和过期判断建立人工标注，计算 precision/recall/F1。
- 统一记录 reflection terminal、repair round、lock wait、duration、token cost 和最终失败原因。
- 统计反思前后 memory recall 质量和 downstream task gain。
- 将 active≤150 的容量目标达成率纳入回归门禁。

## 8. 评估结论

Reflect 的流程可靠性基本具备，触发、执行、应用、写锁和任务生命周期已有代码、测试与运行事件证据；但内容质量尚未评估。当前应定义为“运行链路已落地，质量评测尚未闭环”。
