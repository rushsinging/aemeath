# 实施计划：日志分化 input.log / output.log / tool.log

日期：2026-05-09
关联 Spec：`docs/superpowers/specs/2026-05-09-log-split-design.md`
关联 Feature：Feature #27

## 任务拆解

### 任务 1：配置层扩展（aemeath-core）
**文件**：`aemeath-core/src/config/logging.rs`、`aemeath-core/src/config/manager/merge.rs`

在 `LoggingConfig` 新增两个字段：
- `logs_dir: Option<String>` — 日志目录路径，`#[serde(default)]`
- `role_logs_enabled: bool` — 是否启用分化日志，`#[serde(default = "default_true")]`

在 `merge.rs` 的 `logging` 合并段新增两个字段的 merge 逻辑（遵循现有 pattern：overlay 非默认值 → 用 overlay，否则用 base）。

### 任务 2：JsonLogger 结构体 + LogFile 扩展（aemeath-core）
**文件**：`aemeath-core/src/logging.rs`

2.1 `LogFile` 枚举新增 `Input`、`Output`、`Tool` 三个变体，`file_name()` 返回对应文件名

2.2 新增 `JsonLogger` 结构体：
```rust
pub struct JsonLogger {
    input: BufWriter<File>,
    output: BufWriter<File>,
    tool: BufWriter<File>,
    session_id: String,
    logs_dir: PathBuf,
    logging_config: LoggingConfig,
}
```

2.3 公开方法：
- `new(session_id, logs_dir, logging_config) -> io::Result<Self>` — 创建目录，打开三个文件
- `log_input(turn, role, model, data: serde_json::Value)` 
- `log_output(turn, role, model, data: serde_json::Value)`
- `log_tool_call(turn, role, model, data: serde_json::Value)`
- `log_tool_result(turn, role, model, data: serde_json::Value)`

每个方法内部流程：
1. 构建 `serde_json::json!({ "ts": ..., "session": ..., "turn": ..., "role": ..., "model": ..., "type": ..., "data": data })`
2. `serde_json::to_string` → 单行
3. `writeln!` 到对应 writer
4. 检查文件大小 > max_bytes，需要时调用 `rotate_if_needed`

文件轮转：复用现有 `rotate_if_needed(&mut log_file, max_bytes, max_backups, retention_days, &file_path)` 的重载版本（接受 `BufWriter<File>` 而非 `PathBuf`）。

2.4 `LogFile` 的 `file_name()` 方法扩展：
```rust
LogFile::Input => "input.log",
LogFile::Output => "output.log",
LogFile::Tool => "tool.log",
```

### 任务 3：JsonLogger 初始化（aemeath-cli main.rs）
**文件**：`aemeath-cli/src/main.rs`

在 `run_chat()` 函数中，`SESSION_ID` 设置完成后、初始化 logging 后：
1. 读取 `config_file.logging.logs_dir`，确定日志目录
2. 如果 `logging.role_logs_enabled`，创建 `Arc<Mutex<JsonLogger>>`
3. 通过 `SpawnContext` 传递给 TUI 和 agent runner

### 任务 4：SpawnContext 传递 JsonLogger
**文件**：`aemeath-cli/src/tui/app/processing.rs`

`SpawnContext` 结构体新增字段：
```rust
pub json_logger: Option<Arc<Mutex<JsonLogger>>>,
```

### 任务 5：SpawnContext 构造点注入 JsonLogger
**文件**：`aemeath-cli/src/tui/app/mod.rs`

App struct 新增字段：
```rust
json_logger: Option<Arc<Mutex<JsonLogger>>>,
```

在 `App::new()` 接收 json_logger 参数。

在 3 处 `SpawnContext` 构造点和 `input_handler.rs` 的 2 处构造点：
```rust
json_logger: self.json_logger.clone(),
```

在 `Cmd::SpawnProcessing` 匹配处同样传递。

### 任务 6：stream.rs 替换旧日志
**文件**：`aemeath-cli/src/tui/app/stream.rs`

6.1 `process_in_background()` 函数签名新增参数：
```rust
json_logger: Option<Arc<Mutex<JsonLogger>>>,
```

6.2 在 LLM 请求前（~line 313，`log_llm_request_messages` 调用点）：
- 移除 `log_llm_request_messages(...)` 调用
- 新增：构建 input data（messages 摘要、system blocks、tool schemas），调用 `json_logger.log_input()`

6.3 在 LLM 响应后（~line 355）：
- 移除 `log_agent_loop_event(..., "llm_response", ...)` 调用
- 新增：构建 output data（stop_reason, tokens, elapsed, content_blocks），调用 `json_logger.log_output()`
- 同时遍历 content_blocks，为每个 tool_use block 调用 `json_logger.log_tool_call()`

6.4 在 tool result 后（~line 470-550 附近）：
- 移除 `log_tool_result_event(...)` 调用
- 新增：调用 `json_logger.log_tool_result()`

6.5 移除 turn start log（`stream.rs` 中 `log::info!("turn started: ...")`）

### 任务 7：agent_runner.rs 注入 JsonLogger
**文件**：`aemeath-cli/src/agent_runner.rs`

7.1 `CliAgentRunner` 结构体新增字段：
```rust
pub json_logger: Option<Arc<Mutex<JsonLogger>>>,
```

7.2 在 main.rs 中构造 `CliAgentRunner` 时传入 json_logger

7.3 在 sub-agent 的 LLM 请求前（`agent_runner.rs` ~line 335 `client.stream_messages` 调用前）：
- 调用 `json_logger.log_input(turn, role, model, data)`
- role 使用 sub-agent 的 `agent_role.name`（如 "searcher"、"coder"）

7.4 在 sub-agent 响应后（`client.stream_messages` 完成后）：
- 调用 `json_logger.log_output(turn, role, model, data)`

7.5 在 sub-agent 执行 tool calls 时（~line 553-620）：
- 每个 tool_use block 调用 `json_logger.log_tool_call()`
- 每个 tool_result 后调用 `json_logger.log_tool_result()`

### 任务 8：编译验证
- `cargo build -p aemeath-core`
- `cargo build -p aemeath-cli`
- `cargo check`
- 确保无编译错误

### 任务 9：测试
- `cargo test -p aemeath-core` — JsonLogger 单元测试
- `cargo test -p aemeath-cli` — 集成测试

## 依赖关系

```
任务 1 (配置) ─┬► 任务 2 (JsonLogger) ─► 任务 3 (main.rs 初始化) ─┬► 任务 4 (SpawnContext)
              │                                                    │
              └────────────────────────────────────────────────────┤
                                                                   ├► 任务 6 (stream.rs)
                                                                   ├► 任务 7 (agent_runner.rs)
                                                                   └► 任务 5 (构造点注入)

任务 4 ─► 任务 5
任务 5 ─► 任务 6
任务 6, 7 ─► 任务 8 (编译) ─► 任务 9 (测试)
```

## 文件变更汇总

| 文件 | 变更类型 | 行数估计 |
|------|---------|---------|
| `aemeath-core/src/config/logging.rs` | +2 字段, +默认函数 | +15 |
| `aemeath-core/src/config/manager/merge.rs` | merge 逻辑扩展 | +15 |
| `aemeath-core/src/logging.rs` | JsonLogger + LogFile 扩展 | +120 |
| `aemeath-cli/src/main.rs` | JsonLogger 初始化 + 传递 | +25 |
| `aemeath-cli/src/tui/app/processing.rs` | SpawnContext + 字段 | +3 |
| `aemeath-cli/src/tui/app/mod.rs` | App + 字段, 4 构造点传递 | +6 |
| `aemeath-cli/src/tui/app/input_handler.rs` | 2 构造点传递 | +2 |
| `aemeath-cli/src/tui/app/stream.rs` | 替换旧日志, 新增 JsonLogger 调用 | +60 / -40 |
| `aemeath-cli/src/agent_runner.rs` | 字段 + 写入点 | +50 |
