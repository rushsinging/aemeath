# 非交互外部进程控制终端隔离设计

> 对应 Issue：[#1577](https://github.com/rushsinging/aemeath/issues/1577)
> Milestone：v0.1.0 — Context Engineering + 架构重构
> 状态：已批准

## 1. 问题与根因

Aemeath 启动的 Bash、Hook、Git、curl、Grep、MCP stdio、剪贴板和图片处理等非交互外部进程会继承父进程的 Unix session 与控制终端。即使调用方已设置 `stdin=null`、`stdout/stderr=pipe` 并通过 `process_group(0)` 创建独立进程组，child 仍可访问父控制终端。

macOS NFS 客户端在 OrbStack 挂载短暂失联时，会把状态通知发送到触发访问进程关联的控制终端。这类通知不经过 child 的 stdout/stderr 管道，因此会直接覆盖 Aemeath alternate-screen TUI。Aemeath 并未主动扫描 OrbStack 挂载；触发链是 Aemeath 启动的命令间接访问 Docker/OrbStack，而 child 继承控制终端使异步通知进入同一 TTY。

## 2. 目标与非目标

### 2.1 目标

1. Aemeath 在所有运行模式下启动的生产非交互外部进程都不继承父控制终端。
2. Unix child 在独立 session 与进程组中运行；依赖 `/dev/tty` 的命令明确失败，不回退共享终端。
3. 保留既有 cwd、env、stdin/stdout/stderr、流式输出、exit/signal、timeout、cancel 和进程树回收语义。
4. 用唯一跨 crate 基础设施能力配置非交互 child，阻止调用点复制 session 隔离逻辑。
5. 用架构 Guard 阻止新的生产调用点绕过统一边界。

### 2.2 非目标

1. 不支持 `sudo`、交互式 SSH、编辑器、pager 或全屏程序。
2. 不新增 PTY 代理、交互式 shell 或终端复用能力。
3. 不改变用户显式执行命令的业务授权与安全策略。
4. 不通过周期性重绘掩盖控制终端污染。

## 3. 候选方案与决策

### 3.1 最小止血：只修改 Bash 与 Hook

为 Bash 和 Hook 添加 `setsid()`。改动小，但 Git、curl、MCP stdio 等生产调用仍继承控制终端，同类污染可复发。该方案只处理当前最显眼入口，不解决分散进程启动的结构性根因，因此不采用。

### 3.2 调用点分别添加 session 隔离

在每个 `Command` 调用点重复配置 `setsid()`。短期能覆盖，但跨 crate 重复平台条件、错误语义和系统调用顺序，后续容易漏迁移或重新引入裸启动，也违反 DRY，不采用。

### 3.3 统一非交互进程边界

建立一个职责单一的共享能力，只负责将 `std::process::Command` 配置为严格非交互 child；Tokio 调用通过 `as_std_mut()` 使用同一实现。所有生产外部进程调用迁移到该能力，并由 Guard 锁定。这是根因级方案，采用。

## 4. 架构与职责

### 4.1 统一配置能力

共享基础设施发布一个窄函数，输入可变的 `std::process::Command`，输出配置结果。它不创建业务命令、不决定 cwd/env/stdio、不执行 spawn，也不拥有 timeout/cancel；唯一职责是配置“无控制终端”的进程边界。

Unix 实现通过 spawn 前回调调用 `setsid()`。成功后 child 同时成为新 session leader 和新进程组 leader，不再拥有父控制终端。因此调用方不得再叠加 `process_group(0)`；否则系统调用顺序可能使 `setsid()` 因 child 已是进程组 leader 而失败。

非 Unix 平台不伪造等价能力。现阶段产品支持边界应采用 typed unsupported 或平台受限实现，具体以现有各 adapter 的平台契约为准；不得静默声称已隔离。

### 4.2 调用方职责

- Bash、Hook：保留 stdout/stderr 并发 drain、deadline、取消、`TERM → KILL → wait` 和进程组残留清理；进程组 ID 继续取直接 child PID，因为 `setsid()` 后该 PID 同时是 PGID。
- Project、Runtime、Context 的 Git 与图像/剪贴板命令：保留参数、cwd 和输出捕获，只替换启动边界。
- Tools 的 Grep、curl 与 MCP stdio：保留流式或协议管道，统一脱离控制终端。
- CLI 的剪贴板、外部打开和 Git 查询：保留用户可见行为，但不允许 child 访问父 `/dev/tty`。

### 4.3 交互命令语义

严格无 TTY，不提供回退。若 child 打开 `/dev/tty`，操作系统应返回“无此设备或地址”等失败；Aemeath 通过现有 stderr/exit 结果报告失败。这样能保证任何命令都不能为兼容交互而重新污染父终端。

## 5. 数据流与生命周期

1. 业务 adapter 构造 `Command` 并配置参数、cwd、env 和 stdio。
2. adapter 调用统一非交互配置能力。
3. spawn 前回调在 child 中执行 `setsid()`；失败则 spawn 返回 I/O 错误，不运行目标程序。
4. child 在独立 session/进程组中运行，stdout/stderr 仍流入原有管道。
5. 正常结束时调用方按既有协议等待并读取结果。
6. timeout/cancel 时调用方按 child PID 对应的新进程组执行回收。

## 6. 错误处理

- `setsid()` 失败必须阻止 exec，并以 spawn I/O 错误沿既有 typed error 边界返回；不得忽略或回退。
- 调用方原有命令不存在、非零 exit、signal、输出解码和 timeout 错误保持不变。
- 不在通用基础设施中记录命令参数或环境，避免泄露密钥；需要日志时由 owning adapter 使用既有安全字段记录阶段与错误类别。
- MCP 等长生命周期 child 的启动失败继续由其 transport/error 层表达，不新增第二套错误模型。

## 7. 测试策略

按照 L0–L5 模型分层：

- **L0**：Guard 扫描生产 Rust 源码，禁止统一边界之外的外部进程构造/启动；平台编译和 all-target clippy 验证 API 可达性。
- **L1**：共享能力测试 spawn 后 `getsid(pid) == pid`、PGID 等于 PID，并验证 `/dev/tty` 不可打开。
- **L2**：Bash、Hook 等 owning adapter 保留 cwd/env/stdio、timeout/cancel 和进程组回收契约。
- **L3**：跨 crate 公共边界及 MCP stdio、Project Git 等 adapter 契约不回归。
- **L4**：Bash/Hook/工作区用户旅程继续通过现有场景测试。
- **L5**：真实 PTY 启动 Aemeath 或专用测试 child，证明 child 无控制终端且不能将 `/dev/tty` 内容写入父 PTY；保留现有 alternate-screen/恢复 smoke。

核心逻辑遵循 TDD：先加入能在当前实现下复现共享 session/控制终端继承的失败测试，再实现统一边界与迁移调用点。

## 8. Guard 与迁移完成定义

Guard 应扫描生产 target，而非测试、构建脚本或治理工具。统一底层模块是允许直接配置 `Command` 的唯一基础设施入口；业务 adapter 可构造具体命令，但 spawn 前必须经过统一配置。若静态扫描无法可靠证明数据流，可要求调用点统一使用命名构造入口，使违规可机械识别。

完成前必须重新枚举生产 `std::process::Command` / `tokio::process::Command` 调用点，逐项确认已迁移或记录具有可验证理由的例外。不得遗留仅覆盖 Bash/Hook 的部分修复。

## 9. 验收

1. macOS/Linux 上所有生产非交互 child 都是独立 session leader 且无控制终端。
2. OrbStack/NFS 等面向 child 控制终端的通知不再进入 Aemeath TUI 或调用者终端。
3. Bash、Hook 的进程树回收、流式输出和错误语义不回归。
4. Git、Grep、curl、MCP stdio、剪贴板和图片命令行为不回归。
5. 依赖 `/dev/tty` 的命令明确失败，不回退。
6. fmt、相关 crate 测试、workspace 测试、all-target clippy、架构守卫和 PTY smoke 通过。
