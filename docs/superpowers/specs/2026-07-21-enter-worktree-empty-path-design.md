# EnterWorktree 空 path 分层修复设计

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/1297

## 目标

修复 LLM 调用 `EnterWorktree({"branch":"fix/example","path":""})` 时把空
`path` 解析为主 checkout、返回误导错误并触发重复调用的问题，同时让 TUI 在失败结果
没有结构化 data 时仍准确展示原始调用目标。`EnterWorktree` 新增可选 `base` 参数，
允许调用方选择新 worktree 的创建起点；省略、空串或纯空白时默认使用 `main`。

本修复保持现有工具名、正常参数形状、worktree 栈语义和 StuckGuard 行为不变。

## 根因

数据流中有三个相邻缺口：

1. Tools adapter 将空字符串直接转换为 `PathBuf("")`。
2. Project 把 `Some(PathBuf(""))` 解析为当前 `path_base`；该路径存在，因此跳过
   `git worktree add`。随后 Primary kind 被错误映射成 `RepoMismatch`。
3. TUI 的 result-aware EnterWorktree header 忽略 input；失败结果没有 data 时固定显示
   `branch=(default)`。

## 方案比较

### 方案 A：只强化 LLM 提示

在 schema 字段描述中要求省略未使用的 `path`。改动最小，但不能保证所有 provider 和
模型都不再产生空字符串，无法建立输入边界的不变量。

### 方案 B：只在 Project 容错

Project 将空 PathBuf 视为未提供。可以恢复执行，但 Tool schema 仍诱发错误参数，TUI
仍会显示 `(default)`。

### 方案 C：分层防御（采用）

- Tools 规范化 LLM 的字符串输入并强化 schema 描述。
- Project 防御空 PathBuf、统一解析 `base` 默认值，并区分“仓库不匹配”与“目标不是
  linked worktree”。
- TUI 成功时显示实际 result，失败时回退显示 input。

该方案让每层守护自己拥有的语义，且不复制路径创建或 Git 校验逻辑。

## 详细设计

### Tools 输入边界

`EnterWorktreeTool::call` 在构造 Project 参数前对 `path` 执行 trim 判空：

- 空或仅空白：转换为 `None`。
- 非空：保留用户原始路径内容并转换为 `PathBuf`。

`branch` 继续由 Project 校验；路径省略且 branch 为空时仍返回
`MissingPathAndBranch`。`EnterWorktreeInput.path` 的 schema 描述明确要求无 path 时
省略字段，禁止用空字符串占位。

`EnterWorktreeInput` 新增 `base: Option<String>`：

- 省略、空串或纯空白：按 `main` 处理。
- 非空：原样作为 Git revision/branch 起点传给 Project。
- 仅在目标路径不存在、需要创建新 worktree 时生效；进入已有 linked worktree 时忽略。

### Project 领域边界

`resolve_worktree_path` 将 empty `PathBuf` 与 `None` 视为同一语义，从 branch 推导
`.worktrees/<safe-branch-name>`。这是对所有 `WorkspaceControl` 调用方的防御，不替代
Tools adapter 的协议规范化。

`WorkspaceControl::enter` 增加可选 `base`，Project domain 是默认值的唯一真相源：
`None`、空串或纯空白归一为 `DEFAULT_WORKTREE_BASE`（`main`），非空值传给
`GitWorktreeOps::worktree_add`。Tools adapter 不复制 `main` 默认值。

同仓库路径通过 repository identity 校验、但 `worktree_kind != Linked` 时，返回新的
`NotLinkedWorktree` 领域错误。`RepoMismatch` 仅表示 git common dir 不同。任何失败都
不得修改 stack、workspace_root、path_base 或 worktree_kind。

### TUI 展示

EnterWorktree header 的目标选择顺序：

1. 成功 result 中的实际 branch，并附实际 workspace_root。
2. 失败或尚无 result 时，input 中非空 branch。
3. input 中非空 path。
4. 两者都没有时显示 `worktree`，不再伪造 `(default)`。

目标解析由一个私有 helper 复用在 `format_header` 和
`format_header_line_with_result`，避免两套展示规则漂移。

## 文档对齐

- `docs/design/02-modules/project/01-domain-model.md`：补充空 path 推导语义及
  `base` 默认规则、`NotLinkedWorktree` 错误分类。
- `docs/design/02-modules/project/02-ports-and-adapters.md`：明确 enter 的 optional
  path/base 边界。
- `docs/design/02-modules/tools/01-domain-model.md`：输入描述无需复制具体工具 schema；
  核对 ToolInvocation/Outcome 边界后若现有通用规则已覆盖，仅在 Issue/PR 记录已对齐。
- `specs/tui-cli.md`：补充 result-aware header 必须在 result 缺失时消费 input 的约束。

## 测试策略

- L1 Project：
  - empty PathBuf + branch 推导安全路径并创建 linked worktree。
  - base 省略/空串/纯空白时使用 `main`；非空 base 原样传给 Git port。
  - Primary 路径返回 `NotLinkedWorktree`，状态保持不变。
- L2 Tools / L4 real-git 场景：
  - `{"branch":"fix/example","path":""}` 成功创建并进入
    `.worktrees/fix-example`。
  - `base` 省略或为空时从 `main` 创建；指定其他有效 base 时从该引用创建。
  - schema 描述明确禁止空字符串占位。
- L1 CLI/TUI：
  - 失败 result 使用 input branch。
  - path-only 失败使用 input path。
  - 成功 result 优先显示实际 branch/root。
- 门禁：
  - 相关 Project、Tools、CLI 测试。
  - `cargo fmt --check`、`cargo clippy`、架构守卫和 workspace 测试。

## 非目标

- 不修改 StuckGuard 的重复调用阈值或判定算法。
- 不改变 worktree 默认基线 `main`；仅允许调用方通过显式非空 `base` 覆盖创建起点。
- 不允许进入 Primary checkout，也不放宽同仓库/嵌套 worktree 校验。
- 不改变历史 session 的序列化格式。
