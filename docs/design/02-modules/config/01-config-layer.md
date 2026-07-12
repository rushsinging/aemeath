# Config · 分层与 Published Language

> 层级：02-modules / config（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#792（S2）
> 本文定义 Config 的分层优先级链、ConfigSnapshot 作为 Published Language、ConfigReader/ConfigAppService 端口边界、adapter 接入路径、reasoning 静态阈值。Config 是通用域 BC。

## 1. 定位

Config 是 **通用域 BC**——为所有其他 BC 提供配置真相：

- **ConfigSnapshot 是 Published Language**：跨 BC 的配置契约，通过 watch channel 推送
- **ConfigReader 是出站端口**：消费方（Runtime / TUI / Tool 等）通过此端口获取配置
- **ConfigAppService 是应用服务**：编排 adapter、合并配置、推送 snapshot
- **不包含业务逻辑**——Config 只承载配置数据，不做业务决策

## 2. ConfigPort / ConfigReader trait

```rust
trait ConfigReader: Send + Sync {
    /// 获取当前配置快照（同步）
    fn snapshot(&self) -> ConfigSnapshot;
    /// 订阅配置变更（异步 watch channel）
    fn watch(&self) -> watch::Receiver<ConfigSnapshot>;
}
```

```rust
trait ConfigWriter: Send + Sync {
    /// 更新配置（闭包修改 + 持久化 + push snapshot）
    async fn update<F>(&self, f: F) -> Result<()>
    where F: FnOnce(&mut Config) + Send;
    /// 设置 CLI 覆盖（最高优先级）
    fn set_cli_patch(&self, patch: ConfigPatch);
}
```

### 2.1 消费方接口

| 方法 | 用途 | 消费方 |
|---|---|---|
| `snapshot()` | 获取当前配置 | Runtime 初始化、TUI 渲染、Tool 装配 |
| `watch()` | 订阅配置变更 | Runtime 热更新、TUI 动态刷新 |
| `set_cli_patch()` | CLI 参数覆盖 | CLI 启动 |
| `update()` | 运行时修改配置 | `/config` 命令、设置面板 |

## 3. Config 分层

### 3.1 优先级链

从低到高（后者覆盖前者）：

```
Config::default()           ← 内置默认值
  ↓ apply_patch
ClaudeSettingsAdapter       ← ~/.claude/settings.json（兼容 Claude Code）
  ↓ apply_patch
FileAdapter (global)        ← ~/.agents/aemeath.json
  ↓ apply_patch
FileAdapter (project)       ← <project>/.agents/aemeath.json
  ↓ apply_patch
EnvAdapter                  ← AEMEATH_* 环境变量
  ↓ apply_patch
CliArgsAdapter              ← CLI 参数（--model 等）
  ↓
resolve_provider_api_keys   ← driver_env 后处理
  ↓
Config (in-memory)
  ↓
ConfigSnapshot::new(Arc::new(config))
  ↓
watch::Sender::send_replace → 所有 watch Receiver
```

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
    // ... 14 个 section
    hooks: Option<HooksConfig>,            // 整块覆盖（非 patch 粒度）
}
```

- 14 个 section 走 `apply_patch`（字段级合并）
- `hooks` 和 `reasoning_graph` 是例外——整块覆盖，不走 patch 粒度
  - **hooks**：语义是合并事件表，key 级合并在 `merge_hooks` 中做
  - **reasoning_graph**：后续粒度可能细化，当前占位

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
```

## 4. ConfigSnapshot — Published Language

### 4.1 设计决策（#586）

ConfigSnapshot 持有 `Arc<Config>`，但字段全私有，只暴露只读 accessor 方法：

```rust
struct ConfigSnapshot {
    inner: Arc<Config>,
}

impl ConfigSnapshot {
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

### 4.2 watch channel 推送

```rust
// ConfigAppService 内部
fn push_snapshot(&self, config: Config) {
    let snapshot = ConfigSnapshot::new(Arc::new(config));
    self.tx.send_replace(snapshot);  // send_replace 而非 send
}
```

> **关键**：使用 `send_replace` 而非 `send`——`send` 在无 receiver 时返回 Err 且值不更新（静默失败）。这是已修复的 bug（见 memory: `tokio::sync::watch::Sender::send` 坑）。

## 5. ConfigAppService

### 5.1 职责

```rust
struct ConfigAppService {
    config: RwLock<Config>,
    tx: watch::Sender<ConfigSnapshot>,
    cli_patch: RwLock<ConfigPatch>,
}
```

| 方法 | 职责 |
|---|---|
| `load()` | 编排 adapter 读取 → 合并 → resolve keys → push snapshot |
| `update(closure)` | 闭包修改 Config → 写回 global_path → push snapshot |
| `set_cli_patch(patch)` | 设置 CLI 覆盖 → 触发 reload |

### 5.2 load() 目标流程

```rust
async fn load(&self) -> Result<()> {
    let mut base = Config::default();

    // 1. 编排 adapter（目标：fs IO 在 adapter 内部，不在 AppService）
    if let Some(patch) = ClaudeSettingsAdapter::read(&global_claude_path).await? {
        base = merge_config(base, vec![patch]);
    }
    if let Some(patch) = FileAdapter::read(&global_config_path).await? {
        base = merge_config(base, vec![patch]);
    }
    if let Some(patch) = FileAdapter::read(&project_config_path).await? {
        base = merge_config(base, vec![patch]);
    }
    if let Some(patch) = EnvAdapter::read() {
        base = merge_config(base, vec![patch]);
    }
    let cli = self.cli_patch.read().unwrap().clone();
    base = merge_config(base, vec![cli]);

    // 2. 后处理
    resolve_provider_api_keys(&mut base);

    // 3. 写入 + push
    *self.config.write().unwrap() = base.clone();
    self.push_snapshot(base);

    Ok(())
}
```

### 5.3 update() 流程

```rust
async fn update<F>(&self, f: F) -> Result<()>
where F: FnOnce(&mut Config)
{
    let mut config = self.config.read().unwrap().clone();
    f(&mut config);

    // 持久化（写整份 Config 到 global_path）
    let global_path = paths::global_config_path();
    if let Some(parent) = global_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let json = serde_json::to_string_pretty(&config)?;
    tokio::fs::write(&global_path, json).await?;

    // push snapshot
    *self.config.write().unwrap() = config.clone();
    self.push_snapshot(config);

    Ok(())
}
```

> `update()` 总是写整份 Config 到 global_path——不管闭包改了什么。后续可能改为写 patch + 全量 fallback。

## 6. adapter 接入

### 6.1 当前问题

三个 adapter 是 stub，`ConfigAppService.load()` 直接调 `tokio::fs` 绕过 adapter：

| Adapter | 状态 | 问题 |
|---|---|---|
| `EnvAdapter` | ✅ 完整实现 | 无 |
| `FileAdapter` | ❌ stub | `load()` 直接读文件，不调 `FileAdapter::read()` |
| `CliArgsAdapter` | ❌ stub | `load()` 用 `cli_patch` RwLock，不调 `CliArgsAdapter::read()` |
| `ClaudeSettingsAdapter` | ❌ stub | `load()` 直接读文件，不调 `ClaudeSettingsAdapter::read()` |

### 6.2 目标

- `FileAdapter::read(path)` — 接收路径，读文件 → 反序列化 `ConfigPatch`
- `ClaudeSettingsAdapter::read(path)` — 读 Claude settings.json → 转换为 `ConfigPatch`
- `CliArgsAdapter::read(args)` — 从 CLI 参数构造 `ConfigPatch`
- `ConfigAppService.load()` 只编排 adapter + 合并，不做 fs IO

### 6.3 迁移动作

1. 实现 `FileAdapter::read(path) -> Option<ConfigPatch>`（从 AppService 的内联 fs IO 提取）
2. 实现 `ClaudeSettingsAdapter::read(path) -> Option<ConfigPatch>`（从 AppService 的内联 Claude 适配提取）
3. 实现 `CliArgsAdapter::read(args) -> ConfigPatch`（从 `cli_patch` RwLock 提取）
4. `ConfigAppService.load()` 改为调 adapter
5. 删除 AppService 中的内联 `tokio::fs::read_to_string`

## 7. reasoning 静态阈值

### 7.1 ReasoningGraphConfig

```rust
struct ReasoningGraphConfig {
    enabled: bool,
    nodes: HashMap<ReasoningNode, NodeConfig>,
    max_reasoning: Option<ReasoningLevel>,   // 用户配置上限
}

struct NodeConfig {
    default_effort: ReasoningLevel,
    override_effort: Option<ReasoningLevel>,
}
```

### 7.2 静态阈值的含义

- 节点 effort 映射是**静态配置**——从 config 文件读取，不动态计算
- `max_reasoning` 是用户配置的 reasoning level 上限
- **当前问题**：`max_reasoning` 已解析存储但从未生效——只有 provider ceiling 在 clamp
- **目标**：`max_reasoning` 接入 ReasoningPort 的 clamp 链（见 [../workflow/01-reasoning-graph.md](../workflow/01-reasoning-graph.md) §5）

### 7.3 Config 中的 reasoning 相关字段

| 字段 | 位置 | 用途 |
|---|---|---|
| `model.reasoning` | `ModelEntryConfig` | 模型是否支持 reasoning |
| `model.reasoning_effort` | `ModelEntryConfig` | 默认 reasoning effort |
| `reasoning_graph.enabled` | `ReasoningGraphConfig` | 是否启用 ReasoningGraph |
| `reasoning_graph.nodes` | `ReasoningGraphConfig` | 节点 effort 映射 |
| `reasoning_graph.max_reasoning` | `ReasoningGraphConfig` | 用户配置上限（目标接入 clamp） |

## 8. driver_env — 环境变量后处理

### 8.1 职责

`driver_env` 是 config 合并后的后处理步骤：

```rust
fn resolve_provider_api_keys(config: &mut Config) {
    // 对每个 provider，如果 API key 未在 config 中设置，
    // 从环境变量查找（AEMEATH_<PROVIDER>_API_KEY / <PROVIDER>_API_KEY）
    for provider in &mut config.providers {
        if provider.api_key.is_none() {
            provider.api_key = driver_env::lookup_api_key(&provider.name);
        }
    }
}
```

- **不进 patch 层**——跨 adapter/domain 边界的后处理
- 逻辑抽到 `domain/driver_env.rs`，干净独立

## 9. 现状缺口与迁移动作

| 目标 | 现状 | 迁移动作 |
|---|---|---|
| adapter 接入调用链 | ⚠️ 三个 stub 未接入 | 实现 FileAdapter / ClaudeSettingsAdapter / CliArgsAdapter，AppService 改为调 adapter |
| fs IO 移到 adapter | ⚠️ AppService 内联 `tokio::fs` | 从 AppService 提取 fs IO 到 adapter |
| `max_reasoning` 接入 clamp | ⚠️ 已解析未生效 | 接入 ReasoningPort clamp 链 |
| `update()` 写整份 Config | ⚠️ 不分 patch | 后续可改为写 patch + 全量 fallback |
| `LOG_TARGET` 未使用 | ⚠️ dead_code | S2 logging 合流时启用 |
| `reasoning_graph` 无 patch 粒度 | ⚠️ 整块覆盖 | 后续细化，当前占位 |
| `CliArgsAdapter::read()` 返回空 patch | ⚠️ `set_cli_patch` 手动注入 | 接入 clap 直接结果 |

## 10. 相关文档

- Workflow 战术设计（ReasoningPort + clamp 链）：[../workflow/01-reasoning-graph.md](../workflow/01-reasoning-graph.md)
- Runtime 端口（ConfigReader = Runtime 出站端口）：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Provider 端口（模型 reasoning 配置）：[../provider/02-ports-stream-and-client-scope.md](../provider/02-ports-stream-and-client-scope.md)
- 上下文地图（Config = 通用域 BC）：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- 系统架构（Composition Root 装配 ConfigAppService）：[../../01-system/04-system-architecture.md](../../01-system/04-system-architecture.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：Config 分层、ConfigSnapshot PL、ConfigReader/ConfigAppService、adapter 接入、reasoning 静态阈值 | #792 |
