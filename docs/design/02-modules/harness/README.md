# Harness · 模块设计索引

> 层级：02-modules / harness（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.2.0
> 背景：google-research/harness2 调研结论（见 `docs/reports/harness2-research-2026-10-21.md`）驱动的「harness 当可优化对象」落地设计。**只描述目标态**。

## 分片

| 分片 | 主题 |
|---|---|
| [01-component-tree.md](01-component-tree.md) | 8 组件定义、ComponentRegistry 契约、omitted-is-inherited 编辑契约、原子写 |
| [02-hot-reload.md](02-hot-reload.md) | 热加载：run 边界生效、校验回退、watch 面扩展、一致性模型 |
| [03-rsi-daemon.md](03-rsi-daemon.md) | RSI 独立进程：UDS 协议、生命周期、三触发、单步改进循环、harness.rsi.* 配置 |
| [04-evidence-and-library.md](04-evidence-and-library.md) | 行为对比信号、证据绑定、经验库 corpus、与 memory 反思的职责边界 |
| [05-component-adoption.md](05-component-adoption.md) | 各组件接入契约与排期、开放决策清单 |

## 核心决策（已拍板）

1. **承载**：RSI 常驻独立进程（daemon），NEVER 由 memory reflect 承担——职责正交：reflect 治理记忆条目，RSI 编辑 harness 组件。daemon **负责落盘**（validate + 原子写），主进程只做热载检测与 Remind 通知。
2. **触发**：定时为主（`harness.rsi.interval_secs` 可配置，与交互会话零耦合）+ Manual 控制面触发；材料量大 MUST 分块执行；多 rollout 探测不进本里程碑。
3. **热载**：run 边界轮询复用 config_reload 骨架，NEVER 引入文件系统事件监听。
4. **组件职责分层**：policy（能力面）与 guardrails（行为规则）NEVER 合并；STATIC_SYSTEM_PROMPT 本体 NEVER 开放编辑，开放的是 overlay 追加层。
5. **AlwaysOnProse 组件族**：c1 overlay / c2 guardrails / c4 tool_guidance 机制一份（共享 adapter），语义分槽（内容性质、RSI 证据维度、预算记账、Remind 语义各自独立）。
6. **经验库**：独立 corpus，schema 与 memory 分离；采纳编辑 MUST 绑定行为证据。
7. **职责边界（2026-10-22 拍板）**：harness daemon 保持单一职责（纯组件治理），reflect NEVER 进入 harness 进程（保持主进程 detached 现状，PreCompact 保留）；执行模式两档——`aemeath rsi run`（一次性）与 `aemeath rsi serve`（后台定时），TaskKind 插拔面保留、当前仅 HarnessRsi。
