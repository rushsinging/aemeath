# 01-system · 系统级设计

> 层级：01-system（总体战略设计）
> 状态：Target｜Milestone：v0.1.0
> 本层承载 aemeath 的**系统级设计**：产品与领域、统一语言、上下文地图、系统架构、依赖规则与代码组织。**只描述目标态。** 各 BC 战术设计见 [../02-modules/](../02-modules/)。

## 文档索引

| # | 文档 | 内容 |
|---|---|---|
| 01 | [产品与领域](01-product-and-domain.md) | 产品目标、核心问题、子域三分类、15 BC 责任章程与判断规则 |
| 02 | [统一语言](02-ubiquitous-language.md) | 统一语言术语表、术语辨析 |
| 03 | [上下文地图](03-context-map.md) | 15 BC 集成关系（C/S · ACL · Pub/Sub · OHS · PL · SK）、交付层、Future 预留 |
| 04 | [系统架构](04-system-architecture.md) | capability-first 模块化单体 + 选择性 Hexagonal seam + 唯一组合根 + crate 映射 + 传输透明 |
| 05 | [依赖规则](05-dependency-rules.md) | Clean 策略 / 细节依赖方向、7 条依赖铁律、跨能力边界、Agent 执行生命周期状态机原则 |
| 06 | [代码组织](06-code-organization.md) | 系统级代码组织真相：能力优先、用例共置、按需 port 与渐进 crate 边界 |

## 阅读路径

| 你要做什么 | 先读 |
|---|---|
| 理解 aemeath 是什么、怎么划分 BC | [01](01-product-and-domain.md) |
| 查术语精确定义 | [02](02-ubiquitous-language.md) |
| 理解 BC 之间怎么集成 / 端口 | [03](03-context-map.md) |
| 理解系统形态与组合根 | [04](04-system-architecture.md) + [06](06-code-organization.md) |
| 审查依赖方向与跨能力边界 | [05](05-dependency-rules.md) + [06](06-code-organization.md) |

## 相关文档

- [02-modules 模块级设计](../02-modules/README.md)
- [03-engineering 工程守则](../03-engineering/README.md)
- [设计导航总入口](../README.md)
- [仓库级工作约束（AGENTS.md）](../../../AGENTS.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-14 | 新增本索引 | #972 |
