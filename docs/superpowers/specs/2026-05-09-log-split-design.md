# 日志分化：input.log / output.log / tool.log

日期：2026-05-09
关联：Feature #27（日志系统职责分层）

## 目标

在 Feature #27 的基础上，将 agent 交互日志从 `aemeath.log` 中分离出来，按职责细分为三个独立文件。`aemeath.log` 收窄为应用诊断日志。

## 文件布局

```
~/.aemeath/logs/            # 新增 logs_dir，默认 ~/.aemeath/logs/
├── aemeath.log             # 应用诊断日志（MCP、hook、session、技能、UI调试）
├── input.log               # LLM 输入快照
├── output.log              # LLM 完整输出
├── tool.log                # 工具调用请求 + 结果
└── panic.log               # panic 信息（不变）
```

`logs_dir` 在 `logging.logs_dir` 配置中可自定义，不配时回退到 `~/.aemeath/`。

## 统一 JSON 格式

所有新增日志行均为单行 JSON：

```json
{
  "ts": "2026-05-09T10:30:00+08:00",
  "session": "abc123",
  "turn": 3,
  "role": "searcher",
  "model": "gpt-5.5",
  "type": "input",
  "data": { ... }
}
```

字段说明：
- `ts`：ISO 8601 时间戳，含时区
- `session`：session id（全局 `SESSION_ID`）
- `turn`：当前 turn 序号（1-based，主 agent 和 sub-agent 各自计数）
- `role`：agent role 名称，主 agent 固定 `"default"`
- `model`：实际使用的 model id（如 `"gpt-5.5"`、`"claude-sonnet-4-5"`）
- `type`：事件类型，各文件取值不同
- `data`：事件数据，各文件结构不同

## 各文件定义

### input.log — LLM 输入快照

仅记录本次 API 调用新增的消息，不重复记录历史。

`type`：`"input"`

`data`：
```json
{
  "messages": [
    {
      "role": "user",
      "content": "请读取 src/main.rs 文件",
      "len": 28
    }
  ],
  "system_blocks_count": 3,
  "system_blocks": [
    {"type": "text", "len": 4500},
    {"type": "text", "len": 3200},
    {"type": "image", "len": 0}
  ],
  "tool_schemas_count": 12,
  "tool_schemas_names": ["Read", "Write", "Edit", "Bash", "Glob", "Grep", "Agent", "TaskCreate", "TaskUpdate", "TaskList", "AskUserQuestion", "Skill"]
}
```

说明：
- `messages`：本次请求中**新增**的 messages（不含历史），每条只含 `role`、`content`、`len` 三个字段
- `system_blocks`：每个 system block 的类型和字符长度，不存储完整内容
- `tool_schemas_names`：工具名列表，不存储完整 schema

### output.log — LLM 完整输出

记录 LLM API 调用返回的完整 assistant message。

`type`：`"output"`

`data`：
```json
{
  "stop_reason": "ToolUse",
  "input_tokens": 5000,
  "output_tokens": 300,
  "elapsed_secs": 1.234,
  "provider": "anthropic",
  "content_blocks": [
    {
      "type": "thinking",
      "content": "用户要求读取 src/main.rs，我需要..."
    },
    {
      "type": "text",
      "content": "好的，让我读取文件。"
    },
    {
      "type": "tool_use",
      "tool_name": "Read",
      "tool_id": "abc123",
      "input": {"file_path": "src/main.rs"}
    }
  ]
}
```

说明：
- `content_blocks`：模型返回的完整 blocks 数组，含 thinking、text、tool_use 三种类型
- tool_use block 包含工具名和参数（与 tool.log 有少量冗余，但互不依赖）

### tool.log — 工具调用记录

一个工具调用对应两条 JSON 行：`tool_call` + `tool_result`。

**第一条：tool_call**

`type`：`"tool_call"`

`data`：
```json
{
  "tool_use_id": "abc123",
  "tool_name": "Read",
  "input": {
    "file_path": "src/main.rs"
  }
}
```

**第二条：tool_result**

`type`：`"tool_result"`

`data`：
```json
{
  "tool_use_id": "abc123",
  "tool_name": "Read",
  "is_error": false,
  "output": "use std::io;\n\nfn main() {\n    println!(\"Hello\");\n}\n"
}
```

说明：
- `output` 存储完整工具输出，不截断
- 通过 `tool_use_id` 关联 call 和 result

## aemeath.log 收窄

### 保留的内容
- MCP 连接/注册/工具发现
- Hook 执行错误
- Session 生命周期（启动、结束、保存）
- 技能加载
- TUI UI 调试（spinner、selection、dialog）
- 状态栏运行时状态

### 移除的内容
- `log_agent_loop_event()` — 所有调用
- `log_llm_request_messages()` — 所有调用
- `log_tool_result_event()` — 所有调用
- Turn start/end log（`stream.rs:80` 的 `log::info!("turn started: ...")`）

## 实现组件

### JsonLogger（`aemeath-core/src/logging.rs`）

```rust
pub struct JsonLogger {
    input: BufWriter<File>,
    output: BufWriter<File>,
    tool: BufWriter<File>,
    session_id: String,
}
```

公开方法：
- `log_input(turn, role, model, data: Value)` → `input.log`
- `log_output(turn, role, model, data: Value)` → `output.log`
- `log_tool_call(turn, role, model, data: Value)` → `tool.log`
- `log_tool_result(turn, role, model, data: Value)` → `tool.log`

每个方法内部：
1. 构建统一 JSON 结构（ts + session + turn + role + model + type + data）
2. `serde_json::to_string` 序列化为单行
3. `writeln!` 写入对应文件

初始化时：
- 读取 `LoggingConfig::logs_dir`，创建目录 `logs/`
- 打开三个文件（create + append）
- 文件轮转复用 `rotate_if_needed()` + `cleanup_old_rotated_logs()`

所有方法为 `&mut self`，调用方通过 `Arc<Mutex<JsonLogger>>` 共享。

### LogFile 枚举扩展

`LogFile` 新增 `Input`、`Output`、`Tool` 三个变体及对应的 `file_name()`：

```rust
pub enum LogFile {
    Aemeath,   // aemeath.log
    Agent,     // agent.log（保留但废弃，无写入点）
    Panic,     // panic.log
    Input,     // input.log
    Output,    // output.log
    Tool,      // tool.log
}
```

### 配置扩展

`LoggingConfig` 新增字段：

```rust
/// 日志文件存放目录。默认 ~/.aemeath/logs/，不配时回退 ~/.aemeath/
#[serde(default)]
pub logs_dir: Option<String>,

/// 是否启用 input/output/tool 分化日志
#[serde(default = "default_true")]
pub role_logs_enabled: bool,
```

`log_path()` 根据 `logs_dir` 确定基础目录：
- 有配置 → 使用 `logs_dir`
- 无配置 → 使用 `log_dir()`（即 `~/.aemeath`）

若使用非默认目录，自动创建。

## 写入点映射

| 写入点 | 当前代码 | 改为 |
|--------|---------|------|
| LLM 请求前 | `log_llm_request_messages(...)` in `stream.rs:313` | `json_logger.log_input(turn, role, model, data)` |
| LLM 响应后 | `log_agent_loop_event(..., "llm_response", ...)` in `stream.rs:355` | `json_logger.log_output(turn, role, model, data)` |
| Tool calls 提取 | 无显式日志，在 `stream.rs:354` 提取后 | `json_logger.log_tool_call(turn, role, model, data)` |
| Tool result 后 | `log_tool_result_event(...)` in `stream.rs` | `json_logger.log_tool_result(turn, role, model, data)` |
| Sub-agent 请求前 | `agent_runner.rs` LLM call 前 | `json_logger.log_input(turn, role, model, data)` |
| Sub-agent 响应后 | `agent_runner.rs` LLM call 后 | `json_logger.log_output(turn, role, model, data)` |
| Sub-agent tool call | `agent_runner.rs` tool 提取 | `json_logger.log_tool_call(turn, role, model, data)` |
| Sub-agent tool result | `agent_runner.rs` tool 完成 | `json_logger.log_tool_result(turn, role, model, data)` |

## 涉及文件

| 文件 | 改动 |
|------|------|
| `aemeath-core/src/config/logging.rs` | 新增 `logs_dir`、`role_logs_enabled` 字段 |
| `aemeath-core/src/logging.rs` | 新增 `JsonLogger` 结构体、`LogFile` 新增变体 |
| `aemeath-cli/src/main.rs` | `init_logging()` 初始化 `JsonLogger`，shared 注入 |
| `aemeath-cli/src/tui/app/stream.rs` | 注入 `JsonLogger`，替换旧日志调用，移除 dead 函数 |
| `aemeath-cli/src/agent_runner.rs` | 注入 `JsonLogger`，sub-agent 写入 |

## 过渡策略

- `LogFile::Agent` 枚举保留，`file_name()` 仍返回 `"agent.log"`，但无写入点——渐进式废弃
- 旧 `agent.log` 文件不删除，用户自行清理
- 配置文件增加 `role_logs_enabled` 开关，默认 `true`。设 `false` 时不分化，保持兼容
- `logs_dir` 未配置时，路径回退到原来 `~/.aemeath/`，与旧布局兼容

## 测试覆盖

- `JsonLogger::new()` — 目录不存在时自动创建
- `JsonLogger` 各方法 — JSON 格式正确、字段齐全
- 文件轮转 — 超过 `max_bytes` 后正确轮转
- `LogFile::Input/Output/Tool` — `file_name()` 返回正确文件名
- `logs_dir` 配置 — 有配置 vs 无配置（回退）路径
- `role_logs_enabled = false` — 不创建分化日志文件

## 关联

- Feature #27 — 本 feature 基于 #27 的日志分层进一步分化
- CLAUDE.md 日志规范节 — 需同步更新文件职责描述
