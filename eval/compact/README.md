# Compact 链路评测（#2133）

对压缩（compact）链路做可量化评测：**压缩后上下文还够不够用、哪个 compact 模型性价比最好、改动是否回归**。

## 评测边界（MUST 如实呈现）

- **确定性指标不是端到端质量**：`must_keep` 召回度量「typed id / 文件路径是否还在 summary+tail 中」，是覆盖率代理，不判断语义等价、不判断模型能否续接任务。
- **P0 遥测只描述发生过什么**：频率、耗时、分块形状、降级；不评价摘要内容。日志未记录触发来源（自动/手动），因此不输出「自动压缩占比」。
- **历史日志字段缺口**：2026-10-28 之前的日志没有 `model=` / `quality=` / `elapsed_ms=`，分别归入 `unknown` 与「按开始/完成时间戳推算」，不与新日志混算。
- **判官不设默认**：语义判定（P3 续接问答）由 `--judge <provider/model>` 在评测时显式指定；未指定时只跑确定性指标，**不静默降级**为某个默认判官。

## 目录

```
eval/compact/
  harness/
    telemetry_baseline.py   P0：从 context.log 出压缩画像
    compact_snapshot.py     P1：解析 session dataset（content-addressed）还原压缩输入
    export_cases.py         P1：导出 case（messages.jsonl + case.json + must_keep）
    metrics.py              P1：确定性指标（召回/压缩率/结构健康）+ 边界声明
    test_metrics.py         P1：指标函数单测（分母、空集合、降级判定）
    run_eval.py             P1：按 case 汇总指标（需外部提供压缩输出）
  results/
    telemetry_baseline_2026-10.json  P0 真实画像（10 月）
    case_input_baseline.json         P1 输入侧基线（8 case 规模）
  datasets/                 本地导出的 case（体积大，不入库）
```

## 用法

```bash
# P0：真实日志画像
python3 eval/compact/harness/telemetry_baseline.py \
    --log ~/.agents/logs/context.log --since 2026-10-01 \
    --output eval/compact/results/telemetry_baseline_2026-10.json

# P1：导出 case（真实 session 快照）
python3 eval/compact/harness/export_cases.py \
    --root ~/.agents/session --limit 8 --output-dir /tmp/compact-cases

# P1：指标单测
cd eval/compact/harness && python3 -m unittest test_metrics

# P1：按 case 汇总（需要压缩输出；文件名 <case_id>.summary.txt）
python3 eval/compact/harness/run_eval.py \
    --cases /tmp/compact-cases --summary-dir /tmp/compact-summaries \
    --output eval/compact/results/run_eval.json
```

## 阶段状态

| 阶段 | 内容 | 状态 |
|---|---|---|
| P0 | 遥测基线（频率/耗时/分块/降级） | ✅ 已落地，10 月真实数据已入库 |
| P1 | case 导出 + 确定性指标 + 单测 | ✅ 已落地；**压缩输出执行面待接**（见下） |
| P2 | 多模型矩阵 + REPORT | ⏳ 待 P1 执行面 |
| P3 | 续接问答 e2e（答题模型固定主模型、判官 `--judge` 指定） | ⏳ |
| P4 | 回归门禁（确定性指标不劣于基线） | ⏳ |

### P1 执行面缺口（必须补齐才算完成）

`run_eval.py` 目前从 `--summary-dir` 读取压缩输出，**没有自己跑压缩**。生产路径应改为隐藏 CLI 子命令（`aemeath eval compact --case … --model …`），走真实 runtime 装配，避免测试简化导致假绿。该子命令未落地前，P2/P3 不出正式结论。

## 输入侧基线（8 case，真实 session）

| 指标 | p50 | p95 | max |
|---|---|---|---|
| message_count | 597 | 8075 | 8075 |
| estimated_tokens | 479831 | 2263883 | 2263883 |
| must_keep（上限 200） | 200 | 200 | 200 |

## P0 画像（2026-10 真实日志）

| 指标 | 值 |
|---|---|
| 压缩次数 | 122（23 个活跃日） |
| 单次耗时 | p50 20s / p95 77s / max 758s |
| 触发时 early_tokens | p50 18.6 万 / p95 54.2 万 |
| map 分块 | 112 次走 map；chunks p50 6 / p95 15 / max 26 |
| 降级块数累计 | 195 |
| 模型分桶 | 全部 `unknown`（历史日志无 `model=`；新日志起才可归桶） |

## 判官口径（P3）

- `--judge <provider/model>`：评测时指定，代码与配置不写默认值。
- 必须**跨族**：不得与压缩候选模型重叠（避免 self-preference bias）。
- `k=3` 多数投票，每次判定要求引用摘要原文片段作证据。
- 落地前用 20–30 条人工 gold 校准，一致率 **≥ 0.9** 才可用；与第二判官分歧 > 10% 时先改 rubric。
- 判官空响应/失败计为「judge 不可用」，不得计入「压缩失败」。
