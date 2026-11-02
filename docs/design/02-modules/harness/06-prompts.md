# 06 · RSI 提示词（Prompt）契约

> 本文是 RSI 改进质量的第一真相源：**提示词文本即产品质量**。改动任一 prompt MUST 同步本文，并在 PR 中附 e2e 证据（真实模型输出片段）。
>
> 代码位置：`agent/features/harness/src/domain/improvement.rs`（`build_block_prompt` / `run_two_phase_cycle` / `run_compose_phase` / `render_route_evidence_block`）。

## 0. 必读：四个不能破坏的点

> 改这份契约前先读这四条——它们每一个都对应真实故障，破坏后 RSI 会静默产出劣质改动（没有报错，只有质量下降）。

1. **`MUST 引用 evidence_refs` 门禁（§2.2）**——这是「证据驱动」的全部意义。模型只引用 E0/E1 短编号，runtime 必须映射完整 signal id；去掉它会回到无依据编辑或长 signal 抄错。
2. **`no_change` 是正当答案（§2.2/§3.2）**——任何暗示「必须产出编辑」的措辞都会诱发凑数提案。真实故障：模型曾为空壳写空泛规则。
3. **两阶段分工不可合并（§1）**——改进块看「局部证据」，Compose 看「周期终态」。混为一谈会失去块间矛盾仲裁能力。
4. **改动 MUST 走 §7 验收**——prompt 改动不是文档修改，是真模型行为变更：至少一次真模型 e2e + `proposal_rejected` 为空 + dry-run 报告可读。

另需留意的常见坑见 §5「已知边界与调优记录」（裸信封 / 显示名 404 / 高思考档空响应 / content 写进 id）。

## 1. 两处提示词的分工

| 阶段 | 函数 | 调用次数 | 职责 |
|---|---|---|---|
| **改进块**（Block） | `build_block_prompt` | 每数据源块 1 次 | 看证据 + 组件现状 + 历史经验 → 提案编辑 |
| **周期汇总**（Compose） | `run_compose_phase` 内联 | 每周期 1 次（有采纳时） | 看本周期全部采纳 + 组件终态 → 仲裁矛盾/合并/精炼 |

> 当前默认执行模式为 **two_phase**：宿主先生成受限候选，诊断选择固定组件，执行只写正文。`single_phase` 保留为兼容回退路径，并同样接受路由门禁校验。两条路径共用落盘链路（校验 → 整份作废 → 原子写 → lesson 回写 → 事件流）。

## 1.1 落点、schema 与预算安全契约（所有改进 prompt 共用）

- 组件材料标注 `scope`（`global` 或 `project:<id>`），证据行标注来源项目；RSI 数据源只含项目布局 dataset，顶层 legacy dataset NEVER 进入 prompt。
- `scope=global` 只承接跨项目通用规律；单项目的项目特定经验 NEVER 写 global；项目 A 证据 NEVER 写项目 B；没有同项目候选时输出 no_change/no_issue。
- 工具参数类问题 NEVER 复述或改写工具 schema（必填性、类型、枚举、示例）。只写「读取错误 → 核对当前 schema → 修正 payload → 禁止无效原样重试」的通用纪律。
- guardrails/tool-guidance body 预算 3800 字符，overlay 7800 字符。初次产出超预算时 runtime 追加一次压缩 prompt；仍超预算整份提案作废，NEVER 硬截断。既有正文已超预算时，执行 prompt 的第一任务是语义压缩，而非逐字保留超长原文。
- 空文本响应最多重试一次；成功时 usage 聚合并在 `llm_call.attempts` 记录 2，连续空文本显式失败。
- TwoPhase 诊断/执行 JSON 解析失败各追加一次紧凑 JSON 重试：诊断重试仍失败则周期失败；执行重试仍失败则该任务作废。NEVER 把解析失败当作 no_issue、空 issues 或空正文继续收口。

## 2. 改进块提示词

### 2.1 结构（四段材料 + 任务 + 契约 + 输出格式）

```
你是 harness 组件编辑器。以下是若干组件的当前内容：

{组件材料 JSON：id / frontmatter / body}

近期执行证据（session 行为聚合信号——改进的主要依据）：
{证据块：每行 "- [E0] <signal kind> | <摘要> | 摘录: <摘录>"，按优先级排序}

历史经验（此前采纳的改进结论，供参考、可被新证据推翻）：
{lessons 块：每行 "- [<组件类型>] <lesson 文本>"；空则「（暂无历史经验）」}

任务：基于执行证据判断组件是否存在值得改进的问题（缺失、冗余、错误、
空壳占位、重复失败未覆盖的规则）。空壳占位（TODO、无实际内容的正文）
视为应修复的缺陷——为其写出有用的实际内容。每条编辑 MUST 在 evidence_refs
中结构化列出它回应的短证据编号（如 E0）；无法对应任何证据的编辑是投机，
NEVER 提出。证据与组件现状都不足时选择 no_change——那是一个正当答案，
比牵强的编辑更好。

写规则的质量标准（每次落笔先自检）：
- **可执行**：每条规则必须写清「什么条件下」+「必须/禁止做什么」，
  让模型在下次同类场景能照着做；禁止「要小心」「注意安全」这类空泛话。
- **可验证**：写下后自问「下次出现 repeated_failure 里的场景，这条规则
  能阻止它吗？」——不能就不要写。
- **不破坏**：只改证据指向的部分；与证据无关的现有规则 MUST 原样保留
  （保留原有结构与有效条款，NEVER 顺手重写整篇）。
- **不冲突**：新规则不得与现有内容矛盾；与已有条款重复时只补差异部分。
- **成本意识**：always-on 组件（guardrails/tool_guidance/overlay）每轮
  对话都要付费，越简洁越好——用最短的篇幅表达完整语义。

改进时优先关注的失效模式（证据命中哪类就修哪类，未命中不臆测）：
**证据中的每一类 repeated_failure 都应至少得到一条对应规则（除非现有规则
已覆盖该场景——此时在 reason 中说明哪条已覆盖）。**
- **token 与上下文**：输出冗长/话痨、重复指令、上下文浪费（同一文件反复读、
  无效工具调用）、always-on 段落膨胀——规则能合并就合并，能删就删。
- **工具异常调用**：用错工具（搜代码该 Grep 却用 Bash cat）、参数错误、
  同参数机械重试、忽略 stderr/错误详情就下结论。
- **skills 异常**：description 缺少触发词导致该用未用；不该触发时误触发；
  skill 步骤与当前工具面不符（过时）。
- **subagent 异常**：简单任务过度委派（成本失控）；委派 prompt 不自包含导致
  子代理猜上下文；委派后不整合结果；并发数与任务量不匹配。
- **执行纪律**：未读先写（Write/Edit 前不 Read）、跳过验证就声称完成、
  破坏性操作无确认、失败后不诊断根因只重试。
- **结论质量**：空泛建议、无证据断言、不复述关键约束就动手。

编辑契约：
- 提案只携带要变更的组件（未提及的组件自动保持原样）
- `evidence_refs` MUST 非空，且每个 ref 都来自上方证据块（E0/E1…）；空/重复/未知引用整份作废
- 任一条目校验失败会作废整份提案

只输出一个 JSON 对象，格式二选一：
{"decision":"propose","reason":"<提案依据，一句话>","evidence_refs":["E0"],"change_set":{"edits":[{"action":"upsert","id":{"kind":"tool-guidance","name":"<名>"},"content":{"frontmatter":null,"body":"<完整正文>"}}]}}
{"decision":"no_change","reason":"<一句话理由>"}
不要输出 JSON 以外的任何文字。
```

### 2.2 关键设计约束（每条都对应真实故障或 Harness² 纪律）

| 约束 | 原因 |
|---|---|
| 证据置于组件材料**之后**、lessons 之前 | 组件是编辑对象，证据是依据，经验是参考——阅读顺序即决策优先级 |
| 每条编辑 **MUST 结构化引用 evidence_refs** | 对齐 Harness²「每条编辑 MUST 追溯到轨迹时刻，否则是投机」。模型只复制 E0/E1 短编号；runtime 映射完整 signal id，lesson 与事件 NEVER 持久化短 ref |
| 空壳占位（TODO）显式点名为缺陷 | 首轮 e2e 实证：不点名则模型倾向 no_change，组件永远不长大 |
| `no_change` 是**正当答案** | 防止为凑数产出牵强编辑（真实失败模式：模型会写空泛规则） |
| omitted-is-inherited + 整份作废 | 半成品提案 NEVER 上线（任一条校验失败整份拒绝） |
| 明确 JSON 格式 + "不要输出其他文字" | 模型会加围栏/说明文字；解析侧另有流式容忍（见 §4） |
| **写规则质量标准**（可执行/可验证/不破坏/不冲突/成本意识） | 实测：无标准时产出「要小心」类空泛建议；有标准后规则为「条件+动作」形态且原有条款被完整保留 |
| **失效模式清单**（token 与上下文 / 工具异常 / skills 异常 / subagent 异常 / 执行纪律 / 结论质量） | 目标化提示：证据命中哪类修哪类，不臆测；用户 2026-10-25 明确要求覆盖 token 消耗与工具/skill/subagent 异常 |
| **每类 repeated_failure 至少一条规则**（已覆盖则 reason 说明） | 实测：无此约束时模型只修最显著的 1 类，其余信号被忽略（覆盖不完整） |

### 2.3 证据块渲染

```rust
// route_evidence_facts：按 SignalKind::render_priority() 排序
// repeated_failure(0) > retry_pattern(1) > oversized_result(2) > run_outcome(3) > cycle_observation(4)
"- [E0] repeated_failure | {summary} | 摘录: {excerpt}"
```
- 模型只引用 `E0/E1`；完整 signal id 由 runtime 映射（长 retry fingerprint NEVER 要求模型逐字复制）
- 空证据 → 「（近期无执行证据）」（周期照常，NEVER 跳过）
- 摘录有界（`EVIDENCE_EXCERPT_BOUND` = 200 字符）

### 2.4 lessons 块

```
- [guardrails] upsert guardrails/default：…（理由文本）
```
- 只列 Active（`load_active`）；被覆盖组件的旧 lesson 已自动 retired（§3.2）
- 空 corpus → 「（暂无历史经验）」

### 2.5 TwoPhase：路由候选诊断 + 固定组件执行

TwoPhase 不再把全部组件 chunk 与证据混合后让模型自由选择落点。

诊断 prompt 结构：

```text
证据与候选：
- [E0] repeated_failure | {summary} | 摘录: {excerpt} | 候选: C0=tool-guidance/default,C1=guardrails/default

候选组件当前内容：
{仅路由命中组件的 JSON 材料}

输出：
{"decision":"diagnose","reason":"...","issues":[
  {
    "evidence_ref":"E0",
    "component_candidate":0,
    "problem":"...",
    "direction":"...",
    "route_reason":"..."
  }
]}
```

约束：

- 内置工具纪律 signal MUST 至少存在 `tool-guidance` 或 `guardrails` 主候选；overlay 只能在主候选存在时作为低优先级候选参与。主候选缺失时 MUST unresolved，NEVER 让 always-on overlay 独自兜底（真实三轮运行曾把 overlay 从 1408 字符扩到 3999 字符）。
- 宿主先按 signal/tool/skill/context 生成候选集；诊断只能在候选 index 内选择。
- Skill 证据必须从来源上下文提取精确 skill 名；无法识别则 unresolved。
- `run_outcome:aggregate` / `cycle_observation` 无组件路由，只观察。
- 诊断输出 NEVER 包含组件正文。

固定执行 prompt：

```text
目标组件：{由 runtime 注入的 ComponentId}
当前内容：{该组件完整内容}
诊断任务：[E0] problem / direction / occurrences

输出：
{"decision":"propose","reason":"...","evidence_refs":["E0"],"body":"<完整正文>"}
```

约束：

- 执行输出 NEVER 包含 component id、action、change_set。
- runtime 用诊断结果组装唯一 upsert；delete 不允许。
- evidence_refs 只能指向本次输入的短编号。
- dry-run 只写虚拟 overlay；正式路径 validate 后原子写盘并回写 lesson。

## 3. 周期汇总（Compose）提示词

### 3.1 结构

```
你是 harness 仲裁器（Compose）。本周期多个证据块分别产生了以下采纳：

{采纳清单：每行 "- <组件 id>：<reason>"}

涉及组件的当前（各块编辑叠加后的）终态：
{组件材料 JSON}

历史经验（Active lesson，供识别语义重复与既有覆盖）：
{lesson 列表}

候选相似对与来源 session 上下文：
{候选对：初筛相似度 + 双方文本 + 双方来源上下文 / 上下文不可用}

任务：纵观全部采纳、终态与候选相似上下文——检测块间矛盾（同一问题不同
规则、重复条款、相互冲突的指令）、可合并项（多块各写一半的同类规则）与
可精炼项（冗余展开）。仅当发现真实矛盾或明确可合并/精炼时产出修正编辑；
各块结论一致且终态良好时选择 no_change——那是一个正当答案。

仲裁的判断标准（每次落笔先自检）：
- **矛盾**：同一场景下两条规则给出相反指令 → 保留更具体、更接近证据的
  那条，删除另一条。
- **合并**：多条规则讲同一主题的不同侧面 → 合成一条，**保留每条的独特
  信息**（合并 NEVER 丢语义）；合并前 MUST 依据候选对来源上下文确认
  同一根因、同一场景、同一约束，初筛相似度与文本相似 NEVER 作为依据。
- **精炼**：只删重复表述与冗余展开，NEVER 删掉任何一条实质规则。
- **不引入**：本阶段只仲裁现有采纳，NEVER 凭想象新增未被证据支持的规则。
- **可执行优先**：合并/精炼后必须仍是「条件 + 动作」的可执行形态。

编辑契约同改进块（omitted-is-inherited；upsert 的 content.body 为
完整正文）。只输出一个 JSON 对象，格式二选一（NEVER 输出其他形态）：
{"decision":"propose","reason":"<仲裁依据，一句话>","signal_ids":[],"change_set":{...}}
{"decision":"no_change","reason":"<一句话理由>"}
不要输出 JSON 以外的任何文字。
```

### 3.2 设计约束

| 约束 | 原因 |
|---|---|
| 只在**有采纳**时触发 | 无采纳时无可仲裁对象（NEVER 为凑 Compose 造编辑） |
| 材料 = 采纳清单 + **叠加后终态** + 采纳前 Active lesson 快照 | 终态是各块 Last-Write-Wins 的结果；历史 lesson 不能被 Block 采纳退役，也不能混入本周期自身派生 lesson |
| 候选相似对先召回、再回读 session 上下文 | 文本相似只召回；合并判定 MUST 依据来源上下文 |
| 候选上限 8 对、上下文总预算 12000 字符、单来源 1200 字符（目标 800 + 单邻居 200） | 防止 Compose 材料无界膨胀，同时保证目标 step 事实不被超大邻居挤出 |
| 三种仲裁动作显式列举（矛盾/合并/精炼） | 模型对"汇总"理解宽泛，需枚举目标形态 |
| 上下文不同或缺失时禁止合并 | 表面相似的任务/失败不得被合并成一条错误经验 |
| 显式格式示例（NEVER 其他形态） | 实测失败：只写"格式同改进块"时模型输出裸 `{"edits":[]}` → 解析失败 |
| Compose 无新增执行证据 | `signal_ids` 可空表示继承被仲裁采纳证据；非空必须是采纳 signal 的去重子集；只能修正本周期已采纳组件，修正 lesson 保留原 signal/source |
| dry-run 同样进入 Compose | 有虚拟采纳时 MUST 调用 Compose；材料使用内存 overlay 的叠加终态，仲裁修正只进预览，NEVER 落盘 |

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
3. **Compose 相似仲裁 e2e**：表面相似但来源上下文不同的提案/lesson 不被合并；同根因同类可合并且保留语义
4. `rsi run --dry-run` 报告可读且覆盖全链路：块划分/证据/提案/虚拟采纳/Compose 仲裁/落盘预演；有虚拟采纳时 Compose MUST 被调用并使用虚拟叠加终态，同时保持零持久副作用
5. 事件流落九类事件（#2070），且 `proposal_rejected` 为空
