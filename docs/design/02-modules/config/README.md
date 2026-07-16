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

## Target 物理目录

Config 采用 Hexagonal + Clean 组织（`domain + adapters`）。effective-config 生命周期的领域策略（merge 优先级链、校验、prepare/commit 发布、`ConfigSnapshot` 不变量）收在 `domain`；File、Env、CLI、Runtime Override 与 Compatibility 等外部来源 I/O 终止在 `adapters`：

```text
src/
├── lib.rs                       # 窄 façade：Config PL / OHS / composition-only wiring
├── domain.rs                    # 领域策略入口
├── domain/
│   ├── model.rs                 #   Config / Snapshot / Patch / Revision 的共同不变量
│   ├── app_service.rs           #   唯一 active state 与 prepare/commit 发布
│   ├── merge.rs                 #   优先级链
│   └── validation.rs            #   统一校验
└── adapters/
    ├── file.rs                  #   文件来源 I/O
    ├── env.rs                   #   环境变量来源
    ├── cli_args.rs              #   CLI 参数来源
    ├── runtime_override.rs      #   运行时覆盖
    └── compatibility.rs         #   外部配置格式 ACL；按 translator 证据再展开
```

`adapters/` 只承载外部来源 I/O、wire DTO 与 ACL，**NEVER** 持有 active state 或 merge policy；`ConfigReader`、`ConfigQuery`、`ConfigWriter` 与 `ProjectConfigParticipant` 是同一能力的窄视图，不据此建立横向 `ports/`。单文件来源必须保持单文件，禁止为对称预建目录。

## 相关文档

- Workflow 战术设计：[../workflow/01-reasoning-graph.md](../workflow/01-reasoning-graph.md)
- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Provider 端口：[../provider/02-ports-stream-and-client-scope.md](../provider/02-ports-stream-and-client-scope.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-16 | 冻结 Config Target 物理目录：扁平 effective-config 核心 + 外部来源 `adapters/` 技术目录，明确不建 `capabilities/` 或横向 `ports/` | [#972](https://github.com/rushsinging/aemeath/issues/972) |
