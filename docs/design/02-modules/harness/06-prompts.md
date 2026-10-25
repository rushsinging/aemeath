# 06 · RSI 提示词（Prompt）契约

> 本文是 RSI 改进质量的第一真相源：**提示词文本即产品质量**。改动任一 prompt MUST 同步本文，并在 PR 中附 e2e 证据（真实模型输出片段）。
>
> 代码位置：`agent/features/harness/src/domain/improvement.rs`（`build_block_prompt` / `run_compose_phase` / `render_evidence_block`）。

## 0. 必读：四个不能破坏的点

> 改这份契约前先读这四条——它们每一个都对应真实故障，破坏后 RSI 会静默产出劣质改动（没有报错，只有质量下降）。

1. **`MUST 引用 signal id` 门禁（§2.2）**——这是「证据驱动」的全部意义。去掉它，模型会写通用套话，组件被无依据的编辑反复重写。
2. **`no_change` 是正当答案（§2.2/§3.2）**——任何暗示「必须产出编辑」的措辞都会诱发凑数提案。真实故障：模型曾为空壳写空泛规则。
3. **两阶段分工不可合并（§1）**——改进块看「局部证据」，Compose 看「周期终态」。混为一谈会失去块间矛盾仲裁能力。
4. **改动 MUST 走 §7 验收**——prompt 改动不是文档修改，是真模型行为变更：至少一次真模型 e2e + `proposal_rejected` 为空 + dry-run 报告可读。

另需留意的常见坑见 §5「已知边界与调优记录」（裸信封 / 显示名 404 / 高思考档空响应 / content 写进 id）。

## 1. 两处提示词的分工

| 阶段 | 函数 | 调用次数 | 职责 |
|---|---|---|---|
| **改进块**（Block） | `build_block_prompt` | 每数据源块 1 次 | 看证据 + 组件现状 + 历史经验 → 提案编辑 |
| **周期汇总**（Compose） | `run_compose_phase` 内联 | 每周期 1 次（有采纳时） | 看本周期全部采纳 + 组件终态 → 仲裁矛盾/合并/精炼 |

两阶段共用同一应答信封（`BlockProposal`）与同一落盘链路（校验 → 整份作废 → 原子写 → lesson 回写 → 事件流）。

## 2. 改进块提示词

### 2.1 结构（四段材料 + 任务 + 契约 + 输出格式）

```
你是 harness 组件编辑器。以下是若干组件的当前内容：

{组件材料 JSON：id / frontmatter / body}

近期执行证据（session 行为聚合信号——改进的主要依据）：
{证据块：每行 "- <signal_id> | <摘要> | 摘录: <摘录>"，按优先级排序}

历史经验（此前采纳的改进结论，供参考、可被新证据推翻）：
{lessons 块：每行 "- [<组件类型>] <lesson 文本>"；空则「（暂无历史经验）」}

任务：基于执行证据判断组件是否存在值得改进的问题（缺失、冗余、错误、
空壳占位、重复失败未覆盖的规则）。空壳占位（TODO、无实际内容的正文）
视为应修复的缺陷——为其写出有用的实际内容。每条编辑 MUST 指出它回应
证据中的哪条信号（reason 中引用其 signal id，如 repeated_failure:Bash:…）；
无法对应任何信号的编辑是投机，NEVER 提出。证据与组件现状都不足时选择
no_change——那是一个正当答案，比牵强的编辑更好。

编辑契约：
- 提案只携带要变更的组件（未提及的组件自动保持原样）
- 任一条目校验失败会作废整份提案

只输出一个 JSON 对象，格式二选一：
{"decision":"propose","reason":"<提案依据，一句话>","change_set":{"edits":[{"action":"upsert","id":{"kind":"skill","name":"<名>"},"content":{"frontmatter":null,"body":"<完整正文>"}}]}}
{"decision":"no_change","reason":"<一句话理由>"}
不要输出 JSON 以外的任何文字。
```

### 2.2 关键设计约束（每条都对应真实故障或 Harness² 纪律）

| 约束 | 原因 |
|---|---|
| 证据置于组件材料**之后**、lessons 之前 | 组件是编辑对象，证据是依据，经验是参考——阅读顺序即决策优先级 |
| 每条编辑 **MUST 引用 signal id** | 对齐 Harness²「每条编辑 MUST 追溯到轨迹时刻，否则是投机」；同时是 lesson 证据绑定的来源 |
| 空壳占位（TODO）显式点名为缺陷 | 首轮 e2e 实证：不点名则模型倾向 no_change，组件永远不长大 |
| `no_change` 是**正当答案** | 防止为凑数产出牵强编辑（真实失败模式：模型会写空泛规则） |
| omitted-is-inherited + 整份作废 | 半成品提案 NEVER 上线（任一条校验失败整份拒绝） |
| 明确 JSON 格式 + "不要输出其他文字" | 模型会加围栏/说明文字；解析侧另有流式容忍（见 §4） |

### 2.3 证据块渲染

```rust
// render_evidence_block：按 SignalKind::render_priority() 排序
// repeated_failure(0) > retry_pattern(1) > oversized_result(2) > run_outcome(3) > cycle_observation(4)
"- {signal_id} | {summary} | 摘录: {excerpt}"
```
- 空证据 → 「（近期无执行证据）」（周期照常，NEVER 跳过）
- 摘录有界（`EVIDENCE_EXCERPT_BOUND` = 200 字符）

### 2.4 lessons 块

```
- [guardrails] upsert guardrails/default：…（理由文本）
```
- 只列 Active（`load_active`）；被覆盖组件的旧 lesson 已自动 retired（§3.2）
- 空 corpus → 「（暂无历史经验）」

## 3. 周期汇总（Compose）提示词

### 3.1 结构

```
你是 harness 仲裁器（Compose）。本周期多个证据块分别产生了以下采纳：

{采纳清单：每行 "- <组件 id>：<reason>"}

涉及组件的当前（各块编辑叠加后的）终态：
{组件材料 JSON}

任务：纵观全部采纳与终态——检测块间矛盾（同一问题不同规则、重复条款、
相互冲突的指令）、可合并项（多块各写一半的同类规则）与可精炼项（冗余
展开）。仅当发现真实矛盾或明确可合并/精炼时产出修正编辑；各块结论
一致且终态良好时选择 no_change——那是一个正当答案。

编辑契约同改进块（omitted-is-inherited；upsert 的 content.body 为
完整正文）。只输出一个 JSON 对象，格式二选一（NEVER 输出其他形态）：
{"decision":"propose",…}
{"decision":"no_change","reason":"<一句话理由>"}
不要输出 JSON 以外的任何文字。
```

### 3.2 设计约束

| 约束 | 原因 |
|---|---|
| 只在**有采纳**时触发 | 无采纳时无可仲裁对象（NEVER 为凑 Compose 造编辑） |
| 材料 = 采纳清单 + **叠加后终态** | 终态是各块 Last-Write-Wins 的结果，Compose 的任务就是审计这个结果 |
| 三种仲裁动作显式列举（矛盾/合并/精炼） | 模型对"汇总"理解宽泛，需枚举目标形态 |
| 显式格式示例（NEVER 其他形态） | 实测失败：只写"格式同改进块"时模型输出裸 `{"edits":[]}` → 解析失败 |
| 无新证据（不注入 evidence 块） | 依据是本周期采纳与终态，不是新行为数据 |

## 4. 应答解析容忍

`parse_block_proposal`：流式取**首个 JSON 值**（`serde_json::Deserializer::into_iter`）——
容忍模型在 JSON 后追加说明文字或 markdown 围栏残迹。解析失败只作废该块（`作废（应答解析失败）`），
NEVER 中断周期；失败原因落事件流 `proposal_rejected`。

## 5. 已知边界与调优记录

| 现象 | 处置 |
|---|---|
| 模型输出裸 `{"edits":[]}`（信封形态错） | Compose 提示词补显式格式示例（#2065） |
| 模型把 content 写进 id 字段 | 改进块格式示例修正为 content 与 id 平级（#2017 e2e 实证） |
| 高思考档导致正文为空 | 生产 LLM 取模型阶梯**最低思考档** + 输出上限 16384（结构化输出任务） |
| reasoning 参数冲突 | wire `reasoning: true`（#2032 阶梯装配要求，requested=false 被判超限） |
| 模型字段用显示名 | 用 `resolved.model.id`（显示名网关 404，Qiniu/Aliyun 实证） |

## 6. 模型的差异与选择

同一 prompt 在不同模型上的产出差异（实测）：

| 模型 | 提案质量 | 速度 | 备注 |
|---|---|---|---|
| glm-5.3 | 高 | ~15s | 稳妥 |
| Aliyun deepseek-v4.1-flash | 高 | 快 | 稳定 |
| Qiniu conatus-free | **优秀**（含领域正解，如 PEP 668 的 venv/pipx/uv 方案） | ~8s | 免费，推荐生产档 |

`harness.rsi.model` 可独立配置（空 = 继承 `models.default`）。

## 7. 改动本契约的验收要求

1. 单测：prompt 结构断言（材料/证据/经验/契约/格式五段齐全）+ 解析容忍
2. **真模型 e2e**：至少一个模型跑 `rsi run`，产出提案 MUST 引用 signal id、lesson MUST 绑定真实 SignalKind
3. `rsi run --dry-run` 报告可读（块划分/证据/提案/落盘预演）
4. 事件流落九类事件（#2070），且 `proposal_rejected` 为空
