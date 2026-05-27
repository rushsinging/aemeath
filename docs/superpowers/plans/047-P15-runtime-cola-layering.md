# Feature 47 P15: runtime 内部 COLA 分层——application / domain / infrastructure 三层

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** runtime 内部按 COLA 分层重新组织：`application`（应用服务 + 编排）、`domain`（核心领域模型 + 领域服务）、`infrastructure`（适配器 + 引导）。消除扁平模块堆叠。

## 当前状态

`agent/runtime/src/` 有 16 个顶层扁平模块，混合了应用编排、领域逻辑和基础设施关注点：

| 层级 | 应属于 | 实际位置 |
|------|--------|---------|
| 应用编排 | application | `chat/service.rs`、`command/`、`client.rs`、`bootstrap/` |
| Agent Looping | domain | `agent.rs`、`agent_runner/`、`chat/looping/` |
| Session 领域模型 | domain | `session/`、`state/`、`scheduler/` |
| Compact/Cost 领域服务 | domain | `compact/`、`cost/` |
| Prompt 构建 | domain | `prompt_build/`、`prompt_build_ext.rs`、`skill_command.rs` |
| Reflection 领域服务 | domain | `reflection/` |
| Image 处理 | infrastructure | `image/` |
| Bootstrap/配置装配 | infrastructure | `bootstrap/` |

## 目标结构

```
agent/runtime/src/
├── lib.rs                    ← pub mod application; pub mod domain; pub mod infrastructure;
├── application/
│   ├── mod.rs                ← ChatApplicationService, AgentClient impl, from_args 编排
│   ├── client/               ← P14 拆分后的 client 子模块
│   └── command/              ← slash 命令分发
├── domain/
│   ├── mod.rs
│   ├── agent/                ← Agent 模型 + Agent Looping（原 agent.rs + agent_runner/）
│   ├── chat/                 ← Chat 聚合根（原 chat/ 去掉 service.rs）
│   ├── session/              ← Session 聚合根
│   ├── compact/              ← 消息压缩领域服务
│   ├── cost/                 ← 成本追踪领域服务
│   ├── reflection/           ← 反思领域服务
│   ├── prompt/               ← Prompt 构建（原 prompt_build/）
│   ├── scheduler/            ← 任务调度
│   └── state/                ← 运行时状态
└── infrastructure/
    ├── mod.rs
    ├── bootstrap/            ← 启动装配（原 bootstrap/）
    └── image/                ← 图片处理适配器
```

## 步骤

- [ ] **1. 创建三层目录结构**
  - `mkdir -p agent/runtime/src/{application,domain,infrastructure}`

- [ ] **2. 迁移 domain 层**
  - `agent.rs` + `agent_runner/` → `domain/agent/`
  - `chat/`（去掉 `service.rs`、`port.rs`）→ `domain/chat/`
  - `session/` → `domain/session/`
  - `state/` → `domain/state/`
  - `compact/` → `domain/compact/`
  - `cost/` → `domain/cost/`
  - `reflection/` → `domain/reflection/`
  - `prompt_build/` + `prompt_build_ext.rs` + `skill_command.rs` + `skill_command_impl.rs` → `domain/prompt/`
  - `scheduler/` → `domain/scheduler/`

- [ ] **3. 迁移 application 层**
  - `chat/service.rs` + `chat/port.rs` → `application/`
  - `client/`（P14 拆分后）→ `application/client/`
  - `command/` → `application/command/`

- [ ] **4. 迁移 infrastructure 层**
  - `bootstrap/` → `infrastructure/bootstrap/`
  - `image/` → `infrastructure/image/`

- [ ] **5. 更新所有 `use crate::` 路径**
  - `crate::agent` → `crate::domain::agent`
  - `crate::chat::looping` → `crate::domain::chat::looping`
  - 等等
  - 这是最机械但最关键的一步

- [ ] **6. 更新 `lib.rs`**
  - 改为 `pub mod application; pub mod domain; pub mod infrastructure;`
  - `api.rs` 改为从三层 re-export 保持向后兼容

- [ ] **7. 更新 `api.rs` re-export**
  - 保持 `runtime::api::*` 对外不变（composition root 仍可用）
  - 但内部路径指向三层

- [ ] **8. 验证**
  - `cargo build` 编译通过
  - `cargo test` 全部通过
  - 每个文件 ≤ 400 行
  - `api.rs` re-export 无断裂
