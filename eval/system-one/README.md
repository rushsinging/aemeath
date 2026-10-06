# System One 决策模型同场对比评测（#1751 阶段一）

对 CLM 及 Jev 生态候选在 Apple M4 / 16GB / 无 GPU 环境下的统一实测。

## 目录

- `datasets/` — 测试集 v1（4 场景 46 case，手工构造 + 公开基准方法论；真实会话数据为第二阶段）
- `harness/` — 评测工具链
- `results/` — 原始结果（JSONL）与汇总（summary.json）
- `REPORT.md` — 效果报告与 go/no-go 结论
- `runtime/` — 引擎运行时（模型/venv/日志，gitignore，不进仓库）

## 候选与部署形态

| 引擎 | 形态 | 端口 | 部署要点 |
|---|---|---|---|
| clm | clm-serve (CPU) + llama-server qwen3:8b Q4_K_M `--embedding --pooling last` | 8700 / 8090 | `contrastive-lm --no-deps` 安装绕开 vllm；embedding 后端必须 last-token pooling |
| jevos | 官方 macOS arm64 二进制（OpenVINO INT8） | 8017 | 零 Python，约 1-2GB 内存 |
| kev | `uv sync --extra serve`，MLX bf16 on MPS，kev-0.8b | 8009 | Python 3.13（uv 自动管理） |
| qwen3-reranker | `harness/qwen3_reranker_serve.py`（独立 venv，mlx-lm + fastapi），Jev 兼容包装 | 8210 | memory_rerank 场景现役引擎；pointwise yes/no 服务端展开，flip 天然为 0 |
| rsi-jev | `scripts/serve.py`，torch MPS fp32，v3.0 2B | 8200 | transformers 必须 5.17.x；fla 内核 CUDA-only，MPS/CPU 走参考实现 |
| laya | `laya[serve]` pip 包，三 checkpoint preload | 8000 | `laya-serve` 不支持 `--port` 参数（固定 8000）；中文走 multilingual |
| semif | CLI `semif-score --backend mlx --mlx-bits 4`，Qwen3.5-4B | —（CLI） | 仅 choice 形态，无 HTTP |
| anyjev | `anyjev[hf]`，HFBackend(Qwen3-4B, mps)，L0 零标签 | —（库内） | 官方 pipeline 依赖 vLLM，HF 后端可脱离 |

## 复现

```bash
# 1. 准备运行时（下载模型、建 venv；见 REPORT.md 附录的逐步命令）

# 2. 生成测试集
python3 harness/make_datasets.py

# 3. 起服务（daemonize 后台，健康检查快速失败）
harness/serve.sh start <engine>   # jevos | llama-emb | clm | rsi-jev | kev | laya
harness/serve.sh status all

# 4. 跑评测（大模型串行，控内存）
python3 harness/run_eval.py --engine <engine>

# 5. 指标汇总
python3 harness/score.py
```

## 测试集设计

| 场景 | 文件 | case 数 | 题型 | 指标 |
|---|---|---|---|---|
| 记忆检索重排 | `memory_rerank.jsonl` | 12（10 中 + 2 英） | rank（无 rank 端点的引擎降级为 choice） | R@1 / MRR / order-flip |
| Stop 验证 | `stop_verify.jsonl` | 12（8 中 + 4 英） | noul | acc / F1 / ECE / Brier |
| 权限预筛 | `permission_triage.jsonl` | 16（12 中 + 4 英） | noul + score(3 档) | acc / F1 / ECE + risk MAE |
| Skill 匹配 | `skill_match.jsonl` | 6（5 中 + 1 英） | rank | R@1 / MRR / order-flip |

每个 case 跑正序 + 反序两遍，检测候选顺序敏感性（order-flip）。
