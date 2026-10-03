# Scoring · System One 决策评分服务

> 层级：02-modules / scoring（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.2.0
> 本文定义 Scoring BC 的领域模型、端口、引擎接入、校准子系统与各消费场景的接入设计。选型与实测依据见 `eval/system-one/REPORT.md`（System One 决策模型同场对比效果报告）。

## 1. 背景与定位

System One 决策模型是一类**非生成式**评分服务：对「状态 × 候选集」一次前向（或少量前向）直接输出校准概率分布，延迟远低于生成式 LLM。Scoring BC 把它作为 aemeath 内部决策点（记忆重排、Skill 匹配、权限预筛等）的可选增强能力接入：

- **可选增强，NEVER 阻断主循环**：服务不可用、超时、开关关闭时，消费点静默回退到原有词法/启发式路径。
- **引擎收敛为单一实现**：kev 0.8B（实测全场景质量满分、p50 29-71ms、MLX 原生）。其余候选只吸收设计优点（见 §7），不作为运行时依赖。
- **协议无锁定**：kev 与 rsi-jev 共用 Jev `/v1/systemone` 线格式，adapter 单实现、配置切换。

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
  ports.rs                — ScoringPort / CalibrationPort
adapters/
  jev_http.rs             — Jev 线格式 HTTP adapter（唯一引擎 adapter）
  null.rs                 — NullScoringAdapter（开关关闭时的零成本实现）
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

## 4. 引擎接入（jev_http adapter）

- 线格式：`POST {base}/v1/systemone`，`{state, model, questions: {id: {type, instructions, criteria}}}`。
- 配置（按 3.9 分层，env 在 config 包内统一读取）：
  - `AEMEATH_SCORING_URL`（默认 `http://127.0.0.1:8009`）
  - `AEMEATH_SCORING_MODEL`（默认 `kev-latest`）
  - `AEMEATH_SCORING_TIMEOUT_MS`（默认 2000；超时即 Unavailable）
- 降级语义：连接失败 / 超时 / 5xx / schema 422 → `ScoringUnavailable`；**单次失败不熔断**，由消费点逐次回退（决策评分是 best-effort，无需熔断器复杂度）。
- 部署：外部 `kev.serve` 进程（`uv sync --extra serve && python -m kev.serve --run jaredpalmer/kev-0.8b`）；部署步骤与内存预算见 `eval/system-one/README.md`。

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
- **Rust 化三步**：
  1. 外部 HTTP 服务（本设计，`jev_http` adapter）
  2. llama.cpp 内嵌（llama-cpp-2；kev = Qwen3.5-0.8B 底座 + LoRA 合并 GGUF + letter logits 读取；与 MLX 版做 46-case 数值对分，答案一致率 ≥99%）
  3. candle 原生（待上游 Qwen3.5 支持合并；白盒形态可读取 hidden states，届时 anyjev 式校准头与 CLM 式投影头可直接挂载）
- **自训练路线（中长期）**：用开源三段式管线（SFT 软标签 → listwise PL/NDCG RL → OOF 校准）在 aemeath 真实 agent 决策轨迹上训练与 kev 同架构（Qwen3.5 小底座 + LoRA + pointer head）的专属权重；训练在 Python/GPU 侧，产出无缝接入 Rust 引擎。

## 9. 验收口径

- 场景接入门禁固定三项：**冷路径延迟**（无 state 缓存）、**中文 case 子集**、**order-flip 率**（候选正反序两遍）。
- 指标基线：`eval/system-one` 46-case 实测（R@1 / acc / ECE / flip）；Rust 化后劣化不得超过 1 个百分点。
- 每场景接入后在真实会话数据上复验（阶段一测试集为 46 条手工构造，统计功效有限）。
- 跨层链路（port → adapter → 消费点）每层有单元测试或场景测试；开关全关回归测试证明行为零变化。
