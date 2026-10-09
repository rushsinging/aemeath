# SystemOne · 决策评分服务

> 层级：02-modules / systemone（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.2.0
> 本文定义 SystemOne BC 的领域模型、端口、引擎接入、校准子系统与各消费场景的接入设计。选型与实测依据见 `eval/system-one/REPORT.md`（System One 决策模型同场对比效果报告）。

## 1. 背景与定位

System One 决策模型是一类**非生成式**评分服务：对「状态 × 候选集」一次前向（或少量前向）直接输出校准概率分布，延迟远低于生成式 LLM。Scoring BC 把它作为 aemeath 内部决策点（记忆重排、Skill 匹配、权限预筛等）的可选增强能力接入：

- **可选增强，NEVER 阻断主循环**：System One 未安装、加载失败、推理超时、开关关闭时，消费点回退到原有词法/启发式路径；初始化失败必须明确报错并禁用 System One，NEVER 静默切换到另一评分后端。
- **生产引擎收敛为 Rust 内嵌实现**：kev 0.8B 的 Q8_0 GGUF + 外置 PointerHead，由 llama.cpp Rust binding 驱动；外部 HTTP 服务不再是生产 fallback。
- **模型按需手动安装**：启动 `aemeath` NEVER 主动下载模型；缺失时提示执行 `aemeath systemone download`。模型安装、校验和缓存由独立命令负责。
- **协议无锁定**：`ScoringPort` 不暴露引擎细节；HTTP adapter 仅保留为测试、离线对分和 embedded 回归基准，不作为用户生产运行路径。

## 2. 领域模型

### 2.1 三题型（Published Language）

```rust
/// 评分问题：三题型，语义与 Jev /v1/systemone 线格式一一对应。
enum ScoringQuestion {
    /// 是非判定：命题为真的概率。
    Noul {
        instructions: String,
        /// 可选 true/false 语义描述（完整句子，NEVER 用短标签——
        /// 实测证据：短标签会使评分头输出退化）。
        criteria: Option<NoulCriteria>,
    },
    /// 选项抉择：候选 key → 完整描述句。
    Choice {
        instructions: String,
        criteria: Vec<(String, String)>,   // 2..=255 项，有序
    },
    /// 有序分档：等级描述升序排列。
    Score {
        instructions: String,
        levels: Vec<String>,               // ≥2 级，有序
    },
}

enum ScoringAnswer {
    Noul { p_true: f64 },
    Choice { choice: String, probabilities: Vec<(String, f64)>, confidence: f64 },
    Score  { score: f64, probabilities: Vec<f64>, confidence: f64 },
}
```

- `ScoringState`：prose 文本（或结构化渲染为 prose）；**NEVER 传 JSON 原文**（评分头按散文训练）。
- 一次请求可携带多题（`Vec<ScoringQuestion>`），利用引擎的 state 前缀缓存摊销延迟。
- rank 场景（记忆重排、Skill 匹配）**统一以 Choice 降级实现**：候选作 criteria，取 probabilities 排序——实测验证该路径质量无损；原生 rank 端点不作为依赖。

### 2.2 校准级别

```rust
enum CalibrationLevel {
    Raw,          // 引擎原始输出
    Temperature,  // 出厂/在线拟合的全局温度缩放
    Head,         // 闭式头（后置，见 §7 吸收项）
}
```

每个 `ScoringAnswer` 携带实际使用的 `CalibrationLevel` 与引擎 revision，供审计与降级判定。

## 3. 端口与分层

```
domain/
  published_language.rs   — ScoringQuestion / ScoringAnswer / ScoringState / CalibrationLevel
  pointer_head.rs         — hidden states → q/k 投影 → 校准 logits（纯领域数学）
ports.rs                  — ScoringPort / CalibrationPort / ModelAssetPort
adapters/
  embedded.rs             — embedded ScoringPort facade（向专用 worker 提交推理）
  llama_worker.rs         — llama.cpp context 的单线程生命周期与逐 token hidden 读取
  model_assets.rs         — manifest、下载、sha256 校验、原子安装与本地解析
  jev_http.rs             — Jev HTTP adapter（仅测试 / 离线对分 / 回归基准）
  null.rs                 — NullScoringAdapter（开关关闭或初始化失败时的零成本实现）
  calibration_store.rs    — observe 落盘与校准 artifact 读写
```

```rust
trait ScoringPort {
    /// 批量评分；服务不可用返回 ScoringUnavailable（消费点据此回退）。
    async fn answer(&self, state: &ScoringState, questions: &[ScoringQuestion])
        -> Result<Vec<ScoringAnswer>, ScoringUnavailable>;
}

trait CalibrationPort {
    /// 落盘一条「评分 + 后续观测标签」，供离线/在线校准拟合。
    async fn observe(&self, record: CalibrationObservation);
    /// 当前生效的校准 artifact（温度向量等）；无则 Raw。
    fn current(&self) -> CalibrationLevel;
}
```

- `ScoringPort` 抽象**不暴露 HTTP 细节**（无 URL/header 概念），为 Rust 内嵌化（llama.cpp / candle）预留：内嵌实现只是换一个 adapter。
- 消费方（memory / skills / policy）只依赖 `ScoringPort`，NEVER 直接感知引擎型号。

## 4. 引擎接入与模型生命周期

### 4.1 生产路径：embedded llama.cpp

- 首批平台：macOS arm64；其他平台未实现 embedded 时明确报告不可用并禁用 System One，NEVER 自动回退 HTTP。
- 模型形态：Qwen3.5-0.8B + kev LoRA 合并后的 **Q8_0 GGUF（约 775MB）**，加外置 PointerHead 权重。Q4_K_M 在 79-case 对分仅 94.9% argmax 一致，NEVER 用于生产评分。
- 推理机制：对 kev 编码协议产生的逐题 causal row 做 llama.cpp embedding 前向，读取逐 token normed hidden states；在 `<|fim_suffix|>` 的 decide 位置与每个 `<|box_end|>` option 位置取 hidden，执行 `q(h_decide)`、`k(h_option)`、点积、温度缩放与 softmax。
- Rust 接入：优先使用 `llama-cpp-2` safe binding；若 safe 层未暴露逐 token hidden，仅对 `llama_get_embeddings_ith` 等必要 API 使用 `llama-cpp-sys-2` 薄封装，unsafe MUST 局限在 adapter 内。
- 生命周期：llama model/context 与 PointerHead 固定驻留专用 worker thread；异步 `ScoringPort` 通过有界 channel 提交请求，NEVER 将 C/C++ context 跨 Tokio task 传递。
- 已知上游缺陷规避：llama.cpp（ggml-metal-device.m，residency sets 释放断言）在 device 释放期可能 SIGABRT；engine init 期设置 `GGML_METAL_NO_RESIDENCY=1` 关闭该特性（用户显式配置时不覆盖），上游修复后移除。

### 4.2 模型安装与缓存

启动 `aemeath` 时 **NEVER 主动下载模型**。仅当任一评分场景开关开启时检查本地资产：

- 校验通过：加载 embedded adapter。
- 模型缺失：显示「System One 模型未安装，评分功能已禁用；执行 `aemeath systemone download` 安装」，并继续无评分原路径。
- 校验或加载失败：明确报告错误、禁用 System One，NEVER 下载，NEVER 回退 HTTP，主循环继续运行。
- 所有评分场景开关关闭：不检查、不下载、不加载模型。

手动安装命令：

```text
aemeath systemone download
```

命令从固定 manifest 下载 Q8_0 GGUF、PointerHead 与 tokenizer 资产到同目录临时文件，逐项校验长度、SHA-256、revision、hidden size（1024）、pointer dimension（256）、temperature 和支持平台，全部成功后原子 rename 到：

```text
~/.agents/models/systemone/<revision>/
  manifest.json
  model.gguf
  pointer_head.safetensors
  tokenizer/
```

有效缓存命中时命令幂等返回，不重复下载；失败时 NEVER 留下可被运行时识别为有效安装的半成品目录，也 NEVER 覆盖已有有效版本。发行 manifest 未落地（仓库无经确认的 URL / SHA-256 元数据）时命令 typed 失败并以非零退出码告知，NEVER 内置占位 URL 假数据。

### 4.2.1 数值回归门禁（批次 2 落地）

- fixture 真相源：`eval/system-one/fixtures/parity_q8/`（79 case：row token ids + torch fp32 golden hidden + Python PointerHead softmax 概率 + head 权重裸二进制），由 `harness/export_parity_fixture.py` 零网络导出，随 git 提交。
- 纯数学门禁（任何环境可跑）：`systemone` crate `tests/fixture_parity.rs`——Rust PointerHead 对拍 fixture golden 概率，79/79 argmax 恒等、max|Δp| ≤1e-4。
- 真机门禁（`--ignored` 显式运行、模型缺失时 skip 且 NEVER 自动下载）：`adapters/embedded_parity_tests.rs`——fixture token ids → llama.cpp Q8_0 前向 → Rust PointerHead → argmax 对拍 golden，实测 79/79 命中、max|Δp|=0.0136（位于 crate 内是因需 `pub(crate)` worker 接口）。

### 4.3 HTTP adapter 退役边界

`jev_http` 从生产 composition 路径退出，不再有 `embedded|http` 用户后端切换，也不作为 embedded 失败后的 fallback。它 MAY 保留在测试和 eval 构建中，用于：

- MLX golden 数值对分；
- Jev 线格式兼容性测试；
- embedded adapter 的回归基准。

生产配置中的 `AEMEATH_SCORING_URL`、`AEMEATH_SCORING_MODEL`、`AEMEATH_SCORING_TIMEOUT_MS` 随 HTTP 退役进入兼容清理；具体移除节奏遵循配置弃用门禁，NEVER 留下看似生效但实际无消费点的死配置。

### 4.4 错误语义

- 初始化失败（模型缺失、下载未执行、校验失败、平台不支持、GGUF/PointerHead 不兼容、llama.cpp 初始化失败）：记录 error 或用户可见提醒，**禁用整个 System One**，不装配评分端口；主 agent 继续运行。
- 单次推理失败（上下文超限、worker 故障、非有限 logits、超时）：返回 `ScoringUnavailable`；消费点按原有词法/启发式路径降级，不阻断主循环。
- 下载命令失败：命令以非零状态退出并说明原因；不改变当前运行中的评分状态。

## 5. 消费场景接入

按 ROI 顺序，接入一个、开一个、验一个。每个场景独立开关（默认关闭），开关全关时行为与现状完全一致（回归测试证明）。

### 5.1 场景 A：记忆检索重排（storage memory）

- 现状：词法召回 + 规则排序。
- 接入：词法召回 top-N（N ≤ 16）→ Choice 题型重排（候选=记忆条目原文）→ 按 probabilities 重排。
- 开关：`AEMEATH_SCORING_MEMORY_RERANK`。

### 5.2 场景 B：Skill 匹配（skills / SkillTool）

- 现状：名称/描述词法匹配。
- 接入：候选 skill（metadata 描述）→ Choice 题型；仅低置信词法结果时触发（省延迟）。
- 开关：`AEMEATH_SCORING_SKILL_MATCH`。

### 5.3 场景 C：权限预筛（policy）

- 现状：规则引擎判定。
- 接入：工具调用描述 → Noul（是否破坏性/需确认）+ Score（风险三档）→ 输出为**预筛建议**；强制规则优先级高于评分建议（评分只能加严、NEVER 放宽规则判定）。
- 开关：`AEMEATH_SCORING_POLICY_TRIAGE`。

## 6. 校准子系统（v0.2.0 最小版）

- **observe 回路**：消费点在获得真实结果后（如重排后被点击/采用、预筛后人工确认结果）落盘 `CalibrationObservation { question, probabilities, label, engine_revision, ts }` 到 `~/.agents/scoring/observations.jsonl`。
- **温度缩放**：攒够阈值（≥100 条/题型）后离线拟合全局温度，artifact 落 `~/.agents/scoring/calibration.json`；加载后 `CalibrationLevel::Temperature`。
- 置信度使用克制原则（吸收 rsi-jev）：校准只做重缩放，**NEVER 改变选项胜负排序**（argmax 不变）。
- 闭式头 / 在线自举（anyjev L2/observe 自举 30/60/120）为后置演进，port 语义已预留。

## 7. 吸收项映射（非引擎依赖）

| 来源 | 吸收点 | 落点 |
|---|---|---|
| anyjev | 分级校准语义、observe 自举回路 | §2.2、§6 |
| rsi-jev | OOF 温度头实现、置信度克制、自训练管线（中长期，训练与 kev 同架构专属权重） | §6；自训练见 §8 |
| kev | 出厂温度校准、permute 顺序扰动自检 | 验收门禁（§9） |
| semif | 引擎 revision + prompt hash 随评分落审计 | §8 审计 |
| jevos | 零依赖单二进制形态 | Rust 化终态目标（§8） |
| CLM | 状态/动作嵌入解耦缓存 | 高频重排需求出现时再评估 |

## 8. 审计与 Rust 化演进

- **审计**：每次评分决策落审计事件，携带引擎 revision、prompt sha256、probabilities、校准级别、延迟——与现有 audit BC 对齐。
- **Rust 化路线**：
  1. ~~外部 HTTP 服务~~：阶段一/二评测与接入基线，生产路径在 embedded 交付后退役，仅留测试 / 离线对分用途。
  2. **llama.cpp 内嵌（当前路线）**：`llama-cpp-2` 驱动 Q8_0 GGUF；逐 token hidden states + Rust PointerHead 外置前向。79-case 数值对分：f16 与 Q8_0 均 100% argmax 一致（`eval/system-one/REPORT.md`）；Q4_K_M 否决。**Rust 化边界 = 推理内核外全链**（编码 / PointerHead / 校准 / 审计均为纯 Rust）。
  3. **未来备选（NEVER 作为当前计划）**：`rlx-qwen35`（白盒 Rust，数值与 llama.cpp 决策位等价；GPL-3.0 与 GPU 路径问题解决后可评估）；`mistral.rs`（待稳定 hidden-state API）；`candle`（已放弃推进，仅当 Qwen3.5 gated-DeltaNet 上游支持成熟且有真实诉求时作备选）。三者均为替代后端候选，接入时 MUST 先过 79-case 数值门禁。
- **自训练路线（中长期）**：用开源三段式管线（SFT 软标签 → listwise PL/NDCG RL → OOF 校准）在 aemeath 真实 agent 决策轨迹上训练与 kev 同架构（Qwen3.5 小底座 + LoRA + pointer head）的专属权重；训练在 Python/GPU 侧，产出无缝接入 Rust 引擎。

## 9. 验收口径

- 场景接入门禁固定三项：**冷路径延迟**（无 state 缓存）、**中文 case 子集**、**order-flip 率**（候选正反序两遍）。
- embedded 数值门禁：Q8_0 GGUF + PointerHead 对 MLX golden 全量 79-case argmax 一致率 ≥99%，各场景指标劣化 ≤1 个百分点；逐 case 比对表入 `eval/system-one/results/`。
- macOS arm64 首批门禁：embedded 加载后的主进程增量内存 ≤1.5GB，记录冷启动、p50/p95 推理延迟；四平台 CI 必须仍能编译不含 embedded 运行能力的目标。
- 模型生命周期门禁：CLI 启动不产生网络下载；`aemeath systemone download` 可幂等安装、校验失败 fail-closed、半成品不可见；模型缺失时提示准确且主循环继续。
- HTTP 退役门禁：生产 composition 不构造 `JevHttpScoringAdapter`，embedded 初始化失败不回退 HTTP；测试 / eval 对分仍能显式使用 HTTP adapter。
- 每场景接入后在真实会话数据上复验；跨层链路（config → asset → adapter → port → 消费点）每层有单元测试或场景测试，NEVER 只测首尾。
- 开关全关回归：不检查模型、不显示下载提醒、不加载 llama.cpp，行为与资源占用同无 System One。
