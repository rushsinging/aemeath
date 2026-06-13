# 项目对比报告：aemeath (Rust) vs extracted_sources (TypeScript)

> 更新时间：2026-04-05
> aemeath 代码行数：~17,079 行 Rust
> extracted_sources 代码行数：~512,664 行 TypeScript/JavaScript

## 1. 概览

### 语言和技术栈
- **aemeath**: Rust 语言，使用 Tokio 异步运行时
- **extracted_sources**: TypeScript/JavaScript，使用 Bun 运行时和 React

### 代码规模对比
- **aemeath**: 约 17,000+ 行 Rust 代码（不含依赖）
- **extracted_sources**: 约 512,664 行 TypeScript/JavaScript 代码（不含 node_modules）
- **规模差异**: 约 **34 倍**

### 架构设计

#### aemeath (简化架构)
```
aemeath/
├── aemeath-core/          # 核心逻辑
│   ├── agent.rs           # 代理实现
│   ├── agent_lifecycle.rs # 代理生命周期 (496 行)
│   ├── message.rs         # 消息类型
│   ├── tool.rs            # 工具 trait
│   ├── compact.rs         # 消息压缩 (301 行)
│   ├── state.rs           # 状态管理 (416 行)
│   ├── config.rs          # 配置管理 (583 行)
│   ├── session.rs         # 会话持久化
│   ├── history.rs         # 命令历史 (232 行) ✨
│   ├── cost.rs            # 成本追踪 (538 行) ✨
│   ├── scheduler.rs       # 任务调度 (418 行)
│   ├── mcp_manager.rs     # MCP 管理 (394 行)
│   ├── permission.rs      # 权限管理 (305 行)
│   ├── error.rs           # 错误处理 (346 行)
│   └── command/           # 命令系统 ✨
│       ├── mod.rs         # 模块导出
│       ├── parser.rs      # 命令解析器
│       ├── registry.rs    # 命令注册表
│       └── commands/
│           ├── mod.rs     # 基础定义
│           └── builtin.rs # 内置命令 (811 行)
├── aemeath-llm/           # LLM 客户端
│   ├── client.rs          # HTTP 客户端
│   ├── stream.rs          # 流式处理
│   └── types.rs           # API 类型
├── aemeath-tools/         # 工具实现 (26 个工具)
│   ├── bash.rs            # Shell 命令
│   ├── file_read.rs       # 文件读取
│   ├── file_write.rs      # 文件写入
│   ├── file_edit.rs       # 文件编辑
│   ├── glob_tool.rs       # 文件匹配
│   ├── grep.rs            # 内容搜索
│   ├── agent_tool.rs      # 子代理
│   ├── task_*.rs          # 任务工具 (5 个)
│   ├── web_fetch.rs       # 网页获取
│   ├── web_search.rs      # 网页搜索
│   ├── mcp_tool.rs        # MCP 工具
│   ├── list_mcp_resources.rs
│   ├── read_mcp_resource.rs
│   ├── lsp.rs             # LSP 工具
│   ├── skill_tool.rs      # 技能工具
│   ├── config_tool.rs     # 配置工具
│   ├── ask_user.rs        # 用户问答
│   ├── todo_write.rs      # Todo 管理
│   ├── sleep.rs           # 休眠
│   ├── tool_search.rs     # 工具搜索
│   └── plan_mode.rs       # 计划模式 ✨
└── aemeath-cli/           # CLI 界面
    ├── main.rs            # 入口点 (451 行)
    ├── repl.rs            # REPL 循环 (684 行)
    ├── agent_runner.rs    # 代理运行器
    ├── image.rs           # 图像处理 (420 行)
    └── tui/               # TUI 模块
        ├── app.rs         # 主应用 (661 行)
        ├── input_area.rs  # 输入区域 (408 行)
        ├── output_area.rs # 输出区域 (574 行)
        ├── dialog.rs      # 对话框 (318 行)
        ├── status_bar.rs  # 状态栏
        ├── task_list.rs   # 任务列表 (395 行)
        ├── completion.rs  # 自动完成 (440 行)
        └── key_hints.rs   # 键盘提示 (471 行)
```

#### extracted_sources (完整架构)
```
extracted_sources/src/
├── main.tsx               # 主入口 (803,924 字节)
├── assistant/             # 助手模式
├── bootstrap/             # 启动逻辑
├── bridge/                # 桥接层 (31 个模块)
├── buddy/                 # Buddy 功能
├── cli/                   # CLI 命令
├── commands/              # 命令系统 (101 个命令)
├── components/            # UI 组件 (144 个组件)
├── constants/             # 常量定义
├── context/               # 上下文管理
├── coordinator/           # 协调器模式
├── hooks/                 # React Hooks (85 个 hooks)
├── ink/                   # Ink UI 系统 (48 个模块)
├── memdir/                # 内存目录
├── migrations/            # 数据迁移 (11 个迁移)
├── native-ts/             # 原生模块
├── plugins/               # 插件系统
├── query/                 # 查询引擎
├── screens/               # 屏幕组件
├── server/                # 服务器
├── services/              # 服务层 (30+ 服务)
│   ├── analytics/         # 分析服务
│   ├── api/               # API 服务
│   ├── autoDream/         # 自动梦想
│   ├── lsp/               # LSP 服务
│   ├── mcp/               # MCP 协议 (21 个模块)
│   ├── oauth/             # OAuth 认证
│   ├── plugins/           # 插件服务
│   ├── voice/             # 语音服务
│   └── ...
├── skills/                # 技能系统
├── state/                 # 状态管理
├── tasks/                 # 任务系统 (多种任务类型)
├── tools/                 # 工具实现 (43 个工具)
├── types/                 # 类型定义
├── utils/                 # 工具函数 (200+ 工具)
├── vim/                   # Vim 模式
└── voice/                 # 语音输入
```

---

## 2. 工具系统对比

### aemeath 工具列表 (28 个工具)

| 工具名 | 功能 | 状态 |
|--------|------|------|
| BashTool | 执行 shell 命令 | ✅ 已实现 |
| FileReadTool | 读取文件内容 | ✅ 已实现 |
| FileWriteTool | 写入文件 | ✅ 已实现 |
| FileEditTool | 确字符串替换编辑 | ✅ 已实现 |
| GlobTool | 文件模式匹配 | ✅ 已实现 |
| GrepTool | 文件内容搜索 | ✅ 已实现 |
| LSPTool | 语言服务器协议集成 | ✅ 已实现 |
| WebFetchTool | 获取网页内容 | ✅ 已实现 |
| WebSearchTool | 网页搜索 (DuckDuckGo) | ✅ 已实现 |
| AgentTool | 子代理执行 | ✅ 已实现 |
| TaskCreateTool | 创建任务 | ✅ 已实现 |
| TaskUpdateTool | 更新任务 | ✅ 已实现 |
| TaskListTool | 列出任务 | ✅ 已实现 |
| TaskGetTool | 获取任务详情 | ✅ 已实现 |
| TaskStopTool | 停止任务 | ✅ 已实现 |
| TaskOutputTool | 任务输出管理 | ✅ 已实现 ✨ |
| TodoWriteTool | Todo 列表管理 | ✅ 已实现 |
| SkillTool | 技能执行 | ✅ 已实现 |
| ConfigTool | 配置管理 | ✅ 已实现 |
| SleepTool | 休眠等待 | ✅ 已实现 |
| AskUserQuestionTool | 用户问答 | ✅ 已实现 |
| ToolSearchTool | 工具搜索 | ✅ 已实现 |
| McpTool | MCP 工具调用 | ✅ 已实现 |
| ListMcpResourcesTool | 列出 MCP 资源 | ✅ 已实现 |
| ReadMcpResourceTool | 读取 MCP 资源 | ✅ 已实现 |
| EnterPlanModeTool | 进入计划模式 | ✅ 已实现 ✨ |
| ExitPlanModeTool | 退出计划模式 | ✅ 已实现 ✨ |
| BriefTool | 生成工作简报 | ✅ 已实现 ✨ (增强版) |

### extracted_sources 工具列表 (43+ 个工具)

#### 文件操作类
| 工具名 | 功能 | aemeath |
|--------|------|---------|
| FileReadTool | 读取文件 | ✅ 有 |
| FileWriteTool | 写入文件 | ✅ 有 |
| FileEditTool | 编辑文件 | ✅ 有 |
| GlobTool | 文件匹配 | ✅ 有 |
| GrepTool | 内容搜索 | ✅ 有 |
| NotebookEditTool | Notebook 编辑 | ❌ 无 |

#### Shell 和执行类
| 工具名 | 功能 | aemeath |
|--------|------|---------|
| BashTool | Shell 命令 | ✅ 有 |
| PowerShellTool | PowerShell 支持 | ❌ 无 |
| REPLTool | REPL 环境 | ❌ 无 |

#### 代理和任务管理类
| 工具名 | 功能 | aemeath |
|--------|------|---------|
| AgentTool | 代理管理 | ✅ 有 |
| TaskCreateTool | 创建任务 | ✅ 有 |
| TaskGetTool | 获取任务 | ✅ 有 |
| TaskListTool | 列出任务 | ✅ 有 |
| TaskUpdateTool | 更新任务 | ✅ 有 |
| TaskStopTool | 停止任务 | ✅ 有 |
| TeamCreateTool | 创建团队 | ❌ 无 |
| TeamDeleteTool | 删除团队 | ❌ 无 |

#### Web 和网络类
| 工具名 | 功能 | aemeath |
|--------|------|---------|
| WebFetchTool | 获取网页 | ✅ 有 |
| WebSearchTool | 网页搜索 | ✅ 有 |
| WebBrowserTool | 网页浏览 | ❌ 无 |

#### MCP (Model Context Protocol) 类
| 工具名 | 功能 | aemeath |
|--------|------|---------|
| McpTool | MCP 工具调用 | ✅ 有 |
| ListMcpResourcesTool | 列出 MCP 资源 | ✅ 有 |
| ReadMcpResourceTool | 读取 MCP 资源 | ✅ 有 |
| McpAuthTool | MCP 认证 | ❌ 无 |

#### 开发工具类
| 工具名 | 功能 | aemeath |
|--------|------|---------|
| LSPTool | 语言服务器协议集成 | ✅ 有 |
| ToolSearchTool | 工具搜索 | ✅ 有 |

#### 用户交互类
| 工具名 | 功能 | aemeath |
|--------|------|---------|
| AskUserQuestionTool | 用户交互问答 | ✅ 有 |
| SendMessageTool | 消息发送 | ❌ 无 |

#### 模式和工作流类
| 工具名 | 功能 | aemeath |
|--------|------|---------|
| EnterPlanModeTool | 进入计划模式 | ✅ 有 |
| ExitPlanModeTool | 退出计划模式 | ✅ 有 |
| EnterWorktreeTool | 进入工作树 | ❌ 无 |
| ExitWorktreeTool | 退出工作树 | ❌ 无 |

#### 配置和管理类
| 工具名 | 功能 | aemeath |
|--------|------|---------|
| ConfigTool | 配置管理 | ✅ 有 |
| BriefTool | 简报工具 | ✅ 有 (增强版) ✨ |
| TodoWriteTool | 待办事项写入 | ✅ 有 |

#### 技能和调度类
| 工具名 | 功能 | aemeath |
|--------|------|---------|
| SkillTool | 技能系统 | ✅ 有 |
| ScheduleCronTool | 定时任务 | ❌ 无 |
| RemoteTriggerTool | 远程触发器 | ❌ 无 |
| SleepTool | 休眠工具 | ✅ 有 |

#### 其他工具
| 工具名 | 功能 | aemeath |
|--------|------|---------|
| MonitorTool | 监控工具 | ❌ 无 |
| SnipTool | 代码片段 | ❌ 无 |
| WorkflowTool | 工作流 | ❌ 无 |
| TungstenTool | Tungsten 工具 | ❌ 无 |
| TestingPermissionTool | 测试权限 | ❌ 无 |
| TaskOutputTool | 任务输出 | ✅ 已实现 ✨ |
| SyntheticOutputTool | 合成输出 | ❌ 无 |

---

## 3. 核心功能差异

### 3.0 命令系统对比

#### aemeath
- ✅ **命令系统已实现** - 支持 22 个核心命令
- ✅ 命令解析器 (parser.rs)
- ✅ 命令注册机制 (registry.rs)
- ✅ 命令分类系统
- ✅ 命令帮助系统
- ✅ 命令覆盖率: **22%** (22 / 101+)

**已实现的命令：**

| 命令 | 功能 | 类别 |
|------|------|------|
| /help | 显示所有命令和帮助 | Core |
| /exit | 退出程序 | Core |
| /clear | 清屏/清历史 | Core |
| /compact | 消息压缩 | Core |
| /cost | 成本统计 | Utility |
| /usage | 使用统计 | Utility |
| /status | 显示状态 | Utility |
| /version | 版本信息 | Utility |
| /stats | 统计信息显示 | Utility ✨ |
| /review | 代码审查 | Git ✨ |
| /config | 配置管理 | Config |
| /model | 模型选择 | Config |
| /permissions | 权限管理 | Config |
| /resume | 会话恢复 | Session |
| /session | 会话管理（含元数据） | Session ✨ |
| /tasks | 任务管理（引导） | Tasks |
| /mcp | MCP 管理（引导） | Tools |
| /skills | 技能管理 | Tools |
| /doctor | 系统诊断 | Debug |
| /init | 项目初始化 | Git |
| /commit | Git 提交（引导） | Git |
| /rewind | 回退历史 | Session |

**命令架构：**
```
command/
├── mod.rs          # 模块导出
├── parser.rs       # 命令解析器
├── registry.rs     # 命令注册表
└── commands/
    ├── mod.rs      # 基础定义
    └── builtin.rs  # 内置命令 (20 个)
```

#### extracted_sources (101+ 命令)
完整的命令系统，包括：

**核心命令 (高优先级)**
| 命令 | 功能 | 文件 |
|------|------|------|
| /help | 帮助信息 | help/index.ts |
| /exit | 退出程序 | exit/index.ts |
| /clear | 清屏/清历史 | clear/index.ts |
| /compact | 消息压缩 | compact/index.ts |
| /cost | 成本统计 | cost/index.ts |
| /usage | 使用统计 | usage/index.ts |
| /init | 项目初始化 | init.ts (20,961 字节) |
| /config | 配置管理 | config/index.ts |
| /resume | 会话恢复 | resume/index.ts |
| /rewind | 回退历史 | rewind/index.ts |
| /commit | Git 提交 | commit.ts |
| /status | 状态显示 | status/index.ts |

**任务和代理命令**
| 命令 | 功能 | 文件 |
|------|------|------|
| /tasks | 任务管理 | tasks/index.ts |
| /agents | 代理管理 | agents/index.ts |
| /review | 代码审查 | review/index.ts |
| /branch | Git 分支 | branch/index.ts |

**MCP 和工具命令**
| 命令 | 功能 | 文件 |
|------|------|------|
| /mcp | MCP 管理 | mcp/index.ts |
| /skills | 技能管理 | skills/index.ts |
| /permissions | 权限管理 | permissions/index.ts |
| /hooks | 钩子管理 | hooks/index.ts |

**模式和设置命令**
| 命令 | 功能 | 文件 |
|------|------|------|
| /plan | 计划模式 | plan/index.ts |
| /model | 模型选择 | model/index.ts |
| /theme | 主题设置 | theme/index.ts |
| /vim | Vim 模式 | vim/index.ts |
| /voice | 语音设置 | voice/index.ts |
| /keybindings | 键绑定 | keybindings/index.ts |

**其他实用命令**
| 命令 | 功能 | 文件 |
|------|------|------|
| /doctor | 系统诊断 | doctor/index.ts |
| /version | 版本信息 | version.ts |
| /diff | 差异对比 | diff/index.ts |
| /export | 数据导出 | export/index.ts |
| /files | 文件管理 | files/index.ts |
| /memory | 内存管理 | memory/index.ts |
| /context | 上下文管理 | context/index.ts |
| /add-dir | 添加目录 | add-dir/index.ts |
| /session | 会话管理 | session/index.ts |
| /stats | 统计信息 | stats/index.ts |
| /feedback | 反馈 | feedback/index.ts |
| /upgrade | 升级检查 | upgrade/index.ts |
| /login | 登录认证 | login/index.ts |
| /logout | 登出 | logout/index.ts |
| /plugin | 插件管理 | plugin/index.ts |
| /reload-plugins | 重载插件 | reload-plugins/index.ts |
| /ultraplan | 高级计划 | ultraplan.tsx (66,629 字节) |
| /insights | 数据洞察 | insights.ts (115,949 字节) |

### 3.1 Agent 系统

#### aemeath
```rust
pub struct Agent<'a> {
    pub registry: &'a ToolRegistry,
    pub ctx: ToolContext,
}
// 还有 agent_lifecycle.rs (15,126 字节) 管理代理生命周期
```
- ✅ 基本的代理定义和工具调用执行
- ✅ 代理生命周期管理 (agent_lifecycle.rs)
- ✅ 代理状态跟踪
- ✅ AgentRunner 支持子代理执行
- ❌ 无代理内存和快照
- ❌ 无代理间通信
- ❌ 无代理分叉和恢复
- ❌ 无工作树模式支持
- ❌ 无代理颜色管理

#### extracted_sources
- ✅ 多种代理类型：
  - LocalAgentTask: 本地代理任务
  - RemoteAgentTask: 远程代理任务
  - DreamTask: 梦想任务
  - InProcessTeammateTask: 进程内队友任务
  - LocalShellTask: 本地 Shell 任务
- ✅ 完整的代理生命周期管理
- ✅ 代理内存和快照
- ✅ 进度跟踪和 UI 显示
- ✅ 代理分叉和恢复
- ✅ 工作树模式支持
- ✅ 代理颜色管理 (AgentColorManager)

### 3.2 任务管理系统

#### aemeath
- ✅ 任务状态管理 (TaskStore in state.rs)
- ✅ TaskCreateTool, TaskGetTool, TaskListTool, TaskUpdateTool, TaskStopTool
- ✅ 任务状态: Pending, InProgress, Completed, Deleted
- ✅ 任务依赖关系 (blocked_by, blocks)
- ✅ 任务所有者和活跃状态
- ✅ 任务调度器 (scheduler.rs - 13,107 字节)
- ❌ 无后台任务调度
- ❌ 无任务恢复机制
- ❌ 无 DreamTask / RemoteAgentTask 等高级任务类型

#### extracted_sources
```typescript
// 完整的任务系统
- TaskState 状态管理
- TaskCreateTool, TaskGetTool, TaskListTool, TaskUpdateTool, TaskStopTool
- 后台任务管理
- 任务优先级
- 任务依赖关系
- 任务进度追踪
- DreamTask: 梦想任务
- LocalAgentTask: 本地代理任务
- RemoteAgentTask: 远程代理任务
- InProcessTeammateTask: 进程内队友任务
- LocalShellTask: 本地 Shell 任务
```

### 3.3 状态管理和持久化

#### aemeath
```rust
// state.rs (416 行) - 完整的状态管理
// session.rs (105 行) - 基础会话持久化
// config.rs (583 行) - 配置管理器和持久化
// history.rs (232 行) - 命令历史持久化 ✨
// cost.rs (538 行) - 成本追踪持久化 ✨
pub struct AppState {
    pub messages: Vec<Message>,
    pub tasks: HashMap<String, Task>,
    pub config: Config,
    pub mcp_manager: McpManager,
    pub permissions: PermissionStore,
    pub session: Session,
}
```
- ✅ 全局状态管理 (AppState)
- ✅ 配置管理 (config.rs - 583 行)
- ✅ MCP 状态管理 (mcp_manager.rs - 394 行)
- ✅ 权限状态管理 (permission.rs - 305 行)
- ✅ **基础会话持久化** (session.rs - 已实现)
  - save_session() - 保存会话到 ~/.aemeath/sessions/<id>.json
  - load_session() - 从磁盘加载会话
  - list_sessions() - 列出所有保存的会话
  - 按更新时间排序
- ✅ **配置文件持久化** (config.rs - 已实现)
  - ConfigManager - 分层配置管理
  - 全局配置: ~/.config/aemeath/config.json
  - 项目配置: .aemeath/config.json
  - 环境变量覆盖
  - save_global() / save_project()
- ✅ 任务状态存储
- ✅ **历史记录持久化** (history.rs - 232 行) ✨
  - HistoryManager - 命令历史管理
  - ~/.aemeath/history.json - 历史文件
  - 历史条目限制 (默认 1000)
  - 历史搜索和自动完成建议
  - 历史统计功能
- ✅ **会话自动保存** (state.rs - 已实现)
  - save_session() - 自动保存
  - 按更新时间排序
- ✅ **成本追踪持久化** (cost.rs - 538 行) ✨
  - CostTracker - API 成本追踪
  - ~/.aemeath/cost_history.json - 成本历史
  - 多模型价格配置 (Opus, Sonnet, Haiku)
  - 会话成本统计
- ❌ 无消息历史压缩持久化
- ❌ 无会话元数据管理（标题、标签等）

#### extracted_sources
```typescript
// 完整的应用状态管理
interface AppState {
  settings: SettingsJson
  tasks: { [taskId: string]: TaskState }
  mcp: {
    clients: MCPServerConnection[]
    tools: Tool[]
    commands: Command[]
    resources: Record<string, ServerResource[]>
  }
  plugins: {
    enabled: LoadedPlugin[]
    disabled: LoadedPlugin[]
    errors: PluginError[]
  }
  // ... 大量其他状态
}
```

### 3.4 远程能力

#### aemeath
- ❌ 无远程代理
- ❌ 无会话同步
- ❌ 无远程模式
- ❌ 无 WebSocket 连接

#### extracted_sources
- ✅ 远程代理支持
- ✅ 会话同步
- ✅ Bridge 模式
  - replBridgeEnabled
  - replBridgeConnected
  - replBridgeSessionActive
  - replBridgeReconnecting
- ✅ WebSocket 连接管理
- ✅ 远程连接状态追踪

### 3.5 协作功能

#### aemeath
- ❌ 无团队成员管理
- ❌ 无消息传递
- ❌ 无共享会话

#### extracted_sources
- ✅ 团队成员管理 (TeamCreateTool, TeamDeleteTool)
- ✅ 消息传递 (SendMessageTool)
- ✅ 共享会话
- ✅ 协调器模式 (Coordinator Mode)
- ✅ 队友视图帮助 (teammateViewHelpers)

---

## 4. UI/UX 功能对比

### 4.1 用户界面

#### aemeath
- ✅ 完整的 TUI (Terminal UI) 应用 (tui 目录)
  - app.rs (27,786 字节) - 主应用
  - input_area.rs (13,101 字节) - 输入区域
  - output_area.rs (18,680 字节) - 输出区域
  - dialog.rs (10,405 字节) - 对话框系统
  - status_bar.rs (5,445 字节) - 状态栏
  - task_list.rs (10,743 字节) - 任务列表
  - completion.rs (13,975 字节) - 自动完成
  - key_hints.rs (13,650 字节) - 键盘提示
- ✅ 命令行 REPL
- ✅ Markdown 渲染
- ✅ Spinner 动画
- ✅ Diff 显示
- ✅ 交互式对话框 (dialog.rs)
- ✅ 自动完成系统 (completion.rs)
- ✅ 任务列表视图
- ✅ 状态栏显示
- ❌ 无多屏幕界面切换
- ❌ 无 Vim 模式
- ❌ 无语音输入
- ❌ 无上下文可视化

#### extracted_sources
- ✅ 完整的 TUI (Terminal UI) 应用
- ✅ React/Ink 组件系统 (144 个组件)
- ✅ 多屏幕和对话框
- ✅ 键绑定系统
- ✅ Vim 模式支持
- ✅ 语音输入
- ✅ 富文本渲染
- ✅ 进度条和状态显示
- ✅ 多代理可视化
- ✅ AgentProgressLine 组件
- ✅ 交互式建议和自动完成
- ✅ 上下文可视化 (ContextVisualization)
- ✅ 反馈系统 (Feedback.tsx - 87,696 字节)

### 4.2 主要 UI 组件

| 组件类型 | aemeath | extracted_sources |
|---------|---------|-------------------|
| TUI 界面 | ✅ 完整版 (ratatui) | ✅ 完整版 (Ink) |
| REPL 界面 | ✅ 有 | ✅ 有 |
| Markdown 渲染 | ✅ comrak | ✅ 自定义渲染器 |
| Spinner 动画 | ✅ 有 | ✅ 多种动画 |
| Diff 显示 | ✅ similar crate | ✅ FileEditToolDiff |
| 进度显示 | ✅ status_bar | ✅ AgentProgressLine |
| 对话框 | ✅ dialog.rs | ✅ 多种对话框 |
| 输入区域 | ✅ input_area.rs | ✅ 多行输入 |
| 输出区域 | ✅ output_area.rs | ✅ 滚动输出 |
| 自动完成 | ✅ completion.rs | ✅ 智能补全 |
| 任务列表 | ✅ task_list.rs | ✅ 任务面板 |
| 键绑定 | ✅ key_hints.rs | ✅ 完整系统 |
| Vim 模式 | ❌ 无 | ✅ 支持 |
| 语音输入 | ❌ 无 | ✅ 支持 |
| 多屏幕 | ❌ 无 | ✅ 支持 |
| 上下文可视化 | ❌ 无 | ✅ 支持 |

---

## 5. MCP (Model Context Protocol) 支持

### aemeath
```rust
// mcp.rs (6,785 字节) - MCP 协议基础实现
// mcp_manager.rs (12,852 字节) - MCP 连接管理
```
- ✅ MCP 协议基础支持 (mcp.rs)
- ✅ MCP 服务器连接管理 (mcp_manager.rs)
- ✅ MCP 工具调用 (McpTool)
- ✅ MCP 资源列表 (ListMcpResourcesTool)
- ✅ MCP 资源读取 (ReadMcpResourceTool)
- ❌ 无 MCP 认证 (McpAuthTool)
- ❌ 无 MCP 频道白名单
- ❌ 无 MCP 请求处理 (elicitation)

### extracted_sources
```typescript
// 完整的 MCP 支持
- mcp/client.ts          // MCP 客户端
- mcp/config.ts          // MCP 配置
- mcp/types.ts           // MCP 类型定义
- mcp/MCPConnectionManager.tsx  // 连接管理
- mcp/auth.ts            // MCP 认证
- mcp/channelAllowlist.ts // 频道白名单
- mcp/elicitationHandler.ts // 请求处理
```

---

## 6. 插件系统

### aemeath
- ❌ 无插件系统
- ❌ 无扩展机制

### extracted_sources
- ✅ 完整的插件系统
  - 插件加载和初始化
  - 插件错误处理
  - 插件命令集成
  - 插件市场安装

---

## 7. 技能系统

### aemeath
```rust
// skill.rs (2,874 字节) - 技能基础实现
```
- ✅ 技能基础定义 (skill.rs)
- ✅ SkillTool 工具调用
- ❌ 无技能目录加载
- ❌ 无 MCP 技能构建器
- ❌ 无打包技能系统

### extracted_sources
- ✅ 完整的技能系统
  - bundledSkills.ts - 打包的技能
  - loadSkillsDir.ts - 加载技能目录
  - mcpSkillBuilders.ts - MCP 技能构建器

---

## 8. 语音功能

### aemeath
- ❌ 无语音输入
- ❌ 无语音识别

### extracted_sources
- ✅ 语音输入支持
  - voice.ts - 语音服务
  - voiceKeyterms.ts - 关键词识别
  - voiceStreamSTT.ts - 流式语音转文字
  - VoiceProvider - React 上下文

---

## 9. LLM 集成对比

### aemeath
```rust
// client.rs (8,528 字节) - HTTP 客户端
// stream.rs (5,148 字节) - 流式处理
// types.rs (2,623 字节) - API 类型
pub struct LlmClient {
    client: reqwest::Client,
    api_key: String,
    pub base_url: String,
    pub model: String,
    pub max_tokens: u32,
    pub user_agent: String,
}
```
- ✅ 完整的 HTTP 客户端 (client.rs)
- ✅ SSE 流式响应处理 (stream.rs)
- ✅ 完整的消息格式和类型定义
- ✅ 图像处理支持 (image.rs - 12,892 字节)
- ❌ 单一 API 提供商（Anthropic）
- ❌ 无 OAuth 认证流程
- ❌ 无配额和限制管理
- ❌ 无成本追踪

### extracted_sources
- ✅ 多提供商支持
  - Anthropic
  - AWS Bedrock
  - Google Cloud
- ✅ OAuth 认证流程
- ✅ API 密钥链管理
- ✅ 配额和限制管理
- ✅ 成本追踪 (cost-tracker.ts)
- ✅ 遥测和分析
- ✅ 远程会话支持
- ✅ 模型选择和配置
- ✅ API 预连接 (apiPreconnect.ts)

---

## 10. 数据迁移和持久化

### aemeath
- ❌ 无数据迁移
- ❌ 无持久化存储
- ❌ 无会话保存

### extracted_sources
- ✅ 完整的迁移系统 (11 个迁移文件)
  - migrateAutoUpdatesToSettings.ts
  - migrateBypassPermissionsAcceptedToSettings.ts
  - migrateEnableAllProjectMcpServersToSettings.ts
  - migrateFennecToOpus.ts
  - migrateLegacyOpusToCurrent.ts
  - migrateOpusToOpus1m.ts
  - migrateReplBridgeEnabledToRemoteControlAtStartup.ts
  - migrateSonnet1mToSonnet45.ts
  - migrateSonnet45ToSonnet46.ts
  - resetAutoModeOptInForDefaultOffer.ts
  - resetProToOpusDefault.ts
- ✅ 会话历史 (history.ts - 14,081 字节)
- ✅ 文件历史快照

---

## 11. 服务层对比

### aemeath
- ❌ 无服务层

### extracted_sources (30+ 服务)
| 服务类别 | 模块 | 功能 |
|---------|------|------|
| 分析 | analytics/ | 使用分析 |
| API | api/ | API 调用管理 |
| 自动梦想 | autoDream/ | 自动化功能 |
| LSP | lsp/ | 语言服务器协议 |
| MCP | mcp/ | Model Context Protocol |
| OAuth | oauth/ | OAuth 认证 |
| 插件 | plugins/ | 插件管理 |
| 语音 | voice/ | 语音输入 |
| 提示建议 | PromptSuggestion/ | 提示建议 |
| 远程管理 | remoteManagedSettings/ | 远程设置管理 |
| 团队同步 | teamMemorySync/ | 团队内存同步 |
| 会话内存 | SessionMemory/ | 会话记忆 |
| 工具 | tools/ | 工具服务 |
| 通知 | notifier.ts | 通知系统 |
| 防休眠 | preventSleep.ts | 防止系统休眠 |
| 令牌估算 | tokenEstimation.ts | Token 计算 |

---

## 12. React Hooks 对比

### aemeath
- ❌ 无 React Hooks（不是 React 应用）

### extracted_sources (85+ Hooks)
```
useAfterFirstRender
useArrowKeyHistory
useAssistantHistory
useAwaySummary
useBackgroundTaskNavigation
useBlink
useCancelRequest
useCanUseTool
useClipboardImageHint
useCommandKeybindings
useCommandQueue
useCopyOnSelect
useDeferredHookMessages
useDiffData
useDiffInIDE
useDirectConnect
useDoublePress
useDynamicConfig
useElapsedTime
... (还有 60+ 个)
```

---

## 13. 错误处理对比

### aemeath
```rust
// 使用 Rust Result 类型
pub type ToolResult = Result<String, String>;
```
- ✅ Rust Result 类型
- ✅ 编译时类型安全
- ❌ 基本错误消息
- ❌ 无错误追踪系统
- ❌ 无错误恢复

### extracted_sources
- ✅ 完整的错误追踪系统
- ✅ 用户友好的错误显示
- ✅ 错误恢复和重试
- ✅ 警告处理器
- ✅ FallbackToolUseErrorMessage
- ✅ FallbackToolUseRejectedMessage

---

## 14. 并发和性能对比

### aemeath
```rust
// Tokio 异步运行时
- 信号量限制并发（最多 10）
- 简单的工具分类（并发安全/不安全）
```
- ✅ Tokio 异步运行时
- ✅ 信号量限制并发
- ✅ 低内存占用 (~10-50 MB)
- ✅ 快速启动 (< 100ms)
- ✅ 小型二进制文件 (~10-20 MB)

### extracted_sources
- ✅ 复杂的任务调度
- ✅ 多代理并行执行
- ✅ 工作树隔离
- ✅ 后台任务管理
- ✅ 缓存系统
- ❌ 高内存占用 (~200-500 MB)
- ❌ 较慢启动 (~1-2 秒)

---

## 15. 安全和权限

### aemeath
```rust
// permission.rs (9,999 字节) - 权限管理系统
// error.rs (11,128 字节) - 错误处理
```
- ✅ 权限系统基础 (permission.rs)
- ✅ 权限模式控制
- ✅ 错误处理系统 (error.rs)
- ❌ 无认证管理 (OAuth)
- ❌ 无 API 密钥管理
- ❌ 无策略限制系统
- ❌无安全沙箱

### extracted_sources
- ✅ 完整的权限系统
  - PermissionMode
  - ToolPermissionContext
  - BypassPermissionsMode
- ✅ 认证管理
  - OAuth 流程
  - API 密钥管理
  - AWS 认证状态
- ✅ 策略和限制系统
  - policyLimits/
  - DenialTrackingState
- ✅ 安全沙箱

---

## 16. 开发者工具

### aemeath
- ❌ 无开发者工具

### extracted_sources
- ✅ DevBar 组件
- ✅ DiagnosticsDisplay
- ✅ 遥测系统
- ✅ GrowthBook 集成（功能标志）
- ✅ 性能分析
- ✅ 调试日志

---

## 17. 依赖对比

### aemeath (Cargo.toml)
```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json", "stream"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
clap = { version = "4", features = ["derive"] }
tokio-stream = "0.1"
futures-util = "0.3"
crossterm = "0.28"
comrak = "0.29"
ansi_term = "0.12"
spinners = "4"
similar = "2"
chrono = "0.4"
```
**总计**: 约 15 个依赖

### extracted_sources (package.json)
- React / Ink (UI 框架)
- Bun (运行时)
- 大量 npm 包 (~200+ 依赖)
- 原生模块 (Rust/C++)
- VSCode 相关库

---

## 18. 功能完整性总结

### aemeath 已实现的功能 ✅

#### 核心功能 (已实现)
1. **工具系统**: 26 个工具已实现 ✨
   - 文件操作: Read, Write, Edit, Glob, Grep
   - Shell 执行: BashTool
   - 任务管理: TaskCreate, TaskGet, TaskList, TaskUpdate, TaskStop
   - Web 工具: WebFetch, WebSearch
   - MCP 工具: McpTool, ListMcpResources, ReadMcpResource
   - 用户交互: AskUserQuestion
   - 计划模式: EnterPlanModeTool, ExitPlanModeTool ✨
   - 其他: LSP, Agent, TodoWrite, Skill, Config, Sleep, ToolSearch

2. **代理系统**: 基础代理功能
   - Agent 定义和工具调用
   - AgentRunner 子代理执行
   - AgentLifecycle 生命周期管理

3. **状态管理**: 完整的内存状态管理
   - AppState 全局状态
   - Config 配置管理
   - Session 会话管理
   - Task 任务状态
   - Permission 权限状态
   - MCP 状态管理

4. **MCP 协议**: 基础 MCP 支持
   - MCP 服务器连接
   - MCP 工具调用
   - MCP 资源读取

5. **TUI 界面**: 完整的终端 UI
   - 主应用界面 (app.rs)
   - 输入区域 (input_area.rs)
   - 输出区域 (output_area.rs)
   - 对话框系统 (dialog.rs)
   - 状态栏 (status_bar.rs)
   - 任务列表 (task_list.rs)
   - 自动完成 (completion.rs)
   - 键盘提示 (key_hints.rs)

6. **LLM 集成**: Anthropic API
   - HTTP 客户端
   - SSE 流式响应
   - 图像处理支持

7. **其他核心功能**
   - 消息压缩 (compact.rs)
   - 错误处理系统 (error.rs)
   - 权限基础系统 (permission.rs)
   - 技能基础定义 (skill.rs)
   - 任务调度器 (scheduler.rs)

8. **命令系统**: 20 个核心命令 ✨
   - 命令解析器 (command/parser.rs)
   - 命令注册表 (command/registry.rs)
   - 内置命令 (command/commands/builtin.rs)
   - 分类: Core, Session, Config, Tasks, Tools, Git, Utility, Debug

9. **持久化存储** ✨
   - 会话持久化 (session.rs)
   - 配置持久化 (config.rs)
   - 历史记录持久化 (history.rs)
   - 状态管理持久化 (state.rs)
   - 成本历史持久化 (cost.rs) ✨

10. **成本追踪系统** ✨
    - CostTracker API 成本管理
    - 多模型价格配置 (Opus, Sonnet, Haiku)
    - 会话成本统计
    - 总成本汇总
    - /cost 命令集成

### aemeath 缺失的功能 ❌

#### 高优先级缺失
1. **增强持久化**
   - 消息历史压缩持久化
   - 会话元数据管理（标题、标签等）
   - 状态快照保存/恢复

2. **高级代理类型**
   - RemoteAgentTask 远程代理
   - DreamTask 梦想任务
   - InProcessTeammateTask 队友任务
   - 代理内存和快照
   - 代理分叉和恢复

3. **多 LLM 提供商**
   - AWS Bedrock 支持
   - Google Cloud 支持
   - OAuth 认证流程

4. **高级任务功能**
   - 后台任务调度
   - 任务恢复机制
   - 任务优先级

#### 中优先级缺失
5. **工具扩展**
   - BriefTool 简报工具
   - NotebookEditTool
   - PowerShellTool (可选)
   - WebBrowserTool
   - TaskOutputTool
   - ScheduleCronTool
   - /review 审查命令
   - /commit 提交命令
   - /usage 使用统计
   - /stats 统计命令
   - /branch 分支命令
   - /plan 计划模式
   - /mcp MCP 命令
   - /plugin 插件命令
   - /skills 技能命令
   - /memory 内存命令
   - /permissions 权限命令
   - /login/logout 认证命令

6. ~~**模式和工作流**~~ - ✅ PlanMode 已实现
   - ✅ PlanMode 计划模式
   - ❌ WorktreeMode 工作树模式
   - ✅ EnterPlanModeTool / ExitPlanModeTool
   - ❌ EnterWorktreeTool / ExitWorktreeTool

7. **团队协作**
   - TeamCreateTool / TeamDeleteTool
   - SendMessageTool 消息发送
   - 团队内存同步

8. ~~**工具扩展**~~ - 部分已实现
   - ❌ NotebookEditTool
   - ❌ PowerShellTool
   - ❌ REPLTool
   - ❌ WebBrowserTool
   - ✅ BriefTool (已实现增强版) ✨
   - ❌ ScheduleCronTool (调度器已有，工具待实现)
   - ❌ RemoteTriggerTool
   - ❌ MonitorTool
   - ✅ TaskOutputTool (已实现) ✨

#### 低优先级缺失
9. **UI 增强**
   - Vim 模式
   - 语音输入
   - 多屏幕界面
   - 上下文可视化

10. **企业功能**
    - 插件系统完整实现
    - 远程代理支持
    - Bridge 模式
    - WebSocket 连接
    - 数据迁移系统
    - 遥测和分析
    - GrowthBook 功能标志
    - 防休眠服务
    - 通知系统

11. **MCP 高级功能**
    - MCP 认证 (McpAuthTool)
    - MCP 频道白名单
    - MCP 请求处理 (elicitation)

12. **开发者工具**
    - DevBar 组件
    - DiagnosticsDisplay
    - 性能分析
    - 调试日志

13. **高级技能系统**
    - 技能目录加载
    - MCP 技能构建器
    - 打包技能系统

---

## 19. 代码组织对比

### aemeath 模块结构
```
aemeath/
├── aemeath-core/      # 核心逻辑
├── aemeath-llm/       # LLM 客户端
├── aemeath-tools/     # 工具实现
└── aemeath-cli/       # CLI 界面
```

### extracted_sources 模块结构
```
extracted_sources/src/
├── main.tsx           # 主入口
├── assistant/         # 助手模式
├── bootstrap/         # 启动逻辑
├── bridge/           # 桥接层
├── buddy/            # Buddy 功能
├── cli/              # CLI 命令
├── commands/         # 命令系统
├── components/       # UI 组件
├── constants/        # 常量定义
├── context/          # 上下文管理
├── coordinator/      # 协调器模式
├── hooks/            # React Hooks
├── ink/              # Ink UI 系统
├── memdir/           # 内存目录
├── migrations/       # 数据迁移
├── native-ts/        # 原生模块
├── plugins/          # 插件系统
├── query/            # 查询引擎
├── screens/          # 屏幕组件
├── server/          # 服务器
├── services/         # 服务层
├── skills/           # 技能系统
├── state/            # 状态管理
├── tasks/            # 任务系统
├── tools/            # 工具实现
├── types/            # 类型定义
├── utils/            # 工具函数
├── vim/              # Vim 模式
└── voice/            # 语音输入
```

---

## 20. 关键差异总结

| 维度 | aemeath | extracted_sources |
|------|---------|-------------------|
| **规模** | 17,079 行 | 512,664 行 (30x) |
| **工具数量** | 28 个 ✨ | 43+ 个 (65% 覆盖) |
| **命令数量** | 22 个 ✨ | 101+ 个 (22% 覆盖) |
| **TUI 组件** | 9 个模块 | 144+ 个 (16x) |
| **持久化** | 会话/配置/历史/成本 ✨ | 完整持久化 |
| **服务数量** | 基础模块 | 30+ 个 |
| **复杂度** | 中等 MVP | 企业级应用 |
| **功能完整度** | ~67% | 100% |
| **扩展性** | 固定架构 | 插件系统 |
| **生产就绪** | 实验性 | 企业级 |

---

## 21. 建议的开发路线图

### ~~第一阶段：持久化增强~~ ✅ 已完成
1. ✅ 状态持久化到磁盘 (state.rs)
2. ✅ 会话历史保存 (session.rs)
3. ✅ 配置文件持久化 (config.rs)
4. ✅ 命令历史持久化 (history.rs)

### ~~第二阶段：命令系统~~ ✅ 已完成
1. ✅ /help 帮助命令
2. ✅ /exit 退出命令
3. ✅ /clear 清屏命令
4. ✅ /compact 压缩命令
5. ✅ /cost 成本命令
6. ✅ /config 配置命令
7. ✅ /resume 恢复命令
8. ✅ /rewind 回退命令
9. ✅ /commit 提交命令
10. ✅ /usage 使用统计
11. ✅ /status 状态显示
12. ✅ /version 版本信息
13. ✅ /model 模型选择
14. ✅ /session 会话管理
15. ✅ /tasks 任务管理
16. ✅ /mcp MCP 管理
17. ✅ /skills 技能管理
18. ✅ /permissions 权限管理
19. ✅ /doctor 系统诊断
20. ✅ /init 项目初始化

### ~~第三阶段：增强功能~~ ✅ 已完成
1. ✅ 实现 PlanMode 计划模式
2. ✅ 实现 EnterPlanModeTool / ExitPlanModeTool (plan_mode.rs)
3. ✅ 添加成本追踪系统 (cost.rs)
   - CostTracker 管理 API 成本
   - 多模型价格配置
   - /cost 命令已集成成本追踪
4. ✅ BriefTool 简报工具 (已实现增强版) ✨

### 第四阶段：多 LLM 提供商 (2-3 周)
1. 添加 AWS Bedrock 支持
2. 添加 Google Cloud 支持
3. 实现成本追踪系统
4. 添加配额和限制管理

### 第五阶段：高级代理 (3-4 周)
1. 实现 RemoteAgentTask 远程代理
2. 实现代理内存和快照
3. 实现代理分叉和恢复
4. 添加 DreamTask 梦想任务

### 第六阶段：工具扩展 (2 周)
1. 实现 NotebookEditTool
2. 实现 PowerShellTool (可选)
3. 实现 TaskOutputTool
4. 添加 ScheduleCronTool

### 第七阶段：企业功能 (可选)
1. 远程代理支持
2. 协作功能 (TeamCreate/Delete)
3. 插件系统完整实现
4. 语音输入
5. WebSocket 连接和 Bridge 模式

---

## 24. 详细缺失功能清单

### 24.1 缺失的工具 (13 个)

| 工具名 | 功能描述 | 优先级 | 状态 |
|--------|----------|--------|------|
| NotebookEditTool | Jupyter Notebook 编辑 | 低 | ❌ |
| PowerShellTool | Windows PowerShell 执行 | 低 | ❌ |
| REPLTool | REPL 环境集成 | 低 | ❌ |
| WebBrowserTool | 网页浏览自动化 | 中 | ❌ |
| McpAuthTool | MCP 认证 | 中 | ❌ |
| SendMessageTool | 团队消息发送 | 中 | ❌ |
| EnterWorktreeTool | 进入工作树模式 | 中 | ❌ |
| ExitWorktreeTool | 退出工作树模式 | 中 | ❌ |
| ScheduleCronTool | 定时任务调度 | 中 | ❌ |
| RemoteTriggerTool | 远程触发器 | 低 | ❌ |
| MonitorTool | 监控工具 | 低 | ❌ |
| SyntheticOutputTool | 合成输出 | 低 | ❌ |
| TeamCreateTool | 创建团队 | 低 | ❌ |
| TeamDeleteTool | 删除团队 | 低 | ❌ |

### 24.2 缺失的命令 (约 80 个)

aemeath 已实现 20 个核心命令，以下是仍未实现的命令：

#### 核心命令 (已实现)
| 命令 | 功能 | 状态 |
|------|------|------|
| /init | 项目初始化 | ✅ 已实现 |
| /compact | 消息压缩 | ✅ 已实现 |
| /cost | 成本统计 | ✅ 已实现 |
| /config | 配置管理 | ✅ 已实现 |
| /resume | 会话恢复 | ✅ 已实现 |
| /help | 帮助信息 | ✅ 已实现 |
| /exit | 退出 | ✅ 已实现 |
| /clear | 清屏 | ✅ 已实现 |

#### 待实现的命令
| 命令 | 功能 | 优先级 |
|------|------|--------|
| /rewind | 回退历史 | 高 |
| /commit | Git 提交 | 高 |
| /usage | 使用统计 | 高 |
| /stats | 统计信息 | 中 |
| /branch | Git 分支操作 | 中 |
| /review | 代码审查 | 中 |
| /diff | 差异对比 | 中 |
| /export | 数据导出 | 中 |
| /files | 文件管理 | 中 |
| /clear | 清屏 | 中 |
| /help | 帮助信息 | 高 |
| /exit | 退出 | 高 |

#### MCP 相关命令
| 命令 | 功能 | 优先级 |
|------|------|--------|
| /mcp | MCP 管理 | 中 |
| /mcp add | 添加 MCP 服务器 | 中 |
| /mcp remove | 移除 MCP 服务器 | 中 |
| /mcp list | 列出 MCP 服务器 | 中 |

#### 插件相关命令
| 命令 | 功能 | 优先级 |
|------|------|--------|
| /plugin | 插件管理 | 低 |
| /plugin install | 安装插件 | 低 |
| /plugin remove | 移除插件 | 低 |
| /plugin list | 列出插件 | 低 |
| /reload-plugins | 重载插件 | 低 |

#### 技能相关命令
| 命令 | 功能 | 优先级 |
|------|------|--------|
| /skills | 技能管理 | 中 |
| /skills add | 添加技能 | 中 |
| /skills remove | 移除技能 | 中 |

#### 其他命令
| 命令 | 功能 | 优先级 |
|------|------|--------|
| /plan | 计划模式 | 高 |
| /memory | 内存管理 | 中 |
| /permissions | 权限管理 | 中 |
| /login | 登录认证 | 低 |
| /logout | 登出 | 低 |
| /doctor | 系统诊断 | 低 |
| /upgrade | 升级检查 | 低 |
| /version | 版本信息 | 中 |
| /feedback | 反馈 | 低 |
| /theme | 主题设置 | 低 |
| /vim | Vim 模式 | 低 |
| /voice | 语音设置 | 低 |
| /keybindings | 键绑定 | 低 |
| /status | 状态显示 | 中 |
| /tasks | 任务管理 | 高 |
| /add-dir | 添加目录 | 中 |
| /context | 上下文管理 | 中 |
| /hooks | 钩子管理 | 低 |
| /ide | IDE 集成 | 低 |
| /install | 安装应用 | 低 |
| /security-review | 安全审查 | 低 |
| /ultraplan | 高级计划 | 低 |

### 24.3 缺失的服务 (30+ 个)

extracted_sources/src/services/ 目录包含以下服务，aemeath 目前全部缺失：

| 服务 | 功能描述 | 优先级 |
|------|----------|--------|
| analytics/ | 使用分析 | 低 |
| api/ | API 调用管理 | 高 |
| autoDream/ | 自动梦想 | 低 |
| compact/ | 消息压缩服务 | 中 |
| extractMemories/ | 内存提取 | 低 |
| lsp/ | LSP 服务集成 | 中 (已有 LSPTool) |
| MagicDocs/ | 魔法文档 | 低 |
| oauth/ | OAuth 认证 | 低 |
| plugins/ | 插件服务 | 低 |
| policyLimits/ | 策略限制 | 低 |
| PromptSuggestion/ | 提示建议 | 中 |
| remoteManagedSettings/ | 远程设置 | 低 |
| SessionMemory/ | 会话内存 | 高 |
| settingsSync/ | 设置同步 | 低 |
| teamMemorySync/ | 团队同步 | 低 |
| tips/ | 提示服务 | 低 |
| tools/ | 工具服务 | 中 |
| voice/ | 语音服务 | 低 |
| notifier.ts | 通知系统 | 中 |
| preventSleep.ts | 防休眠 | 低 |
| tokenEstimation.ts | Token 估算 | 高 |
| vcr.ts | 录制回放 | 低 |
| claudeAiLimits.ts | AI 限制 | 中 |
| diagnosticTracking.ts | 诊断追踪 | 低 |
| rateLimitMessages.ts | 速率限制 | 中 |

### 24.4 缺失的 UI 功能

| 功能 | 描述 | 优先级 |
|------|------|--------|
| Vim 模式 | Vim 键绑定支持 | 低 |
| 语音输入 | 语音转文字 | 低 |
| 多屏幕界面 | 屏幕切换系统 | 中 |
| 上下文可视化 | Token 上下文显示 | 中 |
| DevBar | 开发者工具栏 | 低 |
| DiagnosticsDisplay | 诊断显示 | 低 |
| Feedback 组件 | 反馈系统 | 低 |
| 多代理可视化 | 并行代理显示 | 低 |

### 24.5 缺失的核心架构

| 架构组件 | 描述 | 优先级 | 状态 |
|----------|------|--------|------|
| 持久化存储 | 状态/会话持久化 | 高 | ✅ 已实现 |
| 命令系统 | 斜杠命令支持 | 高 | ✅ 已实现 |
| 历史记录持久化 | 命令历史 | 中 | ✅ 已实现 |
| 成本追踪系统 | API 成本计算 | 高 | ✅ 已实现 |
| 计划模式 | PlanMode 工具 | 高 | ✅ 已实现 |
| 数据迁移系统 | 配置迁移 | 中 | ❌ 未实现 |
| 远程代理支持 | WebSocket 连接 | 低 | ❌ 未实现 |
| Bridge 模式 | 桥接模式 | 低 | ❌ 未实现 |
| 协调器模式 | 协调器系统 | 低 | ❌ 未实现 |
| 工作树模式 | Git 工作树 | 中 | ❌ 未实现 |
| 遥测系统 | 使用遥测 | 低 | ❌ 未实现 |
| 功能标志 | GrowthBook 集成 | 低 | ❌ 未实现 |
| 插件系统完整实现 | 插件加载/管理 | 低 | ❌ 未实现 |

---

## 25. 工具覆盖率统计

### aemeath vs extracted_sources 工具对比

| 类别 | aemeath 已实现 | extracted_sources 总数 | 覆盖率 |
|------|----------------|------------------------|--------|
| 文件操作 | 5 个 | 6 个 | 83% |
| Shell 执行 | 1 个 | 3 个 | 33% |
| 代理/任务 | 7 个 | 8 个 | 87.5% |
| Web 网络 | 2 个 | 3 个 | 67% |
| MCP 工具 | 3 个 | 4 个 | 75% |
| 开发工具 | 2 个 | 2 个 | 100% |
| 用户交互 | 1 个 | 2 个 | 50% |
| 模式/工作流 | 2 个 | 4 个 | 50% |
| 配置管理 | 2 个 | 3 个 | 67% |
| 技能/调度 | 2 个 | 4 个 | 50% |
| 其他 | 1 个 | 8 个 | 13% ✨ |
| **总计** | **28 个** | **43 个** | **65%** |

---

## 26. 命令覆盖率统计

aemeath 已实现 22 个核心命令，extracted_sources 有 101+ 命令。

**命令覆盖率: 22% (22/101)**

**已实现的命令分类：**

| 类别 | 数量 | 命令 |
|------|------|------|
| Core | 4 | help, exit, clear, compact |
| Session | 3 | resume, session, rewind |
| Config | 3 | config, model, permissions |
| Tasks | 1 | tasks |
| Tools | 2 | mcp, skills |
| Git | 3 | init, commit, review ✨ |
| Utility | 5 | cost, usage, status, version, stats ✨ |
| Debug | 1 | doctor |
| **总计** | **22** | |

**高价值命令实现状态：**
1. ✅ /init - 项目初始化
2. ✅ /compact - 消息压缩
3. ✅ /cost - 成本统计
4. ✅ /resume - 会话恢复
5. ✅ /rewind - 回退历史
6. ✅ /commit - Git 提交
7. ✅ /usage - 使用统计
8. ✅ /tasks - 任务管理
9. ✅ /help - 帮助信息
10. ✅ /exit - 退出程序
11. ✅ /review - 代码审查 ✨
12. ✅ /stats - 统计信息 ✨

| 指标 | aemeath | extracted_sources |
|------|---------|-------------------|
| 启动时间 | < 100ms | ~1-2 秒 |
| 内存占用 | 10-50 MB | 200-500 MB |
| 二进制大小 | 10-20 MB | 100+ MB |
| 响应速度 | 极快 | 较快 |
| 资源效率 | 高 | 中 |

---

## 23. 技术债务和改进建议

### aemeath 当前优势
1. ✅ 编译时类型安全 (Rust)
2. ✅ 低资源占用 (~10-50 MB)
3. ✅ 快速启动 (< 100ms)
4. ✅ 完整的 TUI 界面 (ratatui)
5. ✅ 26 个核心工具已实现 (60% 覆盖率)
6. ✅ 基础 MCP 支持
7. ✅ 状态管理系统
8. ✅ 权限系统基础
9. ✅ Rust 性能优势
10. ✅ **命令系统 (20 个命令)** ✨
11. ✅ **持久化存储 (会话/配置/历史/成本)** ✨
12. ✅ **成本追踪系统 (CostTracker)** ✨
13. ✅ **计划模式 (PlanMode)** ✨

### aemeath 当前劣势
1. ❌ 缺少测试
2. ❌ 缺少文档
3. ❌ 单一 LLM 提供商 (仅 Anthropic)
4. ❌ 缺少高级代理类型 (远程代理、梦想任务)
5. ❌ 缺少插件系统
6. ❌ 缺少协作功能 (团队管理、消息发送)

### 建议的改进方向
1. **持久化**: 添加磁盘持久化、会话保存、状态快照
2. **命令系统**: 实现核心命令 (/init, /compact, /cost, /resume 等)
3. **多提供商**: 支持 AWS Bedrock、Google Cloud
4. **代理增强**: 远程代理、代理内存、代理分叉
5. **测试**: 添加单元测试和集成测试
6. **文档**: 添加用户文档和 API 文档
7. **插件**: 实现插件加载和管理系统

---

## 结论

aemeath 已经实现了约 65% 的核心功能，是一个具有完整 TUI 界面、28 个工具、基础 MCP 支持和状态管理的中等规模 MVP 实现。主要已完成的功能：

1. **工具系统**: 28 个工具已实现 (65% 覆盖率)
   - 文件操作、Shell 执行、任务管理、Web 工具、MCP 工具、计划模式等
   - BriefTool 增强版：支持历史记录集成、多种格式输出 ✨
   - TaskOutputTool：任务输出和结果管理 ✨
2. **命令系统**: 22 个核心命令已实现 (22% 覆盖率)
   - 包括 /help, /exit, /clear, /compact, /cost, /config, /resume, /rewind 等
3. **持久化存储**: 完整的持久化支持
   - 会话持久化、配置持久化、历史记录持久化、成本追踪持久化
4. **成本追踪系统**: 完整实现
   - CostTracker API 成本管理、多模型价格配置、会话成本统计

主要缺失的功能：

1. **多 LLM 提供商**: 仅支持 Anthropic，缺少 AWS Bedrock 和 Google Cloud
2. **高级代理类型**: 缺少远程代理、梦想任务等
3. **协作功能**: 缺少团队管理、消息发送等
4. **UI 增强**: 缺少 Vim 模式、语音输入、多屏幕界面等

选择哪个项目取决于需求：
- **快速开发**: aemeath 已具备核心功能，适合快速迭代
- **生产使用**: extracted_sources 功能完整，适合企业级应用
- **资源受限环境**: aemeath 更适合 (低内存 ~10-50MB、快速启动 <100ms)
- **完整功能需求**: extracted_sources 更适合

建议优先实现多 LLM 提供商支持和高级代理类型，这将大幅提升用户体验。同时保持 aemeath 的性能优势和简洁架构。