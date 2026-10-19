# Memory Recall 与 Injection 双通道评测报告

> 日期：2026-10-19
> 范围：双通道影子评测、失效记忆过滤修复与回归证据。
> 本报告替代早期草稿中“候选即实际注入”“跨项目历史样本可直接映射当前桶”的推断。

## 1. 结论与证据边界

- 生产 `recall_relevant` 经 `MemorySearchQuery.exclude_stale=true` 在词法排序前排除 outdated、superseded、TTL 失效条目；显式搜索传 false，仍能查看旧条目及状态。
- 通道 A 原本已有适格性过滤，本次没有重复增加生产过滤器。
- 双通道脚本现在基于 manifest 引用成员冻结的 global+project 快照，不再按 members 文件 mtime 猜当前代。
- 通道 B 是**原候选集内的影子回放**，不是包含全 corpus BM25、双次评分、真实 reminder pipeline 和最终 LLM 输出的端到端测试。
- 历史数据集没有全部候选的相关性标注、完整 corpus 真值与下游任务对照，因此 corpus Recall@20、完整注入 precision、downstream task gain 记为 `null`，不得以 0 或代理数值替代。

## 2. 数据与可复现性

输入：

- `eval/system-one/datasets/memory_recall_real.jsonl`：60 个历史候选集，47 个单一 gold，13 个候选集内无相关项。
- `eval/system-one/results/annotation/memory_recall_real_labels.jsonl`：21 条 high、39 条 low；由 LLM 预标注，未完成用户抽检，质量数字仅为探索性基线。
- `eval/memory/datasets/injection_snapshot.json`：global 8 条、aemeath project 183 条 active，记录采集秒级时间、corpus revision、内容摘要。
- `eval/memory/results/replay_recall.json`：保存每个样本概率和失败/跳过状态，以及输入 SHA-256；可不联网用 `--responses` 复算。

快照加载遵循 `primary/manifest.json` 的 active 内容寻址引用，校验字节数与带领域前缀的存储摘要；采集前后 manifest 变化则报错，避免拼接不同代。

历史样本来自多个项目；本次快照只包含 global+aemeath，因此候选不可寻可能是其他项目、归档、改写或版本差异，**不能统一归因为 reflection 改写**。只有完整文本相等，或达到生产 500 字截断上限且唯一前缀匹配，才视为可对齐；歧义与不可寻均记 unknown。

## 3. 通道 B：候选集内评分与门控

HTTP kev（MLX bf16、8009）的复跑结果，**不等同于生产 UDS Q8 daemon**：

| 过滤前候选集级结果 | 数量 | 定义 |
|---|---:|---|
| correct_selection | 1 | top1 为 gold 且概率达到 0.5 |
| wrong_selection | 1 | 有 gold，但高概率选了别项 |
| false_selection | 2 | 候选集无相关项，却达到门控阈值 |
| missed_selection | 45 | 有 gold，但门控未放行 |
| correct_rejection | 11 | 候选集无相关项且门控拒绝 |

60/60 评分成功，0 scoring error，4 个样本通过门控。拆分 wrong_selection 与 missed_selection，避免把“高置信选错”隐藏在漏注入中。

模拟 runtime 的 top3、每条 preview 200 字符、1200 字符预算后，过滤前选中内容合计 1496 字符（全样本总和）；这是 preview 内容的影子成本，不包含最终 envelope/token 物化。

过滤后：53/60 个样本因候选无法完整映射冻结快照而跳过，只有 7 个可比较样本（0 error）。其结果为 correct_rejection=3、false_selection=1、missed_selection=3。两个阶段分母不同，**不能据此宣称过滤提升或降低真实质量**；过滤后必须重新评分，不能删除旧概率后再当新 softmax。

BM25 只在事件给出的候选子集中近似重算。缺失历史 tags/category/confirmation 等元数据时标记 text-only approximation，不能将其与生产全 corpus 的排序相比并宣称重排收益。历史 gold=-1 也不证明整个 corpus 无相关记忆。

## 4. 通道 A：检索候选属性，不是实际 system block 大小

冻结快照包含：

| 属性 | 数值 |
|---|---:|
| global+project active | 191 |
| eligible | 103 |
| 不可注入（去重计数） | 88 |
| outdated 标记 | 78 |
| superseded 标记 | 23 |
| TTL 失效 | 0 |
| 检索候选（limit=200） | 103 |
| 候选内容字符和 | 30,310 |
| pinned 覆盖 | 8/8 |
| eligible decision/preference 覆盖 | 53/53 |

outdated 与 superseded 可重叠，不能相加作失效条目总数。shadow 排序对齐 synthesized 组优先、组内 injection_score 降序及 ID tie-break；TTL 使用与 Rust 相同的严格过期边界，pinned 不绕过失效过滤。

候选层 category：decision 43、pitfall 24、pattern 16、fact 10、preference 10。精确重复对数和字符 bigram Jaccard≥0.85 的近重复对数均为 0，但这不证明没有语义重复。

Context 在检索后还会按窗口 token 预算、归纳结论 evidence 覆盖与两段填充物化 system block。因此 **30,310 字符是候选原文总量，不是每会话实际注入成本**。最终 system block 字符与 token 成本因缺物化证据记 `null`；过滤后零失效只是 shadow 公式结果，生产行为由 Rust 回归测试证明。

## 5. 生产修复与回归边界

- `MemorySearchQuery.exclude_stale` 是显式布尔字段，无自动默认；既有显式调用点均传 false，recall 传 true。
- 在 `MemoryService.search` 词法排序及 scorer 之前调用现有 `is_injection_eligible`，避免先取 top-N 后过滤导致名额被旧条目挤占。
- 增强 service 两态、recall 查询传递与 runtime reminder 内容断言，覆盖有效项存在、失效项不存在和 pinned 不绕过等边界。
- `mark_outdated` 操作和 reflection 标记过期不能一概而论：不同写路径是否归档由实现决定。active 有失效项本身不能证明所有反思路径都只标记不归档。

## 6. 未完成的质量证据

完整注入相关性、历史 corpus Recall@20、token 实测与 downstream task gain 仍需要独立标注和真实任务对照。低置信标注不应直接用作严格放行门禁；本次不修改概率门控阈值，不处理延迟或权限。

最小止血是召回后丢弃失效结果，成本低但已占用 top-N/scorer 名额。本次采用根因方案：候选入池前按注入适格性过滤，改动查询契约与构造点，成本略高但保留显式搜索兼容并消除名额挤占。评测侧同样采用冻结输入+错误/未知显式计数，而非沿用动态桶及静默跳过。

## 7. 复现

从仓库根运行：

```bash
PYTHONDONTWRITEBYTECODE=1 python3 eval/memory/harness/replay_checks.py
PYTHONDONTWRITEBYTECODE=1 python3 eval/memory/harness/channel_a_checks.py
python3 eval/memory/harness/analyze_channel_a.py --snapshot eval/memory/datasets/injection_snapshot.json --output /tmp/channel-a.json
python3 eval/memory/harness/replay_recall.py --snapshot eval/memory/datasets/injection_snapshot.json --responses eval/memory/results/replay_recall.json --output /tmp/replay-cached.json
```

若需重新评分，改用 `--kev-url http://127.0.0.1:8009`，不能与 `--responses` 同传。未启用评分时报告 `scoring_skipped`；启用后请求/响应失败记 error，并以非零退出，绝不把错误算作正确拒绝。
