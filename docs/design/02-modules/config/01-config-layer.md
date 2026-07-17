# Config · 分层与 Published Language

> 层级：02-modules / config（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#792（S2）/ [#972](https://github.com/rushsinging/aemeath/issues/972)
> 本文定义 Config 的分层优先级链、ConfigSnapshot Published Language、Config-owned OHS、project-aware prepare / commit participant、adapter 接入与 reasoning 静态阈值。Config 是通用域 BC；本文**只描述目标态**，实现差距见 [迁移治理](../../03-engineering/03-migration-governance.md)。

## 1. 定位

Config 是 **通用域 BC**——为所有其他 BC 提供配置真相：

- **ConfigSnapshot 是 Published Language**：跨 BC 的只读配置契约；每个 Run 捕获一个不可变 snapshot
- **ConfigReader 是 Config-owned committed-state view**：只给 Composition bootstrap 与 MainSession façade implementation；它 **NEVER** 直接进入 AgentClient、Run 或其他 BC
- **ConfigQuery / ConfigWriter 是 gate-aware application façade**：非 Run 查询、订阅建立与更新先取得 MainSession shared / exclusive permit；TUI / CLI 只经 Runtime-owned `AgentClient` 命令和 SDK 投影访问配置
- **ProjectConfigParticipant 是 composition-only 切换能力**：Config 自己持有唯一 active project config；Context Management 只协调 prepare / commit，**NEVER** 复制第二份 active Config slot
- **ConfigAppService 是应用服务**：编排 adapter、合并配置、推送 snapshot
- **不包含业务逻辑**——Config 只承载配置数据，不做业务决策

## 2. Config-owned 契约

```rust
trait ConfigReader: Send + Sync {
    /// 只读 Config-owned 已提交 active state；调用方 MUST 处于 bootstrap，
    /// 或已经由 MainSession façade 持有 shared/exclusive permit。
    fn committed_snapshot(&self) -> ConfigSnapshot;
    fn subscribe_committed(&self) -> watch::Receiver<ConfigSnapshot>;
}

#[async_trait]
trait ConfigQuery: Send + Sync {
    async fn snapshot(&self) -> Result<ConfigSnapshot, ConfigQueryError>;
    async fn subscribe(&self) -> Result<ConfigSubscription, ConfigQueryError>;
}

struct ConfigSubscription {
    initial: ConfigSnapshot,
    changes: watch::Receiver<ConfigSnapshot>,
}

enum ConfigQueryError {
    MainSessionUnavailable,
    SessionSwitchClosed,
}
```

```rust
#[async_trait]
trait ConfigWriter: Send + Sync {
    /// 应用类型化命令；其实现是 MainSessionWiring 提供的 gate-aware façade。
    async fn update(&self, command: ConfigUpdate) -> Result<(), ConfigUpdateError>;
}

/// 跨 BC 写入命令；Config 域命令本身 NEVER 携带 session-switch 语义——
/// session-switch gate 与联合 resume/update coordinator 完全属于 #871 composition-owned
/// implementation（见 §2.2），不进入 ConfigUpdate。
enum ConfigUpdate {
    /// 运行期切换 model（经 /model 命令或 Config 写端口）；进入 §3.1 的最高优先级 runtime_override layer
    SetModel { model: String },
    /// 运行期切换 permission mode（经 /permissions 命令）；进入 §3.1 的最高优先级 runtime_override layer
    SetPermissionMode { mode: PermissionMode },
    /// 运行期切换 memory 配置；进入 §3.1 的最高优先级 runtime_override layer
    SetMemoryConfig { config: MemoryConfig },
}
```

```rust
struct MainSessionConfigFacade {
    gate: Arc<SessionSwitchGate>,        // #871 composition-owned；Config 域 NEVER 定义或构造该类型
    reader: Arc<dyn ConfigReader>,
    participant: Arc<dyn ProjectConfigParticipant>,
}

#[async_trait]
impl ConfigQuery for MainSessionConfigFacade {
    async fn snapshot(&self) -> Result<ConfigSnapshot, ConfigQueryError> {
        let _shared = self.gate.acquire_shared().await?;
        Ok(self.reader.committed_snapshot())
    }

    async fn subscribe(&self) -> Result<ConfigSubscription, ConfigQueryError> {
        let _shared = self.gate.acquire_shared().await?;
        let changes = self.reader.subscribe_committed();
        let initial = changes.borrow().clone();
        Ok(ConfigSubscription { initial, changes })
    }
}
```

`ConfigReader` 本身不拥有 session gate，避免 Config → Context / Runtime 的反向依赖；Composition 只把它封装进 `MainSessionConfigFacade`。除 wiring 尚未发布的 bootstrap 外，production 调用 **MUST** 经该 async façade：shared permit 保证 query / subscribe 建立不会落在 resume 的 Project / Config / Memory / Task 无失败提交窗口。subscription 建立后只接收 Config commit 最后一步发布的完整 snapshot；`initial` 与 receiver 在同一 shared permit 下捕获，**NEVER** 丢失切换边界。

每个 Main Run 在 admission 的 shared lease 内捕获一次 `BoundMainRun.config`，随后只使用该不可变值；Run **NEVER** 调用 `ConfigQuery`、`ConfigReader` 或 watch。非 Run 的 AgentClient query / event projection 只持 `ConfigQuery`；TUI / CLI **NEVER** 获得 `ConfigSubscription` 或 watch receiver，只接收 SDK DTO / event。

### 2.1 消费方接口

| 方法 | 用途 | 消费方 |
|---|---|---|
| `ConfigReader::committed_snapshot / subscribe_committed` | 读取 Config-owned committed state | Composition bootstrap；MainSessionConfigFacade（已经持 permit） |
| `ConfigQuery::snapshot()` | gate-aware 非 Run 配置查询 | AgentClient application implementation |
| `ConfigQuery::subscribe()` | gate-aware 建立已提交配置订阅 | AgentClient event projection；先映射成 SDK/TUI-owned DTO |
| `update()` | 运行时修改配置 | AgentClient application command → MainSession gate-aware façade |

CLI 参数覆盖是 Composition bootstrap 的 `ConfigSources.cli_args` 输入，由 `CliArgsAdapter` 在 Config wiring 发布前转换为 bootstrap 来源中的最高优先级 patch；运行期若存在 project-scoped `RuntimeOverrideAdapter`，它仍按 §3.1 排在 CLI 之后。CLI patch **NEVER** 通过运行期 `ConfigWriter` 回灌。`/config` 或设置面板只产生 AgentClient application command，TUI / CLI **NEVER** 持有 `ConfigReader`、`ConfigQuery`、`ConfigWriter`、participant、subscription 或 watch receiver。

### 2.2 #933 seam 与 #871 协调归属

| Issue | 独占范围 | 验收边界 |
|---|---|---|
| [#933](https://github.com/rushsinging/aemeath/issues/933) | 定义 `ConfigQuery` / `ConfigWriter` application seam、AgentClient command/query 与 SDK config event 映射 | 交付层只见 async façade / SDK DTO；无 raw `ConfigReader`、participant 或 watch receiver 泄漏 |
| [#871](https://github.com/rushsinging/aemeath/issues/871) | 实现 `SessionSwitchGate`、联合 resume / update coordinator 与 `MainSessionConfigFacade` shared/exclusive permit 协调 | query / subscribe 建立不能观察切换中间态；update/resume 的 watch **MUST** 最后发布 |
| [#934](https://github.com/rushsinging/aemeath/issues/934) | Config 内部 layer / adapter / validation 与 durable file protocol | 不绕过 #871 gate，也不把 I/O 放入无失败 commit |

#933 发布 seam 但 **NEVER** 自建第二把 gate 或复制 active snapshot；#871 消费该 seam 并提供唯一 gate-aware implementation，但 **NEVER** 重定义 AgentClient / SDK 配置语言。两者的依赖方向是“交付 seam 可先定义，联合协调器随后实现”；端到端验证 **MUST** 同时覆盖 DTO 映射与 shared/exclusive gate 时序。

### 2.3 ProjectConfigParticipant

`ProjectConfigParticipant` 是 Config 为 active Main session 切换发布的 composition-only 窄能力。Config **NEVER** 依赖 Project 类型：启动 / resume 的协调边界先把已验证的 Project identity 经 ACL 映射为 Config-owned `ProjectConfigLocation`（canonical search root + stable opaque key）。opaque token 的字段私有；它只允许协调器读取 location、候选 `ConfigSnapshot` 与 Memory 启动参数：

```rust
struct ProjectConfigLocation {
    canonical_search_root: PathBuf,
    key: ProjectConfigKey,
}

impl ProjectConfigLocation {
    /// Config-owned ACL constructor：协调器传入 Project 已验证的 canonical root 与稳定 identity bytes；
    /// constructor 复核绝对/canonical 约束并做域分隔 key 派生，字段仍保持私有。
    fn try_from_project_identity(
        canonical_search_root: PathBuf,
        stable_identity: &[u8],
    ) -> Result<Self, ProjectConfigLocationError>;

    fn search_root(&self) -> &Path;
    fn key(&self) -> &ProjectConfigKey;
}

#[async_trait]
trait ProjectConfigParticipant: Send + Sync {
    async fn prepare_for_project(
        &self,
        location: &ProjectConfigLocation,
    ) -> Result<PreparedProjectConfig, ProjectConfigError>;

    fn snapshot(&self) -> ConfigSnapshot;
    fn commit_project(&self, prepared: PreparedProjectConfig);

    async fn prepare_update(
        &self,
        command: ConfigUpdate,
    ) -> Result<PreparedConfigUpdate, ConfigUpdateError>;
    async fn persist_update(
        &self,
        prepared: PreparedConfigUpdate,
    ) -> ConfigPersistOutcome;
    fn commit_update(&self, ready: ReadyConfigCommit);
}

enum ConfigPersistOutcome {
    /// Durable commit point 未跨越；active Config / Memory 必须保持旧值。
    NotCommitted(ConfigPersistError),
    /// Durable truth 已是 candidate；warning 不得阻止 live publish。
    Committed(ReadyConfigCommit),
}

enum ConfigPersistError {
    Serialization(ConfigSerializationError),
    Storage(ConfigStorageErrorKind),
}

enum ConfigStorageErrorKind {
    Io,
    PermissionDenied,
    UnsupportedDurability,
    CorruptTransaction,
}

struct ReadyConfigCommit {
    /* private candidate active state + committed receipt */
    warning: Option<ConfigCommitWarning>,
}

#[derive(Clone, Copy)]
enum ConfigCommitWarning {
    PreviousPromotionPending,
    JournalCleanupPending,
}

impl PreparedProjectConfig {
    fn location(&self) -> &ProjectConfigLocation;
    fn snapshot(&self) -> &ConfigSnapshot;
    fn memory_config(&self) -> &MemoryConfig;
}

impl PreparedConfigUpdate {
    fn snapshot(&self) -> &ConfigSnapshot;
    fn memory_config(&self) -> &MemoryConfig;
}

impl ReadyConfigCommit {
    fn warning(&self) -> Option<ConfigCommitWarning>;
}
```

- `ProjectConfigLocation` 的 search root **MUST** 通过 Config-owned `try_from_project_identity` canonicalize / 复核后构造，key **MUST** 由协调 ACL 提供的完整稳定 identity bytes 经 Config constructor 做域分隔派生；Config 只把它当不透明 project scope。Config **NEVER** import `ProjectIdentity` / `WorkspaceRead`，也 **NEVER** 自行读取 cwd。adapter 只能经 `location.search_root()` 获取只读寻址起点，不能接收任意裸 project path。
- `prepare_for_project` 是 async fallible prepare：它 **MUST** 完成目标 location 下 `.agents/aemeath.json`、兼容配置、global / env / CLI layers 的读取、合并与 schema 校验，并验证候选 provider / tool / hook 装配所需输入；它 **NEVER** 修改 live state、发送 watch 事件或懒加载文件。
- `commit_project` **MUST** 同步、无 I/O、无失败：一次替换 Config-owned `{ location, snapshot }`，再以 `send_replace` 发布同一个 snapshot。prepare 之后的任何 fallible 工作 **NEVER** 进入提交段。
- `prepare_update` **MUST** 以当前 location 与完整 layer chain 生成候选 snapshot，并让 File adapter 准备尚未 publish 的 durable write token；它不修改 active state。协调器可先从 opaque token 读取 candidate `MemoryConfig` 并 eager-open Memory。
- `persist_update` 只在所有受影响资源均 prepare 成功后执行 durable write，并返回 typed commitment outcome。`NotCommitted` **MUST** 只在 AtomicBlob 提交点前产生，此时 active Config / Memory 保持旧值；提交点后即使 previous promotion / journal cleanup 延迟，也 **MUST** 返回 `Committed(ReadyConfigCommit { warning })`。其后 `commit_update` 同步、无 I/O、无失败，warning 只在 live publish 后进入诊断，**NEVER** 触发回滚或 `?` 早退。
- Config adapter 将 Storage / serialization 错误 ACL 为上述稳定 `ConfigPersistError`，**NEVER** 暴露文件 adapter error、路径或临时文件名。既有 journal 在新提交点前无法通过 digest 恢复时映射为 `CorruptTransaction` 并保留 Storage quarantine 诊断，**NEVER** 当作缺文件或默认配置继续。`ConfigPersistError` 全部表示本次更新未提交；本次提交点后的正常异常只能映射为 `ConfigCommitWarning`。
- durable publish 与内存提交之间是 **cancellation-shielded critical section**：协调器把 owned exclusive permit、prepared token 与 candidate Memory 一次性交给独立 owned task 后，caller 取消只能停止等待，**NEVER** 取消该 task。task 必须把 `persist_update` 跑到明确失败，或在成功后连续完成 Memory install、Config active install 与 watch publish；取消只允许在 handoff 前生效。
- `ConfigAppService` 是 active config 的唯一可变状态源。`MainSessionWiring` 只持 reader / participant view，在 shared lease 下捕获 snapshot；它 **NEVER** 再保存 active ConfigSnapshot 副本。
- project switch 与可能改变 project-scoped Memory 参数的 `ConfigWriter` 命令 **MUST** 经 `MainSessionWiring` 的 exclusive session-switch gate 协调；非 Run query / subscribe **MUST** 经 `ConfigQuery` 取得 shared permit。TUI / CLI **NEVER** 直接调用 Config 契约。AgentClient event projection 收到的只可能是完整提交后的新 snapshot，并在 SDK ACL 转换后才发往 TUI。

## 3. Config 分层

### 3.1 优先级链

从低到高（后者覆盖前者）：

```
Config::default()           ← 内置默认值
  ↓ apply_patch
CompatibilityAdapter        ← 外部 CLI 配置兼容层（ACL）
  │  ├─ Claude Code:    ~/.claude/settings.json, .claude/settings.json
  │  ├─ <其他 CLI>:     ~/.<cli>/config.json, .<cli>/config.json（远期）
  │  └─ 自动检测格式，翻译为 ConfigPatch
  ↓ apply_patch
FileAdapter (global)        ← ~/.agents/aemeath.json
  ↓ apply_patch
FileAdapter (project)       ← <project>/.agents/aemeath.json
  ↓ apply_patch
EnvAdapter                  ← AEMEATH_* 环境变量
  ↓ apply_patch
CliArgsAdapter              ← CLI 参数（--model 等）
  ↓ apply_patch
RuntimeOverrideAdapter      ← 运行期 ConfigUpdate（SetModel / SetPermissionMode / SetMemoryConfig）
  ↓
resolve_provider_api_keys   ← driver_env 后处理
  ↓
Config (in-memory)
  ↓
ConfigSnapshot::new(Arc::new(config))
  ↓
watch::Sender::send_replace → composition-internal watch Receiver → SDK event projection
```

`RuntimeOverrideAdapter` 是链上唯一的最高优先级层，只由 `ConfigWriter::update` 的 `SetModel` / `SetPermissionMode` / `SetMemoryConfig` 命令经 `prepare_update` / `persist_update` 写入；调用方 **NEVER** 绕过这两步直接构造它的 patch。持久化范围严格限定为发起命令时的 active `ProjectConfigLocation`——project-scoped，**NEVER** 跨 project 复用、**NEVER** 落入 global 层。`persist_update` 把它写进独立于 `FileAdapter (project)` 的 durable override store（与项目原生配置物理隔离的 native patch 段/journal），因此 `prepare_for_project` / `prepare_update` 每次重放（含进程崩溃后 restart 的 bootstrap 重放）都固定把它排在 `CliArgsAdapter` 之后合并：同一 project 下新的 `EnvAdapter` 读数或新的 CLI 参数 **NEVER** 覆盖已持久化的 runtime override，只有新的 `ConfigUpdate` 或显式重置命令才能替换它。`ConfigSnapshot` **NEVER** 暴露这一层的存在——消费方只看到合并后的单一有效值。

### 3.2 ConfigPatch

每个 adapter 产出 `ConfigPatch`——部分字段的覆盖：

```rust
struct ConfigPatch {
    model_name: Option<String>,
    context_size: Option<usize>,
    max_tokens: Option<usize>,
    permission_mode: Option<PermissionMode>,
    memory: Option<MemoryConfig>,
    reasoning_graph: Option<ReasoningGraphConfig>,
    env: Option<HashMap<String, String>>,  // env 注入规则（过滤专有变量后）
    // ... 14 个 section
    hooks: Option<HooksConfig>,            // 事件 key 级合并（见 merge_hooks，非整块覆盖）
}
```

- 14 个 section 走 `apply_patch`（字段级合并）
- `hooks` 和 `reasoning_graph` 都不走 14-section 的字段级 `apply_patch`，但合并算法刻意不同：
  - **hooks**：语义是合并事件表——按事件 `key` 级合并；同 key 以 patch 覆盖 base，不同 key 累加保留，算法见 `merge_hooks`（§3.3）
  - **reasoning_graph**：v0.1.0 固定整块覆盖（存在即整体替换）；未来若要字段级合并，**MUST** 先版本化其 merge 语义

### 3.3 合并算法

```rust
fn merge_config(base: Config, patches: Vec<ConfigPatch>) -> Config {
    patches.into_iter().fold(base, |mut config, patch| {
        if let Some(v) = patch.model_name { config.model_name = v; }
        if let Some(v) = patch.context_size { config.context_size = v; }
        // ... 每个 section
        if let Some(hooks) = patch.hooks { config.hooks = merge_hooks(config.hooks, hooks); }
        if let Some(rg) = patch.reasoning_graph { config.reasoning_graph = rg; }
        config
    })
}

/// hooks 合并算法：按事件 key 级合并——overlay 命中的事件 key 覆盖 base 同 key，
/// overlay 未提及的 base 事件 key 原样保留；NEVER 整块替换 events map。
fn merge_hooks(base: HooksConfig, overlay: HooksConfig) -> HooksConfig {
    let mut events = base.events;
    for (k, v) in overlay.events {
        events.insert(k, v);
    }
    HooksConfig { events }
}
```

## 4. ConfigSnapshot — Published Language

### 4.1 设计决策（#586）

ConfigSnapshot 持有 `Arc<Config>`，但字段全私有，只暴露只读 accessor 方法：

```rust
struct ConfigSnapshot {
    revision: ConfigRevision,
    inner: Arc<Config>,
}

impl ConfigSnapshot {
    pub fn revision(&self) -> ConfigRevision { self.revision }
    pub fn model_name(&self) -> &str { &self.inner.model_name }
    pub fn context_size(&self) -> usize { self.inner.context_size }
    pub fn permission_mode(&self) -> PermissionMode { self.inner.permission_mode }
    pub fn memory_config(&self) -> &MemoryConfig { &self.inner.memory }
    pub fn reasoning_graph_config(&self) -> Option<&ReasoningGraphConfig> { self.inner.reasoning_graph.as_ref() }
    // ... 30+ accessor
}
```

- **消费方拿不到 `&Config`**——无法绕过 port
- **复用 Config 字段定义**——避免重复维护
- **不采用裸 `Arc<Config>`**（暴露 pub 字段）
- **不采用独立 struct**（字段重复维护）

### 4.2 active state 与 watch channel

```rust
// ConfigAppService 内部
struct ActiveProjectConfig {
    location: ProjectConfigLocation,
    snapshot: ConfigSnapshot,
}

fn commit_project(&self, prepared: PreparedProjectConfig) {
    let snapshot = prepared.snapshot().clone();
    *self.active.write().unwrap() = prepared.into_active();
    self.tx.send_replace(snapshot);
}
```

`active` 是 Config 的唯一 current truth；watch channel 保存的是同一 committed snapshot 的只读镜像，用于通知而非独立写入。每个 snapshot 带单调 `ConfigRevision`，commit 在同一临界区先生成一个 revision、写 active，再以 `send_replace` 发布**同一值**；新订阅在 shared permit 下断言 `receiver.borrow().revision() == committed_snapshot().revision()`，不一致属于 wiring invariant violation，必须 fail-fast / 记录 error，**NEVER** 静默选择其中一个。使用 `send_replace` 而非 `send`，保证无 receiver 时 channel 内的最新值仍更新。调用方 **NEVER** 直接调用同步 `subscribe_committed`。

## 5. ConfigAppService

### 5.1 职责

```rust
struct ConfigAppService {
    active: RwLock<ActiveProjectConfig>,
    tx: watch::Sender<ConfigSnapshot>,
    cli_patch: RwLock<ConfigPatch>,
    adapters: ConfigAdapters,
}
```

`ConfigAppService` **NEVER** 以假 default project 构造。Config-owned async factory 先用未发布的 `ConfigBootstrap` 完成初始 prepare，再以已验证 candidate 一次构造 active state 与 watch sender，最后才返回 opaque wiring：

```rust
async fn wire_project_config(
    sources: ConfigSources,
    initial: &ProjectConfigLocation,
) -> Result<ConfigWiring, ProjectConfigError> {
    let bootstrap = ConfigBootstrap::new(sources);
    let prepared = bootstrap.prepare_for_project(initial).await?;
    Ok(ConfigWiring::from_prepared(bootstrap, prepared))
}
```

因此 Config wiring / 内部 `ConfigReader` 一旦存在就必为 Active；内部 **NEVER** 使用 `Option<ActiveProjectConfig>`、假 snapshot 或 `snapshot() -> Result` 把 bootstrap 状态泄漏给消费者。wiring 发布后，reader **MUST** 只被 gate-aware façade / coordinator 持有，**NEVER** 作为通用依赖注入到业务 consumer。

| 方法 | 职责 |
|---|---|
| `prepare_for_project(location)` | 编排 adapter 读取 → 合并 → resolve keys → 完整校验 → 返回 opaque candidate |
| `commit_project(prepared)` | 无失败替换 Config-owned active state → `send_replace` 同一 snapshot |
| `prepare_update / persist_update / commit_update` | 在 session gate 下分离候选验证、不可取消的 durable publish 与无失败 active commit；写入 §3.1 最高优先级 `RuntimeOverrideAdapter` 层（持久化范围仅限当前 active project）；必要时与 Memory 重绑定共同提交 |
| bootstrap CLI patch | 由 `ConfigSources` / `CliArgsAdapter` 在 wiring 发布前加入 bootstrap 来源中的最高优先级 layer；不构成运行期 Writer 命令，运行期 project-scoped `RuntimeOverrideAdapter` 仍在其后覆盖 |

### 5.2 prepare / commit 目标流程

```rust
async fn prepare_for_project(
    &self,
    location: &ProjectConfigLocation,
) -> Result<PreparedProjectConfig, ProjectConfigError> {
    let mut base = Config::default();
    let mut patches = Vec::new();

    // 所有 fallible I/O 都发生在 prepare；路径只从已验证 identity 派生。
    patches.extend(self.adapters.compatibility.read_global().await?);
    patches.extend(self.adapters.compatibility.read_project(location).await?);

    if let Some(p) = self.adapters.file.read_global().await? { patches.push(p); }
    if let Some(p) = self.adapters.file.read_project(location).await? { patches.push(p); }

    if let Some(p) = self.adapters.env.read() { patches.push(p); }
    patches.push(self.cli_patch.read().unwrap().clone());
    if let Some(p) = self.adapters.runtime_override.read_project(location).await? {
        patches.push(p); // 运行期 project-scoped override 是最终 effective layer
    }

    base = merge_config(base, patches);
    self.adapters.driver_env.resolve_provider_api_keys(&mut base)?;
    validate_project_config(&base)?;

    Ok(PreparedProjectConfig::private_new(
        location.clone(),
        ConfigSnapshot::new(Arc::new(base)),
    ))
}
```

启动时，Composition 先把 Project factory 返回的已验证 identity 映射成 `ProjectConfigLocation`，再调用 async Config factory 返回已初始化 wiring，以其 active snapshot 的 `memory_config()` 打开 Memory，最后创建 `MainSessionWiring`。factory 的 bootstrap 与运行期 participant **MUST** 复用同一 layer / validation pipeline；运行期 resume 由 Context coordinator 在 exclusive gate 内对 Project prepare token 的 identity 做同一 ACL 映射并调用 participant prepare，**NEVER** 走第二套 load 语义。

### 5.3 Config update 的联合协议

```rust
async fn update_config(
    session: &Arc<MainSessionWiring>,
    command: ConfigUpdate,
) -> Result<(), ConfigUpdateError> {
    let exclusive: OwnedSessionSwitchPermit = session.acquire_owned_exclusive().await?;
    let prepared = session.config().prepare_update(command).await?;

    // 在 durable write 之前完成依赖资源的全部 fallible prepare。
    let candidate_memory = session.memory_opener().open_for_project(
        session.workspace().project_identity(),
        prepared.memory_config(),
    ).await?;

    // 最后一个可取消点。handoff 后 owned task 即使 caller future 被 drop 也会继续。
    session.cancellation().check_before_durable_handoff()?;
    let completion = session.spawn_config_commit_critical_section(
        exclusive,
        prepared,
        candidate_memory,
    );
    completion.await?
}

async fn config_commit_critical_section(
    session: Arc<MainSessionWiring>,
    exclusive: OwnedSessionSwitchPermit,
    prepared: PreparedConfigUpdate,
    candidate_memory: Arc<dyn MemoryPort>,
) -> Result<(), ConfigUpdateError> {
    // 此 task 不继承 caller cancellation；persist 内部所有 await 都必须跑完。
    let ready = match session.config().persist_update(prepared).await {
        ConfigPersistOutcome::NotCommitted(error) => return Err(error.into()),
        ConfigPersistOutcome::Committed(ready) => ready,
    };

    // 无失败提交段：不 await、不做 I/O、不响应取消；Config watch 最后发布。
    session.install_memory(candidate_memory);
    let warning = ready.warning();
    session.config().commit_update(ready);
    drop(exclusive);
    emit_config_commit_warning_best_effort(warning);
    Ok(())
}
```

`ConfigAppService` **NEVER** 直接调用 `tokio::fs`；native patch 的 staged / durable protocol 由 File adapter 承担。`ConfigWriter` 是 #933 定义、#871 实现的 gate-aware application façade，**NEVER** 直接委托成“持久化后立刻替换 Config”的 shortcut。若某类字段不影响 Memory，coordinator **MAY** 复用当前 Memory Arc，但仍必须走同一 prepare → shielded persist → no-fail commit 骨架，避免两套更新语义。

可执行证明按注入类型分两类点位。**可被 `Result` 捕获的取消或失败** **MUST** 只注入在 handoff 前与 `persist_update` 内部每个 await 点（含 durable rename / fsync 成功但 receipt 尚未返回这一跨越提交点前的最后 await）：提交点前注入 **MUST** 只返回 `NotCommitted` 且磁盘 / Config / Memory 全旧；跨越提交点后注入 **MUST** 返回 `Committed`（可带 warning），owned critical section 随后仍完成三者全新。Memory install 与 Config install 之间属于同步无失败提交段——不 await、不做 I/O、不响应取消——**NEVER** 在此注入可被 `Result` 捕获的失败或取消；验证只允许模拟进程级 panic / crash（如测试 harness 强制终止进程），验证路径是重启后由同一 bootstrap pipeline 从 durable state 恢复，**NEVER** 期待该函数以 `Err` 返回。**NEVER** 允许磁盘已新而 outcome 仍是 `NotCommitted`，也 **NEVER** 允许 watch 先于 Memory install 观察新 snapshot。

## 6. Adapter 接入与兼容层 ACL

### 6.1 CompatibilityAdapter — 外部 CLI 配置兼容层（ACL）

外部 CLI 配置兼容不是简单的文件读取——它需要**检测格式 + 翻译**，是一层防腐蚀层（ACL）。

#### 设计

```rust
/// 外部 CLI 配置兼容层——检测格式并翻译为 ConfigPatch
struct CompatibilityAdapter;

impl CompatibilityAdapter {
    /// 全局级：按 Config 冻结的格式优先级排序；同格式再按规范化路径排序。
    async fn read_global() -> Result<Vec<ConfigPatch>> {
        let paths = discover_external_configs(&paths::global_config_dir()).await?;
        let paths = sort_by_format_precedence_then_canonical_path(paths);
        let mut patches = Vec::new();
        for path in paths {
            if let Some(patch) = Self::read_one(&path).await? {
                patches.push(patch);
            }
        }
        Ok(patches)
    }

    /// 项目级：只接收 Config-owned location。目录从最远 ancestor 到 nearest project 排序，
    /// 每个目录内再按 format precedence + canonical path 排序，因此越近、越高优先级的 patch 越晚应用。
    async fn read_project(location: &ProjectConfigLocation) -> Result<Vec<ConfigPatch>> {
        let dirs = paths::project_config_dirs(location.search_root());
        let mut patches = Vec::new();
        for dir in sort_ancestors_farthest_to_nearest(dirs) {
            let paths = discover_external_configs(&dir).await?;
            for path in sort_by_format_precedence_then_canonical_path(paths) {
                if let Some(patch) = Self::read_one(&path).await? {
                    patches.push(patch);
                }
            }
        }
        Ok(patches)
    }

    /// 读取单个文件，自动检测格式并翻译。
    /// 只把 NotFound 映射为 Ok(None)；其余 I/O 错误（PermissionDenied 等）MUST 传播。
    async fn read_one(path: &Path) -> Result<Option<ConfigPatch>> {
        let content = match tokio::fs::read_to_string(path).await {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let format = Self::detect_format(path, &content)?;
        match format {
            ConfigFormat::ClaudeCode => Ok(Some(ClaudeTranslator::translate(&content)?)),
            ConfigFormat::Cursor    => Ok(Some(CursorTranslator::translate(&content)?)),   // 远期
            ConfigFormat::Other(cli) => {
                log::info!(target: "aemeath:shared", "未支持的外部配置格式: {} (path omitted for security)", cli);
                Ok(None)
            }
        }
    }

    /// 根据文件名 + 内容特征检测格式
    fn detect_format(path: &Path, content: &str) -> Result<ConfigFormat> {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        // 按文件名 + 结构特征判断
        match name {
            "settings.json" if path.to_string_lossy().contains(".claude") => Ok(ConfigFormat::ClaudeCode),
            "settings.json" => {
                // 内容特征：Claude settings 有特定字段（permissions, env, model 等）
                if ClaudeTranslator::looks_like(&content) {
                    Ok(ConfigFormat::ClaudeCode)
                } else {
                    Ok(ConfigFormat::Other("unknown".into()))
                }
            }
            "config.json" if path.to_string_lossy().contains(".cursor") => Ok(ConfigFormat::Cursor),
            _ => Ok(ConfigFormat::Other(name.into())),
        }
    }
}

enum ConfigFormat {
    ClaudeCode,
    Cursor,         // 远期
    Other(String),  // 未识别格式
}
```

`CompatibilityAdapter` 的顺序是协议的一部分，**NEVER** 依赖文件系统遍历顺序：global 先按格式优先级、再按 canonical path 稳定排序；project 先按最远 ancestor → nearest project，再在每层使用同一稳定排序。merge 仍遵循“后者覆盖前者”，因此 nearest project 和排序中更高优先级的格式最终获胜。新增 translator 时 **MUST** 同时把格式加入这张 precedence 表并补冲突用例。

#### Translator trait

每种外部 CLI 格式实现一个 translator，将外部格式**语义翻译**为 `ConfigPatch`。翻译不只是格式转换——外部 CLI 的字段语义和 aemeath 内部模型不对等，translator 要做完整的语义映射：

```rust
trait ConfigTranslator {
    /// 快速判断内容是否属于此格式
    fn looks_like(content: &str) -> bool;
    /// 语义翻译为 ConfigPatch
    fn translate(content: &str) -> Result<ConfigPatch>;
}
```

#### ACL 防腐的两层含义

**第一层：格式检测防腐**——运行时检测文件格式，不假设输入是某种已知格式。未识别格式跳过并记录日志，不报错中断。

**第二层：内容语义翻译防腐**——外部 CLI 的字段语义和 aemeath 内部模型不对等，translator 要做完整的语义映射，而不是字段名 1:1 直通：

| 外部字段 | 内部字段 | 翻译逻辑（不是直通） |
|---|---|---|
| Claude `permissions.allow` / `permissions.deny` | `permission_mode` | Claude 的细粒度 allow/deny 规则列表 → aemeath 的 `PermissionMode` 枚举（需聚合判断） |
| Claude `env` | `env` | Claude 的 env 注入规则（含 `CLAUDE_PROJECT_DIR` 等）→ aemeath 的 env 映射（可能需过滤/重命名） |
| Claude `model` | `model_name` | Claude 的 model 别名 → aemeath 的 model ID（可能需映射表） |
| Claude `apiKeyHelper` | `providers[*].api_key` | Claude 的 key helper 脚本 → aemeath 的静态 key（语义降级，无法执行脚本时跳过） |

> **关键**：translator 的产出是 `ConfigPatch`——aemeath 内部模型。外部格式中的字段名、值类型、语义结构**不泄漏**到 Config domain。如果某个外部字段无法翻译（如 Claude 的 `apiKeyHelper` 脚本），translator 决定是降级还是跳过，而不是把原始结构塞进去。

```rust
struct ClaudeTranslator;
impl ConfigTranslator for ClaudeTranslator {
    fn looks_like(content: &str) -> bool {
        // Claude settings 有 permissions / env / model 等特征字段
        content.contains("\"permissions\"") || content.contains("\"env\"")
    }
    fn translate(content: &str) -> Result<ConfigPatch> {
        let claude: ClaudeSettings = serde_json::from_str(content)?;
        let mut patch = ConfigPatch::default();

        // model 别名 → model ID（不是直通）
        if let Some(model) = claude.model {
            patch.model_name = Some(map_claude_model_alias(&model));
        }

        // permissions 规则列表 → PermissionMode 枚举（聚合判断）
        if let Some(perms) = claude.permissions {
            patch.permission_mode = Some(translate_claude_permissions(&perms));
        }

        // env 注入规则 → env 映射（过滤 Claude 专有变量）
        if let Some(env) = claude.env {
            patch.env = Some(translate_claude_env(&env));
        }

        // apiKeyHelper 脚本 → 无法执行，降级跳过 + warn（NEVER 记录完整脚本内容）
        if let Some(_helper) = claude.api_key_helper {
            log::warn!(
                target: "aemeath:shared",
                "Claude apiKeyHelper 无法翻译（脚本执行不支持），已跳过"
            );
        }

        Ok(patch)
    }
}
```

#### 为什么是 ACL 而非普通 adapter

| 维度 | 普通 adapter | CompatibilityAdapter（ACL） |
|---|---|---|
| 职责 | 读文件 → 反序列化 | 读文件 → **检测格式** → **语义翻译** |
| 输入 | 已知格式 | 未知格式，需运行时检测 |
| 扩展性 | 新格式 = 新 adapter | 新格式 = 新 translator，adapter 不变 |
| 格式防腐 | 无——外部格式直接映射 | 有——运行时检测，未识别格式跳过 |
| 内容防腐 | 无——字段 1:1 直通 | 有——字段语义完整翻译，外部结构不泄漏 |

#### 寻址规则

在寻找 `aemeath.json` 时同时寻找外部 CLI 配置：

```
全局级：
  ~/.agents/aemeath.json        ← 原生
  ~/.claude/settings.json       ← Claude Code 兼容
  ~/.<其他cli>/config.json       ← 远期

项目级（从 project_root 向上 N 级）：
  .agents/aemeath.json          ← 原生
  .claude/settings.json         ← Claude Code 兼容
  .<其他cli>/config.json         ← 远期
```

### 6.2 其他 adapter 契约

- `FileAdapter::read(path)` — 接收路径，读 aemeath.json → 反序列化 `ConfigPatch`
- `CliArgsAdapter::read(args)` — 从 CLI 参数构造 `ConfigPatch`
- `ConfigAppService.prepare_for_project()` 只编排 adapter + 合并 + 校验，不做 fs I/O
- `FileAdapter::prepare_native_patch / commit_native_patch` — 负责 durable 写入协议，application service 不拼接物理路径

### 6.3 扩展规则

1. 新增外部 CLI 格式时只加 translator，不修改 application service 或分层顺序。
2. adapter **MUST** 输出 Config-owned `ConfigPatch` 或结构化错误，外部 wire DTO **NEVER** 泄漏进 active state。
3. 所有 project 路径只从 `ProjectConfigLocation` 的 canonical search root 派生；adapter **NEVER** import Project PL 或自行读取 process cwd。
4. 所有会影响 active state 的读取与校验在 `prepare_for_project` 完成；`commit_project` **NEVER** 触发 adapter。

## 7. reasoning 静态阈值

### 7.1 ReasoningGraphConfig

```rust
struct ReasoningGraphConfig {
    enabled: bool,
    nodes: HashMap<ReasoningNode, NodeOverrideConfig>,
    max_reasoning: Option<ReasoningLevel>,   // 用户配置上限
}

struct NodeOverrideConfig {
    override_effort: ReasoningLevel,
}
```

### 7.2 静态阈值的含义

- 节点默认 effort 映射由 Workflow 唯一拥有，Config **NEVER** 复制默认值
- `nodes` 只保存非 Idle 节点的显式 override；缺失条目使用 Workflow 默认值，Idle 固定为 Off
- 有效节点集合由 Workflow Published Language 限定为 `Idle / Explore / Plan / Execute / Verify`；目标态中未知节点或无效 effort 返回结构化校验错误。当前实现仍会忽略未知节点并对无效 effort 静默回退，校验收口由 #934 承接
- `max_reasoning` 是用户配置的 reasoning level 上限
- `max_reasoning` **MUST** 接入 ReasoningPort 的 clamp 链（见 [../workflow/01-reasoning-graph.md](../workflow/01-reasoning-graph.md) §5）

### 7.3 Config 中的 reasoning 相关字段

| 字段 | 位置 | 用途 |
|---|---|---|
| `model.reasoning` | `ModelEntryConfig` | 模型是否支持 reasoning |
| `model.reasoning_effort` | `ModelEntryConfig` | 默认 reasoning effort |
| `reasoning_graph.enabled` | `ReasoningGraphConfig` | 是否启用 ReasoningGraph |
| `reasoning_graph.nodes` | `ReasoningGraphConfig` | 节点 effort 映射 |
| `reasoning_graph.max_reasoning` | `ReasoningGraphConfig` | 用户配置上限（参与 clamp） |

## 8. driver_env — 环境变量后处理

### 8.1 职责

`driver_env` 是 config 合并后的后处理步骤：

```rust
fn resolve_provider_api_keys(config: &mut Config) -> Result<(), ProjectConfigError> {
    // 对每个 provider，如果 API key 未在 config 中设置，
    // 从环境变量查找（AEMEATH_<PROVIDER>_API_KEY / <PROVIDER>_API_KEY）
    for provider in &mut config.providers {
        if provider.api_key.is_none() {
            provider.api_key = driver_env::lookup_api_key(&provider.name);
        }
    }
    Ok(())
}
```

- **不进 patch 层**——跨 adapter/domain 边界的后处理
- 逻辑与 Config 的 layer merge 用例共置；只在它已被多个用例共同消费时才抽为私有 `driver_env.rs`，**NEVER** 为此预建通用 `domain/` 层

## 9. 相关文档

- Workflow 战术设计（ReasoningPort + clamp 链）：[../workflow/01-reasoning-graph.md](../workflow/01-reasoning-graph.md)
- Runtime 装配（每 Run 捕获 ConfigSnapshot）：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Provider 端口（模型 reasoning 配置）：[../provider/02-ports-stream-and-client-scope.md](../provider/02-ports-stream-and-client-scope.md)
- 上下文地图（Config = 通用域 BC）：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- 系统架构（Composition Root 装配 ConfigAppService）：[../../01-system/04-system-architecture.md](../../01-system/04-system-architecture.md)
- Current → Target 迁移责任：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：Config 分层、ConfigSnapshot PL、ConfigReader/ConfigAppService、adapter 接入、reasoning 静态阈值 | #792 |
| 2026-07-14 | 明确 Config-owned active state、project-aware prepare / commit participant，并与 Session gate、Memory candidate 装配闭环 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | 将非 Run query / subscribe 收口到 async gate-aware façade，明确 #933 delivery seam 与 #871 coordinator 的所有权 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | 修复 review #5/#6/#18/#19：`ConfigUpdate` 删除 `SessionSwitchGate`（gate/coordinator 明确归属 #871 composition）；hooks 字段注释与 key 级 `merge_hooks` 算法对齐；新增最高优先级 `RuntimeOverrideAdapter` layer 并定义其 project-scoped 持久化范围；无失败提交段（Memory install 与 Config install 之间）只允许 panic/crash injection，失败/取消注入收口到 handoff 前与 `persist_update` await 点 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
