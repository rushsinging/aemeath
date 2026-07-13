# #828 user_guidance 多层级收集实施计划

**日期**：2026-07-13  
**对应 Issue**：[#828](https://github.com/rushsinging/aemeath/issues/828)  
**父 Issue**：[#547](https://github.com/rushsinging/aemeath/issues/547)  
**目标分支**：`release/v0.1.0`  
**工作分支**：`feat/828-user-guidance-hierarchy`

## 目标

将 user guidance 从“全局只取第一个、项目只取第一个”改为组合加载：

1. 全局 `~/.agents/AGENTS.md` 与兼容路径 `~/.claude/CLAUDE.md` 均加载；
2. 项目目录从远到近排列，每层 `AGENTS.md` 与 `CLAUDE.md` 均加载；
3. 最终顺序为“全局 → 项目远 → 近”，具体规则位于文本末尾；
4. 每段保留来源路径边界；
5. 每个成功读取的文件触发一次 `InstructionsLoaded` Hook；
6. 对完整组装结果执行安全扫描，任一文件中的风险内容都能生成警告前缀。

## 非目标

- 不实现逐文件 mtime 缓存；
- 不调整 `PromptRequest` 或 cacheable/uncached 分段，这属于 #829；
- 不修改 guidance model-prefix 组合逻辑，该范围已由 #827 交付；
- 不新增公共 API；
- 不改 `config_reload::collect_watched_files`，它已覆盖所有候选路径。

## 设计约束

- 组装顺序以 `docs/design/02-modules/context-management/04-prompt-guidance.md` 为准：全局 → 项目由远到近。
- 同层文件按 `AGENTS.md` → `CLAUDE.md` 排列；两者都存在时均保留，不再 fallback。
- 来源边界采用 `<guidance source="…">…</guidance>`，避免不同文件内容无边界拼接。
- 仅在读取成功后触发 Hook；不存在或读取失败的文件不触发。
- 测试不得直接修改 `HOME` 或 `AEMEATH_AGENTS_DIR`，避免并行测试污染；使用私有、可注入候选路径的辅助函数。

## 原子实施步骤

### 任务 1：把路径顺序测试改为目标语义（Red）

**文件**：`agent/features/runtime/src/business/prompt/build/prompt_build_tests.rs`

1. 将“cwd 在前”的断言改为“最远祖先在前、cwd 在后”。
2. 断言每层候选顺序为 `AGENTS.md` 后 `CLAUDE.md`。
3. 保留深度为 0 时仅包含 cwd 的边界测试，并更新文件顺序断言。
4. 保留“不扫描兄弟/后代目录”的边界测试。
5. 运行：
   `cargo test -p runtime business::prompt::build::prompt_build_tests::test_project_instruction_walk -- --nocapture`
6. 预期：旧实现因近→远和 CLAUDE-first 失败，记录红灯证据。

### 任务 2：实现项目候选路径远→近排序（Green）

**文件**：`agent/features/runtime/src/business/prompt/build/prompt_build.rs`

1. 修改 `project_instruction_walk`：先获取祖先目录，再反转为远→近。
2. 每层生成 `AGENTS.md`、`CLAUDE.md` 两个候选路径。
3. 不读取文件、不触发 Hook，只负责确定性寻址顺序。
4. 重跑任务 1 的目标测试，确认转绿。

### 任务 3：新增组合加载与来源边界测试（Red）

**文件**：`agent/features/runtime/src/business/prompt/build/prompt_build_tests.rs`

1. 新增测试：同一项目层同时存在 `AGENTS.md` 与 `CLAUDE.md` 时，两份内容都出现。
2. 新增测试：父层与子层文件同时存在时，父层内容位于子层内容之前。
3. 新增测试：全局候选与项目候选同时存在时，全局内容位于项目内容之前。
4. 新增测试：每段输出包含对应 `<guidance source="路径">` 与闭合标签。
5. 新增测试：缺失文件和不可读取候选被忽略，其他文件仍正常组装。
6. 测试通过私有可注入辅助入口传入临时全局路径，不修改进程环境。
7. 运行新增测试；预期旧实现因 `break`、无来源标签而失败。

### 任务 4：实现多文件读取与结构化组装（Green）

**文件**：`agent/features/runtime/src/business/prompt/build/prompt_build.rs`

1. 引入私有加载结果类型，保存成功读取文件的 `path` 与 `content`。
2. 新增私有候选加载函数：按输入顺序读取全部候选，不在首个成功项后停止。
3. 新增私有渲染函数：每个文件渲染为带 `source` 的 `<guidance>` 段，并以空行分隔。
4. 生产入口 `load_agents_md` 组合：
   - 现有两个全局候选路径；
   - `project_instruction_walk` 返回的远→近项目候选路径。
5. 保持 `load_agents_md` 的公开签名不变，避免扩大调用面。
6. 重跑任务 3 的新增测试，确认转绿。

### 任务 5：新增逐文件 Hook 测试（Red）

**文件**：`agent/features/runtime/src/business/prompt/build/prompt_build_tests.rs`

1. 构造 `InstructionsLoaded` 测试 Hook，将 `AEMEATH_INSTRUCTIONS_FILE_PATH` 追加写入临时记录文件。
2. 创建至少三个可读 guidance 文件，并加入一个不存在的候选。
3. 调用可注入路径的加载辅助入口。
4. 断言记录文件恰好包含三个成功读取路径，顺序与组装顺序一致；不存在文件没有记录。
5. 运行该测试；预期在辅助加载尚未逐文件触发 Hook 时失败。

### 任务 6：接入逐文件 Hook（Green）

**文件**：`agent/features/runtime/src/business/prompt/build/prompt_build.rs`

1. 每个候选文件读取成功后调用 `on_instructions_loaded`。
2. 传入真实文件路径、既有 `agents_md` instruction type 和 `workspace_root`。
3. 不因 Hook 返回结果改变加载顺序或内容，维持当前观察性语义。
4. 重跑任务 5 测试，确认每个成功读取文件只触发一次。

### 任务 7：新增全量安全扫描测试（Red/Green）

**文件**：
- `agent/features/runtime/src/business/prompt/build/prompt_build_tests.rs`
- `agent/features/runtime/src/business/prompt/build/prompt_build.rs`（仅在测试暴露缺口时修改）

1. 新增测试：把风险文本放在非首个、较远或兼容 guidance 文件中。
2. 断言最终输出包含安全警告前缀，同时仍保留所有 guidance 段。
3. 运行测试验证现有“组装后统一扫描”在多文件实现下覆盖全部内容。
4. 若测试直接转绿，不为制造红灯而改生产逻辑；记录为 characterization test。
5. 若失败，仅修正扫描输入为完整渲染结果，不引入阻断行为。

### 任务 8：运行局部回归并清理旧测试语义

**文件**：
- `agent/features/runtime/src/business/prompt/build/prompt_build_tests.rs`
- `agent/features/runtime/src/business/prompt/build/prompt_build.rs`

1. 删除或重命名 `prefers_project_claude_md`、`falls_back_to_project_agents_md` 等已失效的 fallback 语义测试。
2. 检查实现中不再存在全局/项目首个成功后 `break` 的逻辑。
3. 检查注释不再描述 Claude-first 或 fallback。
4. 运行：
   `cargo test -p runtime business::prompt::build::prompt_build_tests -- --nocapture`

### 任务 9：运行格式与 crate 门禁

1. 运行 `cargo fmt --check`；若失败，运行 `cargo fmt` 后再次检查。
2. 运行 `cargo test -p runtime`。
3. 运行 `cargo clippy -p runtime --all-targets -- -D warnings`。
4. 任一失败先定位根因，只修复 #828 变更直接导致的问题；已有无关失败单独报告。

### 任务 10：检查退役项和变更边界

1. 使用搜索确认 `load_agents_md` 仍只有预期调用点。
2. 搜索并确认旧 `fallback`、`Claude-first`、首项 `break` 描述已清理。
3. 检查 `git diff --check`。
4. 检查 `git status --short`，确保仅有计划内文件；计划文档是否进入最终 PR 由用户确认。
5. 汇报验证证据与任何未清理旧路径，不提交、不推送、不创建 PR，除非用户另行授权。

## 预期改动文件

- `agent/features/runtime/src/business/prompt/build/prompt_build.rs`
- `agent/features/runtime/src/business/prompt/build/prompt_build_tests.rs`
- `docs/superpowers/plans/2026-07-13-828-user-guidance-hierarchy.md`（本计划）

## 风险与控制

- **全局环境污染**：不在测试中修改 `HOME`/`AEMEATH_AGENTS_DIR`；通过私有路径注入测试。
- **顺序语义回归**：对路径顺序和最终内容顺序分别断言。
- **Hook 漏触发或重复触发**：记录实际环境变量并断言精确次数与顺序。
- **安全扫描只覆盖首项**：把风险内容放在非首个文件中验证。
- **范围膨胀到 #829**：不引入 mtime、快照或 Provider cache-control 改动。
- **旧兼容语义残留**：显式搜索并清理 fallback/Claude-first/break 注释和测试。

## 完成标准

- 所有存在且可读的全局/项目 guidance 文件都被加载；
- 顺序严格为全局 → 项目远 → 近，同层 AGENTS → CLAUDE；
- 每段包含可识别的来源路径；
- 每个成功读取文件触发一次 Hook；
- 任一文件中的风险内容都进入统一安全扫描；
- runtime 测试、格式检查与 clippy 全部通过；
- 无 #829 范围改动，无静默遗留的旧 fallback 逻辑。
