# 原生 stderr 统一路由设计

## 背景与根因

TUI 使用 stdout 在 alternate screen 中绘制，但 macOS libc、系统 Framework、FFI 和部分未显式配置 stderr 的辅助子进程可以绕过 Rust `log` 管线，直接写 aemeath 进程的 FD 2。当 stdout 与 stderr 指向同一终端时，这些字节越过 ratatui 双缓冲并覆盖 TUI。

`MallocStackLogging: can't turn off malloc stack logging because it was not enabled.` 是该边界缺失的一个实例，不是内存泄漏证据。按文本过滤或关闭 RSS 采样只能处理当前症状。

## 目标

- TUI 模式下统一管理 aemeath 原生 FD 2，使原生诊断保留在日志文件且不污染终端。
- 保留 Bash、Hook、MCP 等组件现有的业务 stderr 语义。
- 尊重用户显式 stderr 重定向。
- no-TUI 模式保持当前 stderr 用户界面语义。
- 初始化失败时 fail closed，不允许 TUI 在未隔离 stderr 的状态下进入 alternate screen。

## 非目标

- 不过滤特定 malloc 警告。
- 不关闭 macOS RSS 采样。
- 不把 Bash/Hook 的命令 stderr 混入原生诊断日志。
- 不在本次重构 Bash 超限输出、MCP stderr 限流或日志轮转体系。

## 方案

### 责任边界

在 CLI 交付层新增 `NativeStderrRouter`。它只负责判断当前 stderr 是否需要接管，并将 FD 2 追加路由到已解析日志目录中的 `native-stderr.log`。

路由属于进程启动期一次性基础设施，不属于 `TerminalGuard`：成功接管后持续到进程退出，不在 TUI 暂停、恢复或退出时切换。进程退出由操作系统关闭文件描述符。

### 启用规则

只有同时满足以下条件才接管：

1. 运行前端为 TUI，即 CLI 未启用 `--quiet`；
2. stderr 当前是终端；
3. stdout 与 stderr 指向同一终端设备。

stderr 已指向文件、管道或不同终端时视为用户或调用方显式管理，不覆盖。no-TUI 模式不接管。

### 路径与文件语义

路由目标为当前冻结日志设置的 `logs_dir/native-stderr.log`。默认路径是 `$AEMEATH_AGENTS_DIR/logs/native-stderr.log`，未设置运行时根目录时是 `~/.agents/logs/native-stderr.log`；配置中的自定义 `logging.logs_dir` 与 UnifiedLogger 使用同一个已解析目录。

文件使用 create + append，权限遵循平台默认安全创建策略；同一进程不做文本格式化，不假设输入是 UTF-8。原生 stderr 是不受信任的诊断字节流，不纳入 DiagnosticRecord 14 字段 schema。

### 启动顺序

1. CLI 解析参数，确定 TUI/no-TUI。
2. Composition 读取冻结配置并解析 LoggingSettings。
3. 在初始化可能产生后台任务的 Runtime 之前，将最终日志目录发布到 CLI bootstrap，并完成原生 stderr 路由。
4. 路由成功或判定无需接管后，继续 Runtime/TUI 初始化。
5. 路由文件创建、设备检测或 FD 替换失败时返回 `SdkError::Init`；此时尚未进入 raw mode/alternate screen，可安全向原 stderr 报错并终止。

为避免 Composition 反向依赖 CLI，Composition 只在 `AgentClientBootstrap` 发布不可变 `logs_dir` 与 frontend 启动所需数据；FD 操作由 CLI 拥有。若当前 bootstrap 构造顺序会在路由前启动可写 stderr 的后台任务，则把 bootstrap 拆为“配置/日志预备”与“Runtime 构造”两个明确阶段，禁止以暂时可接受为由留下竞态窗口。

### 工具 stderr 边界

- Bash：主命令 stdout/stderr 继续使用独立 pipe，stderr 进入 `BashResult` 和 ToolResult。
- Hook：stdout/stderr 继续有界捕获并在超限时 spill。
- MCP stdio：server stderr 继续由专属 pipe 逐行写入 MCP 日志。
- Agent/Sub-agent：同进程运行；结构化状态继续走 typed event，任何绕过 logger 的原生 FD 2 输出由全局原生路由兜底。
- 未显式 pipe 的辅助子进程：继承已路由 FD 2，诊断进入 `native-stderr.log`，不污染 TUI。

## 错误处理与安全

- 所有 Unix FD 调用检查返回值，错误携带操作阶段和目标路径，不包含敏感内容。
- 打开目标文件后再替换 FD 2；打开失败不改变 stderr。
- FD 替换成功后关闭多余文件描述符，FD 2 保持有效。
- 不读取、解析或回显原生 stderr，避免二次注入 TUI。
- 非 Unix 平台提供明确的空操作或等价平台实现；本次 Unix 实现必须保证 macOS/Linux 编译。

## 测试策略

采用 L0-L5 分层证据：

- L1：启用决策覆盖 TUI/quiet、TTY/文件/管道、同终端/不同终端。
- L2：使用隔离子进程测试 FD 2 append 路由、原重定向保持、失败前 FD 不变，避免并行测试修改测试进程全局 FD。
- L3：验证 bootstrap 发布的 logs_dir 与 `LoggingSettings` 单一解析结果一致。
- L4：CLI 场景验证 quiet 模式仍向调用方 stderr 输出，TUI 模式 native stderr 写入文件。
- L5：`portable-pty` 启动真实 CLI，证明诊断字节不出现在 PTY 屏幕且出现在 `native-stderr.log`。
- L0：`cargo clippy --workspace --all-targets`、架构守卫和跨平台 cfg 编译。

所有生产修改遵循 TDD：先观察复现测试因缺少路由失败，再实现最小代码使其通过。

## 验收

GitHub Issue #1597 的全部 checklist 是交付门禁。最终还需运行 CLI 定向测试、workspace tests、workspace clippy，检查无重复 stderr 管理逻辑、废弃路径或新 warning。
