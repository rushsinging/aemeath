# kev 本地评分服务部署指南

System One 决策模型的本地服务部署（阶段二外部 HTTP 形态；阶段三将内嵌化，见
`01-systemone-scoring.md` 演进路线）。

## 部署（macOS Apple Silicon）

kev 引擎：`jaredpalmer/kev-0.8b`（Qwen3.5-0.8B 冻结底座 + LoRA + pointer head），
MLX bf16 on MPS，自带出厂温度校准（2.35）。

```bash
# 1. 克隆/进入 kev 仓库（评测资产在 ~/.cache/system-one-eval/kev）
cd <kev-repo>

# 2. 安装依赖（uv 自动管理 Python 3.13）
uv sync --extra serve

# 3. 启动服务（端口 8009，与 AEMEATH_SCORING_URL 默认值一致）
uv run --extra serve python -m kev.serve --run jaredpalmer/kev-0.8b --port 8009

# 4. 健康检查
curl -s http://127.0.0.1:8009/v1/models
```

## 资源预算

- 内存：模型 bf16 约 1.7GB + MLX 运行时，实测常驻约 2-3GB
- 延迟：p50 29-71ms（阶段一 46-case 实测）
- 磁盘：模型缓存约 1.7GB（HF cache）

## aemeath 侧配置

```bash
# 引擎端点（默认值即本地 kev；一般无需设置）
export AEMEATH_SCORING_URL=http://127.0.0.1:8009
export AEMEATH_SCORING_MODEL=kev-latest
export AEMEATH_SCORING_TIMEOUT_MS=2000

# 场景开关（默认全关；逐个开启验证）
export AEMEATH_SCORING_MEMORY_RERANK=1   # 记忆检索重排
export AEMEATH_SCORING_SKILL_MATCH=1     # Skill 匹配
export AEMEATH_SCORING_POLICY_TRIAGE=1   # 权限/风险预筛
```

或在配置文件中（camelCase 亦可）：

```json
{
  "scoring": {
    "url": "http://127.0.0.1:8009",
    "model": "kev-latest",
    "timeout_ms": 2000,
    "memory_rerank": true
  }
}
```

## 降级行为

服务未启动 / 超时 / 5xx / 线格式非法时，消费点静默回退原路径（词法/启发式），
NEVER 阻断主循环；单次失败不熔断，下次调用正常重试。连接预检保证服务未启动时
毫秒级回退（规避 hyper-util 对 reusable body 的 connect 重试循环）。

## 验证

```bash
# 46-case 基线复测（需 kev.serve 在线；断言与 eval/system-one 基线偏差 ≤1pt）
cargo test -p systemone --test kev_baseline -- --ignored --test-threads=1
```

## 校准（可选）

- 观测回路：`~/.agents/scoring/observations.jsonl`（评分 + 标签落盘，攒样本）
- 温度 artifact：`~/.agents/scoring/calibration.json`（`{"temperature": 1.35}`），
  离线拟合后落盘，进程重启生效；生效后所有评分答案按温度重缩放
  （只重缩放，NEVER 改变选项胜负排序），`CalibrationLevel` 标注进每个答案与审计事件。
- 审计：`~/.agents/scoring/audit.jsonl`（每次评分决策带 engine revision +
  prompt sha256 + probabilities + 校准级别 + 延迟）。
