# Role Policy（角色策略绑定）

> 层级：02-modules / tools（模块战术设计）
> 状态：Target（目标设计）｜Milestone：待排期
> 跨 BC 影响面：tools（ToolProfile / RegistryScope）、policy（评估维度）、runtime（RunSpec 装配）、config（schema）。核心机制归 Tool BC 拥有，本文档为单一事实源。

## 1. 背景与目标

`agents.roles` 目前只绑定模型（model / description / system_suffix / max_tokens）。本设计为 role 增加 **policy 维度**：不同 role（planner / coder / searcher / tester / reviewer…）拥有不同工具能力集，最基本的能力是**限制 tool**。

目标：

1. role → 工具集（名单 + capability 位）可配置、可内置、可覆盖。
2. **role policy 取代注册表 `[main, sub]` 静态布尔**，成为 sub run 工具裁剪的唯一机制。
3. main agent 也可绑定 role（`agents.main_role`），实现"规划态 main、执行走 sub"的硬裁剪。
4. 全链路防提权：子工具集 ⊆ 父工具集，越权即 `CapabilityEscalation`。

非目标（本期不做）：bash 命令白名单断言、可写路径 glob、sub run 向用户提问的交互代理、session 中途 `/role` 切换。

## 2. 核心决策

| # | 决策 | 理由 |
|---|---|---|
| D1 | **role 是任意 run 的策略绑定**，不是 sub 专属概念 | main 绑 role 即可实现主循环裁剪，与 EnterPlanMode（软性交互审批）互补 |
| D2 | **role policy 取代 `[main, sub]` 静态布尔** | 静态归属无法表达"planner-as-sub 可派发、coder-as-sub 不可"；裁剪点唯一化 |
| D3 | **白名单语义，未配置 = 继承现状** | 向后兼容：不写 policy 的 role 与既有 Sub scope 行为逐字节等价 |
| D4 | **内置 5 role，config 同名整条覆盖**（不做字段级合并） | 开箱即用 + 用户完全控制权；避免半内置半自定义的组合不可预期 |
| D5 | **有效工具集 = 注册池 ∩ role 名单 ∩ role capability 位 ⊆ 父 ceiling** | 名单与 capability 位正交且都只收缩，复用 `ToolProfile::derive_restricted` 防提权 |
| D6 | **可见性裁剪 + Policy 兜底双层执行** | LLM 看不到不可用工具（省 token、防误调用）；Policy 在执行前 evaluate 兜底 deny（防绕过） |

## 3. 配置 schema（Config BC）

```json
{
  "agents": {
    "main_role": "planner",
    "roles": {
      "searcher": {
        "model": "deepseek/deepseek-chat",
        "description": "Read-only code & web exploration",
        "policy": {
          "allowed_tools": ["Read", "Grep", "Glob", "WebSearch", "WebFetch", "ToolSearch"],
          "capabilities": ["ReadWorkspace", "NetworkAccess"]
        }
      }
    }
  }
}
```

`RolePolicyConfig` 字段（全部 `#[serde(default)]`，空 policy 编译为 `None` 走现状路径）：

| 字段 | 语义 |
|---|---|
| `allowed_tools: Vec<String>` | 工具名白名单；未列出者不可见且调用必拒 |
| `denied_tools: Vec<String>` | 黑名单；与白名单同给时 deny 优先（白名单 − 黑名单） |
| `capabilities: Vec<String>` | capability 位收缩；与名单取交集（名单有 `Bash` 但无 `ExecuteProcess` 仍拦） |

`agents.main_role: String`（默认空 = 隐式全量 main role，行为等于今天）。

## 4. 内置 role（fallback，config 同名整条覆盖）

| Role | allowed_tools | 定位 |
|---|---|---|
| **planner** | Read, Grep, Glob, WebSearch, WebFetch, TaskGet, TaskListGet, TaskLists, ToolSearch | 规划与拆解：只读 + 联网 + Task 读；禁写、禁执行、禁派发 |
| **coder** | Read, Write, Edit, Glob, Grep, Bash, ToolSearch, Skill | 执行者：读写执行；禁 AgentDispatch（防递归派发） |
| **searcher** | Read, Grep, Glob, WebSearch, WebFetch, ToolSearch | 检索：本地 + 联网，最瘦 |
| **tester** | Read, Write, Edit, Bash, Grep, Glob, ToolSearch | 测试编写与运行 |
| **reviewer** | Read, Grep, Glob, WebSearch, ToolSearch | 只读审查 |

内置 role 的 capability 位由 allowed_tools 对应工具的 required capabilities 推导，不单独声明。用户要给某 role 加 `Agent` / `TaskCreate` / `AskUserQuestion` 等，config 覆盖即可——机制支持一切名单，内置默认从简。

## 5. 分层装配与执行

```text
config.json
  └─ ConfigSnapshot（新增 main_role + RolePolicyConfig 解析/校验：工具名拼写、capability 拼写）
       └─ runtime resolve_derived_role（现有入口扩展）
            └─ RolePolicy 编译：名单 ∩ capability → ToolFilter
                 ├─ RunSpec：携带 role 绑定与 ToolFilter，ceiling 校验子 ⊆ 父
                 │    ToolProfile 扩展 allowed_tool_names: Option<BTreeSet<ToolName>>
                 │    （None = 不过滤名单，兼容现状）
                 ├─ 可见性裁剪：发给 LLM 的 tools schema 列表按 ToolFilter 过滤
                 └─ Policy 兜底：执行前 evaluate，未过 ToolFilter 的调用
                      Deny(PolicyReason::RestrictedTool)（复用现有 reason）
```

关键约束：

1. **只收缩**：`ToolFilter` 只能从注册池与父 ceiling 里做减法，任何扩展在 `derive_restricted` 处报 `CapabilityEscalation`。
2. **单一裁剪点**：注册表 `builtin!` 的 `[main, sub]` 布尔退役，改为统一注册池 + role ToolFilter。迁移期未配置 policy 且非内置 role 名的 sub run，使用现有 Sub scope 名单作为隐式 default ToolFilter（等价迁移，行为零变化）。
3. **AskUserQuestion 仅 main 形态可用**：sub run 是 NonInteractive + ParentMediated，交互代理链路（问题冒泡→main 转述→答案回灌）不存在；内置 role 均不含 Ask，config 给了也在 sub 形态被 capability 校验拦下，main 绑定形态正常生效。
4. **main_role 生效时机**：仅启动时绑定（config / CLI flag）；session 中途切换涉及运行中 RunSpec 变更，后续独立立项。

## 6. 与现有机制的关系

| 现有机制 | 关系 |
|---|---|
| `ToolScope::Full/Restricted` | 语义上移至 ToolFilter；RunSpec 保留 ToolScope 作为档位标记，ToolFilter 携带精确集合 |
| `RegistryScope`（Main/Sub） | 静态归属退役，统一注册池；scope 概念保留用于装配单元测试分组 |
| `ToolProfile`（capability allow-set） | 扩展 `allowed_tool_names` 维度，`derive_restricted` 同步校验名单不扩张 |
| `PolicyPort` / `PolicyReason::RestrictedTool` | 直接复用，evaluate 增加按 run 的 ToolFilter 判定 |
| `EnterPlanMode/ExitPlanMode` | 互补：plan mode 是交互式软审批，role 绑定是硬性能力裁剪，二者可叠加 |
| `tools.enabled/disabled`（全局） | 保留为全局粗粒度开关，先于 role ToolFilter 生效（全局禁用 > role 名单） |

## 7. 测试策略（跨层每层覆盖）

| 层 | 测试 |
|---|---|
| config | RolePolicyConfig 解析（空 policy = None、deny 优先、非法工具名/capability 报错）、main_role 解析 |
| tools PL | ToolFilter 编译（名单∩capability）、derive_restricted 名单扩张报 CapabilityEscalation、is_authorized 名单维度 |
| runtime | resolve_derived_role 装配 ToolFilter、main_role 绑定、内置 role fallback、config 覆盖整条替换、等价迁移（无 policy sub = 现状名单） |
| 可见性 | LLM schema 列表按 filter 裁剪；被裁工具调用产生 Deny 而非 not found |
| policy | evaluate 按 ToolFilter deny，reason 为 RestrictedTool |
| TUI | sub run 被拒工具调用的展示（复用现有 deny 渲染，不新增状态） |

## 8. 分期

| 期 | 内容 |
|---|---|
| P1 | RolePolicyConfig + 内置 5 role + ToolFilter 编译 + sub run 名单裁剪与 Policy 兜底（等价迁移保证） |
| P2 | `agents.main_role` / CLI `--role` 启动绑定 + main 可见性裁剪 |
| P3（后续独立立项） | bash 命令白名单（复用 is_readonly_command）、可写路径 glob、sub 交互代理（AskUserQuestion 冒泡）、session 中途 `/role` 切换 |
