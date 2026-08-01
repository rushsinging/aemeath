# Issue #1216：Hook 子进程环境隔离设计

## 1. 背景与目标

Hook 的唯一生产执行路径已经收口为：

```text
HookPort → Dispatcher → ProcessDriverExecutor → ProcessDriver
```

当前 `ProcessDriver` 通过 `Command::envs` 增加请求环境，却未调用 `env_clear`。因此 Hook 子进程仍会继承 Aemeath 父进程的完整环境，API key、token、CI 凭据等未批准变量可能被用户配置的 Hook 命令读取。

本设计的目标是：

1. Hook 子进程默认不继承父进程环境；
2. 只注入固定基础白名单和当前 invocation 的兼容变量；
3. 消除 Runtime 或其他调用方任意注入 Hook 环境的旁路；
4. 保持按 invocation 更新 workspace、HookPoint 和 payload 的现有行为；
5. 不回归 timeout、cancel、进程组回收和输出截断契约。

本次明确不支持 Config 自定义扩展环境变量。需要扩展时另行设计类型化配置、敏感键策略与合并语义。

## 2. 方案选择

### 2.1 最小止血方案

只在 `ProcessDriver` 增加 `env_clear`，并在现有 Dispatcher 构造参数中传入基础白名单。

优点是改动较少。缺点是 `HookDispatchContext::with_env` 仍允许调用方注入任意变量，环境策略可能再次分散到 Runtime，安全问题存在复发风险。

### 2.2 根因级方案（采用）

环境投影由 Hook adapter 独占，`ProcessDriver` 只执行已完成投影的 `ProcessRequest`：

```text
Composition
    ↓ 构造唯一 Dispatcher
Hook adapter
    ├── 捕获固定基础白名单
    ├── 按 HookInvocation 生成当前调用变量
    └── 构造 ProcessRequest.env
ProcessDriver
    ├── env_clear
    └── envs(ProcessRequest.env)
```

同时删除 `HookDispatchContext` 的任意 env 注入能力，使 Runtime 只传当前 workspace cwd。

该方案比止血方案多收窄一个接口，但建立了单一环境策略 owner，能够从根因上避免绕过。

## 3. 责任边界

### 3.1 Runtime

Runtime 只负责提供：

- 当前 `HookInvocation`；
- 当前 workspace cwd；
- cancellation signal。

Runtime、RuntimeContextFactory、RunExecutionState、统一 Loop 和 Hook coordinator 均不得读取父进程环境，也不得拼装 Hook 子进程环境。

### 3.2 Hook adapter

Hook adapter 是环境投影的唯一 owner：

- 构造生产 Dispatcher 时捕获基础白名单；
- 每次 dispatch 根据当前 `HookInvocation` 和 `HookDispatchContext.cwd` 生成兼容变量；
- 组合为完整的 `ProcessRequest.env`；
- 保证连续 dispatch 不残留上一次 invocation 的变量。

### 3.3 ProcessDriver

`ProcessDriver` 不理解变量业务含义。它必须：

- 调用 `env_clear`；
- 仅注入 `ProcessRequest.env`；
- 继续遵守现有 stdin、输出 drain、截断、deadline 和进程组回收契约。

### 3.4 Config 与 Composition

本次不新增 Config 字段，也不修改 ConfigSnapshot。Composition 继续负责实例化唯一生产 Dispatcher，但不再传入一个可任意扩展的 `HashMap` 环境参数。基础白名单由 Hook adapter 自身从父进程捕获。

## 4. 允许的环境变量

### 4.1 基础白名单

Hook adapter 在生产 Dispatcher 构造时读取以下父环境变量：

| 变量 | 用途 |
|---|---|
| `PATH` | 查找 `git`、`node`、`python` 等外部命令 |
| `HOME` | 用户主目录及用户级工具配置 |
| `SHELL` | 默认 shell 信息 |
| `LANG` | locale 与字符编码 |
| `LC_ALL` | locale 总覆盖 |
| `TERM` | 终端类型 |

父进程未设置某变量时不注入该键，不使用空字符串占位。

### 4.2 每次 invocation 固定变量

每次 dispatch 都重新生成：

| 变量 | 来源 |
|---|---|
| `AEMEATH_HOOK_EVENT` | 当前 `HookInvocation::point()` |
| `AEMEATH_PROJECT_DIR` | 当前 `HookDispatchContext.cwd` |
| `CLAUDE_PROJECT_DIR` | 当前 `HookDispatchContext.cwd`，用于 Claude Code 兼容 |

### 4.3 现有 payload 兼容变量

保留当前已经发布的兼容投影：

- `PreToolUse`：`AEMEATH_TOOL_NAME`、`AEMEATH_TOOL_INPUT`；
- `PostToolUse`：`AEMEATH_TOOL_NAME`、`AEMEATH_TOOL_INPUT`、`AEMEATH_TOOL_OUTPUT`、`AEMEATH_TOOL_IS_ERROR`；
- `PostToolUseFailure`：上述工具变量，其中 `AEMEATH_TOOL_IS_ERROR=true`；
- `Stop`：`AEMEATH_STOP_TURNS`；
- `PermissionRequest` / `PermissionDenied`：`AEMEATH_PERMISSION_TOOL_NAME`、`AEMEATH_PERMISSION_RULE`；
- `InstructionsLoaded`：`AEMEATH_INSTRUCTIONS_FILE_PATH`、`AEMEATH_INSTRUCTIONS_TYPE`；
- `Notification`：`AEMEATH_NOTIFICATION_TEXT`、`AEMEATH_NOTIFICATION_TYPE`。

其他 invocation 不额外投影 payload 环境变量。完整调用数据仍通过结构化 stdin JSON 传递，环境变量不是完整 payload 的唯一来源。

### 4.4 默认拒绝

除上述固定集合外，父环境一律不继承。该策略是固定 allowlist，不使用敏感名称 blacklist。因此包括但不限于以下变量均不可见：

- `AEMEATH_API_KEY`、`LLM_API_KEY`；
- provider API key；
- `GITHUB_TOKEN`、`GH_TOKEN`；
- `AWS_*`；
- `*_TOKEN`、`*_SECRET`、`*_PASSWORD`；
- `SSH_AUTH_SOCK`、`GIT_ASKPASS`；
- 任意未知父环境变量。

## 5. 接口与数据流

### 5.1 `HookDispatchContext`

删除：

- `env: HashMap<String, String>`；
- `with_env`；
- `env` accessor。

保留 `cwd` 及其 accessor。这样 HookPort 调用方无法绕过 Hook adapter 的固定环境策略。

### 5.2 Dispatcher 构造

生产 Dispatcher 构造不再接受裸 `HashMap<String, String>`。Hook adapter 内部通过一个职责明确的私有辅助函数捕获基础白名单，并交给 `ProcessDriverExecutor` 持有。

测试使用的 scripted Executor 不依赖真实父环境，保持确定性。

### 5.3 每次 dispatch

1. Dispatcher 从自身持有的基础白名单开始；
2. 根据当前 invocation 和 cwd 生成新的 invocation 环境；
3. invocation 变量覆盖任何同名基础键；
4. Executor 将完整环境写入 `ProcessRequest`；
5. ProcessDriver 清空父环境，再注入该请求环境。

Stop 重试耗尽后派发 `StopFailure` 时，必须根据 `StopFailure` invocation 重新生成环境。不得沿用 Stop 的 payload map，因此 `AEMEATH_STOP_TURNS` 不应作为上一 invocation 的残留变量泄漏；StopFailure 的完整 turns/error 仍在 stdin JSON 中。

## 6. 错误与兼容策略

- 基础变量缺失不是错误，对应键不注入；
- `env_clear` 后命令找不到时，按现有非零 exit 或 spawn/执行错误协议处理，不增加特殊 fallback；
- 不伪造默认 `PATH`、`HOME`、locale 或 shell 值；
- 保留当前 `sh -c` 执行方式和 Hook 输出协议；
- 不改变 Hook retry、Block、StopFailure、BoundaryOnly 或 Runtime 状态机语义；
- 不新增日志记录环境值，避免敏感信息进入日志。

## 7. 测试策略

### 7.1 L1：ProcessDriver 单元/adapter 测试

- 设置一个随机命名的父环境变量，验证子进程不可见；
- 在 `ProcessRequest.env` 明确加入变量，验证子进程可见；
- 现有正常退出、并发大输出 drain、输出截断测试保持通过；
- 现有 timeout、cancel、TERM→KILL→wait 进程组回收测试保持通过。

涉及进程全局环境的测试必须串行化或使用项目现有的环境测试隔离方式，避免并行测试竞态。

### 7.2 L2：Dispatcher/Hook adapter 契约测试

- 基础白名单的已设置项可见，缺失项不伪造；
- `AEMEATH_HOOK_EVENT` 和项目目录变量与当前 invocation/cwd 一致；
- payload 兼容变量按 HookPoint 正确生成；
- 同一 Dispatcher 连续 dispatch 不同 workspace 时目录不串值；
- 连续 dispatch 不同 HookPoint 时事件和 payload 不串值；
- Stop → StopFailure 不残留 Stop-only 环境变量；
- `HookDispatchContext` 不再存在任意 env 注入 API。

### 7.3 L3/L4：Composition 与 Runtime 回归

- Composition 通过无裸 env 参数的生产 factory 构造 Dispatcher；
- HookPort 仍注入 RuntimeContextFactory 的统一生产路径；
- #1397 合入后的 Main/Sub 统一 Loop、BoundaryOnly Hook 行为和当前 workspace 传递回归通过；
- Hook、Composition 和 Runtime 的相关测试通过。

### 7.4 验证命令

实施计划至少包含：

- `cargo test -p hook`；
- `cargo test -p composition`；
- `cargo test -p runtime`；
- `cargo fmt --all -- --check`；
- `cargo clippy -p hook -p composition -p runtime --all-targets -- -D warnings`；
- `bash .agents/hooks/check-architecture-guards.sh`；
- `git diff --check`。

## 8. 范围边界

本次包含：

- `agent/features/hook/**` 的环境隔离、接口收口和测试；
- `agent/composition/**` 中生产 Dispatcher 接线调整；
- Hook Target 设计与 Migration Governance 的最终状态回写。

本次不包含：

- Config 自定义环境变量；
- Hook retry/Stop 上限的 ConfigSnapshot 注入；
- 新增 payload 环境变量；
- Runtime Loop、RunExecutionState 或 Hook coordinator 重构；
- Windows 非 Unix 进程组支持；
- Hook 输出 spill 或协议扩展。

## 9. 完成定义

- Hook 子进程不继承未批准父环境变量；
- 固定基础白名单和当前 invocation 变量可见；
- 同一 Dispatcher 的连续 invocation 不串 workspace、HookPoint 或 payload；
- Runtime 无环境拼装或任意注入入口；
- `HookDispatchContext` 只携带 cwd；
- timeout/cancel/进程组回收和输出截断契约无回归；
- 文档、代码、测试和架构边界使用同一责任划分。
