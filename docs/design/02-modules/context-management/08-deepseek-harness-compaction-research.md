# Context Management · DeepSeek Harness 上下文压缩对照调研

> 层级：02-modules / context-management（外部对照调研）
> 状态：Research（调研，非生产契约）
> 作用：为 Compact 家族（尤其是工具结果语义压缩与 Future 持久化增量摘要树）提供外部对照；**不**替代 [02-compact.md](02-compact.md) / [06-persistent-summary-tree.md](06-persistent-summary-tree.md) 的生产真相。

## 1. 范围与非目标

### 1.1 范围

- 梳理 DeepSeek Harness（以下简称 DSH）compaction 能力族如何缩短模型可见 Surface。
- 对照 aemeath Compact 五级管线、当前 L5 持久化边界，以及 Future 持久化增量摘要树。
- 提炼可借鉴项与不宜照搬项，供后续设计拍板引用。

### 1.2 非目标

- **NEVER** 把本文当作 runtime / context 的实现规格或验收门禁。
- **NEVER** 因本文结论在缺少实测成本与保真数据时启动摘要树代码实现。
- **NEVER** 用 DSH「只有两招」否定 aemeath 已规划的 L2–L4 读模型变换。

### 1.3 上游来源（核对基线）

- 子系统说明：`deepseek-ai/deepseek-harness` → `docs/subsystems/compaction.md`
- 包族：`packages/compaction/`（`compaction`、`compaction-basic`、`compaction-tool-result-pruner`、`command-compact`、`compaction-image-offload`）
- 独立解读（非上游权威）：[Compaction and Context Pressure](https://hoco-scy.github.io/deepseek-harness-deep-dive/pages/compaction.html)

## 2. DSH 怎么做

### 2.1 能力 seam，不是 loop 主干

DSH 把 compaction 做成可选能力 seam，结构类似 bash：

| 角色 | 包 | 作用 |
|---|---|---|
| Service Definition | `dsh-compaction` | `ctx.compaction` 契约 |
| Service Provider | `dsh-compaction-basic` 等 | 自动 / 溢出恢复 / 按需摘要 |
| Consumer | `dsh-command-compact` | `/compact` |
| 辅助 | `dsh-compaction-tool-result-pruner`、`dsh-compaction-image-offload` | 先剪工具结果；图片超预算占位 |

依赖会话与 LLM 词汇（`Session`、`ContentBlock`），并由独立 `tokenMeter` 为压力决策定价。

### 2.2 真正缩短 Surface 的只有两套变换

生产路径**没有**固定的 L1/L2/L3 压缩流水线。可观察的缩短机制是：

1. **工具结果剪枝（确定性）**  
   对超大 tool result **砍中间、留头尾**，落成可回放的 session 替换；保留 call id / name / 错误等语义，不靠 LLM。

2. **历史摘要替换（一次额外模型请求）**  
   把一段「平衡的历史前缀」压成 checkpoint 摘要，写入模型可见的普通 user 消息；**只保留摘要文本**。checkpoint 之后仍是普通 Surface 节点，后续压缩可以再把它纳入下一轮摘要——「层级感」来自递归组合，而不是配置了 N 级流水线。

压力仍高时的经验顺序：**先 prune，再摘要**。

### 2.3 两条触发路径（不是两级压缩）

| 路径 | 含义 |
|---|---|
| 压力路径 | token 接近窗口上限时自动压缩 |
| 溢出路径 | 收到规范错误 `CONTEXT_WINDOW_EXCEEDED` 后先压缩再重试 |
| 手动 | `/compact`（与进行中 turn 串行） |

默认量级（`compaction-basic`，可按模型覆盖）：

- `thresholdRatio: 0.8`
- `retainRatio: 0.16`（近期尾部保留预算相对 `W − O`）
- 阈值形态近似：`floor(min(W × 0.8, W − O − B))`，余量 `B = 65,536` tokens
- `compactionRetries` 默认允许有限次溢出后恢复

### 2.4 事务、计量与硬边界

- 压缩是日志事务：`compaction/start` → 摘要 → `compaction/summary` + 替换 → 闭合；失败有 `summary-error`。
- 决策统一走 token meter；启发式约 **4 字符 / token**（对 CJK / JSON Schema 易低估，上游亦标为暂缓精确 tokenizer）。
- **压不掉**：系统提示、工具定义、会话前缀，以及单个不可拆单元（例如一次超大工具调用本身）。

## 3. aemeath 对照

### 3.1 五级管线（设计真相）

以 [02-compact.md](02-compact.md) 为准：

| 级别 | 策略 | 成本 | 破坏性 |
|---|---|---|---|
| L1 | Budget reduction | 零 | 有（超限尾部不进链） |
| L2 | Snip | 零 | 无（读模型跳过） |
| L3 | Microcompact | 零 | 无（读模型变换） |
| L4 | Context collapse | 零 | 无（投影折叠，长期方向） |
| L5 | Auto-compact | LLM | 有（摘要替换历史） |

DSH 的「两招」大致落在 **L1/工具结果侧确定性压缩** 与 **L5 摘要替换**；aemeath 额外保留 L2–L4 的可逆读模型变换，这是刻意差异，不是缺省。

### 3.2 当前生产 vs Future 摘要树

| 形态 | 是什么 | 说明 |
|---|---|---|
| 当前 L5 边界 | `ActiveCompactMarker` + `run_slices` | 累计 summary + 从 marker 起投影；接近「单点摘要替换」，**尚未**长成树 |
| Future 摘要树 | [06-persistent-summary-tree.md](06-persistent-summary-tree.md) | 原始历史始终唯一真相；树是可重建派生索引；后台增量 shard + 临界 warm 激活 |

因此：「摘要树更强」指的是 **目标态在保真 / 增量复用 / 跨退出 / 多分辨率投影等维度的设计目标**，不是「当前实现已全面优于 DSH checkpoint」。

### 3.3 checkpoint 与摘要树各自强在哪

| | DSH checkpoint | aemeath 摘要树（目标态） |
|---|---|---|
| 本质 | 改可见 Surface：旧历史 → 一条摘要消息 | 不毁 canonical；写 sidecar / projection |
| 优点 | 简单、可递归、实现与心智成本低 | coverage 可审计、稳定前缀可预热、跨退出复用、多分辨率选节点 |
| 成本 | 细节随递归丢失；难证明「谁覆盖了哪段」 | scheduler / GC / coverage / 恢复路径复杂 |

**checkpoint 机制本身值得借鉴**；它与摘要树不是简单的上下位替代，而是「轻量替换语义」与「可审计派生索引」两条路线。

## 4. 可借鉴 / 不宜照搬

### 4.1 建议吸收

1. **先便宜确定性压缩，再花钱摘要**  
   工具结果侧语义压缩（RTK 风格 filter/group/truncate/dedup，或「砍中间留头尾」）应定位在 L5 之前；materializer 只做兜底。**NEVER** 再引入跨工具统一字符硬顶冒充 compact。

2. **压力与溢出分触发**  
   预判 compact 与「上下文溢出后 compact-then-retry」在文档与实现上显式分叉，避免混成同一级流水线语义。

3. **显式近期尾部保留预算**  
   类似 `retainRatio` 的可配置 retain（不必抄 0.16），让「压多少前缀 / 留多少尾」可观测、可按模型覆盖。

4. **可递归覆盖不变量**  
   无论单点 marker 还是摘要树：旧摘要必须能被后续摘要合法覆盖，且**禁止** previous_summary 原文线性累加膨胀。

5. **投影折叠 vs 摘要替换语义分离**  
   可逆读模型变换（L2–L4）与不可逆 L5 持久替换分开描述；Context collapse 不得做成又一次 LLM compact。

6. **压缩 attempt 可审计**  
   借鉴 start/summary/error 事务事件与 usage 记账思路，对齐 Compact Usage / Sidecar 方向；**不必**抄 4 字符/token 启发式（aemeath 已有中英加权估算）。

### 4.2 建议不要照搬

- 取消五级、只留 prune + summary。
- 以 4 字符/token 作为生产计量真相。
- 在没有重复 compact / 恢复成本 / 保真缺口数据前，仅因 checkpoint「好用」提前实现摘要树全簇。

## 5. 对后续工作的建议口径

### 5.1 工具结果体积（语义压缩）

- 方向：语义 compact 替代机械硬砍；与 materializer 兜底分层。
- 对齐 DSH 的是 **prune-first 心智**，不是再引入源头硬顶。
- 本调研不规定 RTK 集成形态（内建 / 外挂等），只约束「先确定性语义压缩、后 L5」的层次。

### 5.2 摘要树启动门禁

在启动持久化增量摘要树实现前，仍应先具备：

- 当前 L5 正确性路径稳定；
- 首次 / 二次 compact 的 Provider 调用数、tokens、延迟；
- 退出恢复后重复摘要成本；
- summary 保真缺口；
- sidecar / scheduler 收益是否高于复杂度。

阅读本调研时，把 DSH checkpoint 当作**轻量对照基线**：若实测显示「单点可递归 checkpoint + 工具侧语义压缩」已足够，可推迟树实现；若稳定前缀重复摘要与跨退出重算成本主导，再按 [06-persistent-summary-tree.md](06-persistent-summary-tree.md) 演进。

## 6. 一句话结论

DSH 用「工具结果中段剪枝 + 可递归 checkpoint 摘要」两条 Surface 变换，在压力或溢出时腾上下文；aemeath 应用其 **prune-first、双触发、显式 retain、可递归覆盖** 心智，同时保留五级管线与「原始历史唯一真相」的摘要树目标态，并在有数据前把树留在 Future。
