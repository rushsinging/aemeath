# Config · 模块总览

> 层级：02-modules / config（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#792（S2）/ [#972](https://github.com/rushsinging/aemeath/issues/972) / [#1457](https://github.com/rushsinging/aemeath/issues/1457)

## 文档索引

| 编号 | 文档 | 内容 |
|---|---|---|
| 01 | [config-layer.md](01-config-layer.md) | Config 分层优先级链、ConfigSnapshot PL、Config-owned reader/writer OHS、project-aware prepare / commit participant、CompatibilityAdapter ACL、reasoning 静态阈值 |
| 02 | [provider-catalog-and-connect.md](02-provider-catalog-and-connect.md) | 内置 Provider Catalog、Connect 状态机、首次聊天初始化、连接探测、User-Agent 解析与原子配置写回 |

## 定位

Config 是**通用域 BC**——为所有其他 BC 提供配置真相：

- ConfigSnapshot 是 Published Language；每个 Run 捕获一个不可变 snapshot，watch 只投影已提交的新值
- ConfigReader 只作为 Config-owned committed-state view 交给 bootstrap / MainSession façade；Run 只用 admission 时捕获的 ConfigSnapshot，非 Run query / subscribe / update 经 async gate-aware ConfigQuery / ConfigWriter
- ConfigAppService 独占 active project config；Context Management 经 `ProjectConfigParticipant` 协调切换但不复制第二份 current state
- #933 定义 AgentClient delivery seam，#871 独占 SessionSwitchGate / coordinator 与 façade 实现；TUI / CLI 只见 AgentClient 命令和 SDK 投影
- 不包含其他 BC 的业务逻辑；但拥有 Provider Catalog、Connect 向导、首次配置初始化、配置校验与持久化等配置业务规则

## Target 物理目录

Config 采用 Hexagonal + Clean 组织（`domain ← application ← ports ← adapters`）。effective-config 生命周期与 Connect 共享 Config-owned 模型和持久化能力：领域策略（merge 优先级链、Catalog、向导状态、校验、`ConfigSnapshot` 不变量）收在 `domain`；用例编排收在 `application`；Provider Probe 作为窄出站 `port`；File、Env、CLI、Runtime Override、Compatibility 与系统信息等外部 I/O 终止在 `adapters`：

```text
src/
├── lib.rs                       # 窄 façade：Config PL / OHS / composition-only wiring
├── domain.rs                    # 领域策略入口
├── domain/
│   ├── model.rs                 #   Config / Snapshot / Patch / Revision 的共同不变量
│   ├── app_service.rs           #   唯一 active state 与 prepare/commit 发布
│   ├── merge.rs                 #   优先级链
│   ├── validation.rs            #   统一校验
│   └── connect/
│       ├── catalog.rs           #   Provider Catalog 单一真相
│       ├── state.rs             #   Connect 状态机与 draft
│       └── user_agent.rs        #   Provider UA 四级解析
├── application/
│   └── connect.rs               # Connect、Probe、首次聊天初始化与 receipt 编排
├── ports/
│   ├── provider_probe.rs             # Config-owned 最小 LLM 探测出站 port
│   ├── global_config_connect_store.rs # global document create/CAS/rollback port
│   └── system_information.rs         # 全局默认 UA 所需的受控系统信息 port
└── adapters/
    ├── file.rs                  #   文件来源及 atomic/durable I/O
    ├── env.rs                   #   环境变量来源
    ├── cli_args.rs              #   CLI 参数来源
    ├── runtime_override.rs      #   运行时覆盖
    ├── compatibility.rs         #   外部配置格式 ACL；按 translator 证据再展开
    ├── global_config_file.rs    #   GlobalConfigConnectStore 的 filesystem adapter
    └── system_info.rs           #   SystemInformationPort 的平台 adapter
```

`application/` 只编排 Config-owned 用例，不吸收 Provider HTTP 或 TUI 状态；`ports/` 只表达 Connect 所需的 Provider Probe、global durable store 与系统信息窄语义，分别由 Composition 接入 Provider adapter、由 Config filesystem/platform adapters 实现。`adapters/` 只承载外部来源 I/O、wire DTO 与 ACL，**NEVER** 持有 active state 或 merge policy；`ConfigReader`、`ConfigQuery`、`ConfigWriter` 与 `ProjectConfigParticipant` 是 effective-config 生命周期的窄视图，不据此建立对应横向 port 文件。

## 相关文档

- Workflow 战术设计：[../workflow/01-reasoning-graph.md](../workflow/01-reasoning-graph.md)
- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Provider 端口：[../provider/02-ports-stream-and-client-scope.md](../provider/02-ports-stream-and-client-scope.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-21 | #1457 新增 Provider Catalog、Connect application/state machine、Provider Probe port、首次聊天初始化与 UA resolver；Config 结构扩展为最小必要的 `domain/application/ports/adapters` | [#1457](https://github.com/rushsinging/aemeath/issues/1457) |
| 2026-07-17 | #999 行为等价目录迁移：纯配置模型与策略归入 `domain/`，外部来源及路径解析归入 `adapters/`；保留既有 `share::config::*` 公共 façade，语义接线仍由 #933–#935 承接 | [#999](https://github.com/rushsinging/aemeath/issues/999) |
| 2026-07-16 | 冻结 Config Target 物理目录：扁平 effective-config 核心 + 外部来源 `adapters/` 技术目录，明确不建 `capabilities/` 或横向 `ports/` | [#972](https://github.com/rushsinging/aemeath/issues/972) |
