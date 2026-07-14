# 代码组织规范

> 层级：01-system（系统级总体设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：[#972](https://github.com/rushsinging/aemeath/issues/972)
> 本文定义 aemeath 的代码组织决策、按证据启用的结构选项与边界判据。**只描述目标态，不记录当前代码状态或迁移进度。**

## 1. 决策

> **aemeath 采用 capability-first modular monolith + use-case colocation + ports on demand。**

这项决策包含三层含义：

1. 仓库与 crate 内部首先按稳定业务能力组织，让顶层名称直接表达系统做什么；
2. 单个能力内部优先把同一用例一起变化的代码共置，而不是先拆成横向技术层；
3. 只有真实外部 seam 出现时才定义 port，只有 §3.6 至少一个强边界收益成立时才拆 crate。

Simon Brown 的 [Package by Component](https://simonbrown.je/modular-monolith/) 说明了能力优先组织、窄组件入口和减少公开类型的价值。aemeath **MUST** 让依赖图、Rust 可见性和公开 façade 证明边界；目录名称只能帮助导航，**NEVER** 单独充当架构证据。

### 1.1 四种方法各自回答什么

| 方法 | 回答的问题 | 对 aemeath 的约束 | 不等于 |
|---|---|---|---|
| [DDD 战略设计](https://www.domainlanguage.com/ddd/reference/) | 哪些语言、责任和变化应属于同一边界 | **MUST** 用子域、Bounded Context、统一语言和 Context Map 识别能力边界 | 固定目录模板；也不要求每个模块都拥有复杂领域模型 |
| Hexagonal | 哪些交互跨越应用内外，如何隔离外部参与者 | 易变 detail 的出站 seam **MUST** 由消费策略拥有目的明确的 port；对其他能力开放的入站 façade / OHS **MUST** 由供应能力拥有 | 每个函数一个接口；每个模块都有 port 目录；左右或上下分层图 |
| Clean | 源码依赖应指向哪里 | 依赖 **MUST** 从易变技术细节指向稳定能力策略 | 固定数量、名称或物理目录的同心层；原文也明确圆环数量只是示意 |
| Vertical Slice | 一次业务变化应集中在哪里 | 同一请求或用例的校验、编排和局部转换 **SHOULD** 共置，切片之间 **SHOULD** 低耦合 | 复制共享不变量；取消 Bounded Context；把每个函数独立成切片 |

[Hexagonal 原文](https://alistair.cockburn.us/hexagonal-architecture) 强调 inside/outside 隔离和按交互目的定义 port，而非层数；[Clean Dependency Rule](https://blog.cleancoder.com/uncle-bob/2012/08/13/the-clean-architecture.html) 要求源码依赖指向更高层策略；[Vertical Slice Architecture](https://www.jimmybogard.com/vertical-slice-architecture/) 以变化轴为共置轴。这些原则相互补充，但 **NEVER** 被解释为同一套目录树。

### 1.2 非目标

- 本文 **NEVER** 为所有能力规定相同的子目录。
- 本文 **NEVER** 要求一个 Bounded Context 对应一个 crate，也 **NEVER** 因文件数量增加就拆 crate。
- 本文 **NEVER** 为“未来可能替换”预建 port、空 façade 或无消费者的抽象。
- 本文 **NEVER** 允许模块以“规模小”为理由让核心策略依赖 HTTP、数据库、文件系统、进程或 UI 细节。
- 本文 **NEVER** 记录旧路径、临时兼容层或迁移完成度；这些内容 **MUST** 只进入 Migration Governance。

## 2. 全局组织不变量

1. **MUST** 先确定能力所有者，再决定文件位置。无法说清所有者的代码 **NEVER** 进入通用共享目录。
2. 能力模块 **MUST** 默认私有，只通过一个窄 façade 暴露稳定命令、查询、事件或 Published Language。
3. 模块内部条目 **SHOULD** 默认使用私有或受限可见性；只有真实消费者需要的稳定表面才 **MAY** `pub`。Rust 的默认私有、`pub(crate)` / `pub(super)` 与受控 `pub use` 规则见 [Rust Reference](https://doc.rust-lang.org/reference/visibility-and-privacy.html)。
4. 一次用例变化需要共同修改的策略、校验、局部类型和测试 **SHOULD** 共置；仅因代码“类型相同”而分散到全局技术目录是 **NEVER** 允许的组织依据。
5. 跨能力依赖 **MUST** 只指向对方公开 façade 或 Published Language，**NEVER** 直接引用对方内部实现。
6. 当确认稳定能力策略若不抽象便会依赖易变外部细节时，该策略 **MUST** 定义目的性出站 port；供其他能力调用的入站 façade / OHS **MUST** 由供应能力发布。尚未形成真实边界时，模块 **MAY** 保持私有具体依赖，但 **NEVER** 让该依赖越过能力 façade，也 **NEVER** 预建 port。
7. 具体实现选择与 factory 调用 **MUST** 收敛到 Composition Root；能力 **MAY** 用 composition-only opaque factory 构造模块私有 detail，但内部 **NEVER** 自行读取全局配置、选择候选生产实现或从业务路径触发 factory。
8. 公共抽象 **MUST** 有真实消费者与契约测试；没有行为差异的转发接口 **SHOULD** 内联或删除。
9. 架构边界 **MUST** 尽可能由编译器和机械守卫验证；评审约定 **NEVER** 是循环依赖、越界 import 或公开面膨胀的唯一防线。

## 3. 按证据启用的结构选项

```text
基础粒度：扁平能力模块 → 内聚的用例 / 能力子模块
独立选项：
  ├── 可选：跨用例共享不变量的 model
  ├── 可选：真实外部 seam 的 port
  ├── 可选：按 provider / protocol 命名的技术目录
  └── 可选：满足至少一个强边界收益的 crate
```

这是一组按证据触发的升格选项，**NEVER** 是每个模块必须走完的成熟度等级。基础粒度先在扁平模块与用例 / 能力子模块之间选择；model、port、技术目录和 crate **MUST** 分别独立评估，可以任意组合，也可以永远不出现。证据消失后结构 **SHOULD** 降级或合并。

### 3.1 第一级：扁平能力模块

单一职责、少量文件、一个主要用例或一组紧密行为 **MUST** 先保持扁平：

```text
capability.rs       # 窄 façade
capability/
├── execute.rs      # 用例及其局部类型
└── error.rs        # 仅在多个文件共同消费时存在
```

- **MUST** 把测试放在被测行为附近。
- **NEVER** 为对称美观预建空目录。
- 当一次修改经常跨越三个以上互不相关的职责，或不同用例拥有独立词汇、状态与测试夹具时，**SHOULD** 进入第二级。

### 3.2 第二级：用例或稳定能力子模块

子模块名称 **MUST** 表达用例或稳定能力，例如 `start_run`、`tool_coordination`、`stream_completion`，**NEVER** 只表达代码的技术形态。

```text
capability.rs
capability/
├── create_item.rs
├── create_item/
│   └── validation.rs
├── inspect_item.rs
└── inspect_item/
    └── projection.rs
```

- 当一组文件因同一业务变化共同修改、可独立测试且对外只需一个入口时，**SHOULD** 引入用例子模块。
- 只有一个函数或只有文件长度变化、但无独立词汇与行为边界时，**NEVER** 为其创建子模块。
- 两个切片出现相似代码时，**MUST** 先判断它是偶然相似还是共同不变量；**NEVER** 仅为消除几行重复就建立共享核心。

### 3.3 可选 `model.rs` / `model/`

`model` 是共享业务不变量的家，不是类型垃圾桶。

**引入判据**：同时满足以下条件时 **MAY** 引入：

- 同一概念被两个或更多用例消费；
- 概念拥有必须始终成立的业务不变量、状态迁移或强类型约束；
- 把行为留在各用例会导致规则复制或不一致；
- model 可在不依赖外部技术类型的情况下单元测试。

**不引入判据**：以下情况 **NEVER** 单独建立 `model`：

- 只有一个用例消费的请求、响应或中间值；
- provider wire type、数据库 row、UI view model 或序列化 DTO；
- 只有字段、没有共同业务行为的数据袋；
- 仅为缩短文件或追求目录对称。

共享不变量缩回单一用例后，类型与行为 **SHOULD** 回到该用例，空壳 `model` **MUST** 删除。

### 3.4 可选 port

Port 表达能力边界上一段有业务目的的对话，所有权分两类：隔离易变外部 detail 的**出站 port**由消费策略拥有；供其他能力调用的**入站 façade / OHS**由供应能力拥有并发布稳定 Published Language。出现下列信号时 **SHOULD** 评估对应边界，但信号本身 **NEVER** 自动要求增加抽象。

出站 port 的候选证据：

- 能力策略必须在无网络、文件系统、进程、时钟或 UI 的情况下运行和测试；
- 已存在两个实现，或已有获批交付要求需要第二个实现；
- 外部依赖慢、非确定、会失败，需要可控替身验证策略；

入站 façade / OHS 的候选证据：

- 其他能力已需要调用供应方拥有的稳定命令或查询；
- 跨 Bounded Context 交互需要由供应方发布稳定语言，或需要在调用入口做防腐转换；
- 两个以上消费者需要同一能力，但 **NEVER** 因此获得供应方内部类型。

一旦确认稳定能力策略若不抽象就会依赖易变外部细节，该策略 **MUST** 定义出站 port。尚未形成这一策略 / 细节边界时，模块 **MAY** 保持私有具体依赖，并 **NEVER** 为假设中的未来替换预建 port。

已引入的出站 port **MUST** 由消费策略按目的命名并拥有，例如 `CompletionProvider`、`WorkspaceRepository`、`EventSink`；它 **SHOULD** 靠近消费外部交互的用例。入站 façade / OHS **MUST** 由供应能力拥有，例如 Runtime-owned `AgentClient` 与 Project-owned `WorkspaceRead` / `WorkspaceControl` / `WorkspacePersist`；消费方 **NEVER** 再包一层同义 façade。只有多个稳定 port 需要独立导航时才 **MAY** 建 `ports.rs` 或 `ports/`。

以下情况 **NEVER** 引入 port：纯模块内 helper、稳定且确定的语言库调用、只包一层同签名转发、没有替换或隔离需求的“以防将来”。当 port 不再保护策略、测试或演进 seam 时，**SHOULD** 内联并删除。

### 3.5 可选技术目录

外部实现 **SHOULD** 诚实地按 provider、协议或产品名称组织，例如 `anthropic/`、`openai_compatible/`、`sse/`、`git/`，而不是用含义不明的总括目录隐藏变化来源。

**MAY** 引入技术目录的条件：

- 同一技术拥有多个共同变化的 wire type、错误映射、连接生命周期或协议测试；
- 目录边界 **MUST** 把技术依赖与其余能力隔离；
- 已形成 §3.4 的明确 seam 时，技术实现 **MUST** 终止在该 seam：出站 adapter 实现消费策略拥有的 port，入站 adapter 调用供应能力拥有的 façade / OHS；边界尚未形成时，具体依赖 **MUST** 保持私有。三种情况对外都 **NEVER** 泄漏 wire type。

单文件即可讲清的集成 **MUST** 保持为 `anthropic.rs` 之类的文件；纯业务代码、跨技术共享策略和只有名称相同的 helper **NEVER** 放入技术目录。技术被移除后，其专属目录 **MUST** 连同死转换与配置入口一起退役。

### 3.6 可选 crate

模块只有在至少一个强边界收益成立时才 **MAY** 升格为 crate：

- 必须由编译器禁止反向依赖或限制高成本依赖传播；
- Published Language 已稳定，并被多个 crate、应用或独立进程消费；
- 具有独立生命周期、构建目标、平台约束或 feature / dependency budget；
- 独立发布、复用或安全审计边界已有明确需求。

文件多、团队多人、测试慢、名称重要或“每个能力一个 crate” **NEVER** 单独构成拆分理由。提议新 crate 时 **MUST** 同时说明：所有者、公开 API、允许依赖、禁止依赖、消费者、循环检查与退役路径。若双方只有单一消费者、总是锁步变化，且本节列出的强边界收益均不成立，**MUST** 保持同 crate 私有模块。

Rust visibility 与 Go module layout 支持“先用语言级隐私保持简单、确有编译边界后再拆”的方向；生命周期、构建目标、平台、feature / dependency budget、独立发布与安全审计则是 aemeath 根据自身交付约束形成的综合工程判据，**NEVER** 冒充任一来源的原文结论。

## 4. aemeath 非规范性逻辑投影

本节只投影 §3 的组织决策，帮助比较不同复杂度的逻辑形状；它 **NEVER** 定义模块的具体战术边界、物理 Target 路径或强制文件拆分。Policy、Provider 与 Runtime 的具体战术命名分别以 [Policy 模块设计](../02-modules/policy/README.md)、[Provider 模块设计](../02-modules/provider/README.md) 和 [Runtime 模块边界](../02-modules/runtime/02-module-boundaries.md) 为真相源；若逻辑投影与模块战术设计冲突，**MUST** 以后者为准。

以下 Rust 树统一使用 Rust 2018+ 的 `capability.rs` + `capability/...` 模块布局，**NEVER** 表示每个同类模块都必须复制相同形状。

### 4.1 小型 Policy：保持扁平

```text
policy.rs           # PolicyRequest、PolicyDecision 与窄评估 façade
policy/
└── allow_all.rs    # AllowAll 行为及就近单元测试
```

此投影对应 [Policy 模块设计](../02-modules/policy/README.md) 的小型评估能力：`PolicyRequest` 与 `PolicyDecision` 由 façade 发布，`AllowAll` 行为就近组织。小型 Policy **SHOULD** 保持扁平，**NEVER** 为尚不存在的规则引擎预建 model、port 或技术目录；未来多个决策用例共享真实规则不变量时才 **MAY** 提取 model。

### 4.2 Provider：按 provider / protocol 命名技术集成

```text
provider.rs                 # 窄 façade
provider/
├── capability.rs           # driver + model 能力解析
├── invoke.rs               # 单次调用与统一流语义
├── transport.rs            # endpoint / auth / HTTP transport
├── error.rs                # wire / HTTP 错误映射
├── anthropic.rs            # 单文件起步
├── anthropic/              # 仅在 request / stream 已独立变化时展开
│   ├── request.rs
│   └── stream.rs
└── openai_compatible.rs
```

此投影与 [Provider 模块设计](../02-modules/provider/README.md) 对齐。`anthropic` 按 provider 命名，`openai_compatible` 按协议命名；这些技术子模块 **MUST** 私有，并把 wire type 与错误转换收在边界内。Runtime 消费方 **MUST** 拥有目的性 provider port，Provider 实现该 port；Provider **NEVER** 把 HTTP / SSE 类型作为统一 façade 的 Published Language。若某集成只有一个文件，**MUST** 使用同名 `.rs` 文件而非空壳目录；不再为工厂单建 `api.rs`，composition-only wiring 从根 façade 受控导出。

### 4.3 复杂 Runtime：按 agent_run / loop_engine / coordination 能力组织

```text
runtime.rs
runtime/
├── agent_client.rs
├── agent_run.rs
├── agent_run/
│   ├── state.rs
│   └── step.rs
├── loop_engine.rs
├── loop_engine/
│   ├── drive.rs
│   └── stuck_guard.rs
├── model_invocation.rs
├── model_invocation/
│   └── retry.rs
├── tool_coordination.rs
├── tool_coordination/
│   └── approval.rs
├── context_coordination.rs
├── context_coordination/
│   └── window.rs
├── interaction.rs
└── event_projection.rs
```

此投影沿用 [Runtime 模块边界](../02-modules/runtime/02-module-boundaries.md) 的 `agent_client`、`agent_run`、`loop_engine`、`model_invocation`、`tool_coordination`、`context_coordination`、`interaction` 与 `event_projection` 战术命名。`agent_client` 是稳定入站能力，不是通用 `api` 层；`agent_run` 拥有生命周期不变量，`loop_engine` 驱动单个 Run，各 coordination / invocation 模块封装独立编排能力。它们 **NEVER** 互相装配或穿透内部类型，Loop Engine **MUST** 只经各自 façade 协调它们。外部 seam 的 port **SHOULD** 靠近实际消费方；只有多个模块共享同一 Run 不变量时才 **MAY** 抽取共享 model。

## 5. 跨生态参照

示例树均为帮助理解边界机制的精简投影，**NEVER** 是 aemeath 的复制模板。

### 5.1 JVM：Spring Modulith

```text
com.example/
├── order/
│   ├── OrderManagement.java     # 模块 API
│   ├── internal/...
│   └── spi/
│       ├── package-info.java    # @NamedInterface("spi")
│       └── ...
└── inventory/
    ├── InventoryManagement.java
    └── internal/...
```

- **边界机制**：[Spring Modulith fundamentals](https://docs.spring.io/spring-modulith/reference/fundamentals.html) 默认以直接子包识别 application module，根包作为默认 API，子包默认内部；示例中的 `spi` 只有经 `package-info.java` 声明 `@NamedInterface("spi")` 才成为命名接口，目录名本身不扩大公开面。Named Interface 显式扩大公开面，allowed dependencies 则收窄允许依赖；[Verification](https://docs.spring.io/spring-modulith/reference/verification.html) 可检查模块环和非法依赖。
- **借鉴**：aemeath **MUST** 使用能力根、窄入口和机械依赖验证。
- **未照搬**：aemeath **NEVER** 复制 Java 包可见性、注解或固定 `internal` / `spi` 命名；Rust module privacy 与 guard 承担同类职责。

### 5.2 .NET：eShop + Vertical Slice

```text
src/
├── Ordering.API/
│   └── Application/
│       └── Orders/
│           └── CreateOrder/       # Vertical Slice 概念投影
│               ├── CreateOrderCommand.cs
│               ├── CreateOrderCommandHandler.cs
│               └── CreateOrderValidator.cs
├── Ordering.Domain/
└── Ordering.Infrastructure/
```

- **边界机制**：[dotnet/eShop `src`](https://github.com/dotnet/eShop/tree/main/src) 以服务和 `.csproj` 建编译边界，Ordering 再用独立项目约束依赖；[Microsoft DDD guidance](https://learn.microsoft.com/en-us/dotnet/architecture/microservices/microservice-ddd-cqrs-patterns/ddd-oriented-microservice) 同时指出复杂领域服务与简单数据服务应采用不同复杂度。[Vertical Slice](https://www.jimmybogard.com/vertical-slice-architecture/) 则把 request 视为独立用例，沿变化轴共置关注点。上图的 `CreateOrder/` 是把两者组合后的概念投影，**NEVER** 声称是 eShop 仓库的逐字目录。
- **借鉴**：aemeath **SHOULD** 外层按稳定能力划界、内部按用例共置，并用编译依赖保护真正独立的核心。
- **未照搬**：aemeath **NEVER** 要求每个能力复制 eShop 的项目三分法，也 **NEVER** 强制 CQRS、Mediator 或每请求一个类型体系。

### 5.3 Go：官方 module layout

```text
project/
├── go.mod
├── capability.go
├── internal/
│   ├── auth/...
│   └── hash/...
└── cmd/
    └── app/main.go
```

- **边界机制**：[Go 官方布局指南](https://go.dev/doc/modules/layout) 从根目录单 package 起步，复杂度增长后才增加 supporting package；`internal` 由工具链禁止其父目录树之外的代码导入。对 server project，指南进一步示范把已经适合跨项目复用的 package 拆为独立 module；这是一种适用场景，**NEVER** 被提升为 Go module 的唯一拆分门槛。
- **借鉴**：aemeath **MUST** 先保持扁平，并把编译器可见性当作边界；只有 §3.6 至少一个强边界收益成立时才 **MAY** 拆 crate。
- **未照搬**：aemeath **NEVER** 复制 `cmd` / `internal` 名称或 Go 的“一目录一 package”约束。

### 5.4 Rust：rust-analyzer

```text
crates/
├── syntax/          # 独立 API boundary
├── hir-*/           # 深度协作的内部计算能力
├── hir/             # façade / API boundary
├── ide-*/           # 大型独立 IDE 能力
└── ide/             # 对客户端的 façade
```

- **边界机制**：[rust-analyzer architecture](https://rust-analyzer.github.io/book/contributing/architecture.html) 明确标注哪些 crate 是 API Boundary、哪些永远不是；`hir` / `ide` 作为 façade，内部 crate 按语义计算和 IDE 能力拆分。
- **借鉴**：aemeath **SHOULD** 为公开边界给出明确语言，并让内部能力依赖图服务于不变量与增量变化。
- **未照搬**：aemeath **NEVER** 按 rust-analyzer 的 crate 数量、编译器流水线或 Salsa 约束拆分；crate 升格仍需满足 §3.6。

### 5.5 Rust：Helix

```text
helix/
├── helix-core/
├── helix-view/
├── helix-term/
├── helix-lsp/
└── helix-tui/
```

- **边界机制**：[Helix workspace](https://github.com/helix-editor/helix/blob/master/Cargo.toml) 声明 subsystem-named workspace members；[`helix-term` manifest](https://github.com/helix-editor/helix/blob/master/helix-term/Cargo.toml) 显式声明对 core、view、LSP 与 TUI 等成员的依赖，由 Cargo manifest 形成可检查的 crate dependency graph。
- **借鉴**：Helix 直接提供“子系统命名 + manifest 依赖图”的事实；aemeath 据此推论，跨运行时或稳定能力边界确实需要编译隔离时 **MAY** 使用清楚表达能力的 crate 名称。
- **未照搬**：aemeath **NEVER** 因 Helix 使用多 crate 就把每个内部 coordinator 升格；锁步变化的 Runtime 能力 **SHOULD** 先留在同 crate。

### 5.6 C++：Chromium components

```text
components/foo/
├── BUILD.gn
├── DEPS
├── DIR_METADATA
├── OWNERS
├── README.md
├── browser/...
├── common/...
└── renderer/...
```

- **边界机制**：[Chromium `//components` 规则](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/components/README.md) 要求服务代码库至少两个合适使用位置的 component 不能依赖更高层，组件依赖必须在 `DEPS` 显式声明且不得成环；进程目录只在代码确实跨进程时出现。
- **借鉴**：aemeath **MUST** 显式约束依赖方向和环，并 **SHOULD** 在新 crate 提案中写明所有者与允许依赖。
- **未照搬**：aemeath **NEVER** 复制 Chromium 的进程目录、GN 元数据、代码规模或“至少两个使用位置”的 component 门槛。

## 6. 机械边界与评审顺序

每次新增或重组能力时 **MUST** 分三阶段评审，**NEVER** 把可选结构误作线性成熟度门禁。

### 6.1 先选基础粒度

1. **MUST** 先用统一语言说明能力所有者；无法说明所有者时，**MUST** 停止并重新划定边界。
2. 单一用例或紧密行为 **MUST** 选择扁平模块；已有独立词汇、共同变化与测试边界时，才 **SHOULD** 选择用例 / 能力子模块。

这一步只决定扁平或子模块粒度，**NEVER** 预先决定是否需要 model、port、技术目录或 crate。

### 6.2 独立评估四类可选结构

| 结构 | 独立问题 | 结论 |
|---|---|---|
| model | 是否已有跨用例共享业务不变量 | 是则 **MAY** 引入；否则 **NEVER** 引入 |
| port | 是隔离易变 detail 的出站 seam，还是供应能力的真实入站 OHS | 出站由消费策略拥有；入站由供应能力拥有；两者都无真实边界则 **NEVER** 预建 |
| 技术目录 | 同一 provider / protocol 是否已有多文件共同变化与隔离价值 | 是则 **MAY** 引入；否则 **MUST** 保持单文件 |
| crate | 是否满足 §3.6 至少一个强边界收益 | 是则 **MAY** 升格；否则 **MUST** 保持同 crate 私有模块 |

四项 **MUST** 分别依据 §3 的证据判断；任一项为“否”只表示不引入该结构，**NEVER** 阻断其他三项。

### 6.3 最后验证边界

最终结构 **MUST** 通过以下检查：module privacy 与受控 re-export 锁住公开面；façade 只发布稳定语言；依赖图无越界和环；具体实现只由 Composition Root 选择和发起装配；architecture guard 能机械验证适用规则。评审 **NEVER** 以目录看起来整齐替代可见性、依赖图、守卫和测试证据。

## 7. 决策追溯

| 最终决策 | 主要参考 | 借鉴 | 未照搬 |
|---|---|---|---|
| 采用 capability-first modular monolith | [Package by Component](https://simonbrown.je/modular-monolith/)；[Spring Modulith](https://docs.spring.io/spring-modulith/reference/fundamentals.html) | 能力顶层、窄公开面、组件内部隐藏 | Java package 规则、框架注解、统一内部目录 |
| 用例代码沿变化轴共置 | [Vertical Slice Architecture](https://www.jimmybogard.com/vertical-slice-architecture/) | 切片内高内聚、切片间低耦合 | 强制 CQRS / Mediator；禁止共享真实不变量 |
| 真实 seam 形成时按方向确定 port 所有权 | [Hexagonal Architecture](https://alistair.cockburn.us/hexagonal-architecture)；aemeath [Context Map](03-context-map.md) 的 OHS 关系 | Hexagonal 提供 inside/outside 与目的性 port；aemeath 综合 Context Map 决定出站 port 归消费策略、入站 façade / OHS 归供应能力 | 每用例一个 port、固定端口数量、固定外层目录；不把仓库综合决策冒充原文结论 |
| 依赖由技术细节指向能力策略 | [Clean Architecture](https://blog.cleancoder.com/uncle-bob/2012/08/13/the-clean-architecture.html) | Dependency Rule、依赖反转、边界数据归内侧所有 | 固定圆环数量与名称、物理分层模板 |
| 先 module privacy，存在强边界收益时再 crate | [Rust visibility](https://doc.rust-lang.org/reference/visibility-and-privacy.html)；[Go module layout](https://go.dev/doc/modules/layout)；aemeath §3.6 | 来源支持默认私有、受限公开与简单起步；生命周期、平台、发布、审计等强边界收益是 aemeath 的综合工程决策 | Go 目录约定；为每个能力预建 crate；不把本地判据冒充来源原文 |
| crate **MUST** 承载 §3.6 的强边界收益 | [rust-analyzer architecture](https://rust-analyzer.github.io/book/contributing/architecture.html)；[Helix workspace](https://github.com/helix-editor/helix/blob/master/Cargo.toml) | rust-analyzer 的 façade / API boundary；Helix 的 subsystem-named crates 与 Cargo dependency graph；能力判定是 aemeath 的综合推论 | 按参考项目的 crate 数量或内部流水线照抄 |
| 边界规则 **MUST** 机械验证 | [Spring Modulith verification](https://docs.spring.io/spring-modulith/reference/verification.html)；[Chromium components](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/components/README.md) | 环检查、允许依赖、公开面与构建图 | Spring / GN 专属工具和 Chromium 组织规模 |
| **拒绝：所有能力采用固定横向目录模板** | [Clean Architecture](https://blog.cleancoder.com/uncle-bob/2012/08/13/the-clean-architecture.html)；[Microsoft DDD guidance](https://learn.microsoft.com/en-us/dotnet/architecture/microservices/microservice-ddd-cqrs-patterns/ddd-oriented-microservice) | 保留依赖方向；复杂领域按需隔离 | 拒绝原因：把示意层误当目录，会给小模块增加仪式并掩盖能力边界 |
| **拒绝：纯切片、永不共享 model** | [Vertical Slice Architecture](https://www.jimmybogard.com/vertical-slice-architecture/)；[DDD Reference](https://www.domainlanguage.com/ddd/reference/) | 保留变化局部性 | 拒绝原因：跨用例真实不变量会复制并漂移，必须允许共同模型按证据出现 |
| **拒绝：为所有依赖预建 port** | [Hexagonal Architecture](https://alistair.cockburn.us/hexagonal-architecture) | 对真实外部参与者保持可替换和可测试 | 拒绝原因：无 seam 的转发抽象增加命名、装配与测试成本，却不隔离变化 |
| **拒绝：一个能力一个 crate** | [Go module layout](https://go.dev/doc/modules/layout)；[Helix workspace](https://github.com/helix-editor/helix/blob/master/Cargo.toml) | 需要时使用工具链边界 | 拒绝原因：锁步变化被迫跨 crate，扩大公开面、依赖管理与编译成本 |

## 8. 相关文档

- 系统架构：[04-system-architecture.md](04-system-architecture.md)
- 依赖规则与铁律：[05-dependency-rules.md](05-dependency-rules.md)
- 上下文地图：[03-context-map.md](03-context-map.md)
- 模块级设计导航：[../02-modules/README.md](../02-modules/README.md)
- 架构守卫注册表：[../03-engineering/architecture-guards.md](../03-engineering/architecture-guards.md)
- 迁移治理：[../03-engineering/migration-governance.md](../03-engineering/migration-governance.md)
- 设计目录总览：[../README.md](../README.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-14 | 初稿：确立 capability-first、用例共置、按需 port 与渐进 crate 边界；补 aemeath 及跨生态示例和决策追溯 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | 审查修订：统一 Rust 2018+ 模块布局、独立结构判据、port 强制边界、模块战术真相源与规范等级 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
