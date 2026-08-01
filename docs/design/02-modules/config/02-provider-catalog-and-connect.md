# Config · Provider Catalog 与 Connect 向导

> 层级：02-modules / config（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：[#1457](https://github.com/rushsinging/aemeath/issues/1457)
> 本文定义内置 Provider Catalog、`aemeath connect` 应用用例、首次聊天初始化、候选配置原子提交、连接探测与 User-Agent 解析。Config 是上述配置语义的唯一所有者；TUI 仅作为入站适配器。

## 1. 目标与边界

### 1.1 目标

v0.1.0 必须提供两条共用同一 Config application 能力的入口：

1. 用户主动执行 `aemeath connect`，在 TUI 中选择内置 driver、编辑 LLM 配置、可选测试连接并保存；
2. 仅在启动聊天时发现全局配置不存在，交互终端先创建完整默认配置，再自动进入同一 Connect 向导，成功后继续聊天。

两条入口必须共享 Provider Catalog、状态机、校验、候选配置、连接探测和 durable write 协议，禁止 CLI、TUI 或 Provider Adapter 复制配置默认值与写入逻辑。

### 1.2 责任分配

| 能力 | 所有者 | 约束 |
|---|---|---|
| Provider Catalog、向导状态与迁移规则 | Config BC | 单一真相；不依赖 TUI 类型 |
| 全局配置存在性、完整默认配置初始化、候选合并与原子提交 | Config BC | Config application 依赖 Config-owned store port；路径与文件 I/O 终止于 Config filesystem adapter |
| 最小 LLM 连接探测 | Provider adapter | 实现 Config-owned `ProviderProbePort`，不持有向导状态 |
| 受控 OS 版本信息 | Config platform adapter | 实现 Config-owned `SystemInformationPort`；domain 不直接读取平台 API |
| 用例装配与 AgentClient façade | Composition / SDK | 主动 `connect` 使用 pre-chat Config façade；首次聊天只在 Connect 成功后装配 MainSession；均只投影类型化 DTO |
| 终端输入、页面切换与展示 | CLI / TUI | 不读 config/env/fs，不发 HTTP，不做业务校验 |
| 每个 Run 的最终 UA 冻结 | Runtime admission / Composition | 调用 Config-owned resolver 后注入 Provider invocation scope |

Config 不拥有正常聊天调用、流式解码或 Runtime 生命周期；Provider 不拥有 Catalog、用户默认值、向导状态或配置持久化；TUI 不拥有任何配置业务决策。Config domain 只依赖自身值对象与 port 契约，filesystem、跨进程锁、HTTP 和 OS 查询均终止于 adapter。

## 2. Provider Catalog

### 2.1 Catalog 条目

Provider Catalog 是 Config domain 的静态、版本化数据集。首版必须覆盖代码当前支持的全部 driver，而不是另建“Connect 支持列表”。每个条目至少包含：

```rust
struct ProviderCatalogEntry {
    source: ProviderSource,
    driver: DriverId,
    default_endpoint: Option<DefaultEndpoint>,
    recommended_models: Vec<RecommendedModel>,
    api_key_hint: Option<String>,
    official_sdk_user_agent: Option<OfficialSdkUserAgent>,
}

struct DefaultEndpoint {
    url: String,
    evidence_url: String,
    verified_at: Date,
}

struct RecommendedModel {
    model_id: ModelId,
    context_window: usize,
    max_tokens: u32,
    evidence_url: String,
    verified_at: Date,
}

struct OfficialSdkUserAgent {
    sdk_name: String,
    sdk_version: String,
    value: HeaderValue,
    evidence_url: String,
    verified_at: Date,
}
```

`source` 使用固定内置名称（例如 `Anthropic`、`OpenAI`），作为 `models.providers` 的稳定 key。TUI 只能展示 Catalog DTO，禁止自行拼接 source、base URL 或模型默认值。

### 2.2 Catalog 治理

- Catalog 中的 base URL、推荐模型、token 上限与 UA **必须**来自可核验来源；**禁止**把"待复核/示例/fixture/adapter 默认"伪装为正式默认。
- `default_endpoint` 与 `recommended_models` 在没有可核验证据时**必须**为 `None` / `&[]`，由 Connect 引导用户填写。
- `default_endpoint` 与 `recommended_models` 一旦非空，必须配套 `evidence_url`（非空 `https://`）与 `verified_at`（晚于 1970-01-01 的合法日期）；契约测试 `catalog_default_endpoint_evidence_is_complete_when_present` 与 `catalog_recommended_models_carry_evidence_metadata_when_present` 锁定该约束。
- 官方 SDK UA 必须精确对应所记录 SDK 版本的真实格式；没有可靠证据时 `official_sdk_user_agent` 必须为 `None`。
- SDK 版本更新必须同时更新 Catalog 条目、证据、核验日期和契约测试；运行时禁止联网查询最新版。
- Provider Adapter 只消费已解析值，禁止保留另一份默认 base URL、模型或 UA。若协议实现需要 wire 常量，该常量不得重新表达用户配置默认值。
- `ProviderModelsConfig` 必须新增可选 `userAgent` 字段；缺失字段与旧配置兼容并进入回退链，空白值在 load/Connect candidate 归一为 `None`。该字段只属于 Provider source，不复用 legacy `api.user_agent`。
- Catalog API 必须能按 `ProviderSource` 和 `DriverId` 查询，并拒绝重复 source、重复 driver 映射、非法 URL、非法 HeaderValue 与无效模型窗口。`find_by_driver` 必须就地完成大小写不敏感比较，不得先分配 `String`。

## 3. Connect 应用模型

### 3.1 类型化状态机

Connect application service 持有一次向导会话的服务端状态；客户端仅持 opaque `ConnectSessionId` 与当前 `ConnectView`：

```rust
enum ConnectStage {
    SelectProvider,
    ConfirmOverwrite,
    EditEndpoint,
    EditCredential,
    EditUserAgent,
    SelectModel,
    EditCustomModel,
    ChooseGlobalDefault,
    ChooseProbe,
    Probing,
    Review,
    Saving,
    Completed,
    Cancelled,
}

struct ConnectDraft {
    source: ProviderSource,
    driver: DriverId,
    base_url: String,
    api_key: String,
    provider_user_agent: Option<String>,
    model: ModelDraft,
    set_global_default: bool,
}
```

客户端通过 `ConnectCommand` 推进状态。每条命令必须携带 session id 与预期 revision；Config application service 对非法 stage、过期 revision、未知 Catalog 项和重复终态命令返回类型化错误，禁止由 TUI 猜测下一状态。

`Back` 也是 Config-owned 的类型化状态迁移：除首页、异步执行中和终态外，返回上一编辑阶段并保留已填写 draft；从 Review 返回时清除旧 Probe 展示结果。首页没有上一页，TUI 在首页按 `Esc` 才发送 `Cancel`，其他页面按 `Esc` 必须发送 `Back`，禁止直接终止向导。

### 3.2 向导流程

1. 列出 Catalog 中全部内置 Provider；
2. 选择固定 source；若同名 Provider 已存在，先进入覆盖确认；
3. 拒绝覆盖时返回 Provider 选择页；接受时以现有 Provider 配置预填 draft；
4. 编辑 base URL，Catalog 有默认值时预填且允许覆盖；
5. 编辑 API Key，允许空值；空值不触发环境变量检查；
6. 编辑 Provider 专属 UA，允许空值；空白归一为清除覆盖；
7. 选择推荐模型，或选择“自定义模型”并填写 Model ID、Context Window、Max Tokens；
8. 选择是否将本次模型设为全局默认；
9. 选择跳过或执行连接探测；探测成功时直接进入 review，探测失败时停留在结果页，并且必须显式选择返回编辑或继续保存；
10. 展示不含密钥明文的 review 页面并提交；
11. durable commit 成功后进入 `Completed`。

主动执行 `aemeath connect` 时，取消只产生 `Cancelled` 终态并丢弃内存 draft，配置文件保持逐字节不变。任何步骤都不得提前持久化局部字段。

### 3.3 覆盖与合并

Connect 保存的是基于开始会话时 committed global document revision 生成的完整 candidate。这里的 revision 属于 **global source document**，不是 active project 的 `ConfigSnapshot.revision`：

- 只读取并修改全局原生文档 `~/.agents/aemeath.json`；project、Compatibility、Env、CLI 与 `RuntimeOverrideAdapter` 只参与保存后的 effective-config 验证，绝不被反向序列化进 global 文档；
- 只替换或新增目标 `models.providers.<source>`；其他 Provider 与其他 section 必须保留；
- `set_global_default = true` 时同步更新全局默认模型选择；否则保持原值；
- 空 API Key 在 v0.1.0 写为该 Provider 的空 `apiKey`；Connect 不读取或提示环境变量，后续正常 Runtime bootstrap 仍由既有 EnvAdapter 优先级解析有效凭证；
- 空 Provider UA 表示删除该 Provider 的显式覆盖，而不是写入能阻断回退链的空白值；
- commit 前若 global document revision 已变化，返回 typed conflict 并要求重载，不得覆盖并发更新；
- 序列化、校验或 durable write 失败时，旧 global 文档仍是唯一真相，不发布成功 outcome。

### 3.4 Global document 写入 seam

Connect 不复用 `ConfigWriter::update`。后者只写 active project 的最高优先级 `RuntimeOverrideAdapter`，并受 MainSession exclusive gate 约束；把 Connect 塞入该命令会把全局 Provider 错写为 project override。Config BC 必须提供独立、窄的 bootstrap/application seam：

```rust
#[async_trait]
trait GlobalConfigConnectStore: Send + Sync {
    async fn load_global_document(&self) -> Result<GlobalConfigDocument, GlobalConfigLoadError>;
    async fn create_complete_default(&self) -> Result<BootstrapConfigReceipt, GlobalConfigCreateError>;
    async fn compare_and_swap(
        &self,
        expected: GlobalConfigRevision,
        candidate: GlobalConfigDocument,
    ) -> Result<GlobalConfigCommitReceipt, GlobalConfigCommitError>;
    async fn rollback_bootstrap(
        &self,
        receipt: BootstrapConfigReceipt,
    ) -> Result<(), BootstrapRollbackError>;
}
```

- 该 seam 是 Config-owned 出站 port，由 Config application 消费、filesystem adapter 实现；TUI/CLI/SDK 不得获得 store 或路径。
- 同一运行时根目录下的 create、Connect commit 与 rollback 必须使用跨进程锁和 CAS，不能只靠进程内 mutex。
- candidate 必须先通过 global document schema 校验，再以当前 project location 按 [`specs/3.9-config-compat.md` §3.9.2](../../../../specs/3.9-config-compat.md) 的规范优先级重放完整 effective-config chain；验证只读其他层，不把 Compatibility、Project、Env、CLI 或 dynamic override 的有效值写回 global 文档。
- `aemeath connect` 是 pre-chat 独立应用生命周期：commit 成功后进程可退出，不构造 active MainSession，也不发布 Config watch。
- 首次聊天初始化同样在 Config active wiring 之前运行；Connect 成功后才用新 global 文档执行正常 `wire_project_config` 并创建 MainSession，因此 active snapshot/watch 从一开始就是提交后的值。
- v0.1.0 不提供聊天中的 `/connect` 或活跃 MainSession 内 global-layer 热写；未来若新增，必须先设计 global commit 与 active project prepare/commit、Memory 重绑定及 session gate 的联合协议，禁止复用本 seam 后直接替换 live snapshot。

## 4. 连接探测

### 4.1 Port

Config application 定义满足用例所需的窄出站端口，Composition 以 Provider adapter 实现：

```rust
#[async_trait]
trait ProviderProbePort: Send + Sync {
    async fn probe(&self, request: ProviderProbeRequest)
        -> Result<ProviderProbeResult, ProviderProbeError>;
}
```

`ProviderProbeRequest` 是已校验、已解析且与 draft 隔离的值对象，包含 driver、endpoint、credential、model、token 限额与最终 UA；不得包含 TUI 类型、Config 文件路径或 active Runtime 引用。

### 4.2 语义

- Probe 发送供应商协议允许的最小 LLM 请求，并验证收到有效响应；禁止仅做 TCP、HTTP HEAD 或模型列表检查冒充连接成功。
- Probe 与正式请求复用同一 Provider request ACL、认证 Header 规则和 UA 解析结果；禁止 Connect 专用 HTTP 实现。
- Probe 是单次调用，不透明重试；取消、超时、认证、endpoint、model 与响应协议错误必须映射为稳定类别。
- API Key 为空时仍允许 Probe，以支持环境无鉴权服务；Probe 不自行读取 env。
- Probe 结果只更新向导会话，不修改 committed Config；成功结果直接推进到 `Review`，失败结果保持在 `Probing`，直到用户显式选择 `EditAfterProbeFailure` 或 `ContinueAfterProbe`。
- 错误与日志必须清洗 API Key、Authorization Header 和敏感响应正文。

## 5. User-Agent 解析

### 5.1 唯一优先级

Config domain 的 UA resolver 必须按以下顺序选择第一个非空白且合法的 HeaderValue：

1. Provider 专属配置：`models.providers.<source>.userAgent`；
2. Provider Catalog 中对应已核验官方 SDK 的默认 UA；
3. 全局配置：`api.user_agent`；
4. 全局内置默认 UA。

该顺序有意让 Provider 官方 SDK 默认高于全局配置。只要 Catalog 为该 Provider 定义了可靠官方 UA，全局配置便不会作用于它；全局配置只覆盖没有 Provider 默认 UA 的调用。空白字符串等同未配置并继续回退。

### 5.2 全局默认格式

完整格式为：

```text
Aemeath/<version> cli <os>/<os-version>/<arch>
```

例如：

```text
Aemeath/0.1.0 cli macos/15.5/aarch64
```

- `<version>` 使用编译期 Aemeath 真实版本；
- `<os>` 与 `<arch>` 使用受控平台信息；
- `<os-version>` 由 Config-owned `SystemInformationPort` 获取，平台 adapter 实现；domain 不直接调用系统 API；
- 系统版本不可用或不合法时降级为 `Aemeath/<version> cli <os>/<arch>`；
- 所有动态片段必须先规范化并通过 HeaderValue 校验，禁止换行或控制字符注入；
- UA 不得包含 API Key、用户名、主机名、项目路径或 session id。

### 5.3 生命周期

- Config resolver 是 UA 选择与合法性校验的唯一真相；Connect Probe 和正式调用都使用它。
- 每个 Main/Sub Run 在 admission 创建 Provider invocation scope 时，从该 Run 的冻结 `ConfigSnapshot` 与 Catalog 解析一次 UA；同一 Run 内不得因 config reload 改变。
- Provider Adapter 接收最终 UA，不读取全局 Config、不查询 Catalog，也不自行 fallback。
- 非 Provider 的 Aemeath HTTP 请求继续使用其所属模块定义的全局 UA 语义，不套用 Provider Catalog 层。

## 6. 首次聊天初始化

### 6.1 触发条件

自动初始化只能在“启动聊天”入口执行。`--help`、`--version`、配置检查、其他子命令和非聊天 SDK 用例不得因为配置缺失而创建文件或唤起 TUI。

流程如下：

1. Config adapter 检查全局 `~/.agents/aemeath.json` 是否存在；设置 `AEMEATH_AGENTS_DIR` 时相对该运行时根目录寻址；
2. 文件存在时按正常 bootstrap 读取，绝不覆盖；
3. 文件不存在且 stdin/stdout 不是交互 TTY 时，不创建文件，返回 typed `InteractiveSetupRequired`，CLI 提示用户在交互终端运行 `aemeath connect`；
4. 文件不存在且为交互 TTY 时，Config BC 通过原子创建写入完整默认配置；
5. 创建成功后启动 Connect，标记该会话拥有一个 `BootstrapConfigReceipt`；
6. Connect 成功提交后继续聊天；
7. 用户取消时，使用 receipt 条件删除本次创建且仍未被外部修改的默认配置，然后退出，不进入聊天。

### 6.2 完整默认配置

初始化必须序列化 `Config::default()` 对应的完整、合法、当前 schema 文档，而不是最小骨架或 CLI 手写 JSON。默认文档生成与普通 Config codec 共用同一类型与字段命名，避免默认值漂移。

### 6.3 创建与回滚安全

- 初始化使用 create-new 语义，禁止覆盖在检查后由其他进程创建的文件；竞争失败时重新读取现有配置。
- `BootstrapConfigReceipt` 至少绑定路径、创建 identity 与内容 digest；取消时只有 identity/digest 仍匹配才允许删除。
- 默认配置创建后若 Connect 保存失败，保留可诊断错误并按“本会话是否创建且文件是否未变”决定回滚；不得删除其他进程已修改的配置。
- 初始化、Connect 保存与回滚复用 Config-owned atomic/durable adapter；CLI/TUI 禁止直接调用文件系统。
- successful commit 或 receipt 失配后必须消费/失效 receipt，避免后续取消误删有效配置。

## 7. AgentClient、SDK 与 TUI

### 7.1 入站 façade

Composition 向 AgentClient 注入 Config-owned pre-chat Connect façade；该 façade 不要求 MainSession 已存在，也不暴露 `ConfigReader`、`ConfigWriter` 或 global store。SDK 发布稳定 DTO 与命令：

- `StartConnect { origin }`；
- `ApplyConnectCommand { session_id, revision, command }`；
- `CancelConnect { session_id, revision }`；
- `ConnectView`、`ConnectFailureView` 与终态 outcome。

`origin` 仅区分 `ExplicitCommand` 与 `FirstChatBootstrap(receipt)` 的取消/继续策略；它不改变向导业务流程。

### 7.2 TUI 约束

TUI 的 Connect 界面遵循既有 TEA 管线：SDK DTO → ACL → Intent → Change → Effect → AgentClient command → result Intent。TUI 可以维护输入框、焦点、遮罩与本地提交中状态，但必须满足：

- Provider 列表、默认值、当前 stage、可执行动作和校验错误完全来自 `ConnectView`；
- API Key 输入使用遮罩，review 与诊断不显示明文；
- TUI reducer 维护显式控件焦点与文本 cursor offset：纵向 `SingleSelect` 使用 `↑/↓`，文本输入使用 `←/→` 移动真实终端光标，`Tab/BackTab` 只在存在多个可聚焦区域时切换，禁止单字段页无意义回绕并重置选择；
- 非首页 `Esc` 发送类型化 `Back`，首页 `Esc` 发送 `Cancel`；返回目标与 draft 保留语义由 Config 状态机决定；
- reducer 不解析 URL、模型限制或 UA，不判断是否可覆盖/保存；
- Effect Driver 只发送类型化命令，不读写 config/env/fs/network；
- stale revision、probe failure、persist conflict 与 rollback refusal 均按 SDK DTO 显示，不以字符串匹配推导状态。

## 8. 错误、并发与安全不变量

### 8.1 错误分类

Connect 对外错误至少区分：`InvalidTransition`、`StaleRevision`、`Validation`、`CatalogUnavailable`、`ProbeFailed`、`PersistConflict`、`PersistFailed`、`InteractiveSetupRequired` 与 `BootstrapRollbackRefused`。Adapter 错误必须在 Config/Provider 边界映射，禁止泄漏临时路径、HTTP body 或供应商 wire DTO。

### 8.2 不变量

1. 一个 Connect session 至多产生一次业务终态；终态后命令不再产生副作用。
2. Probe 成功不等于保存成功；保存成功也不伪造 Probe 成功。
3. draft 与 committed Config 严格分离，只有 durable commit 后才发布新 snapshot。
4. 主动 Connect 取消后配置逐字节不变。
5. 首次聊天取消只可删除本次创建且未变的默认配置。
6. 同名 Provider 的覆盖必须经过显式确认。
7. TUI 永远不能获得明文 committed API Key；仅当前受保护输入值可短暂存在于输入组件与类型化命令中。
8. 日志、事件、错误和 task/debug 输出不得包含密钥。
9. Provider Catalog、UA resolver、Probe 与正式调用之间不得存在重复默认值或不同优先级链。

## 9. 测试与验证

按六层模型覆盖相邻边界，禁止只用端到端测试代替中间层：

| 层 | 必须证明的行为 |
|---|---|
| L1 | Catalog 唯一性/合法性；状态转移；draft 校验；覆盖与空值语义；UA 四级优先级及降级；默认文档生成；receipt 删除条件 |
| L2 | Connect service 与 Config durable adapter；Connect service 与 ProviderProbePort；bootstrap coordinator 与 receipt；冲突/取消/故障注入 |
| L3 | AgentClient/SDK DTO 完整映射；Catalog 官方 SDK UA 证据锁定；Probe 与正式请求 Header 一致；API Key 不进入只读 DTO |
| L4 | 主动 Connect 成功/取消；拒绝覆盖后返回选择；自定义模型；跳过 Probe；Probe 失败后明确保存；首次聊天成功/取消；非 TTY |
| L5 | 真实 PTY/TUI 完成配置并进入聊天的 smoke，以及取消后进程退出与文件状态核验 |

故障注入必须覆盖序列化、create-new 竞争、rename/fsync、commit 前并发 revision 变化、Probe 取消/超时和 receipt 失配。每个场景同时断言磁盘文档、committed snapshot、SDK outcome 与 TUI 投影，不得只检查最终画面。

## 10. 迁移与取舍

### 10.1 最小补丁（不采用）

在 CLI/TUI 中直接维护 Provider 列表并写 `aemeath.json`，实现成本较低，但会复制默认值、绕过 Config durable 协议、让展示层拥有业务逻辑，并使 Probe 与正式调用产生不同请求语义。该方案复发和数据损坏风险高，仅适合作为不可接受的临时止血基线。

### 10.2 根因级方案（采用）

在 Config BC 内建立 Catalog、Connect 状态机、candidate/receipt 与 UA resolver；由 Config adapter 统一持久化，由 Provider adapter 实现窄 Probe port，由 Composition/SDK/TUI 逐层投影。成本是新增跨层契约及逐层测试，但能保持单一真相、可恢复写入和调用一致性。

### 10.3 明确延期

系统钥匙链存储 API Key 不属于 v0.1.0 范围。当前按用户确认将 API Key 明文写入 Provider `apiKey`；后续独立工作应评估 macOS Keychain、Linux Secret Service、Windows Credential Manager、Rust 依赖、无钥匙链回退与迁移兼容，未经新设计不得在本 Issue 内扩展。

## 11. 相关文档

- Config 分层与 Published Language：[01-config-layer.md](01-config-layer.md)
- Provider 模块边界：[../provider/README.md](../provider/README.md)
- Provider 端口与 Invocation Scope：[../provider/02-ports-stream-and-client-scope.md](../provider/02-ports-stream-and-client-scope.md)
- TUI 架构与数据流：[../tui/01-architecture-and-dataflow.md](../tui/01-architecture-and-dataflow.md)
- 配置规范：[`specs/3.9-config-compat.md`](../../../../specs/3.9-config-compat.md)
- 测试与覆盖：[../../03-engineering/04-testing-and-coverage.md](../../03-engineering/04-testing-and-coverage.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-21 | 冻结 Provider Catalog、Connect 状态机、首次聊天初始化、连接探测、UA 四级优先级与跨层测试边界 | [#1457](https://github.com/rushsinging/aemeath/issues/1457) |
