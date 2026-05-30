# Feature #51：UI Domain DDD 设计 — 将 apps/cli 提升为核心域

**状态**：✅ 已完成（2026-05-30 用户确认）

**优先级**：中

## 背景

#47（DDD 架构重设计）框架内对 `apps/cli` 的定位讨论：是否将 UI Domain 提升为核心域。

## 解决方案

经讨论回归"支撑域（薄入口）"定位，AgentClient SDK 保留并纳入 #47 §6.5，不单独提升 UI 域为核心域；相关物理结构与 Model/View/Update/Effect 4 个 Context 由 #50/#53/#55/#57 系列落地。

## 关联

并入 #47 一并完成（已确认）。

## 验证

2026-05-30 用户确认 feature #51 已完成。
