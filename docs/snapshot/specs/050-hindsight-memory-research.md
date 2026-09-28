# Hindsight 记忆架构调研（变更设计的证据基础）

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/1764

**日期**：2026-09-28
**状态**：调研完成
**配套**：本文是 [051-hindsight-memory-change-design.md](051-hindsight-memory-change-design.md) 的证据基础

> 本文**只记录外部系统 Hindsight 的可核查事实**，不含本项目决策；本项目决策见上述变更设计。

## 1. 调研对象与方法

| 项 | 内容 |
|---|---|
| 仓库 | `vectorize-io/hindsight`（MIT） |
| 记忆引擎 | `hindsight-api-slim/hindsight_api/engine/`（Python，FastAPI 服务） |
| CLI | `hindsight-cli/`（Rust，含 TUI） |
| 存储 | PostgreSQL + pgvector；可选 Oracle（宣称功能对等）；嵌入式 pg0 |
| 数据模型真相源 | `hindsight-api-slim/hindsight_api/alembic/versions/`（迁移文件） |

**方法**：只读读取 main 分支源码、alembic 迁移与 `.env.example`。文中路径除特别说明外均相对 `hindsight-api-slim/`。

**证据标注约定**：每条结论后标注出处（文件 / 迁移名 / 配置键）。标 **未验证** 的条目表示未读到实现原文，仅有间接或命名证据。

## 2. 设计优势

下述优势均基于本次调研**可核查的机制事实**，而非仓库自述。厂商层面的性能声称单列于 §2.8，与机制结论区分。

### 2.1 记忆按认知角色分离，而非单一向量库

| 事实类型 | 语义 | 检索时的意义 |
|---|---|---|
| `world` | 客观/外部事实，含用户偏好、规则、纠正与约束 | 与环境有关的稳定知识 |
| `experience` | agent 自身执行过的动作、经历或观察 | 与"我做过什么"有关的行为记忆 |

两类事实在 prompt 中使用不同措辞（`assistant`）但**共享同一张存储表**，使同一事件能同时以"客观事实"与"自身经历"两种身份被记住。多数记忆系统把所有内容压进单一向量空间，检索时无法区分"世界是这样"与"我经历过这样"。证据：`engine/retain/fact_extraction.py::ExtractedFact.fact_type`。

### 2.2 信念可演化，但证据不丢失

| 机制 | 作用 |
|---|---|
| `source_memory_ids` + `proof_count` | 每条信念可回溯到具体证据条目，并给出去重后的证据强度 |
| `observation_history` 快照表 | 每次改写保留 `previous_*` 全文，演变可审计 |
| "精炼而非覆盖"的合并规则 | 新证据增强、削弱或扩展既有信念，不静默替换 |

关键设计选择是**用文本合成表达矛盾，而非用数值置信度**：系统曾有的 `confidence_score` 列已被迁移删除（迁移 `g2h3i4j5k6l7`），当前 schema 中不存在数值置信度；矛盾的处理路径是"改写文本 + 保留演变史"（§8.4）。

### 2.3 读取成本与推理深度分级

四层梯度（§3.1）的本质是**把 LLM 成本从读取时刻前移到写入时刻**：最常被问到的问题（心智模型）在后台预生成并落库，读取是纯数据库查询 —— 不检索、不合成、不调 LLM、不等待。

这不是缓存优化，而是**数据结构选择**：写入侧一次性付出 LLM 成本，换取读取侧的确定性延迟。

### 2.4 记忆链路的工程性质

| 性质 | 机制 |
|---|---|
| 非阻塞递进 | 巩固与刷新只入队不内联；子任务在下一轮由后台任务领取 |
| 端到端幂等 | 文档级 hash → chunk 级 hash → `operation_id` → 全文替换语义（§5.4） |
| 崩溃可续 | 进度记在**单条记忆**上（`consolidated_at`），崩溃只影响在途一条 |
| 条件写语义 | 区分"存储暂时不可用"与"条件写竞争失败"两类错误：前者可重试，后者需重读状态 |
| 面向失败的降级 | 重排服务故障时可回退检索序（转移链尾配置为融合模式）；图臂超时降级为子集；时间解析失败降级为"无时间约束" |
| LLM 成本硬门控 | 水位门控（无新数据时零调用）+ 成本地板（窗口内触发停放合并）+ 失败熔断（重试耗尽后暂停自动触发） |

### 2.5 检索的混合策略与预算纪律

- 四路检索（语义 / 关键词 / 图 / 时间）经 RRF 等权融合，再交 cross-encoder 重排；三个次级信号（新近度、时间相关度、证据强度）以**有界乘法**叠加（单信号影响 ≤ ±10~20%），保证不会盖过模型分数（§7.5）。
- 臂间预算公平：融合前对每个源做上限截断，防止单个膨胀的臂挤掉其他臂。
- token 预算有保底原则：宁可返回一条超预算结果，也不返回空结果（§7.6）。
- 后端可插拔：文本检索 5 种实现、向量索引 4 种实现，可按部署环境选择。

### 2.6 接入面

- **25+ LLM provider**，包含订阅制免 API key 通道（Claude Code / Codex / Cursor / GitHub Copilot）与网关（LiteLLM）；本地模型（ollama / lmstudio / llamacpp）同样支持。证据：`engine/providers/`（23 个实现文件）。
- **每个 bank 内建一个 MCP 端点**，无需额外服务。
- **多语言保真**：输入语言被检测并在端到端保留，实体保持原生文字（§5.2）。
- **安全闸门**：可选的写入前密钥与敏感信息扫描（脱敏或阻断）。

### 2.7 优势的适用边界

上述优势都有对应成本，调研中同样可见：

| 优势 | 对应成本 / 适用条件 |
|---|---|
| 四层梯度 | 收益主要来自规模效应；小规模记忆集的归纳层收益有限，却要付出巩固成本与复杂度 |
| 混合检索 + 重排 | 需要向量模型与重排模型（本地或远程），与"零外部依赖"的检索路径不可兼得 |
| 多后端可插拔 | 每种后端都要在查询构造器层生成不同 SQL 形状 —— 抽象成本高于接口本身 |
| 双数据库后端对等 | 专用后端的代码量已超过主后端 |
| 后台链路可靠性 | 需要独立 worker 进程、对账例程与水位字段共同维护 |

### 2.8 厂商层面的性能声称（未独立验证）

仓库 README 声称在 LongMemEval 上达到最优表现，并称结果由 Virginia Tech 的 Sanghani Center 与 The Washington Post 独立复现，其他厂商分数为自报。**这些是性能声称，本次调研未复现基准，不作为机制结论使用**。

## 3. 记忆架构

### 3.1 四层认知梯度

Hindsight 把记忆组织为四层，**每层向"读取更便宜"方向演进**：

| 层 | 载体 | 写入者 | 读取成本 |
|---|---|---|---|
| 源文档 | `documents` + `chunks` 表 | retain | 直读 |
| 原始事实 | `memory_units`（`fact_type` = `world` / `experience`） | retain 抽取 | 四路检索 |
| 观察（observation） | `memory_units`（`fact_type` = `observation`） | 后台巩固（consolidation） | 四路检索 |
| 心智模型（mental model） | `mental_models` 表 | 后台 reflect 刷新 | **零 LLM 调用**（数据库读） |

两个结构性事实：

1. **原始事实与观察同表**（`memory_units`），靠 `fact_type` 区分 —— 二者共享 partial HNSW 向量索引与全文检索列。证据：迁移 `p1k2l3m4n5o6`、`t5o6p7q8r9s0`（`fact_type` 取值 CHECK 约束）。
2. **心智模型独立表**，因为其读取路径不经过检索 —— 官方文档表述为 "Fetching a mental model is a database read. No retrieval, no synthesis, no LLM call, no waiting."

事实类型的语义区分（源码 `engine/retain/fact_extraction.py::ExtractedFact`，已逐行核对）：

```python
fact_type: Literal["world", "assistant"]
# 'world'     = 客观/外部事实，含用户偏好、规则、纠正与约束
# 'assistant' = agent 自身执行过的动作、经历或观察
```

**注意词汇分层**：prompt 侧写 `assistant`（让模型理解为"我做过的事"），入库前归一为 `experience`（存储侧 `Fact` 模型与 `fact_type` CHECK 取值为 `world` / `experience`）。

### 3.2 记忆的类型体系

四种记忆不是同一内容的粗细版本，而是**承担不同认知职责**的独立类型：

| 类型 | 载体 | 语义 | 产生者 | 读者 |
|---|---|---|---|---|
| 源文档 | `documents` / `chunks` | 原样保留的输入 | 写入方 | 溯源展开 |
| `world` 事实 | `memory_units` | 客观/外部事实（含用户偏好、规则、纠正、约束） | retain 抽取 | 四路检索 |
| `experience` 事实 | `memory_units` | agent 自身执行过的动作与经历 | retain 抽取 | 四路检索 |
| 观察 | `memory_units`（`fact_type='observation'`） | 由多条事实**归纳**出的、有证据支撑的信念 | 后台巩固 | 四路检索 / reflect |
| 心智模型 | `mental_models` | 对某个常驻问题的**预生成答案** | 后台 reflect | 直接读取 |

三者的职责边界可表述为：**事实回答"发生了什么"，观察回答"我们相信什么"，心智模型回答"某个问题现在的最优答案是什么"**。

三点设计上的关键区分：

1. **`world` 与 `experience` 的分离是认知角色分离，不是主题分离**：同一事件可同时产生两类事实（"炉子会烫" / "我摸了炉子很疼"），它们在被检索时的意义不同。
2. **观察不是"更精炼的事实"**：事实的粒度是单条陈述，观察的粒度是**跨事实的信念** —— 一条观察由多条事实支撑，并持有指向它们的证据指针。
3. **心智模型不是"更精炼的观察"**：观察是被动归纳（有新事实就更新），心智模型是**面向固定问题的主动合成**（问题定义一次，答案随知识演化被重写）。

### 3.3 记忆的组织维度

记忆由三个正交维度组织：

| 维度 | 机制 | 作用 |
|---|---|---|
| **容器** | `bank`（一个"大脑"） | 隔离单元：所有业务表带 `bank_id` 谓词；bank 自带 `disposition`（影响 reflect 的语气）与 `background` / `mission`（影响巩固的判断标准） |
| **可见性** | `tags` / `observation_scopes` | 同时作为**检索过滤边界**与**归纳隔离边界** |
| **时间** | `occurred_start` / `occurred_end` / `mentioned_at` | 三层时间语义：事件何时发生、何时结束、何时被说到 |

**可见性维度同时约束检索与归纳**是最值得注意的设计：不同 scope 的记忆**绝不共享同一次 LLM 调用**，去重也只在同 scope 内比较。这让"共享一个 bank 但互不污染"成为可能，而不必拆 bank。

### 3.4 分层设计律

四层梯度的存在依据可归纳为四条律：

| 律 | 内容 | 体现 |
|---|---|---|
| **成本梯度律** | 每层向"读取更便宜"演进：写入时付出 LLM 成本，换取读取时的确定性延迟 | 心智模型读取零 LLM（§2.3） |
| **读者决定载体律** | 走检索路径的类型同表共享索引；不走检索的类型独立成表 | 事实与观察同在 `memory_units`；心智模型独立表 |
| **证据守恒律** | 信念可以被改写，但证据与演变史不可丢失 | `source_memory_ids` + `proof_count` + 演变史快照（§8.3） |
| **升级条件显式律** | 层间升级不是时间驱动，而是有明确条件与门控 | 事实→观察需巩固裁决与去重护栏；观察→心智模型需水位门控（§4.1） |

## 4. 记忆链路

本节以**记忆形态的演进**为主线：一条输入如何被逐级转化为更高层的记忆，各层的状态如何被标记、消费与失效。各阶段的实现细节分别见 §5（retain 摄入）、§7（检索）、§8（巩固）、§9（reflect 与心智模型）。

### 4.1 形态演进：四次转化

```text
原始输入 ──①切分──> 文本块 ──②抽取──> 结构化事实 ──③归纳──> 观察 ──④合成──> 心智模型
```

| 转化 | 输入 → 输出 | 触发方式 | 关键条件 / 规则 |
|---|---|---|---|
| ① 切分 | 输入文本 → 内容块 | 同步 | 切分必须幂等（重切结果一致）；块边界即内容身份，内容指纹驱动 delta 判定 |
| ② 抽取 | 内容块 → 结构化事实 | 同步 | 5W + 时间 + 实体 + 因果；`world` / `experience` 分流；相对时间转为绝对日期 |
| ③ 归纳 | 多条事实 → 一条观察 | 后台（异步入队） | LLM 裁决 CREATE/UPDATE/DELETE + 语义去重护栏；证据指针累加、时间边界只增不减 |
| ④ 合成 | 观察 + 事实 → 心智模型 | 后台（门控触发） | 水位门控（无新数据则零调用）+ 成本地板（连续触发归并为一次）；增量模式只应用 typed operations |

**每次转化都提高"证据密度"并降低"读取成本"**：输入是原始文本（无结构），事实是结构化陈述（可检索），观察是带证据的信念（可归纳），心智模型是可直接读取的答案（零 LLM）。

### 4.2 写入方向的产生链

```text
输入 content
  → [①切分] 安全筛查 → 内容指纹（文档级 + 块级）→ 分块
  → [②抽取] LLM 抽取（5W + 时间 + 实体 + 因果）→ 归一化（丢弃退化事实）→ 嵌入增强（日期与实体仅进向量，不改原文）
  → 入库（单事务）：事实 + 实体关联 + 时间/语义/因果链接
  → 标记 consolidated_at = NULL（等待归纳）
  → [③归纳] 后台巩固：检索候选观察 → LLM 裁决 → 去重护栏 → 写观察（累加证据、追加演变史）
  → 标记源记忆 consolidated_at
  → [④合成] 后台刷新：水位门控 → reflect 循环 → 写入心智模型 → 水位前进
```

链路上每一跳都有稳定标识：`document_id` → `chunk_id` → `unit_id` → `observation_id`。

### 4.3 读取方向的两条路径

记忆有两个读者，走**不同的读取路径**：

| 读者 | 路径 | 特点 |
|---|---|---|
| 检索（recall） | 查询 → 时间约束抽取 → 四路检索 → RRF 融合 → 重排 → 预算裁剪 | 面向"查表"：给定查询返回相关记忆，结果携带各臂分数与状态 metadata |
| 反思（reflect） | 三层知识阶梯：心智模型 → 观察 → 事实（上层 stale 时下钻） | 面向"思考"：多轮工具调用按需下钻；0 结果必须落到事实层才允许放弃 |

两条路径共享同一批数据，但**读取顺序与成本完全不同**：检索每次都做完整四路检索；反思从最便宜、最固化的层开始，只有不足时才下钻。

### 4.4 失效与重算链

记忆的失效沿**反方向**传播：

```text
源记忆被删除
  → 由其派生的观察删除
  → 剩余源记忆的 consolidated_at 重置（下轮重新合成）
人工下架
  → 原始记忆移入归档表（检索路径不再需要状态谓词）
矛盾证据到达
  → 不删除旧信念：改写文本 + 追加演变史快照（"曾……现已……"）
  → 彻底被取代时才删除重建
```

关键原则：**失效是重算的触发器，不是终点**。删除源记忆不会留下悬空的观察，而是重置水位、驱动重新归纳。

### 4.5 横向状态字段

跨四种类型携带的状态字段：

| 字段 | 语义 | 所属层 |
|---|---|---|
| `content_hash`（文档级 / 块级） | 内容指纹，驱动 delta 判定与幂等 | 输入 / 块 |
| `consolidated_at` | 是否已被归纳（NULL = 待归纳） | 事实 |
| `consolidation_failed_at` | 归纳终态失败标记（可重试，不静默丢失） | 事实 |
| `source_memory_ids` / `proof_count` | 证据指针与去重后强度 | 观察 |
| `last_memory_seen_at` | 数据水位（只前进） | 心智模型 |
| `last_refreshed_at` | 最后一次刷新时间（与水位分离，避免重复刷新） | 心智模型 |
| `last_refresh_failed_at` | 刷新失败熔断标记 | 心智模型 |

## 5. retain 摄入链路

### 5.1 切分

`engine/retain/fact_extraction.py::chunk_text()` / `iter_chunks()`：

- JSON 对话数组按 **turn 边界**切分；JSONL 按行切分；普通文本走**句子感知的递归切分**（分隔符顺序 `\n\n` → `\n` → `. ` → `! ` → `? ` → `;` → `,` → 空格 → 字符）。
- **切分必须幂等**：对返回的 chunk 再切一次必须得到自身，否则 `chunk_index` → `chunk_id` 会碰撞（实现注释记录该约束由缺陷驱动）。
- `chunk_id` 确定性生成：`build_chunk_id(bank_id, document_id, chunk_index)`（`engine/chunk_ids.py`），建表注释为 "single text PK (bank_id_document_id_chunk_index)"。
- 配置键：`HINDSIGHT_API_RETAIN_CHUNK_SIZE`、`RETAIN_STRUCTURED_CHUNK_SIZE`。

**设计含义**：chunk 边界是内容身份，不是内部实现细节 —— delta retain 依赖它识别"哪些块变了"。

### 5.2 LLM 抽取

输出为 pydantic 模型，供 constrained decoding 使用。已在 `engine/retain/fact_extraction.py` 逐行核对 `ExtractedFact`：

| 字段 | 约束 |
|---|---|
| `what` / `when` / `where` / `who` / `why` | 必填字符串，未知填 `'N/A'` |
| `fact_kind` | `'event'` 或 `'conversation'`（默认后者） |
| `occurred_start` / `occurred_end` | ISO 时间戳，仅 `event` 使用 |
| `fact_type` | `Literal["world", "assistant"]` |
| `entities` | 平铺字符串列表（如 `["Alice", "Kubernetes"]`） |
| `causal_relations` | 仅可指向 **`target_index < 本条 index`** 的事实，最多 2 条 |
| `from_attachments` | 事实信息来源的附件编号（1-based） |

`model_config` 显式声明 `required: ["what","when","where","who","why","fact_type"]`。

**因果边的顺序约束**（`target_index < this fact's index`）从结构上杜绝循环依赖与顺序不确定性。

**多语言保真**（`fact_extraction.py`，已核对原文）：

```python
_DEFAULT_LANGUAGE_RULE = """LANGUAGE: Write every fact in the same language and script as the
input text. Never translate. Names, identifiers, code, and quoted text stay verbatim."""
```

实现注释记录：早期采用"先检测语言、再要求不切换"的**两步式**措辞时，模型在英文转录上有约 18% 概率输出法语或俄语事实，另一家 provider 会把日文译成英文（priming effect）；改为上述单句规则后稳定。**结论：多语言保真是 prompt 工程问题，存储层不做转写**（存储仅清洗代理字符）。

另有三种 schema 变体：verbose 版（字段描述更详细）、`NoCausal` 版（无因果关系）、**verbatim 模式**（`VerbatimExtractedFact`：不输出 `what`，原文直接作为事实文本，模型只抽元数据）。

### 5.3 embedding 增强

`engine/retain/embedding_processing.py::augment_texts_with_dates()`：

```
{f">{fact_text} (happened in {readable_date}) [{', '.join(fact.entities)}]"}
```

日期与实体名**只拼进嵌入文本**，数据库存原文。embedding 以 float32 打包传递（384 维 = 1,616 字节，对比 `list[float]` 的 12,344 字节）。

### 5.4 幂等与去重（四层）

| 层级 | 机制 | 出处 |
|---|---|---|
| 文档级 | `documents.content_hash = sha256(sanitize(original_text))`；字节级相同则默认跳过重抽 | `engine/retain/fact_storage.py`；`force_reextract` docstring |
| chunk 级 | `chunks.content_hash`；delta retain 只重抽变化块 | `chunk_storage.py::compute_chunk_hash` |
| operation 级 | `async_operations.operation_id`（UUID 主键）；实现注释：retain is idempotent by `operation_id` | `engine/retain/orchestrator.py` |
| 文档替换 | full-replace：先删派生 observation 与 links，再插入——不残留孤儿 | `fact_storage.py::handle_document_tracking` |

### 5.5 两阶段写事务

`engine/retain/orchestrator.py` 把读重的操作与写事务分离：

- **Phase 1（事务外，独立连接）**：实体消解（trigram GIN 扫描 + 共现获取 + 打分）、语义 ANN（HNSW 探测）。单元 id 先用 `str(fact_index)` 占位，插入后重映射为真实 UUID。
- **Phase 2（单事务）**：`insert_facts_batch` → 实体断言与关联（`unit_entities`，带 `fact_date`）→ 时间链接 → 语义链接（cosine ≥ `semantic_link_min_similarity`）→ 因果链接（仅 `caused_by`）→ outbox webhook。

设计注释原文说明 Phase 1 的目的是 "eliminates TimeoutErrors under concurrent load"。

**一条显式取舍**：Phase 2 **不插入实体链接**（`memory_links` 的 entity 边），推迟到 Phase 3 事务后 best-effort —— 因为检索走 `unit_entities` 自连接，实体边**只服务可视化**。

## 6. 数据模型

### 6.1 主要表

| 表 | 关键列 | 用途 |
|---|---|---|
| `banks` | `bank_id` PK、`disposition` JSONB、`background` | 记忆库画像；懒创建 |
| `documents` | PK(`id`,`bank_id`)、`original_text`、`content_hash`、`retain_params`、`tags` | 源文档；内容级去重锚点 |
| `chunks` | PK `chunk_id`、`chunk_index`、`chunk_text`、`content_hash`、FK→`documents` CASCADE | 切分块与块级哈希 |
| `memory_units` | 见下 | 原始事实、经历与观察的统一载体 |
| `entities` | `canonical_name`、UNIQUE(`bank_id`, `LOWER(canonical_name)`) | 规范实体（每 bank 一份） |
| `unit_entities` | PK(`unit_id`,`entity_id`) | 事实↔实体关联（检索用） |
| `entity_cooccurrences` | PK(`entity_id_1`,`entity_id_2`)、`cooccurrence_count` | 实体共现缓存 |
| `memory_links` | PK(`from_unit_id`,`to_unit_id`,`link_type`,`entity_id`)、`weight` CHECK 0..1 | 关系边 |
| `async_operations` | PK `operation_id`、`status`、`task_payload`、`retry_count` | 异步任务队列 + 幂等锚点 |
| `observation_history` | `observation_id` FK、`content` JSONB、`changed_at` | 观察演变史快照 |
| `mental_models` | `name`、`source_query`、`content`、`last_refreshed_at`、`last_memory_seen_at`、`trigger` | 预生成答案文档 |
| `directives` | `name`、`content`、`priority`、`is_active` | 硬规则 |
| `invalidated_memory_units` | 归档表 | 人工下架的原始记忆 |

### 6.2 `memory_units` 关键列

| 列 | 语义 |
|---|---|
| `id` | UUID 主键 |
| `bank_id` + `document_id` | 复合外键 → `documents`，`ON DELETE CASCADE` |
| `chunk_id` | FK → `chunks`，`ON DELETE SET NULL` |
| `text` | 事实正文（格式约定 `what | when | where | who | why`） |
| `embedding` | `vector(N)`，维度由模型自动探测并调整 schema |
| `occurred_start` / `occurred_end` | **事件区间** |
| `mentioned_at` | 叙述时刻（区别于事件时刻） |
| `event_date` | 兼容用单时间戳 |
| `fact_type` | CHECK ∈ (`world`, `experience`, `observation`) |
| `source_memory_ids` | `UUID[]`：观察的证据指针 |
| `proof_count` | 证据去重计数 |
| `consolidated_at` | 巩固进度水位（NULL = 未巩固） |
| `consolidation_failed_at` | 失败标记 |
| `observation_scopes` / `metadata` / `tags` | JSONB |

**时间三层分离**是核心：`occurred_start/end`（何时发生）+ `mentioned_at`（何时被说到）+ `event_date`（兜底）。检索时有效时间定义为 `COALESCE(occurred_start, mentioned_at, occurred_end)`（`engine/search/retrieval.py`）。

### 6.3 索引策略

| 能力 | 实现 |
|---|---|
| 向量 | pgvector `HNSW ... vector_cosine_ops`（默认）；可选 pgvectorscale DiskANN、vchord、scann |
| 向量索引分片 | **按 `(bank_id, fact_type)` 的 partial 索引**，新 bank 建行时同步创建；迁移注释说明 `bank_id` B-tree 总是胜出，故谓词必须同时匹配 |
| 文本 | `search_vector` 列（native tsvector + GIN，或 vchord BM25 / ParadeDB / Timescale / pgroonga） |
| 实体匹配 | `pg_trgm` GIN 索引 |
| 时间 | `(bank_id, event_date DESC)`、`(bank_id, fact_type, event_date DESC)` |
| 巩固扫描 | 部分索引 `WHERE consolidated_at IS NULL AND fact_type IN ('experience','world')` |

## 7. 检索架构（recall）

### 7.1 真实编排：不是四路并行

`engine/memories/postgres.py::recall_unified` docstring 原文：

> "one dense+BM25 UNION query and the temporal query share a single connection, then the graph retriever runs per fact_type **on the pool in parallel**, seeded by the same dense results"

即两阶段：dense+BM25 合成一条 `UNION ALL`（每 `fact_type` 独立 `ORDER BY`/`LIMIT` 以命中 partial 索引），temporal 在同连接串行，graph 按 `fact_type` 并行。

臂可独立开关：`HINDSIGHT_API_ENABLE_TEXT_SEARCH`、`ENABLE_TEMPORAL_RETRIEVAL`、`ENABLE_GRAPH_RETRIEVAL`、`ENABLE_RERANKING`。`.env.example` 注释：四者全关后退化为单次向量查询，是延迟最低的路径。

### 7.2 四臂

**语义臂** — pgvector 余弦距离，相似度表达为 `1 - (embedding <=> $1::vector)`。默认本地模型 `BAAI/bge-small-en-v1.5`；阈值 `SEMANTIC_MIN_SIMILARITY=0.3`（按该模型标定）。刻意不再"取 limit×5 再截断"，改由连接级 `hnsw.iterative_scan` 控制召回深度（`ANN_MAX_SCAN_TUPLES=4000`）。

**关键词臂** — 可插拔后端：

| 后端 | 分数表达式 |
|---|---|
| `native`（默认） | `ts_rank_cd(search_vector, to_tsquery('english', $4))` |
| `vchord` | `-(search_vector <&> to_bm25query(...))` |
| `pg_search`（ParadeDB） | `paradedb.score(id)` |
| `pg_textsearch`（Timescale） | 专有 BM25 |
| `pgroonga` | TokenBigram 分词 + OR 拼接 |

native 后端的工程补丁（`engine/search/bm25_term_selection.py`）：查询词 OR 拼接超过 `BM25_MAX_QUERY_TERMS=16` 时，**从 `pg_stats.most_common_elems` / `most_common_elem_freqs` 读取文档频率**，保留 df 最低（最有区分度）的 16 个词。注释说明原因：`ts_rank_cd` 无 IDF 且对每个命中行计算，长 OR 查询曾导致 60 秒以上超时。

**图臂** — 不是递归 CTE，而是"seed → 三信号 CTE 一次展开"（`engine/search/link_expansion_retrieval.py`）：

| 信号 | 分数 | 细节 |
|---|---|---|
| 实体 | `tanh(共享实体数 × 0.5)` | 1 实体→0.46，2→0.76，3→0.91；每实体 LATERAL 上限 `graph_per_entity_limit` 默认 200 |
| 语义 | 边权重 | retain 时预计算的 kNN 图，阈值 `SEMANTIC_LINK_MIN_SIMILARITY=0.7`；双向查（图不对称） |
| 因果 | `weight + 1.0` | `link_type ∈ (causes, caused_by, enables, prevents)`，实现注释称其为最高质量信号 |

三臂加性合并（内部分数 ∈ [0,3]）。超时降级：`asyncio.wait_for` 超时则丢弃实体臂、只跑语义+因果。seed 复用语义臂中 `similarity >= 0.3` 的结果（`GRAPH_SEED_LIMIT=20`），避免二次 ANN 查询。

**时间臂** — 区间重叠过滤（`engine/search/retrieval.py`，已核对 SQL）：

```sql
(occurred_start IS NOT NULL AND occurred_end IS NOT NULL
 AND occurred_start <= $4 AND occurred_end >= $3)
OR mentioned_at   BETWEEN $3 AND $4
OR occurred_start BETWEEN $3 AND $4
OR occurred_end   BETWEEN $3 AND $4
```

覆盖度选择常量（已核对）：

```python
_TEMPORAL_POOL_SIZE = 60        # 每 fact_type 的 ANN 候选池
_TEMPORAL_ENTRY_POINTS = 10     # 覆盖度筛选后保留的入口数
_TEMPORAL_COVERAGE_BUCKETS = 8  # 窗口切成 8 个时间桶
```

实现注释说明了动机：旧策略"按最近 50 条"在**同日批量写入**场景下退化为随机采样，且在 66 万行的 bank 上全扫超过 30 秒。新策略把窗口切 8 桶、轮转取每桶最优，保证窗口各段都有代表。

扩散：从 10 个入口沿 `(temporal, causes, caused_by, enables, prevents)` BFS，`weight >= 0.1`、每源 top-10、最多 5 轮；分数 `propagated = parent × weight × causal_boost × 0.7`（causes/caused_by = 2.0，enables/prevents = 1.5，其他 = 1.0）。

### 7.3 RRF 融合

`engine/search/fusion.py`（已逐行核对）：

```python
def reciprocal_rank_fusion(result_lists: list[list[RetrievalResult]], k: int = 60) -> list[MergedCandidate]:
    """RRF formula: score(d) = sum_over_lists(1 / (k + rank(d)))"""
```

- **k = 60，四路等权**，无 per-arm 权重参数。
- 未命中的臂不贡献分数；每条候选记录各臂原始分（`ArmScores`）用于可解释性。
- 配套 `cap_per_source(results, cap)`：融合前按源截断，docstring 原文为 "so that one over-expanding backend cannot crowd out the others"。
- 备选融合 `interleave_fusion`：轮转交错（各路 #1、#2 …）。实现 docstring 解释了 RRF 的失败模式 —— 语义排第一但与图、词面都无关的"孪生"记忆会被平均下去。

### 7.4 Cross-encoder 重排

`engine/search/reranking.py`、`engine/cross_encoder.py`：

| 项 | 值 |
|---|---|
| 默认模型 | `cross-encoder/ms-marco-MiniLM-L-6-v2`（本地） |
| 候选上限 | `RERANKER_MAX_CANDIDATES=300`，可按 LOW/MID/HIGH 档位分设 |
| 输入构造 | `"[Date: June 5, 2022 (2022-06-05)] {context}: {text}"` —— 日期以人类可读 + ISO 双格式前置 |
| 分数归一 | 已校准分（Cohere/Jina 等）原样保留；logits 走 sigmoid；NaN 清零 |
| 关闭开关 | `ENABLE_RERANKING=false` → 直接用 RRF 序；slim 部署使用 `RRFPassthroughCrossEncoder` |
| 故障转移 | `RERANKER_1_PROVIDER` / `RERANKER_2_PROVIDER` 链；链尾写 `rrf` 即 fail-open 回退检索序 |
| 重试 | `RERANKER_MAX_RETRIES=3`，指数退避 + 抖动，4xx 不重试，`RETRY_BUDGET=10s` |

有 `prunes_candidates` 能力的后端会剪掉判为不相关的候选（分数恰为 0），实现注释说明意图是"不让 junk 进入 token 预算再按 rank 砍"。

passthrough 模式下用 RRF rank 播种基分（`1.0 - 0.9*rank/(n-1)`），否则 recency boost 会成为唯一排序信号。

### 7.5 融合后的乘法 boost

```
recency_boost     = 1 + 0.2 × (recency - 0.5)    # 线性衰减 365 天，下限 0.1
temporal_boost    = 1 + 0.2 × (temporal - 0.5)
proof_count_boost = 1 + 0.1 × (log 归一化 - 0.5)
combined_score    = CE_norm × recency × temporal × proof
```

三个次级信号均采用**有界乘法**而非加权和，保证与模型分数标定无关，且任一信号最多带来 ±10~20% 影响。粗粒度日期（跨恰好一个自然月/年，容差 86400 秒）从**周期末尾**计龄且封顶中性 0.5 —— 实现注释记录该处理修复了"未来事件被当成过去事件"的排序偏差。

### 7.6 Token 预算裁剪

`engine/fact_budget.py::select_facts_within_budget` 三条原则（docstring）：

1. 按 rank 顺序花预算，**截断只砍尾部**
2. 单条超预算**只跳过自己**，不逐出后续的短事实
3. **保底**：正预算下"有命中就不能空手" —— 全都放不下时返回 top-1 整条（宁可超预算也不截断正文）

token 计数使用 `toktok`（Rust 编译进 Python 模块），默认词表 `o200k_base`；`count_tokens()` 只计数不建列表，实现说明比 tiktoken 快 2–16 倍且字节级等价；`truncate_to_tokens` 按字符边界截断以规避替换字符。

预算默认：`max_tokens=4096`（facts）、`max_chunk_tokens=8192`、`max_entity_tokens=500`。`max_tokens=0` 是文档化语义 —— "要 chunks 不要 facts"。

### 7.7 相对时间解析

`engine/query_analyzer.py`：

- `DateparserQueryAnalyzer`：先 `extract_period()`（`engine/temporal_periods.py`，处理 "last/next/this week|month|year" 等周期词；另有 `chinese_temporal_periods.py` 处理中文时期表述），未命中再走 dateparser `search_dates`（205 语言，`RELATIVE_BASE=reference_date`，`PREFER_DATES_FROM=past`）。
- **防误报打分**：数字 +100、月份或相对词 +50、星期 +30、周期词 +20；裸四位数判 0 分拒绝；取最高分匹配并扩成当天 `00:00:00–23:59:59` 窗口；周期词命中则返回整个周期区间。
- 备选 `TransformerQueryAnalyzer`：规则优先 + 小模型兜底。
- **解析失败一律降级为"无时间约束"** —— 实现注释记录该降级修复了"把记忆正文当查询文本时，极端相对时间表达导致整个 bank 检索失败"的缺陷。

`QueryAnalysis` 结构**只有 `temporal_constraint` 一个维度**：recall API 层不做语义改写、不做 query 扩展。语义层面的查询扩展由 reflect 的 prompt 策略承担（见 §9.2）。

## 8. 巩固机制（consolidation）

### 8.1 触发与调度

| 路径 | 说明 |
|---|---|
| retain 后自动入队 | 写入 `async_operations` 子任务（`operation_type='consolidation'`）；由独立 worker 进程领取，**不内联阻塞父任务** |
| Worker 轮询 | `FOR UPDATE SKIP LOCKED` 安全抢占；`poll_interval_ms` 默认 500；巩固默认独占 2 个槽位 |
| 对账例程兜底 | PL/pgSQL 例程 `banks_needing_consolidation()` 扫描遗漏项，并排除已有 pending/processing 的重复入队 |
| 手动 | 显式 consolidate 端点，可带定向 scope |

进度标记：`consolidated_at` **记在单条记忆上**，不是 bank 级水位。迁移 docstring 原文说明动机："track progress at the memory level rather than using a bank-level watermark. If consolidation crashes, already-processed memories won't be reprocessed."

失败标记 `consolidation_failed_at` 的迁移注释："so it is not silently lost and can be retried later via the API."

执行保护：`CONSOLIDATION_WALL_TIMEOUT=7200` 是**无进展的空闲时长**上限，每个提交的 batch 重置时钟；一次 LLM 响应的所有写入在单事务内提交或回滚；事务内不跨 LLM/embedder 持锁（动作先"prepare"为连接无关状态）。

并发控制：按 **scope 加锁**（scope 集合统一排序避免死锁）；不同 observation scope 的记忆**绝不共享同一次 LLM 调用**。

### 8.2 合并算法：两层判定

**第 1 层 — LLM 决策 CREATE / UPDATE / DELETE**（`engine/consolidation/prompts.py`）：

规则原文要点：

1. **PREFER UPDATE OVER CREATE** —— "One canonical observation with many source facts is always better than many siblings"；候选为空才 CREATE
2. ONE OBSERVATION PER DISTINCT FACET（一条观察只盯一个侧面：计数、实体、关系、决策或事件）
3. **MATCH BY ENTITY/FACET, NOT TOPIC**
4. STATE CHANGES → UPDATE CONCISELY（改写文本并带日期）
5. CASCADE TO ALL AFFECTED OBSERVATIONS
6. PRESERVE HISTORY —— "never DELETE ... Be very conservative with deletes"
7. NO COMPUTATION（禁止自行算数或演绎）

输出结构 `{"creates":[], "updates":[], "deletes":[]}`，每条必带 `reason`（审计用）。同一 `observation_id` 在一次响应中至多一个 UPDATE；DELETE 必须携带 id。

**第 2 层 — 语义去重护栏**：

- 触发：CREATE **或** UPDATE 之后都执行
- 候选锚点：以 **observation 文本自身 embedding** 为锚（与巩固检索"以原始事实为锚"不同），`limit=5`，dense + BM25 双臂
- 阈值：`CONSOLIDATION_DEDUP_THRESHOLD=0.97`（cosine）；≥1.0 视为关闭
- 裁决：达阈值的最近邻交给**专注的 1 对 1 LLM 调用**（temperature 0.0）；prompt 要求"若在任何重要细节上不同 —— 数字、实体、语言、否定或条件 —— 就保留而不合并"
- 落地：CREATE 路径跳过插入并把源事实并入孪生行；UPDATE 路径 fold-and-delete
- **乐观并发门**：合并时以 `WHERE text = <探测时文本>` 为条件，防 LLM 窗口期内孪生被改写

**精确文本去重**作为前置：CREATE 文本与已展示观察或本响应 UPDATE 文本相同时直接丢弃。

### 8.3 证据与演变史数据模型

`observation_history` 表（迁移 `a7b8c9d0e1f2`）：

```sql
CREATE TABLE observation_history (
    id BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
    observation_id UUID NOT NULL,
    bank_id TEXT NOT NULL,
    content JSONB NOT NULL,
    changed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (observation_id) REFERENCES memory_units(id) ...
);
```

`content` 快照包含 `previous_text`、`previous_tags`、`previous_occurred_start`、`previous_occurred_end`、`previous_mentioned_at`、`new_source_memory_ids`。

迁移 docstring 说明迁移动机：原历史存放于单列 JSONB 数组，无界增长触及 Postgres 的 256MB jsonb 硬限制。

**证据不单独存 quote**：观察通过 `source_memory_ids[]` 指回源记忆行，引文即源事实的 `text` 列。PG 使用原生数组操作，不存在 junction 表（迁移 `k6l7m8n9o0p1` 说明 Oracle 才创建该表）。

**proof_count = 去重后计数**：

```sql
source_memory_ids = (SELECT array_agg(DISTINCT e) FROM unnest(source_memory_ids || $2::uuid[]) e),
proof_count       = (SELECT count(DISTINCT e) FROM unnest(source_memory_ids || $2::uuid[]) e)
```

**时间边界只增不减**：合并使用 `LEAST(event_date, ...)` / `GREATEST(occurred_end, ...)`，实现注释：`Merging two observations must widen these, never replace them`。

### 8.4 矛盾处理：无数值置信度

**关键事实**：`memory_units` 曾有 `confidence_score` 列，后续迁移将其删除，注释说明"was only used for opinions, always NULL otherwise"。当前 schema 中不存在该列。

矛盾处理的实现路径是**文本合成 + 留史**：

1. 检测冲突
2. 把旧理解并入新文本（示例表述形如"曾偏好 X，但现已切换到 Y"）
3. 合成更丰富文本
4. 更新时间戳

彻底被取代时才 DELETE + CREATE（prompt 中给出专门示例）。观察的"staleness"是**推导值**：当范围内存在未巩固的更新记忆时，reflect 视相关观察为 stale 并下钻原始事实验证。

### 8.5 生命周期

| 事件 | 行为 |
|---|---|
| 源删除 | 派生观察删除，**剩余源记忆的 `consolidated_at` 重置**，下次运行重新合成 |
| 孤儿清理 | 专用 backsweep 迁移清理无存活源的观察；写入时对源行加 `FOR SHARE` 防删除竞态 |
| 单记忆重算 | 删除该记忆支撑的全部观察 + 重置相关记忆水位 + 自动入队 |
| 全量重算 | 重置全 bank 水位，下轮全量重推 |
| 人工策展 | 原始记忆移入 `invalidated_memory_units` 归档表，注释："Recall/consolidation/graph queries never need a state predicate — the rows simply aren't there"；归档不存 embedding，恢复时重算 |
| 过期 | 观察无 TTL；后台保留期清理只作用于审计与 LLM 请求日志 |

## 9. reflect 与心智模型

### 9.1 reflect 是多轮 tool-calling agent

`engine/reflect/agent.py` 为 agentic loop（模块 docstring："agentic loop for reflection with native tool calling"），工具集：

| 工具 | 用途 | 默认预算 |
|---|---|---|
| `search_mental_models` | 语义搜策展页（最优命中全文，其余 280 字符摘要） | max_results=5 |
| `read_mental_models` | 按 id 读全文 | 6000 tokens |
| `search_observations` | 搜整合观察 + 新鲜度 | 5000 tokens |
| `recall` | 走完整四路检索（world / experience） | 2048 tokens |
| `expand` | memory → chunk → document 上下文展开 | — |
| `done` | 提交答案 + 引用 id | — |

**三层知识阶梯**写进 system prompt：Mental Models → Observations → Raw Facts；上层 stale 时下钻；"0 结果必须调用 `recall()` 才允许放弃"。工具参数 token 上限有 floor/ceiling，实现注释说明超过 ceiling 时单次调用会吃掉整个 reflect 上下文预算并触发慢速的 split-synthesis 路径。

**上下文超预算的 map-reduce**：`split_context_history()` 按 0.8 × max_context 贪心分块；超大块按数组**条目边界**拆分（不丢证据）；每块并行执行"抽取带溯源的 claims"（禁止综合）；最后单次跨块合成。超过 4 块时告警。

reflect 的默认 temperature 高于其他用途（reflect 0.9，对照 verification 0.0 / retain 0.1）。

### 9.2 查询扩展的位置

recall API 层不做改写；**查询扩展由 reflect 的 prompt 策略承担**：

- system prompt 要求三阶段检索计划
- Query Strategy 明确要求"NEVER echo 用户问题"，把问题拆成分量搜索（示例形如 `recall('lessons')`、`recall('teaching sessions')`）
- 前若干轮用 `tool_choice` **强制**工具调用，之后放开

### 9.3 心智模型：预生成 + 水位门控

`mental_models` 表关键字段：`name`、`source_query`、`content`（markdown 正文）、`max_tokens`、`tags`、`trigger` JSONB、`last_refreshed_at`、`last_memory_seen_at`、`last_refresh_failed_at`、`reflect_response`（含 `based_on` 证据链）。

**"零 LLM 读取"的实现**：答案预生成并落库，读取即数据库查询；官方文档表述为 "Two users asking the same question get the same document"。

刷新触发的成本控制：

1. **触发源**：巩固产出新知识后触发，或 UTC cron 触发（两者互斥）
2. **Staleness gate — 先查再花 LLM**：判断"该模型作用域内是否存在比上次刷新更新的记忆"；比较对象是 `last_memory_seen_at`（数据水位，**只在新数据出现时前进**）与作用域内最新记忆。文档原文："a cron tick over an unchanged scope is skipped entirely"、"No LLM call at all, if there is nothing to read"
3. **成本地板**：`MENTAL_MODEL_MIN_REFRESH_INTERVAL_SECONDS` 窗口内的触发**不丢弃而是停放合并** —— 文档表述为"二十次连续 retain 只产生一次刷新"
4. **失败熔断**：重试耗尽后暂停自动触发，直到手动刷新成功

迁移文档记录了 `last_refreshed_at` 曾同时承担"刷新时间"与"数据水位"双职责而导致重复刷新，拆分出独立水位字段修复。

**增量 delta 模式**：文档被建模为 sections → blocks 结构化对象，LLM 只产出 typed operations（add_section / append_block / replace_block / remove_block 等）；未被操作的 section **逐字节原样复制**；按 id 寻址，错误 id 的操作直接丢弃而非猜测；无基线或 `source_query` 变更时回退全量；delta **永不以部分内容覆盖整文**（编辑全失败则刷新失败、水位不前进）。文档建议周期性执行 clear + refresh 以治理漂移。

历史：`mental_model_history` 表保存 `previous_content` 与 `previous_reflect_response`；默认保留 50 条；**失败也写历史**（`kind: "refresh_failed"`）；版本与失败记录按 kind 分别限流，互不挤兑。

### 9.4 写回闭环

reflect agent 的工具集中**没有写记忆的工具**。闭环在别处：reflect 结果被持久化为心智模型，并携带 `based_on` 记忆 id 用于失效检查（retraction）。

## 10. bank、disposition 与 directives

| 概念 | 存储 | 生效位置 |
|---|---|---|
| bank | `banks` 表 | 记忆容器与隔离单元；新 bank 建行时同步创建 partial 向量索引 |
| disposition traits | `banks.disposition` JSONB | **仅影响 reflect 的 prompt**，不影响 recall；全中性时只输出数值，非中性时追加自然语言指令 |
| background / mission | `banks` 表 | 巩固时 mission 优先级高于处理规则 |
| directives | `directives` 表（`priority`、`is_active`） | system prompt 标为强制项，且 `done` 工具的 schema 含必填合规字段 |

**bank 隔离实现**：所有业务表带 `bank_id` 列，检索与写入 SQL 一律带该谓词；`documents` 使用复合主键 `(id, bank_id)`；实体唯一性约束为 `UNIQUE(bank_id, LOWER(canonical_name))`。多租户可选 **schema 级隔离**：PostgreSQL 用 schema 前缀（`engine/schema.py::fq_table`），Oracle 用会话级 `ALTER SESSION SET CURRENT_SCHEMA`；跨租户发现例程刻意忽略请求级 schema 上下文（因为由后台循环跨租户调用）。**未发现** Postgres 行级安全（RLS）策略。

安全闸门：Memory Defense 为按 bank 可选的策略，扫描写入内容中的密钥与敏感信息（45 种模式），可脱敏或整条阻断。

## 11. 关键常量速查

| 常量 / 配置键 | 值 | 作用 |
|---|---|---|
| RRF `k` | 60 | 融合常数 |
| `SEMANTIC_MIN_SIMILARITY` | 0.3 | 语义臂下限 |
| `TEMPORAL_SEMANTIC_MIN_SIMILARITY` | 0.1 | 时间臂语义下限 |
| `SEMANTIC_LINK_MIN_SIMILARITY` | 0.7 | 语义链接构建阈值 |
| `CONSOLIDATION_DEDUP_THRESHOLD` | 0.97 | 观察去重阈值 |
| `RERANKER_MAX_CANDIDATES` | 300 | 重排候选上限 |
| `BM25_MAX_QUERY_TERMS` | 16 | 关键词臂查询词上限 |
| `GRAPH_SEED_LIMIT` | 20 | 图臂种子数 |
| `_TEMPORAL_POOL_SIZE` / `_ENTRY_POINTS` / `_COVERAGE_BUCKETS` | 60 / 10 / 8 | 时间臂覆盖度选择 |
| `CONSOLIDATION_WALL_TIMEOUT` | 7200 | 巩固无进展空闲上限（秒） |
| `EMBEDDINGS_LOCAL_MODEL` | `BAAI/bge-small-en-v1.5` | 默认嵌入模型 |
| `RERANKER_LOCAL_MODEL` | `cross-encoder/ms-marco-MiniLM-L-6-v2` | 默认重排模型 |
| `MENTAL_MODEL_HISTORY_MAX_ENTRIES` | 50 | 心智模型历史保留条数 |

## 12. 未验证项

以下条目在调研中未读到实现原文，仅有间接或命名证据，引用时需谨慎：

1. **recall 的 budget 档位具体数值**：`recall_unified` 的 `limit` 由 budget 档位决定，但具体映射位于超大编排文件中，未逐行核对。
2. **`cap_per_source` 的调用点与取值**：函数已核对，调用位置与配置键未定位。
3. **`memory_units` 是否存在数值化置信度**：已确认历史列被删除；当前是否存在其他等价机制未穷尽检查。
4. **观察的"证据引文"是否有独立存储**：结论为通过 `source_memory_ids` 指针引用源行文本；是否存在额外的引文快照列未穷尽检查。
5. **直接写入路径的 Observation 上限行为**：`max_observations_per_scope` 相关策略覆盖逻辑未逐行核对。
6. **核心编排文件规模**：`engine/memory_engine.py` 体积巨大，超出可读窗口，其内部职责划分未逐段核对。

## 13. 相关文档

- 变更设计：[051-hindsight-memory-change-design.md](051-hindsight-memory-change-design.md)
- 目标态模块入口：[Memory README](../../design/02-modules/memory/README.md)
- 目标态领域模型：[01-domain-model.md](../../design/02-modules/memory/01-domain-model.md)
- 目标态检索与注入：[02-retrieval-and-injection.md](../../design/02-modules/memory/02-retrieval-and-injection.md)
- 目标态 Reflection 引擎：[03-reflection.md](../../design/02-modules/memory/03-reflection.md)
- 目标态端口与适配器：[04-ports-and-adapters.md](../../design/02-modules/memory/04-ports-and-adapters.md)

## 修改历史

| 日期 | 变更 |
|---|---|
| 2026-09-28 | 初稿：Hindsight 四层梯度、retain 链路、数据模型、检索融合、巩固机制与心智模型的调研记录 |
| 2026-09-28 | 补充设计优势（§2）与记忆链路（§4）；「记忆体系总览」并入「记忆架构」（§3）并补充记忆类型体系、组织维度与分层设计律，后续章节顺延重编号 |
| 2026-09-28 | 按变更设计治理约定从 design 目录迁入 snapshot/specs 并重命名，补齐快照格式头部与变更设计指针，修正跨目录链接 |