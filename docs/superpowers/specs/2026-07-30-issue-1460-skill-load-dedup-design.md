# 按 Agent 作用域持久化 Skill revision 去重设计

> 对应 Issue：[#1460](https://github.com/rushsinging/aemeath/issues/1460)  
> Milestone：v0.1.0 — Context Engineering + 架构重构  
> 状态：已批准；实现必须遵循 TDD 与逐层验证

## 1. 问题

当前唯一动态 `Skill` Tool 每次调用都会通过 `SkillLoadPort` 重新读取正文并把完整内容返回模型。即使同一 Main Agent 已在当前 Session 加载过该 Skill，下一轮新 Run 再次调用仍会得到完整正文。现有内容 revision 只作为 Tool 结果 metadata 返回，没有成为可比较、可恢复的 Session 事实。

直接按 `run_id` 缓存不能解决问题：一个 Session 包含多个短生命周期 Main Run，下一轮对话会产生新 `run_id`。进程内缓存也无法跨 Resume，并可能成为 `CanonicalSession` 之外的第二状态源。

## 2. 目标与非目标

### 2.1 目标

- 同一 Main Agent 在一个 Session 的所有 Run 中共享 Skill 加载记录。
- 同一个 Sub-agent 实例内部共享记录；不同 Sub-agent 实例互相隔离。
- 首次加载或内容 revision 变化时返回完整正文并持久化最新 revision。
- revision 未变化时不重复返回正文，而返回明确的“已加载、内容未更新”提示。
- Session Resume 后保持相同语义。
- 同一作用域并发加载相同 revision 时，最多一次返回完整正文。
- 状态更新失败时不得先返回正文再静默丢失 revision。

### 2.2 非目标

- 不改变 Skill metadata Catalog 的刷新、Slash route 或补全协议。
- 不缓存 `SKILL.md` 正文，也不把正文写入 Session。
- 不让 Storage 解释 Skill 业务语义。
- 不以 `run_id`、角色名、任务文本或 parent run 推导 Agent 实例身份。
- 不跨不同 Session 共享加载记录。

## 3. 已确认语义

### 3.1 加载作用域

定义稳定 typed `SkillLoadScope`：

- `Main`：当前 Session 的 Main Agent；跨所有 Main Run 稳定。
- `Subagent(instance_id)`：一个 Sub-agent 实例；该实例内稳定，新建 Sub-agent 必须获得新的 identity。

`run_id` 只用于调用关联、审计和 Runtime 生命周期，禁止参与去重键。角色、prompt 或 task 相同的新 Sub-agent 仍是新实例，可首次取得完整 Skill 正文。

### 3.2 去重键和值

`CanonicalSession` 保存按 `(SkillLoadScope, canonical_skill_name)` 索引的最新 revision。只持久化 canonical name、scope identity 和 revision，不保存正文、source path、Run 状态或 Tool future。

### 3.3 调用结果

- 无记录：原子记录 revision，返回完整正文。
- 已有记录且 revision 不同：原子替换 revision，返回完整新正文。
- 已有记录且 revision 相同：不修改 Session，不返回正文，返回：
  `Skill <name> 已加载，内容未更新（revision: <revision>）。请继续使用已有指令。`

## 4. 架构与所有权

### 4.1 Tools

Tools 继续拥有：

- Skill identity 规范化与 canonical identity；
- 当前 workspace / Run 冻结查询下的文件发现和读取；
- 内容 revision 计算；
- `LoadedSkill` 与加载错误。

Tools 不持有 Session 状态，不建立进程内 revision 缓存。Skill Tool 通过窄端口请求 compare-and-record，不得读取或修改 `CanonicalSession` 实现类型。

### 4.2 Runtime

Runtime 拥有当前执行主体的稳定 `SkillLoadScope` 装配：

- Main Runtime 总是使用 `Main`；
- Sub-agent 创建边界生成并携带稳定 `instance_id`，同一实例的后续执行复用该 identity；
- `run_id` 仍留在 `ExecutionScope`，但不是 Skill 去重 identity。

Runtime 负责把 Context-owned Skill revision 能力作为窄 port 注入 Tool execution context，不自行缓存或扫描历史 Tool Result。

### 4.3 Context Management

`CanonicalSession` 是 Skill 加载记录的唯一持久化真相源。Context 定义 typed mutation 与原子 compare-and-record 结果：

- `Fresh`：此前无记录；
- `Updated`：此前 revision 不同；
- `AlreadyLoaded`：revision 相同。

mutation 与 accepted input、outcome、Tool receipt 共用 Canonical Session mutation gate：读取当前 generation、构造 candidate、收集必要 Published Snapshot、durable save、再 publish。持久化失败不得污染 live generation。

### 4.4 Storage、SDK 与 TUI

Storage 只通过既有 AtomicBlob 保存 Session envelope，不解释加载记录。SDK/TUI 不新增 Skill 正文副本；普通 Tool result 继续进入模型上下文，TUI 维持 Skill result 隐藏策略。

## 5. 数据流与原子性

1. Skill Tool 解析输入，调用 `SkillLoadPort`。
2. Loader 返回 canonical name、完整正文和当前内容 revision。
3. Skill Tool 经注入的窄端口提交 `(scope, canonical name, revision)`。
4. Context 在单一 mutation gate 内比较并按需持久化：
   - 无记录或 revision 变化：先 durable save，再 publish，返回允许正文的判定；
   - revision 相同：不写盘，返回 already-loaded。
5. Tool 根据 typed 判定返回完整正文或已加载提示。

并发调用由 Context mutation gate 串行比较。同一 scope、name、revision 的并发请求中，首个成功提交者获得 `Fresh/Updated`；后续请求观察已提交 revision，只获得 `AlreadyLoaded`。

## 6. Session wire 与兼容

Session schema 新增 Skill revision 记录并提升 current schema version：

- 旧 schema 缺少记录时升级为空集合；
- writer 只输出当前 schema；
- unknown future schema 继续保留原字节并 fail-closed；
- Resume 恢复 canonical records 后，不依赖 Runtime 内存重建；
- compact 只推进对话读取 marker，不删除 Skill revision 记录。

Sub-agent 私有完整消息链仍不进入父 Session；这里持久化的只是父 Session 所拥有的轻量加载事实和稳定 Sub-agent instance identity key，不是 Sub Run 状态机。

## 7. 错误语义

- identity 无效、Skill 不存在、读取或解析失败：不调用 compare-and-record。
- Context mutation conflict 或 durable write 失败：Skill Tool 返回失败，不返回正文。
- 已加载提示不是错误，Tool outcome 保持 Success。
- revision 更新只有 durable save 成功后才能允许正文返回。
- 重试相同 mutation 保持幂等；不同 revision 以 mutation gate 中观察到的当前值顺序更新。

## 8. 测试矩阵

### L1

- `SkillLoadScope::Main` 跨 Run 无 Run identity 成分。
- 同一 Sub-agent instance 相等，不同 instance 不等。
- canonical name + scope 的键稳定。
- compare 结果覆盖 Fresh、Updated、AlreadyLoaded。

### L2

- Context compare-and-record 原子更新。
- revision 相同不增加 Session revision、不触发写盘。
- durable save 失败不发布 candidate。
- Session codec 当前版本 round-trip，旧版本缺失字段升级为空，future version fail-closed。
- Skill Tool 根据 typed 判定选择正文或已加载提示。

### L3

- Tools → Context 窄端口保持 canonical name、revision、scope 字段完整。
- Main/Sub ToolExecutionContext 注入正确 scope，不从 `run_id` 推导。
- Context → AtomicBlob reopen 后记录完整。

### L4

- Main 在同一 Session 的两个不同 Run 中，首次返回正文、后续返回已加载提示。
- 同一 Sub-agent 实例复用，另一个新实例可分别首次加载。
- Resume 后未变化返回已加载；文件正文变化后返回完整新正文并更新 revision。
- 同一 scope 并发加载相同 revision 时最多一次返回正文。

### L5

该能力是进程内 Tool/Runtime/Context 与既有 Session 文件边界的组合，不新增网络、PTY、平台或安装行为；L1–L4 与 AtomicBlob reopen 场景足以覆盖，L5 不适用。

## 9. 文档同步

实现时同步核对并更新：

- `docs/design/02-modules/tools/02-ports-and-lifecycle.md`
- `docs/design/02-modules/runtime/03-loop-and-state-machine.md`
- `docs/design/02-modules/context-management/01-session.md`
- `docs/design/03-engineering/04-testing-and-coverage.md`
- `specs/3.4-runtime.md`、`specs/3.5-tools.md`、`specs/3.10-storage.md`（仅当正式规则需要调整）

## 10. 方案取舍

### 最小补丁（不采用）

在 `SkillTool` 中保存进程内 `run_id/name → revision` 缓存。成本低，但下一 Main Run 仍重复加载、Resume 丢失、多个 Tool 实例形成第二状态源，不能满足 Issue。

### 根因方案（采用）

建立稳定 Main/Sub-agent instance scope，由 Context-owned Canonical Session 原子持久化 revision，并经窄端口注入 Tool。改动跨 Tools、Runtime、Context 与 Session codec，成本和测试范围更高，但能同时解决跨 Run、Resume、内容更新和并发重复正文问题。

## 11. 完成定义

- Issue #1460 全部验收项完成或记录可验证的不适用理由；
- 相关 L1–L4 测试按 TDD 通过；
- `cargo fmt --check`、相关 crate tests、workspace clippy 与架构守卫通过；
- 不存在 Runtime/Tool 进程内第二缓存、历史 Tool Result 反查或 `run_id` 去重旁路；
- 目标文档与代码术语、端口和持久化语义一致。
