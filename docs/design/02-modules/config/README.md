# Config · 模块总览

> 层级：02-modules / config（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#792（S2）/ [#972](https://github.com/rushsinging/aemeath/issues/972)

## 文档索引

| 编号 | 文档 | 内容 |
|---|---|---|
| 01 | [config-layer.md](01-config-layer.md) | Config 分层优先级链、ConfigSnapshot PL、Config-owned reader/writer OHS、project-aware prepare / commit participant、CompatibilityAdapter ACL、reasoning 静态阈值 |

## 定位

Config 是**通用域 BC**——为所有其他 BC 提供配置真相：

- ConfigSnapshot 是 Published Language；每个 Run 捕获一个不可变 snapshot，watch 只投影已提交的新值
- ConfigReader 只作为 Config-owned committed-state view 交给 bootstrap / MainSession façade；Run 只用 admission 时捕获的 ConfigSnapshot，非 Run query / subscribe / update 经 async gate-aware ConfigQuery / ConfigWriter
- ConfigAppService 独占 active project config；Context Management 经 `ProjectConfigParticipant` 协调切换但不复制第二份 current state
- #933 定义 AgentClient delivery seam，#871 独占 SessionSwitchGate / coordinator 与 façade 实现；TUI / CLI 只见 AgentClient 命令和 SDK 投影
- 不包含业务逻辑——只承载配置数据

## 相关文档

- Workflow 战术设计：[../workflow/01-reasoning-graph.md](../workflow/01-reasoning-graph.md)
- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Provider 端口：[../provider/02-ports-stream-and-client-scope.md](../provider/02-ports-stream-and-client-scope.md)
