<!-- Migrated from: docs/feature/archived/010-log-file-spec.md -->
# #10 日志文件规范化

**归档日期**：2026-05-01

**目标**：明确 `aemeath.log`、`debug.log`、`agent.log`、`panic.log` 各日志文件的职责边界、写入入口、格式约定与轮转策略。

**实现**：

- 主日志 `aemeath.log`：env_logger 路由，warn 默认；`AEMEATH_LOG_STDERR=1` 可恢复 stderr 输出
- `debug.log`：debug 级独立文件
- `agent.log`：子 agent 执行轨迹（turn N、tool call 名）
- `panic.log`：进程 panic 捕获（panic message + backtrace + 当前 session id）
- TUI 模式下所有日志统一路由到 `~/.aemeath/aemeath.log`，避免污染 ratatui 渲染

**修复 commit**：e8dd00d `feat: 规范日志并完善 TUI 运行态体验`

**涉及文件**：
- `aemeath-cli/src/main.rs`（panic handler 注册、log dispatch 初始化）
- `aemeath-core/src/lib.rs`（env_logger 配置）
- `aemeath-cli/src/agent_runner.rs`（agent.log 写入入口）

**未纳入本期**：
- 自动按文件大小轮转 + 保留最近 N 份
- 启动时清理超过 30 天的旧日志
- 每会话独立子目录（`~/.aemeath/sessions/<id>/aemeath.log`）

如后续需要轮转，可在此基础上新增独立 feature。
