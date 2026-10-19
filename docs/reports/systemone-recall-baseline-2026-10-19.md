# System One memory_recall 真实质量基线报告

> 日期：2026-10-19
> Issue：#1997（父 #1988）
> 引擎：kev-0.8b（MLX bf16，本机 8009）
> 数据集：`eval/system-one/datasets/memory_recall_real.jsonl`（60 case，seed=1997 分层抽样）

## 1. 背景与构建方法

`memory_recall` 是门控+注入场景（top1 p<0.5 不注入，预算 1200 字符），与 `memory_rerank`（排序）语义不同。此前线上事件（343 条 ok / 321 unique）没有对应 gold 数据集。

构建流程（`harness/build_memory_recall_real.py`，幂等可复现）：

1. 读取 `~/.agents/scoring/events/*.jsonl`，过滤 `scenario=memory_recall` 且 `outcome=ok`；
2. 按 `prompt_sha256` 去重（350 → 321），排除 0 候选（12 条）；
3. 固定 seed=1997 分层抽样：40 条满 10 候选 + 20 条 2-9 候选；
4. case schema 与 `memory_rerank_real` 对齐（context/question/answers/gold）；
5. gold 由语义标注产生（分批人工判读 + 低置信标记）：47 条有效 gold + 13 条无相关候选（gold=-1，正确行为是拒绝注入）。

标注记录：`results/annotation/memory_recall_real_labels.jsonl`（60 条，39 条 low confidence 供后续抽检）。

## 2. kev 基线结果

`python3 harness/score.py --gate kev memory_recall_real`：

| 指标 | 值 | 门禁 | 判定 |
|---|---:|---|---|
| n_forward | 60 | — | — |
| no_relevant_n | 13 | — | — |
| R@1 | 0.340 | ≥ 0.60 | **违规** |
| MRR | 0.536 | ≥ 0.70 | **违规** |
| order_flip_rate | 0.700 | ≤ 0.30 | **违规** |
| correct_reject_rate | 0.923 | ≥ 0.50 | 通过 |
| chinese_r_at_1 | 0.341 | — | — |
| errors | 0 | — | — |

## 3. 三层质量对比（核心结论）

| 层 | 场景 | R@1 | flip | 说明 |
|---|---|---:|---:|---|
| 合成集 | memory_rerank（12 case） | 1.00 | 0.00 | 人工构造、主题明确 |
| 真实排序 | memory_rerank_real（20 case） | 0.60 | 0.35 | 真实消息 × 词法 top5 |
| **真实门控** | **memory_recall_real（60 case）** | **0.34** | **0.70** | 真实消息 × 词法 top10，含负例 |

结论：**合成集完全不能外推真实场景**。同一引擎从合成 100% 跌到真实门控 34%，且顺序翻转率 70% 说明 kev 在真实候选集上概率区分度严重不足。

### 3.1 失败归因

47 条有效 case 的 gold 在模型排序中的名次分布：

```text
rank1=16  rank2=7  rank3=6  rank4=9  rank5=3  rank6=2  rank8=3  rank9=1
```

错误分散在 rank2-9，无固定偏移——不是系统性把 gold 排到某位置，而是候选间概率差距过小（线上观测 top1-top2 gap p50 仅 0.038）。

### 3.2 correct_reject_rate 反而良好

13 条无相关候选中 12 条 top1 p<0.5 被正确拒绝（92.3%）。负例拒绝不是问题；**正例选择与门控放行才是问题**。

## 4. 线上门控行为：结构性失效（重要发现）

对 321 条 unique 线上事件按候选数分层的 top1 概率：

| 候选数 | n | top1 p50 | top1 max | 通过 0.5 门控 |
|---:|---:|---:|---:|---:|
| 2 | 13 | 0.634 | 0.791 | 13 |
| 3 | 13 | 0.476 | 0.682 | 4 |
| 4-9 | 60 | 0.224-0.374 | 0.277-0.490 | **0** |
| 10 | 223 | 0.183 | **0.359** | **0** |

**满候选（≥4）的 memory_recall 请求在当前 0.5 阈值下 100% 被拒绝注入**。10 候选时 softmax 概率被稀释，top1 max 仅 0.359，结构性无法过 0.5——不可能 223 条满候选请求全部无相关记忆。这是「概率随候选数稀释 × 固定绝对阈值」的机制性矛盾，不是模型质量问题。

该发现指回 Memory 子任务（#1998）：门控阈值需按候选数自适应或改用相对判据（top1-top2 gap、归一化分差）。

## 5. 合成集与真实集差异归因

1. **query 形态**：真实消息大量为短指令/指代性消息（「继续」「修复，继续」「同意，abce」），主题信号弱；合成 query 主题完整明确。
2. **候选噪声**：真实候选含大量跨域记忆（60 条中 13 条无相关候选），干扰概率分布；合成候选同域近义。
3. **候选长度**：真实候选最长 500 字符（p50 264），信息密度低于合成短句。
4. **instruct 定向**：RERANK_INSTRUCTIONS 面向「retrieve the most relevant memory」，对指代性短消息的判别帮助有限。

## 6. 建议

1. **门控阈值重设计（优先，归 #1998）**：固定 0.5 绝对阈值在多候选下机制性失效；候选应按候选数自适应，或改 top1-top2 相对判据。
2. **memory_recall 当前不应作为注入决策的唯一依据**：真实 R@1=34%，选错注入比不注入更伤上下文；建议在门控修复前维持保守（少注入）。
3. **数据集扩充与抽检**：60 case 中 39 条 low confidence，建议人工抽检后扩到 100+；负例占比（22%）应保持。
4. **skill_match 结果文件历史不一致**：`results/kev/skill_match.jsonl` 与数据集 gold 前缀不匹配（引擎输出剥前缀），重算 summary 显示 R@1=0%，属历史遗留非本次引入；后续跑 skill_match 时重跑覆盖即可修复。

## 7. 复现命令

```bash
cd eval/system-one
harness/serve.sh start kev && harness/serve.sh wait kev
python3 harness/build_memory_recall_real.py   # 重建数据集（幂等）
python3 harness/fill_gold.py <labels.jsonl>   # 回填标注
python3 harness/run_eval.py --engine kev --scenario memory_recall_real
python3 harness/score.py --gate kev memory_recall_real
```
