# Agent Runtime · 防 Stuck 机制

> 层级：02-modules / runtime（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#761（S2）
> 本文定义 Loop Engine 内置的 StuckGuard 四层防线。Main / Sub 通过同一 Loop Engine 获得一致保护；实现差距与退役责任只在 [迁移治理](../../03-engineering/03-migration-governance.md) 维护。

## 1. 四层防线总览

```
Loop Engine 内置 StuckGuard（Main/Sub 共用）
├── L1 StallGuard      LLM 文本重复检测
├── L2 ToolLoopGuard   工具调用循环熔断（含周期检测）
├── L3 TimeoutGuard    墙钟时间兜底（替代 max_turns；timeout=0 无限）
└── L4 StopHookGuard   Stop hook 反复阻断上限
```

**统一不变量**：StuckGuard 内置 `run_loop`；Main / Sub **MUST** 同时经过 L1-L4，差异只允许来自 `RunSpec` 的显式策略值。

## 2. L1 · StallGuard（文本重复）

- **检测**：assistant 输出文本指纹（trim 后前 N 字符）
- **触发**：最近窗口内同一指纹重复达阈值
- **默认参数**：窗口 `4`，指纹长度 `200 字符`，重复阈值 `3`
- **触发处理**：标记 stuck → 分级响应（§6）

## 3. L2 · ToolLoopGuard（工具循环熔断）

- **指纹**：`tool_name + 规范化 JSON 输入`（JSON key 排序，避免顺序误判）
- **两种模式**：
  - **连续重复**：同一指纹连续出现
  - **周期循环**：period 长度 `2-5`、重复 `3` 次的循环模式（如 `A→B→C→A→B→C→A→B→C`）
- **默认参数**：连续 soft `≥3` / hard `≥5`；周期重复 `3`；`blocked_count≥3` → hard；recent 窗口 `64`
- **触发处理**：
  - **SoftBlock**：阻断本次调用，喂回错误结果提示 LLM"不要重复、换策略/总结/问用户"
  - **HardPause**：升级为暂停

## 4. L3 · TimeoutGuard（时间兜底）

- **检测**：墙钟时间 vs `RunSpec.timeout`
- **语义**：`timeout = 0` → **无限**（Main 默认 0）；`timeout > 0` → 超时强制 `Failed`
- **作用**：替代 max_turns，作为无限循环的最终硬防线；Sub 可配有限值

## 5. L4 · StopHookGuard（阻断上限）

- **检测**：Stop hook 反复阻断 Run 完成的次数
- **参数**：`stop_hook_block_limit`（可配）
- **触发处理**：超限强制终止（防 Stop hook 造成的完成死循环）

## 6. 分级响应（渐进升级，非直接杀）

```
SoftBlock  → 喂回错误提示，要求 LLM 换策略 / 总结 / 问用户（给自救机会）
   ↓ 仍不改（升级）
HardPause  → 暂停：Main 转 AwaitingUser（等人介入）；Sub 转 Failed 回传父（无人应答）
   ↓ 兜底
Timeout    → 墙钟硬上限强制 Failed（最终防线）
```

## 7. 与状态机集成

StuckGuard 触发是 Run 状态机的一等公民，而非散落的 `if`：

| StuckGuard 结果 | Run 状态机迁移 | 领域事件 |
|---|---|---|
| SoftBlock | 保持当前态（tool 标记 blocked，喂回提示继续）| `ToolCallFailed`(fuse) |
| HardPause（Main）| → `AwaitingUser` | `StuckDetected` |
| HardPause（Sub）| → `Failed` | `StuckDetected` + `RunFailed` |
| Timeout | → `Failed` | `RunFailed` |
| StopHook 超限 | → `Failed` | `RunFailed` |

## 8. 可配置

阈值经 `RunSpec` / `ConfigSnapshot` 配置：
- **Sub 建议更严阈值**（更快熔断，因为无人盯着）
- Main 阈值可宽松（有人实时观察 + 可 ask_user 介入）
- 默认值以 §2-§5 为单一配置基线

## 9. 相关文档

- 状态机与 Loop：[03-loop-and-state-machine.md](03-loop-and-state-machine.md)
- 模块边界（StuckGuard 归 loop_engine / ToolLoopGuard 归 tool_coordination）：[02-module-boundaries.md](02-module-boundaries.md)
- 领域模型（timeout 字段）：[01-domain-model.md](01-domain-model.md)
- Current → Target 迁移责任：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：四层防线（Stall/ToolLoop/Timeout/StopHook）、统一进 Loop Engine 补 Sub 缺口、分级响应、状态机集成 | #761 |
