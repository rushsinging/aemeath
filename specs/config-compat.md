# 配置分层 / Claude Code 兼容

**Scope**：`agent/shared/src/config/**`、`agent/features/runtime/src/application/config_app_service.rs`、`agent/features/runtime/src/ports/config.rs`——配置分层、provider 默认值（base URL / 默认 model / env 名）、Claude Code 兼容、运行时路径。
**主触发**：改 `agent/shared/src/config/**` 或 `config_app_service.rs` / `config_port.rs`。
**次触发**：新增 `AEMEATH_*` 配置项，或改指令 / 配置 / skills / hooks 的读取优先级。

## 架构（DDD + 六边形 + Clean）

配置域采用分层架构：

```
Domain 层 (share/config/domain/)
  纯数据 + 纯函数，不接触 fs/env/网络
  - Config 聚合根
  - ConfigSnapshot 只读视图（Arc<Config>，accessor）
  - ConfigPatch + PriorityChain 合并策略
  - driver_env driver→API key env name 映射

Port 层 (runtime/ports/config.rs)
  ConfigReader trait（async snapshot + watch）
  ※ async_trait 是行为，不属于 share kernel

Adapter 层 (share/config/adapters/)
  把外部格式翻译成 ConfigPatch
  - EnvAdapter 唯一业务 env 读取点
  - CliArgsAdapter / FileAdapter / ClaudeSettingsAdapter（stub）

Application Service (runtime/application/config_app_service.rs)
  ConfigAppService 编排 adapter 链 load/merge/watch/update/reload
  resolve_provider_api_keys：per-provider API key 从 env 注入
```

### 层间依赖规则

| 层 | 可依赖 | 不可依赖 |
|---|---|---|
| Domain (share) | 标准库 + serde | 任何外部 crate、fs、env |
| Port (runtime) | Domain + async_trait | 具体 adapter 实现 |
| Adapter (share) | Domain + 外部 crate | 反向被依赖 |
| AppService (runtime) | Port + Domain + adapter | 反向被依赖 |

## 配置分层（优先级从高到低）

1. CLI 参数（`--provider`、`--model` 等）
2. 环境变量（`AEMEATH_*`、`*_API_KEY` 等）——由 `EnvAdapter` 统一读取
3. 项目级配置：`.agents/aemeath.json` 优先，其次兼容 `.claude/settings.json` 的 hooks 配置
4. 全局配置（`~/.agents/aemeath.json`）
5. Claude Code settings.json（ACL 兼容层）
6. 硬编码默认值

**关键约束**：业务 env **NEVER** 在 config 包外读取。`check-config-env-guard.sh` 守卫强制执行此约束。消费方通过 `ConfigReader` port 或 `ConfigView` 获取配置值。

## 环境变量

### 业务配置 env

以下 env 由 `EnvAdapter` 统一读取，进入 `ConfigPatch`：

| env | 说明 |
|---|---|
| `AEMEATH_PROVIDER` | provider 名称 |
| `AEMEATH_API_KEY` / `LLM_API_KEY` | 全局 API key |
| `AEMEATH_BASE_URL` / `LLM_BASE_URL` | API base URL |
| `AEMEATH_MODEL` | 模型名称 |
| `AEMEATH_MAX_TOKENS` | 最大输出 token |
| `AEMEATH_CONTEXT_SIZE` | 上下文窗口大小 |
| `AEMEATH_PERMISSION_MODE` | 权限模式（ask / auto_read / allow_all） |
| `AEMEATH_MAX_TOOL_CONCURRENCY` | 工具并发数 |
| `AEMEATH_MAX_AGENT_CONCURRENCY` | 子代理并发数 |
| `AEMEATH_VERBOSE` | verbose 模式 |
| `NO_COLOR` | 禁用颜色 |
| `AEMEATH_LOG_LEVEL` | 日志级别（全局级别或 per-target directive） |

### 日志级别

`AEMEATH_LOG_LEVEL` 替代了旧的 `RUST_LOG`，支持两种写法：

```
AEMEATH_LOG_LEVEL=info                                           # 全局级别
AEMEATH_LOG_LEVEL=aemeath:tui=debug,aemeath:agent:runtime=trace  # per-target
```

`set_max_level` 跟随 directive 中的最宽松级别（不再被 `Info` 硬闸门挡死）。

### Driver-specific API key env

Per-provider 的 driver-specific API key env 在 `ConfigAppService::load()` 的 `resolve_provider_api_keys` 后处理中注入：

| driver | env |
|---|---|
| anthropic | `ANTHROPIC_API_KEY` |
| openai | `OPENAI_API_KEY` |
| deepseek | `DEEPSEEK_API_KEY` |
| minimax | `MINIMAX_API_KEY` |
| mimo | `MIMO_API_KEY` |
| volcengine | `VOLCENGINE_CODING_PLAN_API_KEY` |
| zhipu / litellm | 无 driver-specific env |

fallback 链：driver-specific → `LLM_API_KEY` → `OPENAI_API_KEY`。

### 系统级 env（白名单）

以下 env 不属于业务配置，不在 `EnvAdapter` 管辖范围：

| env | 用途 | 读取位置 |
|---|---|---|
| `HOME` | 用户主目录 | 全局 |
| `AEMEATH_AGENTS_DIR` | 运行时根目录 | `config/adapters/paths.rs` |
| `AEMEATH_LOG_STDERR` | 日志输出到 stderr（CLI 模式） | `logging_setup.rs` |
| `AEMEATH_VERSION` | 版本号（编译期注入） | `build.rs` |

## Provider 默认值

- 每个 provider 的默认 base URL、默认 model、API key 环境变量名定义在 `agent/shared/src/config/models/`（`types.rs`、`resolve.rs`）与 `agent/shared/src/config/legacy.rs`。
- driver→env name 映射定义在 `agent/shared/src/config/domain/driver_env.rs`（单一真相）。
- **NEVER** 硬编码 API key、base URL；新增 provider 的默认值在此补充（实现层见 `provider.md`）。

## TUI 纯展示层约束

TUI **NEVER** 直接读 config/env。config 变更通过事件流推送给 TUI：

- runtime 通过 `ChangeSet::CONFIG` flag 通知 config 变更
- TUI 调用 `AgentClient::config_view()` 获取 `ConfigView`（只读快照）
- `ConfigView` 包含 TUI 需要的展示字段（model name、provider、api_key 状态、permission mode 等）

## Claude Code 兼容

- 项目指令读取 **MUST** 从 cwd 向上 5 级祖先目录搜索，每层级 `CLAUDE.md` 优先于 `AGENTS.md`，找到第一个存在的文件即停止（break 语义）；全局指令优先 `~/.agents/AGENTS.md`，不存在时 fallback `~/.claude/CLAUDE.md`。目录发现逻辑共享 `paths::project_instruction_dirs`，config reload snapshot 监控同一组路径。
- 项目配置读取 **MUST** `.agents/aemeath.json` 优先，其次兼容 `.claude/settings.json`；Claude Code hooks 结构需转换为 Aemeath hooks（转换逻辑在 `agent/shared/src/config/hooks.rs`）。
- 项目 skills 读取 **MUST** `.claude/skills` 优先，其次 `.agents/skills`；同名 skill 以 Claude Code 项目 skill 为准。
- Hook 执行环境的 `AEMEATH_PROJECT_DIR` / `CLAUDE_PROJECT_DIR` 注入在 hook 域，见 `policy-hook-audit.md`。

## ConfigSnapshot 统一入口

### 设计原则

`ConfigAppService::load()` 不再返回裸 `Config`。消费方必须通过 `ConfigReader` port 的 `snapshot()` 方法获取 `ConfigSnapshot`（`Arc<Config>` 的只读封装，内部字段全私有，只暴露 accessor 方法）。

- `ConfigAppService::new()` 只允许在 **composition 装配根**（`agent/composition/src/`）调用。
- runtime 消费方通过注入的 `Arc<dyn ConfigReader>` 获取 `ConfigSnapshot`，不得直接构造 `ConfigAppService`。
- 架构守卫 `check-config-reader-injection.sh` 强制执行此约束（例外：`from_args.rs` / `trait_model.rs` 暂未改造注入）。

### ConfigSnapshot accessor 列表

`ConfigSnapshot` 暴露以下只读 accessor：

| accessor | 返回类型 | 说明 |
|---|---|---|
| `api_key()` | `Option<&str>` | 全局 API key |
| `base_url()` | `Option<&str>` | API base URL |
| `provider()` | `Option<&str>` | provider 名称 |
| `model_name()` | `&str` | 模型名称 |
| `max_tokens()` | `u32` | 最大输出 token |
| `context_size()` | `usize` | 上下文窗口大小 |
| `resolve_context_size(...)` | `usize` | 带 override 的上下文窗口解析 |
| `permission_mode()` | `PermissionModeConfig` | 权限模式 |
| `allow_all()` | `bool` | 是否允许全部操作 |
| `max_tool_concurrency()` | `usize` | 工具并发上限 |
| `max_agent_concurrency()` | `usize` | 子代理并发上限 |
| `logging_level()` | `&str` | 日志 filter directive |
| `logs_dir()` | `Option<&str>` | 日志目录 |
| `logging_max_bytes()` | `u64` | 单文件轮转阈值 |
| `logging_max_backups()` | `usize` | 轮转备份数 |
| `logging_retention_days()` | `u64` | 日志保留天数（由 #939 lifecycle 消费） |
| `verbose()` | `bool` | verbose 模式 |
| `color()` | `bool` | 颜色输出 |
| `markdown()` | `bool` | Markdown 渲染 |
| `tui()` | `bool` | TUI 模式 |
| `memory_enabled()` | `bool` | 记忆功能 |
| `persist_sessions()` | `bool` | 会话持久化 |
| `language()` | `&str` | 语言设置 |
| `resolve_model_selection(...)` | — | 模型选择解析 |
| `models()` | `&ModelsConfig` | models 配置 |
| `agents()` | `&AgentsConfig` | agents 配置 |
| `hooks()` | `&HooksConfig` | hooks 配置 |
| `memory()` | `&MemoryConfig` | memory 配置 |
| `skills()` | `&SkillsConfig` | skills 配置 |

Logging 不再暴露完整 `LoggingConfig` 子结构；Composition 只通过上述五个细粒度 accessor 映射不可变 `LoggingSettings`。

### free fn 签名改造

随着 `ConfigSnapshot` 成为统一入口，部分原先直接接受 `&Config` 或散点参数的 free fn 已改为接受 `&ConfigSnapshot` 或被其 accessor 替代：

- **已删除**：直接返回 `Config` 的构造/合并函数（消费方不再持有裸 `Config`）。
- **签名变更**：`load()` 返回 `ConfigSnapshot`（经 `ConfigReader::snapshot()`），不再返回 `Config`。
- **消费方改造**：原先直接读 `config.field` 的代码改为 `snapshot.accessor()`，保证封装性。
