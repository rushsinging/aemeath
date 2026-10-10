# SystemOne（支撑域）

> 层级：02-modules / systemone（模块战术设计）
> 状态：Target｜Milestone：v0.2.0

## 模块定位

SystemOne 拥有 System One 决策评分的唯一端口语言：三题型（Noul/Choice/Score）+ 校准级别。消费方（memory 重排、Skill 匹配、policy 预筛）只经 `ScoringPort` 获取校准概率分布，NEVER 感知引擎型号与传输细节。服务为可选增强：不可用、超时、开关关闭时消费点静默回退原路径，NEVER 阻断主循环。

## 文档

| 文档 | 内容 |
|---|---|
| [01-systemone-scoring.md](01-systemone-scoring.md) | 三题型领域模型、ScoringPort / CalibrationPort、jev_http adapter、场景接入（记忆重排 / Skill 匹配 / 权限预筛）、校准子系统、吸收项映射、审计与 Rust 化演进、验收口径 |
| [02-kev-deployment.md](02-kev-deployment.md) | kev / 内嵌部署、scoring 目录、校准与观测回路、模型产物 |
| [03-event-stream.md](03-event-stream.md) | 可复盘评分事件流：升级替换 `audit.jsonl` 为按日 append-only 全量现场；对照 Memory 事件流；两 PR；无报告面 |

## 相关文档

- 实测与选型依据：`../../../../eval/system-one/REPORT.md`
- Memory 事件流对照：[../memory/06-event-stream.md](../memory/06-event-stream.md)
- 工程守则：[../03-engineering/README.md](../03-engineering/README.md)

## 修改历史

| 日期 | 变更 |
|---|---|
| 2026-10-10 | 文档表补 02/03；增加可复盘评分事件流与 Memory 对照链接 |
| 2026-10-03 | 初稿：三题型端口、kev 单引擎接入、校准子系统最小版、Rust 化三步演进 |
