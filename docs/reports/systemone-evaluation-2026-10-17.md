# System One 评测报告

> 评估日期：2026-10-17
> 评估范围：当前仓库中的 System One 评测 harness、设计文档、daemon 事件流、运行日志与既有结果文件。
> 证据边界：本报告使用已有结果和日志，未重新启动 daemon 或执行 Cargo 测试。

## 1. 结论

System One 的工程链路已基本闭环，但业务质量尚未全部达到既定门禁。

- daemon、UDS、事件落盘、fail-open、候选模型对比和真机 parity 均已有实现或结果。
- 线上事件共 549 条，548 条成功、1 条不可用，总体可用率约 99.82%。
- `memory_rerank` 合成集表现优秀，但真实会话集明显下降。
- `memory_recall` 是线上主要场景之一，但当前没有对应离线 gold 数据集。
- `permission_triage` 当前保持关闭是合理的：kev 未达到质量门槛，rsi-jev 可作为后续复验候选。

## 2. 评测机制

主要入口：

- `eval/system-one/harness/run_eval.py`
- `eval/system-one/harness/score.py`
- `eval/system-one/REPORT.md`
- `agent/features/systemone/tests/`
- `~/.agents/scoring/events/*.jsonl`

当前分为三层：

| 层级 | 内容 | 状态 |
|---|---|---|
| 离线候选对比 | 多引擎、多个场景、正反序测试 | 已有 |
| Rust/真机 parity | GGUF、Rust adapter、数值对拍 | 已有 |
| 真实运行观测 | daemon 事件、延迟、不可用、引擎版本 | 已有，但尚未自动生成质量报告 |

既有 rank 门禁包括：`memory_rerank` 的 R@1/MRR、order-flip、p95 和中文 R@1；`skill_match` 使用更严格的 p95 门槛。

## 3. 主要离线结果

当前主力候选 kev：

| 场景 | 结果 | 判断 |
|---|---:|---|
| 合成 memory rerank | R@1=1.00，MRR=1.00，flip=0，p95=79.5ms | 通过 |
| 真实 memory rerank | R@1=0.65，MRR=0.792，flip=0.30，p95=520ms | 质量不通过 |
| skill match | R@1=1.00，flip=0，p95=67.5ms | 通过 |
| stop verify | accuracy=1.00，F1=1.00，p95=55ms | 通过 |
| permission triage | accuracy=0.688，F1=0.706，risk MAE=0.637 | 不通过 |

候选对比显示：

- qwen3-reranker 的真实 rerank R@1 同为 0.65，但 flip=0.05，仍未达到 0.02 门槛。
- semif 的真实 rerank R@1=0.75，但 flip=0.65，顺序敏感性较高。
- rsi-jev 的 permission triage accuracy=0.813、risk MAE=0.404，适合后续权限预筛复验。

## 4. 线上事件结果

数据目录：`~/.agents/scoring/events/`。

| 场景 | 调用数 | 成功数 | p50 | p95 | 最大值 |
|---|---:|---:|---:|---:|---:|
| memory_recall | 232 | 232 | 854ms | 1.27s | 4.61s |
| memory_rerank | 292 | 291 | 1.53s | 6.36s | 30.03s |
| skill_match | 25 | 25 | 13.92s | 20.14s | 20.74s |

线上全部使用 `kev-0.8b-q8-r1`。`agent-systemone.log` 当前为空，实际有效证据位于 scoring 事件目录。

## 5. 主要缺口和风险

1. **线上延迟显著高于离线延迟。** kev 离线 rerank p50 约 54ms，线上约 1.53s；skill match 离线 p50 约 45ms，线上约 13.92s。需要排查 daemon 排队、冷启动、连接、串行 worker、重试以及 latency 计时边界。
2. **真实 rerank 质量不达标。** 合成集不能代表真实记忆场景。
3. **评测集与线上场景不匹配。** 线上 `memory_recall` 有 232 条事件，但 `eval/system-one/datasets/` 没有 `memory_recall.jsonl`。
4. **事件流尚未自动转化为质量报表。** 目前有现场，没有统一的 p95、unavailable 率和按场景回归门禁脚本。

## 6. 建议

- 从线上 `memory_recall` 事件抽样并人工补 gold，建立 `memory_recall.jsonl`。
- 新增只读事件汇总脚本，输出调用量、成功率、不可用原因、p50/p95、引擎版本和超时样本。
- 用真 daemon 集成测试复核离线与线上延迟差异。
- 在权限预筛重新选用 rsi-jev 复验前，不打开生产开关。

## 7. 评估结论

工程可用性通过；业务质量部分通过。合成 rerank、skill match、stop verify 已有正向证据，真实 rerank 和 permission triage 未达到门禁，memory recall 尚无独立质量结论。
