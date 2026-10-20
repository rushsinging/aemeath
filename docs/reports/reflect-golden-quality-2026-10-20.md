# Reflect Golden Fixtures 与质量门禁报告

> 日期：2026-10-20
> Issue：#1999（父 #1988）
> 基线：main @ 3c614bf71（#2002 反思链路统一异步已合入）

## 1. 结论

1. **Rust golden 层落地**：5 个 fixture 场景走完整 `ReflectionWorkflow::complete` 链路（解析 → 引用解析 → apply → record 落盘），固定 LLM 响应文件 + 冻结 corpus，进 CI 确定性通过。修复过程中抓到一个真实 bug：`active_after` 曾复用容量门禁的试算旧值（supersede/归档未发生的时点值），已改为 apply 完成后盘面重算。
2. **可观测补缺（C）**：`ReflectionApplyResult` 新增 `active_after/capacity_target`，`ReflectionRecord` 新增 `stale_swept`；`reflection_cost` 事件 summary 携带容量注脚。收敛率从「不可算」变为「record/事件直接可读」。
3. **真实历史质量基线（B）**：236 条历史反思 record 折叠后，parse 成功率 80.1%，supersede 建立率 100%，outdated 标记率 95.8%，建议落盘率 71.8%。
4. **精度边界**：旧 record 无逐条人工 gold，本报告的 validity 是结构代理口径（引用可解析 + apply 计数一致），**不是内容正确性的 precision/recall**——后者需人工标注，未在本期范围。

## 2. Golden Fixtures（A 层，进 CI）

`agent/features/memory/src/reflection_golden_tests.rs` × `tests/fixtures/reflection/*.json`：

| 场景 | fixture | 断言要点 |
|---|---|---|
| 正常归纳 | synthesis_normal | 建议+supersede+outdated 全生效；active_after=1（有效口径）；cost 事件含容量注脚 |
| 无变化 no-op | synthesis_noop | attempted=0、盘面不变、Succeeded |
| 非法引用 | synthesis_invalid_reference | 坏 slug 跳过不计数、好引用生效 |
| 容量超限 | synthesis_capacity_breach | Failed(CapacityNotMet)、整批不写、盘面不变 |
| supersede 越界 | synthesis_supersede_cycle | 好引用建立、越界引用跳过、建议本体落盘 |

supersede 成环（M9）由 service 层单测既有覆盖，golden 不重复。repair/空语料兜底由 #2002 的 runtime execution 层覆盖。

## 3. 历史质量基线（B 层，236 条 record）

| 指标 | 值 | 说明 |
|---|---:|---|
| parse_success_rate | 80.1% | 失败分布：timed_out 23 / parse 11 / empty 5 / cancelled 4 / llm_call 1 / apply 3 |
| suggestion_validity（代理） | 71.8% | 418 条建议 → 300 条落盘 |
| supersede_established_rate | 100% | 56/56 引用全部建立 |
| outdated_marked_rate | 95.8% | 167 条引用 → 160 条生效 |
| duration p50 / p95 / max | 37.6s / 120s / 291s | 超时类失败与此吻合 |
| input tokens p50 / p95 | 17.1K / 40.7K | |
| output tokens p50 / p95 | 2.0K / 12.1K | |

容量新字段（active_after/capacity_target/stale_swept）在旧 record 覆盖率 0——预期：C 的字段只对今后反思生效，历史收敛率只能靠外部快照对账。

## 4. 主要发现

1. **timed_out 是最大失败类**（23/47 失败）：duration p95=120s 与默认 600s 超时并存说明有长尾请求超时——反思模型配置（#2002 已调 High/64000）后是否缓解需新数据验证。
2. **parse 失败 11 次**：repair 机制已吸收大部分（#2002 两阶段修复），残余 4.7% 失败率。
3. **supersede 建立率 100%**：引用解析（M1/M2 行号制）质量可靠。
4. **建议落盘 71.8%**：约 28% 建议未落盘（非法内容/容量/CapacityNotMet），其中容量整批不写占多少需新字段数据。

## 5. 复现

```bash
cargo test -p memory --lib reflection_golden    # A 层 golden
cd eval/reflection
python3 harness/build_reflection_dataset.py --output datasets/reflection_records.jsonl
python3 harness/evaluate.py --dataset datasets/reflection_records.jsonl --output results/quality_baseline.json
```

## 6. 未覆盖（如实声明）

- 内容正确性 precision/recall/F1（需人工 gold 标注）
- downstream task gain
- 输出稳定性 N 次重放（明确不做，D 项）
- 新容量字段的历史收敛率（需上线后新 record 积累）
