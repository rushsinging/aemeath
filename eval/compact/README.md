# Compact 链路评测（#2133）

对压缩（compact）链路做可量化评测：**压缩后上下文还够不够用、哪个 compact 模型性价比最好、改动是否回归**。

实现语言：**Rust**（生产 CLI 的隐藏子命令树 `aemeath eval compact`）。不使用 Python——压缩链路的输入/输出都在本仓库 Rust 侧，第二条解析路径只会带来漂移。

## 命令

```bash
# P0：真实日志画像（频率/耗时/分块/降级/模型分桶）
aemeath eval compact telemetry \
    --log ~/.agents/logs/context.log --since 2026-10-01 \
    --output eval/compact/results/telemetry_baseline_2026-10.json

# P1：从真实 session dataset 导出 case
aemeath eval compact export \
    --root ~/.agents/session --limit 8 --output-dir /tmp/compact-cases

# P1：按 case 计算确定性指标（需要压缩输出 <case_id>.summary.txt）
aemeath eval compact score \
    --cases /tmp/compact-cases --summaries /tmp/compact-summaries \
    --output eval/compact/results/run_eval.json

# P1 执行面：对导出的 case 跑一次真实压缩（走生产装配与管线）
aemeath eval compact run \
    --cases /tmp/compact-cases --output-dir /tmp/compact-run --limit 3

# 导出真实压缩大 session（生产数据：--only-compacted + early 规模门槛）
aemeath eval compact export --only-compacted --min-compacted-tokens 100000 \
    --limit 3 --output-dir /tmp/cases-big

# 续接问答（P3）：出题=答题模型读 early 历史，答题=只看 summary+tail，判卷=独立 --judge
aemeath eval compact quiz \
    --cases /tmp/compact-cases --summaries /tmp/compact-run \
    --model "Zhipu Coding Plan/glm-5.3" \
    --judge "Bailian Token Plan/qwen3.8-max" \
    --limit 1 --output eval/compact/results/quiz.json

# 真实摘要质量（must_keep 召回 + 压缩率，用已落盘摘要，不重跑压缩）
aemeath eval compact recall \
    --root ~/.agents/session --limit 200 \
    --output eval/compact/results/recall_baseline.json

# 生产实际反馈指标（重问率/重复读取/错误率/中断/不满/成本）
aemeath eval compact feedback \
    --root ~/.agents/session --limit 200 \
    --output eval/compact/results/feedback_baseline.json
```

`telemetry` / `export` / `score` 都是只读 + 计算；省略 `--output` 时打印到 stdout。

## 评测边界（MUST 如实呈现）

- **确定性指标不是端到端质量**：`must_keep` 召回度量「typed id / 文件路径是否还在 summary 中」，是覆盖率代理，不判断语义等价、不判断模型能否续接任务。
- **P0 遥测只描述发生过什么**：频率、耗时、分块形状、降级；不评价摘要内容。日志未记录触发来源（自动/手动），因此不输出「自动压缩占比」。
- **历史日志字段缺口**：2026-10-28 之前的日志没有 `model=` / `quality=` / `elapsed_ms=`，分别归入 `unknown` 或按「开始→完成」时间戳推算，不与新日志混算。
- **判官不设默认**（P3）：语义判定由 `--judge <provider/model>` 在评测时显式指定；未指定时只跑确定性指标，不静默降级为某个默认判官。
- **跳过 case 不污染分母**：无压缩输出的 case 计 `skipped`；空 `must_keep` 不算 0 分。

## 实现位置

```
apps/cli/src/subcommand/eval/
  mod.rs          子命令分发与默认路径
  telemetry.rs    日志解析（新旧格式）+ 画像汇总
  dataset.rs      session dataset 读取（manifest → members），估算口径对齐 runtime
  cases.rs        case 导出、must_keep 抽取与限流、输入侧汇总
  score.rs        确定性指标（召回/压缩率/结构健康）
  feedback.rs     生产实际反馈指标（tail vs early 对比）
  recall.rs       真实摘要质量（must_keep 召回 + 压缩率）
  constants.rs    日志正则、阈值与噪声前缀
  *_tests.rs      单元测试（日志格式、dataset、抽取、分母规则）
eval/compact/
  results/        P0/P1 基线（真实数据）
  .gitignore      本地导出的 datasets/ 不入库
```

## 阶段状态

| 阶段 | 内容 | 状态 |
|---|---|---|
| P0 | 遥测基线 | ✅ Rust 实现，10 月真实数据已入库 |
| P1 | case 导出 + 确定性指标 + 单测 + 真实压缩执行面 | ✅ Rust 实现（metrics 待 #2136） |
| P2 | 多模型对照 | ✅ 由多次 `run --model X` 收集（无一次性矩阵命令：单模型失败不牵连其他） |
| P3 | 续接问答 e2e（判官 `--judge` 指定） | ✅ 已落地（判官校准集待做） |
| P4 | 回归门禁（确定性指标不劣于基线） | ⏳ |
| 生产反馈 | 重问率/重复读取/错误率/中断/不满/成本 | ✅ 已实现（趋势口径，不进 CI 门禁） |
| 摘要质量 | 真实摘要的 must_keep 召回 + 压缩率 | ✅ 已实现（不重跑压缩） |

### P1 执行面缺口

`score` 目前从 `--summaries` 目录读取压缩输出（`<case_id>.summary.txt`），**不会自己跑压缩**。生产路径应接 `aemeath eval compact run`：加载 case → 真实 runtime 装配 → 压缩 → 输出 summary 与 `CompactMetricsData`。该子命令未落地前 P2/P3 不出正式结论。

## 生产实际反馈指标（已实现：`eval compact feedback`）

数据来源：真实 session dataset。压缩点取 `session-state.compact.start_at`，
`early` = 被摘要覆盖的 committed steps，`tail` = 压缩后保留的 steps；
用户输入取自 `step.accepted_input.messages`（`outcome.messages` 只有模型输出与工具结果）。

| 指标 | 含义 | 数据来源 |
|---|---|---|
| 压缩后重问率 | 压缩后 5 个 turn 内再次提出相似请求 | steps + CJK bigram Jaccard ≥0.8 |
| 压缩后重复读取率 | 重新 Read/Grep 压缩前已访问文件 | tool_use 路径集合差分 |
| 压缩前后工具错误率 | 信息缺失导致乱调工具/参数错 | step outcome / tool_result |
| 压缩后中断/取消率 | 用户被停顿或质量劝退 | runtime / TUI 日志 |
| 显式不满信号 | 「前面说过/不是说了」等（弱信号，只做趋势） | user messages |
| 压缩成本 | input/output token | provider usage |
| 触发时机比 | `early_tokens` / 阈值、剩余窗口占用 | context.log + 预算公式 |

约束：全部标注「趋势/代理」，不做单点因果断言；不做 CI 硬门禁，只做周报/版本对比。
实现细节：重问只比较 ≥8 字符的实质请求（短指令不参与），观察压缩后最近 5 个 user turn，
字符 bigram Jaccard ≥0.8 记命中；`<system-reminder>` / `<hook-feedback>` 等注入块不计入用户输入。

## 基线（真实数据）

### P0（2026-10，122 次压缩）

| 指标 | 值 |
|---|---|
| 单次耗时 | p50 20086ms / p95 77063ms / max 758318ms |
| early_tokens | p50 185789 / p95 542328 / max 1004355 |
| map 分块 | 112 次走 map；chunks p50 6 / p95 15 / max 26 |
| 降级块数累计 | 195 |
| 模型分桶 | 全部 `unknown`（历史日志无 `model=`；需 #2132 合入后新日志） |

### P1 输入侧（8 个真实 case，上限 300 万 token）

| 指标 | p50 | p95 | max |
|---|---|---|---|
| message_count | 648 | 8365 | 8365 |
| estimated_tokens | 479560 | 2296755 | 2296755 |
| must_keep（上限 200） | 200 | 200 | 200 |

### 多模型对照（1 case，#2148 修复后；由多次 `run --model` 收集）

| 模型 | quality | 耗时 | summary | must_keep 召回 |
|---|---|---|---|---|
| Bailian/deepseek-v4.1-flash | Llm | 223.8s | 12941 chars | 0.078 |
| Zhipu/glm-5.3-flash | Llm | 72.4s | 10918 chars | 0.102 |

对照首跑曾暴露 #2148（server_effort_map 抬升 Off 与 InvocationScope
「effective≤requested」校验冲突：Zhipu 系 compact LLM 调用必失败、五块全降级、
1.5s 假完成），本仓已顺手修复（放宽该校验，归并表为权威）。修复前 glm 行为
PartialMapFallback ×5、召回 0.349（虚高）——**degraded 行的召回不可比**。

### 续接问答（真实大 session，44.5 万 token、真实压缩点 early≈28 万 token）

run：`quality=Llm`、3.6 分钟、summary 49706 chars。
quiz（出题预算 800K token、glm-5.3 出题答题、qwen3.8-max 判卷）：6 题（early/mid/recent
各 2 题，题目为真实工程决断：review 结论、gate 分类、must-fix、阶段划分）答 6、
判对 3、correct_rate **0.5**；per_epoch：early 0.5 / mid 0.5 / recent 0.5
（单 case 无衰减差异，多 case 收集后才有统计意义——机制已就位，
专门验证「多次压缩后早期决断的记忆衰减」）。

小 session 首跑（20 万 token）：5 题判对 2、rate 0.4；判错项为细粒度事实，
与 recall 0.078 信号一致。correct_rate 是续接可用性代理，只做版本间对比。

### 摘要质量（199 个 session，13 个有压缩；已落盘摘要）

| 指标 | 值 |
|---|---|
| must_keep 召回 | 均值 0.216 / p50 0.205 |
| 压缩率（摘要 token / early token） | 均值 0.021 / p50 0.022 |
| early_tokens p50 | 462718 |

**解读口径（MUST 如实呈现）**：召回 0.22 不代表压缩质量差——摘要是抽象归纳，
不可能保留早期历史里的全部 typed id / 文件路径。它是**保留率基线**：用于
版本对比（同一会话不同压缩策略/模型）与解释「压缩后重复读取 310 次」的关系，
不能单独作为质量结论；续接可用性需 P3 真模型问答。

### 生产反馈（199 个 session，13 个有压缩）

| 指标 | 值 |
|---|---|
| 重问率（压缩后 5 turn 内、≥8 字符） | 0.0（无命中） |
| 压缩后重复读取路径 | 310 |
| 工具错误率 early → tail | 7.13% → 5.94% |
| 压缩后中断 steps | 3 |
| 显式不满信号 | 1 |

## 判官口径（P3）

- `--judge <provider/model>`：评测时指定，代码与配置不写默认值。
- 必须**跨族**：不得与压缩候选模型重叠（避免自 preference bias）。
- `k=3` 多数投票，每次判定要求引用摘要原文片段作证据。
- 落地前用 20–30 条人工 gold 校准，一致率 **≥ 0.9** 才可用；与第二判官分歧 > 10% 时先改 rubric。
- 判官空响应/失败计为「judge 不可用」，不得计入「压缩失败」。
