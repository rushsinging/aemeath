# 通用 Config Form TUI 与 Connect 迁移设计

## 1. 背景与根因

`aemeath connect` 当前在 raw mode 下使用 `print!` / `println!` 手工绘制页面。终端收到单独的 LF 后不保证回到第 0 列，导致每个逻辑行沿上一行末尾继续绘制，形成阶梯式错位。现有测试只检查 Connect 字符串展示数据和类型化命令，没有通过真实终端布局模型验证完整帧，因此未能捕获该问题。

仅把 LF 改为 CRLF 可以止血，但会继续保留命令式终端绘制、专用 Connect 页面协议和不可复用的测试边界。根因级方案是在 TUI 内建立 Schema-driven 通用 Config Form 能力，以 Ratatui 统一终端生命周期、布局和场景测试；Connect 是第一个工作流，后续 Config 设置复用同一能力。

## 2. 已冻结决策

- Config Form 是 TUI 的通用能力，不是 Connect 专用 renderer。
- 使用 Schema-driven 表单，不让 TUI 理解 Provider、模型、Probe 或 Connect stage。
- 使用双宿主：独立全屏宿主与聊天内宿主共享同一 model、reducer、view assembler 和 renderer。
- 本次实现独立全屏宿主，并定义可组合的聊天宿主接口；暂不增加聊天内配置入口。
- Form Schema 由 Config BC 定义，SDK 原样发布。
- Connect 对外 UI 协议一次性替换为通用 Form 协议，不保留双协议兼容层。
- 通用会话通过 `ConfigFormClient` 和稳定 workflow ID 启动。
- 首个 workflow ID 为 `provider_connect`；`aemeath connect` 只选择该 ID。
- 页面按页原子提交；TUI 只持有当前页面尚未提交的输入、焦点和滚动状态。
- 输入值使用类型化值协议，不使用全字符串或任意 JSON。
- 动作使用 opaque action ID；TUI 不根据标签、错误文本或 ID 内容推导业务语义。
- Config 发布字段级与页面级错误，并保持所有业务校验的唯一所有权。
- 异步阶段由 View 发布 Busy 状态和刷新策略；TUI 不猜测 Probe 或 Saving。
- 当前 Issue #1457 和 PR #1473 承载该根因修正，不拆分新 Issue。
- 全仓 Rust 标识符禁止宽泛 `Projection` / `projection` 命名，并新增机械 Guard。

## 3. 所有权与依赖方向

### 3.1 Config BC

Config BC 拥有：

- 配置工作流注册表和稳定 workflow ID；
- workflow session、revision、业务状态、草稿和终态；
- Form Schema 生成；
- 字段归一化、页面校验、业务校验和页面导航；
- 异步操作、刷新策略、持久化和取消；
- Connect 领域状态机与通用 Form Published Language 之间的 ACL。

Connect 内部状态机继续使用领域明确的 stage、command、draft 和 outcome。通用 Form 是 Config 向外发布的交互语言，不反向替代 Connect 领域模型。

### 3.2 SDK

SDK 原样发布 Config-owned Published Language：

- `ConfigFormClient`；
- workflow、session、revision、page 和 field 的稳定值对象；
- `ConfigFormView`、字段 schema、option、error、busy 和 terminal DTO；
- `ConfigFormCommand` 与类型化 `ConfigFormValue`。

SDK 不解释字段含义，不维护工作流注册表，也不把 Config Form 并入宽泛的 Config reader/client。

### 3.3 TUI

TUI 拥有：

- 通用 Config Form model；
- 纯 reducer 和 Effect 描述；
- 当前页面本地输入、焦点、选择和滚动状态；
- Config Form view assembler；
- Ratatui renderer；
- 可复用的全屏和聊天内宿主边界。

TUI 不读取 config/env/fs，不发起 Provider 请求，不解析 URL、模型限制或 User-Agent，不识别 Connect stage，不根据 action ID、标签或错误文本重建业务状态。

### 3.4 Composition 与 CLI

Composition 装配 Config Form client、workflow registry 所需 adapter 和宿主依赖。CLI 的 `connect` 路由只启动 `provider_connect`，然后把 client 和初始 View 交给通用全屏宿主。

首次聊天初始化继续使用同一 workflow，仅 origin 不同。Config/Composition 保持默认配置 receipt、成功后继续聊天、取消安全回滚和非 TTY 拒绝创建配置的既有语义。

## 4. 通用会话协议

### 4.1 生命周期

通用 client 支持以下意图：

- 以 workflow ID 和 origin 启动会话；
- 以 session ID、expected revision 和一组类型化字段值提交当前页面；
- 以 session ID、expected revision 和 opaque action ID 调用动作；
- 请求服务端返回上一页面；
- 取消会话；
- 读取当前 View。

所有改变状态的命令携带 expected revision。过期 revision、未知 workflow、未知字段、未知选项、非法动作和非法转换均返回稳定 typed error，且不得部分修改会话。

### 4.2 Schema

每个 View 包含：

- workflow/session/revision；
- 页面 ID、标题、说明和步骤信息；
- 有序字段列表；
- 页面级错误；
- 有序动作列表；
- Busy 状态与刷新策略；
- 可选终态。

首版字段类型：

- 单行文本；
- Secret；
- Number；
- SingleSelect；
- Confirm/Boolean；
- 只读 Summary；
- Status。

字段使用稳定 field ID。SingleSelect 选项使用稳定 option ID。动作包含 opaque action ID、标签、展示样式和快捷键提示。TUI 只回传 ID，不解释 ID 内容。

### 4.3 类型化值

字段提交值明确区分：

- Text；
- Secret；
- Number；
- Boolean；
- SelectedOption。

Config 在 page schema 与提交值之间执行完整类型、字段集合和业务校验。页面提交为原子操作：任一字段失败时页面不推进，并返回字段级或页面级错误。

## 5. Secret 安全边界

- Secret 明文只存在于 TUI 当前输入 buffer、提交命令和 Config 内部受控处理路径。
- Config Form View 永不返回 Secret 明文；已配置状态只发布 `has_value`。
- SDK DTO、`Debug`、错误、日志、review、场景屏幕和快照不得包含 Secret 明文。
- 页面替换、提交、取消和宿主退出时立即清空本地 Secret buffer。
- TUI 显示固定遮罩，不把遮罩字符串提交为真实配置值。
- Config ACL 和 SDK 契约测试必须证明 Secret 不会从命令回流到 View。

## 6. TUI 能力设计

### 6.1 共享核心

Config Form TUI 核心拆为职责单一的组件：

- Form model：当前服务端 View 与页面本地编辑状态；
- reducer：键盘、粘贴、resize、refresh result 和服务端 View replacement 的纯状态转换；
- Effect：submit page、invoke action、back、cancel、refresh；
- view assembler：把 model 转为与 Ratatui 无关的展示值；
- renderer：把展示值绘制到 Ratatui Frame；
- host contract：驱动事件、执行 Effect、替换 View 和处理终态。

这些组件不得使用 `Projection` 命名。名称必须说明来源、目标或职责，例如 `ConfigFormViewAssembler`、`ConfigFormRenderModel`、`ConfigFormEffect`、`ConfigFormHost`。

### 6.2 双宿主

独立全屏宿主：

- 使用现有 Ratatui `TerminalGuard` 进入 raw mode 和 alternate screen；
- 运行事件循环并执行 Config Form Effect；
- 适用于 `aemeath connect` 和首次聊天初始化；
- 终态后恢复终端并把 outcome 返回调用方。

聊天内宿主接口：

- 接受同一 Form model/reducer/renderer；
- 由聊天 App 提供 Frame 区域、事件和 Effect executor；
- 本次只建立可组合接口与组合测试，不注册用户入口。

共享核心不得依赖任一具体宿主。独立宿主不得初始化 MainSession、Provider binding 或聊天 Runtime。

### 6.3 布局

宽终端使用双栏：左侧为字段或选项，右侧为说明、当前摘要和状态。窄终端自动降级为单栏。顶部显示工作流标题和步骤，底部固定当前输入、字段错误、动作和快捷键。中部内容超高时独立滚动。

resize 后 Ratatui 完整重绘。页面切换和连续帧不得保留上一页面内容。生产路径不再使用手写清屏序列或 `print!` / `println!` 绘制 Config Form。

## 7. Connect ACL 与迁移

Config 内部为 `provider_connect` 注册适配器：

- 将每个 Connect state 映射为通用 page schema；
- 将 page values 和 action ID 转换为 Connect typed command；
- 将 Connect field/business error 映射为通用 field/page error；
- 将 Probe/Saving 映射为 Busy 与 refresh policy；
- 将 Completed/Cancelled 映射为通用 terminal outcome。

迁移一次性删除：

- `ConnectProjection`；
- Connect 专用文本输入解析；
- Connect 手写 `print!` / `println!` renderer 和 raw-mode guard；
- TUI/CLI 对 `ConnectStage`、`ConnectDraftView`、`ConnectAvailableAction` 的判断；
- 被通用 Form DTO 替代的 SDK Connect UI DTO；
- 仅服务旧 UI 协议的测试和兼容路径。

Connect 领域状态机、Provider Probe、持久化、首次初始化和取消回滚行为保持不变。

## 8. Projection 命名治理

`specs/3.2-rust-coding.md` 已禁止生产代码和测试中的类型、trait、模块、函数、方法与变量使用宽泛 `Projection` / `projection` 标识符。本次新增全仓机械 Guard，使文字规范与执行门禁一致。

Guard 扫描 workspace Rust 源码中的标识符，至少拒绝：

- 名称包含 `Projection` 的类型、trait、模块、函数、方法和变量；
- `projection_*` 与 `*_projection` 形式的函数、方法和变量。

Guard 应区分标识符与注释、字符串及外部稳定数据，不因普通技术说明或外部序列化内容误报。Guard 覆盖生产和测试，不为历史命名建立永久白名单；当前命中必须按真实职责重命名。已有 Runtime 专用规则迁移到全仓 Guard 后删除重复实现，避免两套扫描逻辑漂移。

当前 PR 至少清理：

- Connect 的 `ConnectProjection` 及相关局部变量、测试名称；
- TUI 中使用宽泛 `projection` 的测试函数名称；
- 全仓 Guard 首次启用时发现的其他 Rust 标识符命中。

普通注释中的 `projection` 不作为机械失败条件，但触及文件时应优先改为 `view model`、`render state`、`display data`、`record` 或其他明确领域术语。

新增 Guard 必须：

- 注册到唯一 architecture guard 编排器；
- 增加故意违规与恢复后的元测试；
- 更新机器注册表；
- 同步架构守卫文档。

## 9. 错误与异步行为

Config Form View 同时支持字段级和页面级错误。字段级错误通过稳定 field ID 关联；TUI 不解析消息文本。stale revision、非法 action、Persist 冲突和 workflow 不可用使用稳定错误类别。

Busy View 明确是否允许取消、是否需要刷新及刷新节奏。独立宿主只按刷新策略产生通用 Refresh Effect，不根据页面或 action ID 判断 Probe/Saving。服务端返回新 View 后，reducer 原子替换服务端状态并按 page ID 决定是否保留非敏感本地输入；Secret 始终清空。

## 10. 测试策略

### 10.1 Config Domain/Application

- workflow ID 注册、未知 workflow 拒绝；
- 每个 Connect state 对应正确 page/field/action schema；
- 每种通用提交和 action 正确转换为 Connect typed command；
- 页面值类型、字段集合、option ID、revision 和导航校验；
- Probe/Saving Busy 与 terminal 映射；
- Secret 不进入 View、错误或 Debug。

### 10.2 SDK Published Language

- 所有 Form DTO 序列化与 schema 契约；
- session/revision/workflow/page/field/action ID 完整传递；
- Secret command 可提交但 View 无明文；
- 独立命令与首次聊天 origin 可区分。

### 10.3 TUI Model/Reducer

- Text、Secret、Number、SingleSelect 和 Confirm 编辑；
- 焦点、选择、滚动、Back、Cancel 和 action；
- 页面原子提交生成类型化 values；
- View replacement 和终态处理；
- 页面切换、提交和取消清空 Secret；
- Busy 只产生 schema 允许的通用 refresh。

### 10.4 Ratatui TestBackend 场景

- Provider 选择页在宽屏双栏完整绘制；
- 窄屏自动降级单栏；
- resize 后布局正确；
- 连续页面无旧内容残留；
- 长 Base URL、模型 ID 和错误文本不破坏下一行或底部操作区；
- Secret 输入与 review 始终遮罩；
- Provider 选择到 Review 的完整页面旅程；
- 屏幕不包含手写 ANSI 帧或阶梯式行首偏移。

### 10.5 宿主与组合场景

- 独立宿主启动、提交、action、refresh、取消和终态退出；
- `aemeath connect` 不创建 MainSession；
- 首次聊天成功后继续聊天；
- 首次聊天取消按 receipt 安全回滚；
- 非 TTY 不创建配置；
- 聊天宿主接口可组合同一 reducer/renderer，但本次无用户入口。

### 10.6 Guard

- 故意加入生产 `*Projection` 类型时失败；
- 故意加入测试 `*_projection` 函数或变量时失败；
- 注释、字符串和明确外部数据不误报；
- 删除探针后单 Guard 与完整编排通过；
- TUI 生产代码不引用 Connect UI 领域类型或解析 Config 业务规则。

## 11. 验收标准

- `aemeath connect` 使用 Ratatui 全屏宿主，所有逻辑行稳定对齐。
- 宽屏、窄屏和 resize 场景自动化通过。
- Connect UI 只消费通用 Config Form Published Language。
- Config 是 schema、校验、导航、异步状态和业务动作的唯一所有者。
- Secret 明文不进入任何 View、日志、错误、Debug 或屏幕快照。
- 旧 Connect UI DTO、输入 parser 和手写 renderer 已删除。
- 全仓不存在违反规则的 `Projection` / `projection` Rust 标识符。
- 新命名 Guard、注册表、文档和故意违规元测试一致。
- Config、SDK、Composition、CLI/TUI 各层相邻契约测试及最终场景测试均通过。
