# SystemOne · Scoring Daemon（本机常驻评分服务）

> 层级：02-modules / systemone（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.2.0
> 对应 Issue：https://github.com/rushsinging/aemeath/issues/1935
> 本文定义本机 scoring daemon 的进程模型、传输、协议、生命周期、与客户端装配边界；
> 领域模型与 `ScoringPort` 仍以 [01-systemone-scoring.md](01-systemone-scoring.md) 为准。

## 1. 问题与目标

### 1.1 问题

进程内 llama.cpp embedded（#1833）在任意评分场景开启时，**每个 aemeath 进程各自加载一份**
~812MB Q8_0 GGUF + Metal worker ≈0.9G。多开会话成倍吃内存并拖慢启动。

### 1.2 目标

- 提供**单一本机 scoring daemon**，常驻加载 1 份权重。
- 各 aemeath 会话经本地 Unix domain socket 调用；日常 CLI **不**链接进程内引擎。
- 保持「可选增强、NEVER 阻断主循环」；daemon 不可用时 fail-open。
- **彻底退役**生产与测试路径上的进程内 embedded；测试用 mock `ScoringPort` 或真 daemon。

### 1.3 非目标

- 恢复 Jev HTTP 为生产 fallback。
- 切换 candle / mistral.rs / rlx-qwen35 引擎。
- 评分 UI / 查询报告面。
- 首版 Linux / 非 arm64 macOS（明确不可用即可）。

## 2. 已拍板决策（Q1–Q17）

| # | 题 | 结论 |
|---|---|---|
| Q1 | 进程入口 | 同一 `aemeath` 二进制子命令：`aemeath scoring serve` |
| Q2 | 传输 | Unix domain socket（不做 HTTP localhost） |
| Q3 | 生命周期 | 客户端惰性拉起 + 可选挂 launchd/system daemon（非默认硬依赖） |
| Q4 | 失败语义 | 短超时；**首次** TUI/系统提示后 fail-open；NEVER 回退 Jev HTTP |
| Q5 | 协议 | UDS 上 length-prefixed JSON；对齐 `ScoringPort::answer` |
| Q6 | 与 #1833 | **彻底退役**进程内 embedded；生产与测试一律走 daemon 或 mock |
| Q7 | 事件流 | 仍由客户端 `AuditedScoringAdapter` 写 `#1909` 日切事件；daemon 可另记引擎诊断 |
| Q8 | 路径 | `~/.agents/scoring/scoring.sock` + `scoring.lock`（跟 `AEMEATH_AGENTS_DIR`） |
| Q9 | 超时 | 连接默认 2s / 推理默认 30s（可配） |
| Q10 | 并发 | 单 worker 串行队列（Metal/llama 单上下文） |
| Q11 | 模型缺失 | Unavailable + 首次提示引导 `aemeath systemone download`；NEVER 自动下载 |
| Q12 | 权限 | sock `0600`，仅当前用户 |
| Q13 | Cargo | feature `systemone-daemon` 编进 serve 路径；日常会话不加载引擎 |
| Q14 | 版本 | 请求带 `protocol_version`；不匹配 → Unavailable |
| Q15 | 平台 | 首版仅 macOS arm64 |
| Q16 | 交付 | 三 PR：设计 → daemon+协议 → 客户端+退役 embedded |
| Q17 | 配置 | 沿用 `scoring.enabled` + 场景开关；可选 `socket_path` / timeout；无 `prefer=` |

## 3. 架构总览

```
aemeath (会话进程)
  composition: DaemonScoringClient ──UDS──► aemeath scoring serve
       │                                        │
       │                                        ├─ 单实例锁 scoring.lock
       │                                        ├─ 加载 1× GGUF + PointerHead
       │                                        └─ 单 worker 串行推理
       └─ AuditedScoringAdapter
            → ~/.agents/scoring/events/{yyyy-mm-dd}.jsonl  (#1909)
```

- 消费方（memory / skills / policy）仍只依赖 `ScoringPort`，**NEVER** 感知 UDS / daemon。
- `DaemonScoringClient` 实现 `ScoringPort`；装饰链保持 `calibrated → audited → client`。
- daemon 进程内继续复用现有 llama.cpp + PointerHead 实现（从原 embedded adapter 迁入 serve 路径），
  但对会话进程不可见。

## 4. 进程与生命周期

### 4.1 入口

```bash
aemeath scoring serve          # 前台 / 受管进程
aemeath scoring serve --detach # 如需后台（实现 MAY 延后；launchd 优先）
```

- 同一二进制；**仅** `serve`（及 `systemone download`）路径编译并加载引擎（feature `systemone-daemon`）。
- 日常 `aemeath` 交互会话路径 **NEVER** 链接 llama.cpp / 加载 GGUF。

### 4.2 单实例

- 启动时获取 `~/.agents/scoring/scoring.lock`（文件锁）；已占用则：
  - 若 sock 健康 → 退出 0（或打印「已在运行」后退出 0）；
  - 若 sock 不健康 → 报告并失败（避免双实例抢 sock）。
- 监听 `~/.agents/scoring/scoring.sock`（或 `scoring.socket_path`）；创建前 unlink 陈旧 sock（仅当锁归本进程）。

### 4.3 客户端惰性拉起

当且仅当：

1. `scoring.enabled == true`，且
2. 至少一个场景开关开启，且
3. 连接 sock 失败（不存在 / 拒绝），

则客户端尝试 `spawn` 当前可执行文件的 `scoring serve`（带单实例锁语义），短暂等待 sock 就绪后重连。

- 会话退出 **NEVER** 杀掉 daemon（多会话共享）。
- `scoring.enabled=false` 或四场景全关 → **零成本**（不连、不 spawn、不检查模型）。

### 4.4 可选 system daemon

- 文档提供 launchd plist 示例，将 `aemeath scoring serve` 注册为用户级 agent。
- 默认安装 **不**自动注册 launchd；惰性拉起是默认路径。

### 4.5 模型缺失

- `serve` 启动发现模型缺失 / 校验失败 → 进程失败退出并打印引导：`aemeath systemone download`。
- 客户端侧 → `ScoringUnavailable` + **首次**用户可见提示（Q4）；之后同场景静默降级。
- **NEVER** 自动下载；**NEVER** 回退旧 Jev HTTP。

## 5. 传输与协议

### 5.1 传输

- Unix domain socket，路径默认 `${AEMEATH_AGENTS_DIR:-~/.agents}/scoring/scoring.sock`。
- 权限：sock 文件 `0600`；目录遵循现有 agents 根权限。
- 首版仅本机；NEVER 监听 TCP。

### 5.2 帧格式

- Length-prefixed JSON：`u32` little-endian 字节长度 + UTF-8 JSON body。
- 每连接可多请求；服务端按到达顺序串行处理（单 worker）。

### 5.3 消息形状（示意）

请求对齐 `ScoringPort::answer` 输入；响应对齐校准后概率分布。字段名以实施 PR 的 serde 类型为准，本稿锁定语义：

```json
// Request
{
  "protocol_version": 1,
  "method": "answer",
  "id": "corr-optional",
  "params": {
    "scenario": "memory_rerank",
    "questions": [ /* ScoringQuestion 序列化 */ ]
  }
}

// Response (ok)
{
  "protocol_version": 1,
  "id": "corr-optional",
  "result": {
    "answers": [ /* 概率分布 + CalibrationLevel */ ],
    "engine_revision": "kev-0.8b@…",
    "latency_ms": 42
  }
}

// Response (error)
{
  "protocol_version": 1,
  "id": "corr-optional",
  "error": {
    "kind": "unavailable|timeout|protocol|internal",
    "message": "…"
  }
}
```

- `protocol_version` 不匹配 → `error.kind=protocol` → 客户端映射 `ScoringUnavailable`。
- 健康检查 MAY 另提供 `method=ping`（实现期决定；非阻塞主路径）。

### 5.4 超时

| 阶段 | 默认 | 配置键（建议） |
|---|---|---|
| 连接（含惰性拉起等待） | 2s | `scoring.connect_timeout_secs` |
| 单次推理 | 30s | `scoring.infer_timeout_secs` |

超时 → Unavailable，消费点降级。

## 6. 与 #1833 / embedded 的关系

**修订口径（推翻「进程内 embedded 为唯一生产终态」）：**

| 路径 | 状态 |
|---|---|
| 生产会话 | **仅** daemon 客户端；CLI 默认不编 `systemone-embedded` |
| `aemeath scoring serve` | 唯一加载 GGUF 的用户路径（feature `systemone-daemon`） |
| 进程内 embedded adapter | **退役**：删除生产 composition 装配；测试 **NEVER** 依赖进程内 load |
| 单测 | mock `ScoringPort` |
| 集成 / 数值对分 | 起本地 daemon，或 eval harness 显式管理 serve 生命周期 |
| Jev HTTP | 仍仅测试 / 离线对分（`http-adapter` feature）；**NEVER** 生产 fallback |
| candle / mistral / rlx | 仍为远期备选，**NEVER** 本 issue 范围 |

数值门禁（79-case PointerHead 纯数学 + 真机对分）保留；真机对分改为「daemon 在线」前提，模型缺失时 skip，NEVER 自动下载。

## 7. 事件流与观测边界

| 层 | 写什么 | 路径 |
|---|---|---|
| 客户端 audited（#1909） | 全量现场 + 关联字段 | `~/.agents/scoring/events/{date}.jsonl` |
| daemon | 引擎级诊断（加载、OOM、前向失败）；**不含**业务关联 | `~/.agents/logs/` 既有 systemone target 或独立 daemon log |
| 校准旁路 | 不变 | `observations.jsonl` / `calibration.json` |

- 关联字段（session / run / step / tool_call）只在客户端可得 → **MUST** 继续由客户端 emit（Q7=A）。
- daemon **NEVER** 替代 #1909 业务事件流。

## 8. 配置面

沿用现有：

- `scoring.enabled`（总闸）
- `scoring.memory_rerank` / `skill_match` / `memory_recall` / `policy_triage`
- `scoring.event_retention_days`

新增（可选，有默认即可不写配置）：

- `scoring.socket_path`（默认 agents 根下 `scoring/scoring.sock`）
- `scoring.connect_timeout_secs`（默认 2）
- `scoring.infer_timeout_secs`（默认 30）

**NEVER** 引入 `prefer=embedded|daemon`（Q6=B 已退役 embedded）。

环境变量双通道：既有 `AEMEATH_SCORING_*` 场景开关保留；新增超时 / socket 的 env 名在实施 PR 登记 `specs/3.9-config-compat.md`。

## 9. 分层落点（实施）

| 层 | 职责 |
|---|---|
| `systemone` domain | 协议版本常量、请求/响应 DTO（与 `ScoringPort` 同形） |
| `systemone` adapters | `DaemonScoringClient`；serve 侧 listener + worker 队列；单实例锁 |
| `composition` | 装配客户端装饰链；惰性 spawn 策略；**删除** embedded 生产装配 |
| `apps/cli` | `scoring serve` 子命令；launchd 文档；首次 Unavailable 提示接线 |
| `build_cli.sh` | 默认不启 embedded；serve 所需 feature 在子命令/独立构建说明中明确 |

## 10. 交付拆分

### PR0 — 设计（本文档批次）

- 新增本文件；修订 `01-systemone-scoring.md` / README / 必要交叉引用。
- 明确推翻 #1833「embedded 唯一生产终态」。
- 无 `.rs` 变更。

### PR1 — daemon + 协议 + 单实例

- `aemeath scoring serve`、UDS、锁、模型加载、串行 worker、`protocol_version`。
- 单元测试：帧编解码、锁互斥、协议不匹配、模型缺失失败路径。
- 真机 smoke（`--ignored`）：serve + 单次 answer。

### PR2 — 客户端 + 退役 embedded + 惰性拉起 + 首次提示

- `DaemonScoringClient` 装配；惰性 spawn；超时与首次提示。
- 删除 / 停用生产 embedded wiring；测试改 mock 或 daemon。
- 79-case / 场景契约在新路径上复验；`scoring.enabled=false` 零成本回归。

## 11. 验收

- 单 daemon + 多 aemeath 进程：内存仅一份 GGUF 量级驻留。
- 冷启动会话（daemon 已就绪）显著快于原进程内 load。
- daemon down / 超时 / 协议不匹配：fail-open；首次有提示；主循环继续。
- `scoring.enabled=false`：不连、不 spawn、不加载引擎。
- 模型缺失：serve 失败 + 客户端引导 download；无自动下载、无 Jev 回退。
- 数值门禁：PointerHead 纯数学 79/79；真机对分在 daemon 在线时不回归。
- 事件流：评分后客户端日切 jsonl 仍有全量现场；daemon 不写业务关联事件。

## 12. 风险

| 风险 | 缓解 |
|---|---|
| 惰性 spawn 竞态双实例 | 文件锁 + sock 健康检查 |
| Metal 进程独占 / 崩溃 | 单 worker；客户端超时 fail-open；可手动重启 serve |
| 测试依赖真 daemon 变慢 | 单测强制 mock；集成测 `--ignored` |
| feature 误链进日常 CLI | `systemone-daemon` 仅 serve 路径；守卫 / 文档 / build_cli 默认不启 embedded |
| 与旧 HTTP 配置混淆 | 不恢复 `AEMEATH_SCORING_URL` 生产消费；文档明确 daemon ≠ Jev |

## 13. 相关文档

- [01-systemone-scoring.md](01-systemone-scoring.md) — 领域模型与端口
- [02-kev-deployment.md](02-kev-deployment.md) — 模型产物与目录（legacy HTTP 部署仅历史参考）
- [03-event-stream.md](03-event-stream.md) — 可复盘事件流
- Issue #1935

## 修改历史

| 日期 | 变更 |
|---|---|
| 2026-10-12 | 初稿：Q1–Q17 拍板落入设计；daemon 生产路径；退役进程内 embedded |
