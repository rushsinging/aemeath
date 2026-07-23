# 配置分层 / Claude Code 兼容

**Scope**：`agent/shared/src/config/**`、`agent/features/config/**`、`agent/composition/**` 中的 Config wiring，以及 Runtime/SDK/TUI 的 Config 消费链——配置分层、provider 默认值、Claude Code 兼容与运行时路径。
**主触发**：改 `agent/shared/src/config/**`、`agent/features/config/**` 或 Composition Config wiring。
**次触发**：新增 `AEMEATH_*` 配置项，或改指令 / 配置 / skills / hooks 的读取优先级。

## 3.3.1. 架构（DDD + 六边形 + Clean）

配置域采用分层架构：

```
Config BC (`agent/features/config/`)
  - `ConfigAppService`：唯一 active Config 状态与 watch sender
  - `ConfigWiring`：向 Composition 发布同一 service 的 reader/query/writer/participant 窄视图
  - `ConfigReader`：同步 committed snapshot + composition-internal watch
  - `ConfigQuery` / `ConfigWriter`：AgentClient application seam
  - `ProjectConfigParticipant`：供 #871 联合切换协调

Shared Config Domain / Adapter (`agent/shared/src/config/`)
  - Domain：Config、ConfigSnapshot、ConfigPatch、PriorityChain、driver_env
  - Adapters：Env/File/CLI/兼容来源；完整 I/O/ACL 收口由 #934/#935 承接

Composition (`agent/composition/`)
  - 每个 deployable bootstrap 只调用一次 `wire_project_config`
  - 将同一 `Arc<ConfigAppService>` 的窄视图注入 Runtime

Runtime / SDK / TUI
  - Runtime bootstrap 通过 injected `ConfigReader` 捕获 committed snapshot
  - model switch/list 复用同一 reader，NEVER 重新 load 文件
  - TUI/CLI 只经 AgentClient + SDK Config DTO，NEVER 持 Config 契约/watch
```

### 3.3.1.1. 层间依赖规则

| 层 / 边界 | 可依赖 | 不可依赖 |
|---|---|---|
| Shared Domain | 标准库 + serde | fs、env、网络、Config application service |
| Config BC | Shared Config domain/adapters | Runtime、TUI、Project 内部类型 |
| Composition | Config/Runtime/其他 feature 窄 façade | 业务执行时回读全局 service |
| Runtime | injected ConfigReader/Query/Writer + ConfigSnapshot PL | 构造 ConfigAppService、直接 load 文件 |
| TUI / CLI | AgentClient + SDK Config DTO | ConfigReader/Query/Writer/participant/watch |

## 3.3.2. 配置分层（优先级从高到低）

1. CLI 参数（启动期永久覆盖）
2. 环境变量（启动期覆盖，由 `EnvAdapter` 统一读取）
3. Local config：项目 `.agents/aemeath.json`、兼容 `.claude/settings.json`，以及 AgentClient 动态更新持久化的项目级补丁
4. Global config（`~/.agents/aemeath.json`）
5. 硬编码默认值

**MUST** 合并顺序固定为 `Default → Global → Local → Env → CLI`。动态 Config 更新属于 Local 层，提交 candidate 前必须重新应用 Env 与 CLI；CLI/Env 未提供对应字段时，动态 Local 更新才可生效。**NEVER** 新增高于 CLI/Env 的 runtime/native override 层。

**关键约束**：业务 env **NEVER** 在 config 包外读取。`check-config-env-guard.sh` 守卫强制执行此约束。消费方通过 `ConfigReader` port 或 `ConfigView` 获取配置值。

## 3.3.3. 环境变量

### 3.3.3.1. 业务配置 env

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

### 3.3.3.2. 日志级别

`AEMEATH_LOG_LEVEL` 替代了旧的 `RUST_LOG`，支持两种写法：

```
AEMEATH_LOG_LEVEL=info                                           # 全局级别
AEMEATH_LOG_LEVEL=aemeath:tui=debug,aemeath:agent:runtime=trace  # per-target
```

`set_max_level` 跟随 directive 中的最宽松级别（不再被 `Info` 硬闸门挡死）。

### 3.3.3.3. Driver-specific API key env

全部 provider-specific key 与 `AEMEATH_API_KEY` / `LLM_API_KEY` 均由 Config BC `EnvAdapter` 一次读取并进入 `ConfigPatch`；Config application 与 Runtime **NEVER** 再读 env：

| driver | env |
|---|---|
| anthropic | `ANTHROPIC_API_KEY` |
| openai | `OPENAI_API_KEY` |
| deepseek | `DEEPSEEK_API_KEY` |
| minimax | `MINIMAX_API_KEY` |
| mimo | `MIMO_API_KEY` |
| volcengine | `VOLCENGINE_CODING_PLAN_API_KEY` |
| zhipu / litellm | 无 driver-specific env |

优先级：driver-specific → `AEMEATH_API_KEY` → `LLM_API_KEY`。`OPENAI_API_KEY` 只作为 openai driver-specific key，不再作为其他 driver 的全局 fallback。

### 3.3.3.4. 系统级 env（白名单）

以下 env 不属于业务配置，不在 `EnvAdapter` 管辖范围：

| env | 用途 | 读取位置 |
|---|---|---|
| `HOME` | 用户主目录 | 全局 |
| `AEMEATH_AGENTS_DIR` | 运行时根目录 | `config/adapters/paths.rs` |
| `AEMEATH_VERSION` | 版本号（编译期注入） | `build.rs` |

## 3.3.4. Provider 默认值

- 每个 provider 的默认 base URL、默认 model、API key 环境变量名定义在 `agent/shared/src/config/models/`（`types.rs`、`resolve.rs`）与 `agent/shared/src/config/legacy.rs`。
- driver→env name 映射定义在 `agent/shared/src/config/domain/driver_env.rs`（单一真相）。
- **NEVER** 硬编码 API key、base URL；新增 provider 的默认值在此补充（实现层见 `provider.md`）。

## 3.3.5. TUI 纯展示层约束

TUI **NEVER** 直接读 config/env。config 变更通过事件流推送给 TUI：

- runtime 通过 `ChangeSet::CONFIG` flag 通知 config 变更
- TUI 调用 `AgentClient::config_view()` 获取 `ConfigView`（只读快照）
- `ConfigView` 包含 TUI 需要的展示字段（model name、provider、api_key 状态、permission mode 等）

## 3.3.6. Claude Code 兼容

- 项目指令读取 **MUST** 从 cwd 向上 5 级祖先目录搜索，每层级 `CLAUDE.md` 优先于 `AGENTS.md`，找到第一个存在的文件即停止（break 语义）；全局指令优先 `~/.agents/AGENTS.md`，不存在时 fallback `~/.claude/CLAUDE.md`。目录发现逻辑共享 `paths::project_instruction_dirs`，config reload snapshot 监控同一组路径。
- 项目配置读取 **MUST** `.agents/aemeath.json` 优先，其次兼容 `.claude/settings.json`；Claude Code hooks 结构需转换为 Aemeath hooks（转换逻辑在 `agent/shared/src/config/hooks.rs`）。
- 项目 skills 读取 **MUST** `.claude/skills` 优先，其次 `.agents/skills`；同名 skill 以 Claude Code 项目 skill 为准。
- Hook 执行环境的 `AEMEATH_PROJECT_DIR` / `CLAUDE_PROJECT_DIR` 注入在 hook 域，见 `policy-hook-audit.md`。

## 配置应用作用域（#1345）

有效 JSON Config 的 reload 只发布新的 committed `ConfigSnapshot`；不同字段按最早安全边界应用：

| 作用域 | 字段 / 资源 | 生效规则 |
|---|---|---|
| Session restart-required | `ui.tui`、日志 sink/目录/rotation、skills 目录、storage 路径 | 活跃 session 不重建；Runtime 标记 pending revision 并经 SDK/TUI 提示，成功 resume 后清除。 |
| Run | provider/model/API key/base URL/timeout、reasoning、context size、permissions（含 `allow_all`）、hooks、roles、并发、tool-result policy、memory 注入和 language | 每个新 Main Run / 新 Subagent Run 捕获一次；已运行 Run 不变化。 |
| Run Step | 不单独 reload Config；仅消费所属 Run 的 frozen snapshot | 每个 invocation 可更新 messages/tool schemas/task reminder/token budget，但 `ConfigSnapshot` revision 不变。 |

`allow_all` **MUST** 属于 Run 级，**NEVER** 在同一 Run 的 tool round / Step 中改变授权语义。guidance、`AGENTS.md`、`CLAUDE.md` 是 prompt assets，不属于 JSON Config：内容变化仅在下一 Run 注入带路径的 Read reminder，**NEVER** 重建 cacheable system prompt。

## 3.3.7. ConfigSnapshot 统一入口

### 3.3.7.1. 设计原则

`ConfigAppService::load()` 初始化唯一 committed state。消费方通过 Config-owned `ConfigReader::committed_snapshot()` 获取 `ConfigSnapshot`；非 Run query/update 通过 `ConfigQuery` / `ConfigWriter`，TUI/CLI 只见 SDK DTO。

- `ConfigAppService::new()` 只允许在 Config BC 的 production factory 内调用；Composition 只调用 `wire_project_config()`。
- Runtime 通过注入的 `Arc<dyn ConfigReader>` 获取 snapshot，NEVER 构造或 load service。
- `ConfigWiring` 的 reader/query/writer/participant 都 clone 同一 `Arc<ConfigAppService>`；Runtime 持有这些 Arc，因此 wiring 局部变量释放不会结束 service/watch 生命周期。
- model switch/list 复用同一 committed reader，不重复文件 I/O。
- `check-config-reader-injection.sh` 对 Runtime/TUI/CLI 生产源码零例外；测试文件和文件尾 `#[cfg(test)] mod tests { ... }` 按类别排除，NEVER 整文件放行生产路径。

### 3.3.7.2. ConfigSnapshot accessor 列表

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

Logging 不再暴露完整 `LoggingConfig` 子结构；Composition 只通过细粒度 accessor 映射不可变 `LoggingSettings`。

### 3.3.7.3. free fn 签名改造

随着 `ConfigSnapshot` 成为统一入口，部分原先直接接受 `&Config` 或散点参数的 free fn 已改为接受 `&ConfigSnapshot` 或被其 accessor 替代：

- **已删除**：直接返回裸 `Config` 的消费入口，以及 Runtime 内部 `ConfigAppService::new/load`。
- **读取入口**：bootstrap/model switch/list 通过同一 `ConfigReader::committed_snapshot()`；AgentClient 查询通过 async `ConfigQuery::snapshot()`。
- **消费方改造**：业务代码只使用 `ConfigSnapshot` accessor，保证封装性；Config `reasoning_graph` 已退役，NEVER 恢复旧 accessor。
