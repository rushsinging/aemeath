# Feature 47 P15: runtime 内部分层——核心流程 / 业务规则 / 工具

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** runtime 内部按职责重新组织为三层：`application`（核心流程——聊天怎么推进）、`domain`（业务规则——什么情况下该干什么）、`infrastructure`（工具——怎么发 HTTP、读文件、跑 shell）。消除 18 个扁平模块堆叠。

## 三层含义（直观版）

| 层 | 别称 | 只管什么 | 不管什么 |
|---|------|---------|---------|
| **application/** | 核心流程 / 指挥官 | 编排流程：先 A 后 B 再 C | 不执行业务规则细节 |
| **domain/** | 业务规则 / 规则专家 | 业务判断：该不该压缩？权限够不够？ | 不知道文件系统、HTTP、模型 API |
| **infrastructure/** | 工具 / 跑腿 | 干脏活：发 HTTP、读文件、跑 shell | 不替 domain 做业务决策 |

**一句话**：application 管"做什么"、domain 管"怎么判断"、infrastructure 管"怎么实现"。

## 为什么必须先做分层

**不分层做不了 P16**——P16 要 domain 层通过 port trait 隔离 supporting domain（provider/tools/storage/hook），但不分层就不知道哪些代码算 domain，也就没法判断哪些 `provider::` 引用合法、哪些该被 port trait 替代。

## 当前状态

`agent/runtime/src/` 有 18 个顶层扁平模块，混合了核心流程、业务规则和工具：

| 职责 | 应属于 | 实际位置 |
|------|--------|---------|
| 核心流程 | application | `chat/service.rs`、`chat/port.rs`、`command/`、`client/` |
| Agent Looping 业务 | domain | `agent.rs`、`agent_tests.rs`、`agent_runner/`、`agent_runner.rs`、`chat/looping/` |
| Session 业务 | domain | `session/`、`state/`、`scheduler/` |
| Compact/Cost 业务 | domain | `compact/`、`cost/` |
| Prompt 业务 | domain | `prompt_build/`、`prompt_build_ext.rs`、`skill_command.rs`、`skill_command_impl.rs` |
| Reflection 业务 | domain | `reflection/` |
| 图片处理 | 工具 | `image/` |
| 启动装配 | 工具 | `bootstrap/` |

## 目标结构

```
agent/runtime/src/
├── lib.rs                    ← pub mod application; pub mod domain; pub mod infrastructure; pub mod api;
├── api.rs                    ← 向后兼容 re-export（对外不变）
│
├── application/              ← 核心流程（指挥官）
│   ├── mod.rs
│   ├── service.rs            ← Chat 启动/停止/恢复编排  ← 原 chat/service.rs
│   ├── port.rs               ← Chat 端口定义            ← 原 chat/port.rs
│   ├── client/               ← AgentClient 入口 + SDK 投影 ← 原 client/（P14 拆分后）
│   └── command/              ← slash 命令分发            ← 原 command/
│
├── domain/                   ← 业务规则（规则专家）
│   ├── mod.rs
│   ├── agent/                ← Agent 定义 + Agent Looping
│   │   ├── mod.rs            ← 原 agent.rs
│   │   ├── tests.rs          ← 原 agent_tests.rs（改名）
│   │   └── runner/           ← 原 agent_runner/ + agent_runner.rs
│   ├── chat/                 ← Chat 聚合根
│   │   └── 原 chat/ 去掉 service.rs、port.rs
│   ├── session/              ← Session 业务         ← 原 session/
│   ├── state/                ← 运行时状态            ← 原 state/
│   ├── compact/              ← 消息压缩触发 + 算法    ← 原 compact/
│   ├── cost/                 ← 成本追踪业务规则      ← 原 cost/
│   ├── reflection/           ← 反思业务              ← 原 reflection/
│   ├── prompt/               ← Prompt 组装规则
│   │   ├── mod.rs            ← 收口原 prompt_build_ext.rs + skill_command.rs + skill_command_impl.rs
│   │   └── build/            ← 原 prompt_build/
│   └── scheduler/            ← 任务调度状态机         ← 原 scheduler/
│
└── infrastructure/           ← 工具（跑腿）
    ├── mod.rs
    ├── bootstrap/            ← 启动装配（config→provider→tools→hooks） ← 原 bootstrap/
    └── image/                ← 图片文件读写 + 格式处理                  ← 原 image/
```

## 核心路径替换表

所有 `crate::` 引用需要全局替换：

| 旧路径 | 新路径 |
|--------|--------|
| `crate::agent` | `crate::domain::agent` |
| `crate::agent_runner` | `crate::domain::agent::runner` |
| `crate::chat` | `crate::domain::chat` |
| `crate::chat::looping` | `crate::domain::chat::looping` |
| `crate::chat::reflection` | `crate::domain::chat::reflection` |
| `crate::chat::request` | `crate::domain::chat::request` |
| `crate::chat::service` | `crate::application::service` |
| `crate::chat::port` | `crate::application::port` |
| `crate::session` | `crate::domain::session` |
| `crate::state` | `crate::domain::state` |
| `crate::compact` | `crate::domain::compact` |
| `crate::cost` | `crate::domain::cost` |
| `crate::reflection` | `crate::domain::reflection` |
| `crate::prompt_build` | `crate::domain::prompt::build` |
| `crate::prompt_build_ext` | `crate::domain::prompt` |
| `crate::skill_command` | `crate::domain::prompt` |
| `crate::skill_command_impl` | `crate::domain::prompt` |
| `crate::scheduler` | `crate::domain::scheduler` |
| `crate::client` | `crate::application::client` |
| `crate::command` | `crate::application::command` |
| `crate::bootstrap` | `crate::infrastructure::bootstrap` |
| `crate::image` | `crate::infrastructure::image` |
| `crate::tui_launch` | （删除，P13 已移除消费者） |

## 执行步骤

- [ ] **1. 创建三层目录结构**
  - `mkdir -p agent/runtime/src/{application,domain/agent/runner,domain/chat,domain/prompt/build,infrastructure}`

- [ ] **2. 迁移 domain 层文件（git mv）**
  - `agent.rs` + `agent_tests.rs` → `domain/agent/`
  - `agent_runner.rs` + `agent_runner/` → `domain/agent/runner/`
  - `chat/` 整体（含 looping/、reflection.rs、request.rs、mod.rs） → `domain/chat/`
  - `session/`、`state/`、`compact/`、`cost/`、`reflection/`、`scheduler/` → `domain/`
  - `prompt_build/` → `domain/prompt/build/`
  - `prompt_build_ext.rs`、`skill_command.rs`、`skill_command_impl.rs` → `domain/prompt/`

- [ ] **3. 迁移 application 层文件（git mv）**
  - `chat/service.rs` + `chat/port.rs` → `application/`
  - `client/` → `application/client/`
  - `command/` → `application/command/`

- [ ] **4. 迁移 infrastructure 层文件（git mv）**
  - `bootstrap/` → `infrastructure/bootstrap/`
  - `image/` → `infrastructure/image/`

- [ ] **5. 全局替换所有 `crate::` 引用路径**
  - 按照路径替换表逐项替换
  - domain 内部互相引用同步更新
  - 每批替换后 `cargo check` 验证

- [ ] **6. 更新 `lib.rs`**
  - 改为只声明三层 + api

- [ ] **7. 更新 `api.rs` 保持向后兼容**
  - 从三层路径 re-export，使 `runtime::api::*` 对外不变

- [ ] **8. 补各层 `mod.rs`**
  - `application/mod.rs`、`domain/mod.rs`、`domain/agent/mod.rs`、`domain/prompt/mod.rs`、`infrastructure/mod.rs`

- [ ] **9. 删除废弃文件**
  - `tui_launch.rs`（P13 已移除消费者）
  - 空 `chat/` 顶层目录

- [ ] **10. 验证**
  - `cargo build` 编译通过
  - `cargo test` 全部通过
  - `cargo clippy` 无新增 warning
  - 每个文件 ≤ 400 行

## 风险点

| 风险 | 应对 |
|------|------|
| `agent_runner/` 内部 `crate::agent_runner::*` 断裂 | 先 grep 统计所有引用再统一替换 |
| `chat/looping/` 对 `crate::chat::service` 引用断裂 | 替换为 `crate::application::service` |
| `prompt_build/` 内互相引用断裂 | 路径调整为 `crate::domain::prompt::build::*` |
| domain 之间互相引用路径不一致 | 全部先用 `grep` 审计，再按表替换 |
| 大型 diff 遗漏个别文件 | 每批替换后立即 `cargo check`，不等到最后 |
