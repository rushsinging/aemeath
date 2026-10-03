# System One 决策模型同场对比效果报告（#1751 阶段一）

> 日期：2026-10-03 ｜ 环境：Apple M4 / 16GB 统一内存 / 无 GPU（MPS + CPU）/ macOS 27.0
> 数据：`datasets/`（4 场景 46 case，手工构造，中英混合）；原始结果 `results/`，汇总 `results/summary.json`
> 复现：见 `README.md`

## 结论（go/no-go）

**go，但落地主力候选从 CLM 调整为 kev / rsi-jev / anyjev / semif / jevos 梯队，CLM 降级为备选。**

- CLM 在本评测的**中文场景系统性失效**（中文记忆重排 R@1 仅 42%，失败 case 全部为中文；英文 case 全对），与其官方 issue #22「中文支持不佳」一致。在中文为主的 aemeath 场景下，CLM 当前形态不可用。
- kev / rsi-jev / anyjev / semif 在全部四个场景质量均为满分或接近满分，且全部可在 M4 无 GPU 环境运行。
- 六家可部署候选与 CLM 均暴露 **Jev 兼容 `/v1/systemone` 线格式**（或易适配），阶段二的评分 port 设计可直接按 noul/choice/score 三题型抽象，无协议锁定风险。

## 候选总评

| 引擎 | 底座/形态 | rank R@1 | stop acc / ECE | perm acc / ECE / riskMAE | p50 延迟 | 部署成本 |
|---|---|---|---|---|---|---|
| **kev** 0.8B | MLX bf16 on MPS | **100% / 100%** | **100%** / 21% | 69% / 27% / 0.64 | **29-71ms** | ~1.5GB 权重，自带 server |
| **rsi-jev** v3.0 | torch MPS fp32，2B | **100% / 100%** | **100%** / 8% | **81% / 17% / 0.40** | 235-440ms | 5.6GB 权重，自带 server |
| **anyjev** L0 | transformers MPS，Qwen3-4B | **100% / 100%** | **100% / 0%** | **94% / 6% / 0.45** | 727-1947ms | 8GB 权重 + 库内调用 |
| **semif** | MLX 4bit，Qwen3.5-4B | **100% / 100%** | **100% / 0%** | 88% / 20%（无 score） | 349-606ms | 9GB 权重，仅 CLI |
| **jevos** v3 | OpenVINO INT8 CPU | 100% / 83% | **100%** / 19% | 75% / 15% / 0.61 | 97-517ms | 619MB 二进制，~2.2GB RSS |
| **clm** | clm-serve CPU + qwen3:8b Q4 llama-server | **42%** / 83% | 50% / 41% | 50% / 40% / 0.58 | 3ms（缓存）~1s（冷） | 75MB 头 + 5.2GB embedder，双层栈 |
| **laya** | torch MPS，421M×3 checkpoint | 17% / 33% | 50% / 33% | 56% / 41% / 0.74 | 24-47ms | ~1.5GB，自带 server |

（rank 两列为 memory_rerank / skill_match；延迟为两 rank 场景与 noul 场景的 p50 范围）

## 详细结果

### rank 场景（R@1 / MRR / order-flip / 延迟 ms）

| engine | memory_rerank R@1 | MRR | flip | skill_match R@1 | MRR | flip | p50 | p95 |
|---|---|---|---|---|---|---|---|---|
| anyjev | 100% | 100% | 0% | 100% | 100% | 0% | 1494-1947 | 1727-2842 |
| kev | 100% | 100% | 0% | 100% | 100% | 0% | 45-71 | 68-179 |
| rsi-jev | 100% | 100% | 8% | 100% | 100% | 0% | 239-357 | 300-440 |
| semif | 100% | 100% | 0% | 100% | 100% | 0% | 518-606 | 540-692 |
| jevos | 100% | 100% | 8% | 83% | 89% | 0% | 341-436 | 474-517 |
| clm | 42% | 65% | 0% | 83% | 88% | 0% | 3 | 512-983 |
| laya | 17% | 45% | **67%** | 33% | 56% | 33% | 24-33 | 48-2357 |

### noul 场景（accuracy / F1 / ECE / Brier / flip）

| engine | stop_verify acc | F1 | ECE | Brier | permission_triage acc | F1 | ECE | riskMAE |
|---|---|---|---|---|---|---|---|---|
| anyjev | 100% | 100% | **0%** | **0%** | **94%** | 94% | **6%** | 0.45 |
| rsi-jev | 100% | 100% | 8% | 1% | 81% | 82% | 17% | **0.40** |
| kev | 100% | 100% | 21% | 6% | 69% | 71% | 27% | 0.64 |
| jevos | 100% | 100% | 19% | 4% | 75% | 71% | 15% | 0.61 |
| semif | 100% | 100% | **0%** | **0%** | 88% | 89% | 20% | —（无 score 题型） |
| clm | 50% | 0% | 41% | 39% | 50% | 67% | 40% | 0.58 |
| laya | 50% | 57% | 33% | 26% | 56% | 22% | 41% | 0.74 |

### 延迟公平性注记

各引擎缓存机制不同，p50 不可直接横比：

- **clm** p50=3ms 是向量缓存命中（同文本第二遍嵌入缓存）；冷路径（首次嵌入 8B Q4 on CPU）p95 达 983ms。其架构优势是状态/动作嵌入解耦缓存，高频重复重排场景摊销后最快。
- **kev / rsi-jev / jevos** 有 state 前缀缓存，同 state 连续多题后续题显著加速（rsi-jev 实测冷 17s → 热 0.14s）。
- **anyjev** L0 模式做 cyclic shifts 消除位置偏置，每题 K 次前向，故延迟最高（但换来 flip=0 与 ECE=0）。
- **semif** 单次前向读 option logits，无缓存，延迟稳定 ~350-600ms。
- 横评结论应看「质量 × 延迟档」组合：kev 是质量满分中延迟最低档（<100ms）。

## 关键发现

1. **CLM 中文场景失效（实证）**：memory_rerank 7 个失败 case 全部为中文（英文 mem-011/012 全对）；gold 概率被压到 0.00-0.37。根因是投影头训练分布（Qwen3-8B 英文语料对比学习），与 embedding 后端量化无关（潮汐基准 prob=0.854 vs 官方 0.993 的偏移属 Q4_K_M 量化预期，不影响排序正确性）。CLM 若继续推进，需评估多语言重训练或限定英文场景。
2. **laya 质量不可用**：R@1 17-33%、order-flip 67%，且其 server 报「checkpoint ships invalid temperatures…confidence uncalibrated」警告——与第三方横评（local-jev-bench）结论一致。即使最快（24-47ms）也不具备候选资格。
3. **kev 综合性价比最高**：0.8B 全场景满分 + p50 29-71ms + MLX 原生 Apple Silicon + 出厂温度校准 + Apache-2.0。perm 场景 acc 69% 是其短板。
4. **rsi-jev 质量最均衡**：唯一 perm 场景 acc>80% 且 MAE 最低的 HTTP 服务候选；记忆重排正是其 RL 训练主打场景（实测 R@1 100% 印证）。
5. **anyjev 校准质量天花板**：ECE 0%/6% 全场最优（其零标签校准设计目标即此），适合对置信度质量要求最高的场景（权限预筛），代价是延迟 0.7-2.8s 且为库内调用（需自行服务化）。
6. **jevos 部署最轻**：619MB 二进制零 Python，质量略低于第一梯队但全面可用，适合边缘/离线形态。
7. **SemIf 仅 choice 形态**，不支持 score 题型；作为 CLI 批处理工具适合离线分析，不适合在线热路径服务化。

## 对阶段二的建议

1. **评分 port 抽象按 noul/choice/score 三题型设计**（六家候选语义一致），rank 场景统一以 choice 降级实现（本评测验证该路径质量无损）；CLM 原生 `/v1/rank` 的嵌入缓存优势可作为后续优化专项。
2. **主力候选**：kev（默认，速度+质量+校准均衡）与 rsi-jev（高质量档位/记忆重排场景优先）双 adapter；anyjev 作为「高校准档」备选（权限预筛等置信度敏感场景），需评估库内调用 vs 服务化封装。
3. **开关与降级**：按 issue 既定方案，每场景独立开关默认关闭，服务不可用静默降级。
4. **场景优先级**：按 ROI 从 #1 记忆重排、#2 Skill 匹配开始（本评测两者 rank 场景头部候选全满分，接入风险最低）；权限预筛（#5）是区分度最大的场景（acc 从 50% 到 94%），建议作为第三优先级。
5. **CLM 处理**：保留其 agentic 基准（Terminal-Bench 87.6%）参考价值，但本机实测路径（clm-serve + llama.cpp last-token pooling）已走通存档，中文问题解决前不作为落地候选。
6. **遗留**：NanoJev 未实测（推理入口硬编码 CUDA，改 ~10 行可跑，估 0.5-1 天，质量上限预期不超过已测梯队，建议仅在需要 0.6B 极致小模型时补测）；真实会话数据集的第二阶段构造（本测试集为手工构造，规模 46 case，结论方向可信但统计功效有限，阶段二接入时应在真实数据上复验）。

## 技术路线对比（训练 / 推理 / 校准机制）

| 候选 | 模型构造 | 训练方式 | 校准机制 | 推理机制 |
|---|---|---|---|---|
| kev | Qwen3.5-0.8B 冻结 + LoRA r16（11.3M）+ pointer head | 离线训练（管线未开源） | 出厂温度缩放 | MLX 单次前向读 option logits；state 前缀 KV 缓存 |
| rsi-jev | Qwen3.5-2B 全参微调：塔 + 交叉注意力 scorer + 置信度头 | SFT（教师软标签 CE，266k 题）→ RL（listwise Plackett-Luce + NDCG@5，RLOO + KL 锚定，1,500 步/21min）→ OOF 校准 | per-input 温度头（OOF 拟合；argmax 不变；score 只能软化） | 每候选一次前向 + state 前缀缓存 |
| anyjev | 任意 LLM 截断 2/3 + 闭式读出头 | 零梯度：shrunk-LDA/ridge 闭式解 | 分级 raw→L0（cyclic shifts）→L1（温度）→L2（闭式头），在线自举 | L0 每题 K 次前向 |
| jevos | 单模型 INT8（OpenVINO） | 未公开 | 无独立校准层 | choice 分解为逐选项 yes/no + 前缀树共享 state |
| semif | 公开底座直用（Qwen3.5-4B），无自有权重 | 不训练 | 无 | 单次前向直读 option logits |
| CLM | Qwen3-8B 冻结 + 双投影头（各 20M） | 对比学习 InfoNCE | 无 | 嵌入点积 + softmax；状态/动作嵌入解耦可缓存 |
| laya | ModernBERT/mmBERT encoder + 分类头 | 非自回归微调 | 有温度但部分非法（启动告警） | 单编码器前向 |
| NanoJev | 0.6B 端到端 | 游戏化多任务（管线开源） | 无 | 单次前向 |

### RSI-Jev 自训练体系（RSI = Recursive Self-Improvement）

v3.0 管线：SFT（266,131 题 / 36 源，软标签，16,416 步 ≈ 5h @ 1×H100）→ listwise RL（16 候选 = 1 正例 + 15 BM25 难负例，PL 采样排序 × NDCG@5 reward，REINFORCE + LOO 基线 + KL=0.1，每步混入 SFT replay 防遗忘；记忆重排 R@1 +60%，McNemar p=2.4e-13）→ OOF 温度头校准（9,862 留出题，泄漏防护丢弃 534 条）。

方法论亮点（对 aemeath 最有迁移价值）：
- **EDITABLE/PROTECTED 护栏**：模型/数据/配方可改，决策契约与评测代码受保护——"改模型永远不能悄悄变成改评测"，与架构守卫思想同构
- **注册预测纪律**：假设先注册再花 GPU；374 个 arm 全部留档（含 61 个失败的 RL reward 设计）；错过自定门槛的版本按失败发布
- **弱点定向数据生成**：profile 当前模型 dev 弱点 → 定向造 case → 验证 grounding/平凡性；corpus_gate 闸门拒绝标签先验漂移的语料
- **自改进闭环**：错答案 → 测试用例 → 注册预测 → 下一周期 KEEP/淘汰；执行者为 AutoScientists 多 agent 团队（GPU 之前先互相批评提案）

迁移到 coding-agent 决策的可复用件：训练/评测骨架、Case schema、corpus_gate、PL listwise loss、校准管线。耦合点：评测目标写死、校准 group 权重绑其 suite、2048 token 左截断（长 agent trace 硬伤）、两个输入文件未发布。官方指路：「真实 agent 循环（工具选择/重试/停止，成功信号延迟出现）」正是其缺失而 aemeath 天然拥有的数据形态。

## 宣称 vs 实测 交叉验证

各家 repo 自我宣称与本机实测的对照（完整矩阵见 #1751 评论）：

**速度**：kev（29-71ms vs 宣称 28-149ms @ M5）✅ 吻合；laya ✅ 速度属实；rsi-jev（宣称 ~10ms 实为 CUDA 口径，MPS 实测 235-440ms）⚠️；jevos（宣称 25-110ms/1GB，实测 97-517ms/2.2GB）⚠️；CLM（9× 对比对象是生成式 LLM）⚠️；anyjev（推理成本被淡化，L0 实测 0.7-2.8s）⚠️。

**质量**：kev/anyjev 实测超预期（L0 零标签 ECE 0-6%）；rsi-jev/jevos/semif 宣称与实测一致（含自曝短板）；CLM 中文失效 ❌；laya 严重夸大（R@1 17-33%、flip 67%，与第三方 local-jev-bench 结论一致）❌。

**部署**：kev 最顺滑（一条 uv 命令）、jevos 最佳形态（22MB 单二进制）、CLM 淡化双层栈复杂度、anyjev 官方 pipeline 文档有误导（HF 后端可绕开 vLLM）。

**规律**：研究型 repo（rsi-jev/anyjev/semif）宣称保守、实测超预期；营销型 repo（laya，30k★）星数与质量倒挂；延迟宣称普遍偏乐观约一个量级（均挑最优硬件/缓存命中口径）。验收门禁应固定采用「冷路径 + 中文长文本 + 无 CUDA」口径。

## 选择决定

**总方针（2026-10-03 定稿）：只保留一个引擎（kev），推理侧 Rust 内嵌化；其余引擎只吸收设计优点，不作为运行时依赖。**

- **D1 唯一引擎：kev 0.8B** —— 全场景质量满分 + p50 29-71ms + 0.8B + 出厂校准。不选 rsi-jev（perm 质量略优）的原因：其交叉注意力 scorer 为自定义架构，llama.cpp/candle 均不支持，Rust 化工作量远高于 kev。
- **D2 Rust 化三步**：阶段二外部 HTTP 服务（kev.serve/MLX）；阶段三 llama.cpp 内嵌（llama-cpp-2；llama.cpp 已支持 Qwen3.5 且 LoRA 转换已修 #28324；kev = Qwen3.5-0.8B + LoRA 合并 GGUF + letter logits 读取；用本评测 46-case 做与 MLX 版的数值对分，一致率 ≥99% 对齐 AnyJev 跨引擎口径）；远期 candle 原生（Qwen3.5 支持在途，PR #3897）。
- **D3 校准子系统**（吸收 anyjev 分级校准/在线自举 + rsi-jev OOF 温度头，引擎无关，port 层）。
- **D4 淘汰/留存**：laya 淘汰；CLM 降级（中文失效，嵌入解耦缓存思想备用）；semif 只吸收审计溯源；jevos 留边缘形态；NanoJev 不投入。
- **D5 接入架构**：单 Jev adapter + 三题型 port（rank 用 choice 降级）+ 逐场景开关默认关 + 静默降级。
- **D6 自训练路线**：rsi-jev 开源管线 + aemeath 真实 agent 决策轨迹，训练与 kev 同架构的专属权重（训练在 Python/GPU，产出无缝接入 Rust 引擎）。
- **验收口径**：冷路径延迟、中文 case、order-flip 率进门禁；Rust 化后指标劣化不得超过 MLX 基线 1 个百分点。

## 部署注记（附录）## 部署注记（附录）

- **clm-serve on macOS**：`pip install contrastive-lm` 在 macOS 因 vllm 无条件依赖失败（官方 issue #16/#17），需 `--no-deps` + 手动装依赖；embedding 后端用 llama.cpp `llama-server --embedding --pooling last`（复用 Ollama 的 qwen3:8b GGUF blob，零额外下载）；option/criteria 文案必须用完整句子（短标签退化，官方 issue #3）。
- **Ollama 0.34.4 的 chat 模型不再支持 `/v1/embeddings`**（报 "Start it with --embeddings"），CLM 链路必须走 llama-server 而非 Ollama。
- **rsi-jev**：transformers 必须锁 5.17.x（5.18 改 pre-tokenizer）；MPS/CPU 走 PyTorch 参考实现，首次请求约 17s（mmap 冷加载），热延迟 0.14-0.5s。
- **kev**：`uv sync --extra serve` 一条命令（自动装 Python 3.13 + MLX）；0.8B 模型约 1.5GB。
- **jevos**：release 的 `jev-macos-arm64` 二进制 + OpenVINO INT8 zip 解压即用；评论中「choice/score 返回 422」的信息**不准**——v3 实测三题型全支持。
- **laya**：`laya-serve` 不接受 `--port`（固定 8000）；Router 自动按语言路由 checkpoint；启动警告提示部分 checkpoint 温度参数非法，置信度未校准。
- **semif**：`semif-score` 必须显式传 `--revision`（40 位 commit）；`--mlx-bits 4` 内存友好；输出 schema 为 `option_ids` + 位置对齐 `probabilities` 数组（非 per-option 对象）。
- **anyjev**：PyPI 包零 vLLM 依赖（仅 `pipeline` 一键命令硬依赖），`HFBackend(device="mps")` 显式适配 Apple Silicon；score 题型结果读 `Decision.value`（不是 `.score`）。
- **新版 huggingface_hub（hf_xet）**：大文件落到 `~/.cache/huggingface/hub/blobs/` 共享池，模型目录仅符号链接，du 看模型目录会严重低估占用。
