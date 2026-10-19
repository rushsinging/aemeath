# Memory Recall 与 Injection 端到端质量评测报告

> 日期：2026-10-19
> Issue：#1998（父 #1988）
> 评测对象：通道 A（memory-context system block）与通道 B（per-message recall 门控注入）
> 工具：`eval/memory/harness/replay_recall.py`、`eval/memory/harness/analyze_channel_a.py`

## 1. 执行摘要

1. **通道 B 门控四象限（60 真实 case，kev 在线重放）**：miss_inject=45（75%）、correct_inject=1、wrong_inject=3、correct_reject=11/13。结合 #1997 的 R@1=0.34，per-message 注入当前几乎不产生有效注入——门控 0.5 阈值在多候选下的结构性失效（#1997 发现）在四象限口径下被完整量化。
2. **stale 污染修复（本 PR 生产代码）**：`MemorySearchQuery.exclude_stale` 落地。修复前 60 case 候选中含 90 条 outdated/superseded 条目、10 个 case 词法 top1 即过期条目；修复后注入链路不再召回过期条目。记忆桶 active=183 中 88 条（48%）为 stale——**mark_outdated 只标标志不移出 active**，近半存储被过期条目占据。
3. **通道 A 质量属性（当前桶快照）**：eligible 95 全量注入（limit 200 未截断），stale 泄漏=0（`is_injection_eligible` 已过滤，M5/M10 兜底有效）；pinned 6 条全部占据 top10；注入总量 29,194 字符——每会话 system block 携带约 29K 字符记忆，是上下文成本的常量项。

## 2. 通道 B 四象限（kev 重放）

重放口径：#1997 数据集 60 case（候选=生产词法 top-N 原文，gold 含 13 条 no-relevant），BM25 层用 `lexical_baseline`（生产同口径），kev 层经 8009 在线打分。

| 象限 | 定义 | n | 占比 |
|---|---|---:|---:|
| correct_inject | gold=-1 之外、top1=gold 且 p≥0.5 | 1 | 1.7% |
| wrong_inject | p≥0.5 但 top1≠gold（含负例误放） | 3 | 5.0% |
| **miss_inject** | **有 gold 但被门控拒绝或选错** | **45** | **75.0%** |
| correct_reject | 无相关候选且 p<0.5 | 11 | 18.3% |

分层事实：

- 词法 top1 命中 gold 仅 23.4%，kev 重排 R@1=34%（#1997）——重排对词法有 +11pp 增益，但绝对水平仍低。
- 负例拒绝 11/13（84.6%）良好——通道 B 的问题集中在「该注入时注不进」，不是「不该注入时乱注入」。

## 3. stale 污染与修复

### 3.1 修复前污染（量化）

- 60 case 的 533 个候选条目中，90 条（16.9%）在当前桶状态为 outdated/superseded；
- 10 个 case（16.7%）的词法 top1 即过期条目——过期条目在词法与重排两层都在挤占有效记忆名额；
- 记忆桶 active=183 中 88 条 stale（48%）：`mark_outdated`/反思标记过期后条目仍留在 active 列表，仅 `exclude_stale` 视角与通道 A 的 `is_injection_eligible` 将其排除。

### 3.2 修复内容（本 PR）

- `MemorySearchQuery` 新增 `exclude_stale: bool`（默认 false，显式搜索保持「过期可见+标注」契约）；
- `recall_relevant_inner` 传 `exclude_stale: true`：过期条目不再进入词法候选，不占召回与重排名额（根因级，优于召回后过滤）；
- 测试：`search_exclude_stale_drops_ineligible_entries_from_candidates`（service 层两态行为）+ `recall_relevant_excludes_stale_entries_from_injection`（注入链路名额不被过期条目挤占），memory 170 全绿、runtime 536、tools 63、context 5 无回归。

## 4. 通道 A 质量属性（当前桶快照）

| 属性 | 值 |
|---|---:|
| active 总数 | 183 |
| eligible（注入视角） | 95 |
| stale（outdated/superseded） | 88（48%） |
| 实际注入（limit 200） | 95（未截断） |
| stale 泄漏 | 0 |
| pinned 注入 | 6（全部进入 top10） |
| 注入总字符 | 29,194 |
| category 分布 | decision 41 / pitfall 23 / pattern 15 / fact 10 / preference 6 |

结论：通道 A 的过滤与排序兜底有效；主要风险是**全量注入的上下文常量成本**（29K 字符/会话）与 active 中 48% 的 stale 占比对容量口径的挤压（指向 #1999 反思收敛）。

## 5. 后续建议

1. **门控阈值重设计**（最高优先，属生产代码变更）：0.5 绝对阈值在 ≥4 候选下机制性失效；建议改候选数自适应阈值或 top1-top2 相对判据。数据支撑：四象限 miss_inject=75%。
2. **过期待收敛**（指向 #1999）：active 48% stale 说明「标记过期但不出 active」的存储口径需要反思闭环收敛或迁移策略。
3. **通道 A 成本治理**：29K 字符常量注入建议做按需分档（如 top-K by score 或会话相关性过滤），属独立 issue。
4. **数据集续用**：本评测复用 #1997 数据集；桶快照漂移（gold 前缀不可寻率 66%）表明跨期重放需固定桶快照，建议后续评测随数据集附桶快照。

## 6. 复现命令

```bash
# 通道 B（BM25 层离线必跑；kev 在线时叠加四象限）
cd eval/memory && python3 harness/replay_recall.py

# 通道 A（离线）
python3 harness/analyze_channel_a.py

# 修复验证
cargo test -p memory --lib   # 含 exclude_stale 两测试
```
