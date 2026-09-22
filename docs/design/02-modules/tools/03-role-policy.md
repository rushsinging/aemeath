# Role Policy（角色策略绑定）

> 层级：02-modules / tools（模块战术设计）
> 状态：Target（目标设计）｜Milestone：待排期
> 跨 BC 影响面：tools（ToolProfile / RegistryScope）、policy（评估维度）、runtime（RunSpec 装配）、config（schema）。核心机制归 Tool BC 拥有，本文档为单一事实源。

## 1. 背景与目标

`agents.roles` 目前只绑定模型（model / description / system_suffix / max_tokens）。本设计为 role 增加 **policy 维度**：不同 role（planner / coder / searcher / tester / reviewer…）拥有不同工具能力集，最基本的能力是**限制 tool**。

目标：

1. role → 工具集（名单 + capability 位）可配置、可内置、可覆盖。
2. **role policy 取代注册表 `[main, sub]` 静态布尔**，成为 sub run 工具裁剪的唯一机制。
3. 全链路防提权：子工具集 ⊆ 父工具集，越权即 `CapabilityEscalation`。

role 只作用于 sub run；main agent 不做 role 裁剪（规划态 main 由既有 EnterPlanMode 承担）。bash 命令白名单断言、可写路径 glob、sub run 向用户提问的交互代理均不在本期范围。

## 2. 核心决策

| # | 决策 | 理由 |
|---|---|---|
| D1 | **role 是 sub run 的策略绑定**；main agent 不绑 role | main 裁剪由 EnterPlanMode（交互式软审批）承担，避免双机制重叠 |
| D2 | **role policy 取代 `[main, sub]` 静态布尔** | 静态归属无法表达"planner-as-sub 可派发、coder-as-sub 不可"；裁剪点唯一化 |
| D3 | **白名单语义，未配置 = 继承现状** | 向后兼容：不写 policy 的 role 与既有 Sub scope 行为逐字节等价 |
| D4 | **内置 5 role，config 同名整条覆盖**（不做字段级合并） | 开箱即用 + 用户完全控制权；避免半内置半自定义的组合不可预期 |
| D5 | **有效工具集 = 注册池 ∩ role 名单 ∩ role capability 位 ⊆ 父 ceiling** | 名单与 capability 位正交且都只收缩，复用 `ToolProfile::derive_restricted` 防提权 |
| D6 | **可见性裁剪即硬边界** | LLM 看不到不可用工具（省 token、防误调用）；catalog 外调用走现有 deny 路径（`prepare_tool_round` 的 catalog-miss deny），Policy 层零改动 |

## 3. 配置 schema（Config BC）

`agents` 下两个正交集合：**`roles`（职能定义）**与 **`names`（具名 agent 实例）**。职能持有策略，实例持有模型——`reviewer-glm` / `reviewer-ds` 是同一职能 `reviewer` 的两个模型实例，共享一份 policy。

```json
{
  "agents": {
    "roles": {
      "reviewer": {
        "description": "Reviews code quality",
        "policy": { "allowed_tools": ["Read", "Grep", "Glob", "WebSearch", "ToolSearch"] }
      }
    },
    "names": {
      "reviewer-glm": { "role": "reviewer", "model": "Zhipu/glm-5.2", "enabled": true,
                         "system_suffix": "Focus on correctness.", "max_tokens": 16384 },
      "reviewer-ds":  { "role": "reviewer", "model": "DeepSeek/deepseek-v4-pro", "enabled": true }
    },
    "default_model": "Zhipu/glm-5.3"
  }
}
```

**职能定义（`roles`，`AgentRoleDefinition`）**：

| 字段 | 语义 |
|---|---|
| `description: String` | 职能说明（实例未带 description 时的 fallback） |
| `policy: Option<RolePolicyConfig>` | `allowed_tools` 白名单 + `capabilities` 收缩（见下表） |

**具名实例（`names`，`AgentInstanceConfig`）**：

| 字段 | 语义 |
|---|---|
| `role: String` | 引用的职能名，MUST 存在于内置或 config `roles`，否则配置校验错 |
| `model: String` | 模型，空时回退 `agents.default_model` |
| `enabled: bool` | 默认 true；false 时派发报 disabled |
| `description / system_suffix / max_tokens` | 实例级提示与预算（沿用旧字段语义） |

`RolePolicyConfig` 字段（全部 `#[serde(default)]`，空 policy = `None` 走现状路径）：

| 字段 | 语义 |
|---|---|
| `allowed_tools: Vec<String>` | 白名单：只允许列出者，其余不可见且调用必拒 |
| `capabilities: Vec<String>` | capability 位收缩；与名单取交集（名单有 `Bash` 但无 `ExecuteProcess` 仍拦） |

只保留白名单，不设黑名单：白名单是显式、可审计的能力声明，黑名单（全量 − 排除项）会随内置工具集增长而隐式扩权。未写 `allowed_tools` 时仅由 `capabilities` 收缩（两者都未写则 `None` 走现状路径）。

### 3.1 派发与自定义

- **派发传 agent 名**：`Agent` 工具参数为 `agent`（值为 `names` 的 key），runtime `resolve_agent` 解析出实例（model/enabled/suffix/max_tokens）与职能（policy），装配 `role:<职能名>` profile。未知名或 disabled 实例报错。
- **自定义职能**：`roles` 加任意 key（无保留字），与内置职能同名时整条覆盖内置定义。
- **自定义实例**：`names` 加任意 key 引用任意职能；多实例共享同一份 policy 与 profile。
- **Breaking**：旧扁平格式（`agents.roles.<name>` 直接带 `model`/`enabled` 等实例字段）被 `AgentRoleDefinition` 的 `deny_unknown_fields` 直接拒绝，报错附迁移指引；不做读时兼容迁移（自有 schema、pre-release）。

## 4. 内置 role（fallback，config 同名整条覆盖）

| Role | allowed_tools | 定位 |
|---|---|---|
| **planner** | Read, Grep, Glob, WebSearch, WebFetch, TaskGet, TaskListGet, TaskLists, ToolSearch | 规划与拆解：只读 + 联网 + Task 读；禁写、禁执行、禁派发 |
| **coder** | Read, Write, Edit, Glob, Grep, Bash, ToolSearch, Skill | 执行者：读写执行；禁 AgentDispatch（防递归派发） |
| **searcher** | Read, Grep, Glob, WebSearch, WebFetch, ToolSearch | 检索：本地 + 联网，最瘦 |
| **tester** | Read, Write, Edit, Bash, Grep, Glob, ToolSearch | 测试编写与运行 |
| **reviewer** | Read, Grep, Glob, WebSearch, ToolSearch | 只读审查 |

内置 role 是**职能定义**（policy + description），不含 model；内置职能没有隐式实例——派发必须命中 `names` 中的具名实例（model 取实例值或回退 `default_model`）。内置 capability 位由 allowed_tools 对应工具的 required capabilities 推导。用户要给某职能加 `Agent` / `TaskCreate` / `AskUserQuestion` 等，config 覆盖职能即可——机制支持一切名单，内置默认从简。

## 5. 分层装配与执行

```text
config.json
  └─ ConfigSnapshot（RolePolicyConfig 解析/校验：工具名拼写、capability 拼写）
       └─ runtime resolve_derived_role（现有入口扩展）
            └─ RolePolicy 编译：名单 ∩ capability → ToolFilter
                 ├─ RunSpec：携带 role 绑定与 ToolFilter，ceiling 校验子 ⊆ 父
                 │    ToolProfile 扩展 allowed_tool_names: Option<BTreeSet<ToolName>>
                 │    （None = 不过滤名单，兼容现状）
                 ├─ 可见性裁剪：发给 LLM 的 tools schema 列表按 ToolFilter 过滤
                 └─ 硬边界：被裁工具的调用（幻觉/绕过）走现有 catalog-miss deny
                    （prepare_tool_round："Tool is not present in the catalog"）
```

关键约束：

1. **只收缩**：`ToolFilter` 只能从注册池与父 ceiling 里做减法，任何扩展在 `derive_restricted` 处报 `CapabilityEscalation`。
2. **单一裁剪点**：注册表 `builtin!` 的 `[main, sub]` 布尔退役，改为统一注册池 + role ToolFilter。迁移期未配置 policy 且非内置 role 名的 sub run，使用现有 Sub scope 名单作为隐式 default ToolFilter（等价迁移，行为零变化）。
3. **AskUserQuestion 不进内置名单**：内置 role 均不含 Ask。sub run 是 NonInteractive + ParentMediated，交互代理链路（问题冒泡→main 转述→答案回灌）不存在；config 显式把 Ask 放进自定义 role 名单时工具可见可调用，但挂起后无法在 sub 环境完成恢复（等待至 timeout）——交互代理是 P2 独立特性，落地前不建议任何 role 名单包含 Ask。
4. **绑定时机固定**：role 在 sub run 创建（`Agent` 工具调用）时解析并冻结进 RunSpec，run 存续期内不变更。

## 6. 与现有机制的关系

| 现有机制 | 关系 |
|---|---|
| `ToolScope::Full/Restricted` | 语义上移至 ToolFilter；RunSpec 保留 ToolScope 作为档位标记，ToolFilter 携带精确集合 |
| `RegistryScope`（Main/Sub） | 静态归属退役，统一注册池；scope 概念保留用于装配单元测试分组 |
| `ToolProfile`（capability allow-set） | 扩展 `allowed_tool_names` 维度，`derive_restricted` 同步校验名单不扩张 |
| `PolicyPort` / `PolicyReason::RestrictedTool` | 直接复用，evaluate 增加按 run 的 ToolFilter 判定 |
| `EnterPlanMode/ExitPlanMode` | 分工：main 的规划态由 plan mode（交互式软审批）承担，role 只裁剪 sub run，二者不重叠 |
| `tools.enabled/disabled`（全局） | 保留为全局粗粒度开关，先于 role ToolFilter 生效（全局禁用 > role 名单） |

## 7. 测试策略（跨层每层覆盖）

| 层 | 测试 |
|---|---|
| config | RolePolicyConfig 解析（空 policy = None、非法工具名/capability 报错）、自定义 role 名任意性、同名覆盖内置 |
| tools PL | ToolFilter 编译（名单∩capability）、derive_restricted 名单扩张报 CapabilityEscalation、is_authorized 名单维度 |
| runtime | resolve_derived_role 装配 ToolFilter、内置 role fallback、config 覆盖整条替换、等价迁移（无 policy sub = 现状名单） |
| 可见性 | LLM schema 列表按 filter 裁剪；被裁工具调用产生 Deny 而非 not found |
| policy | evaluate 按 ToolFilter deny，reason 为 RestrictedTool（本期零改动；catalog-miss deny 已覆盖被裁工具调用） |
| TUI | sub run 被拒工具调用的展示（复用现有 deny 渲染，不新增状态） |

## 8. 分期

| 期 | 内容 |
|---|---|
| P1 | RolePolicyConfig + 内置 5 role + ToolFilter 编译 + sub run 名单裁剪（等价迁移保证；catalog-miss deny 天然兜底） |
| P2（后续独立立项） | bash 命令白名单（复用 is_readonly_command）、可写路径 glob、sub 交互代理（AskUserQuestion 冒泡） |
