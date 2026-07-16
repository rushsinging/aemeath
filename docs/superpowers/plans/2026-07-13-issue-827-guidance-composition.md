# Issue #827 Guidance 组合加载实施计划

> **执行要求：** 实施时使用 `superpowers:executing-plans` 或 `superpowers:subagent-driven-development`，严格按 TDD 的 Red → Green → Refactor 顺序推进。

**目标：** 将模型 Guidance 从“最长匹配 / config fallback 取首”改为确定性的组合加载：`_default → 文件短前缀 → 文件长前缀 → config 通用匹配 → config 具体匹配 → _reasoning`。

**架构：** 保留现有公开 API，抽出同步、异步共用的“发现候选并稳定排序”纯逻辑。语言目录中的同名前缀文件覆盖根目录文件，避免重复注入；文件 Guidance 与 config Guidance 是组合关系，不再提前返回。async 生产路径负责逐文件触发 `InstructionsLoadedHook`，sync 兼容路径复用同一解析结果，防止双轨漂移。

**技术栈：** Rust、async-trait、标准库文件系统、Cargo test/clippy。

**Issue / 基线：** #827；milestone `v0.1.0 — Context Engineering + 架构重构`；分支 `feature/827-guidance-composition` 基于 `origin/release/v0.1.0@83bd2707`（包含 PR #834）。

---

## 范围边界

### 本计划包含

- 全部模型前缀文件按短到长组合。
- 前缀匹配大小写不敏感。
- `{language}/` 与根目录同名前缀去重，语言文件优先。
- config guidance map 所有匹配项按通用到具体稳定组合。
- 文件 Guidance 与 config Guidance 同时生效。
- `_default` 始终最前，`_reasoning` 始终最后。
- async hook 对每个实际加载的文件调用一次，顺序与注入顺序一致。
- 空文件、不可读文件、无匹配文件不产生空片段或阻断后续候选。
- sync/async 共享候选发现、排序、读取与安全扫描逻辑。

### 本计划不包含

- `AGENTS.md` / `CLAUDE.md` 向上搜索；属于 #828。
- Guidance 路径标签、mtime snapshot、PromptPort 完整体；属于后续 Context/Prompt 演进。
- cacheable/uncached 前缀拆分；属于 #829。
- 修改 Guidance 设计文档或 specs；当前目标设计已经要求组合加载，本 PR 只修代码与测试。

---

## Task 1：建立测试夹具并锁定纯排序语义

**Files:**
- Modify: `agent/features/context/src/prompt/business/guidance/resolver_tests.rs`

- [ ] **Step 1：新增独立临时目录夹具**

实现测试专用的：

- 全局环境变量互斥锁；
- `AGENTS_DIR_ENV` 保存/恢复 guard；
- 唯一临时目录；
- 文件创建辅助函数；
- 测试结束自动删除目录。

要求测试不读取或污染真实 `~/.agents/guidance`。

- [ ] **Step 2：新增文件前缀组合失败测试**

创建：

- `_default.md = default`
- `claude.md = generic`
- `claude-sonnet.md = family`
- `claude-sonnet-4.md = specific`
- `other.md = ignored`
- `_reasoning.md = reasoning`

调用 model `Claude-Sonnet-4.5`，断言输出顺序严格为：

`default < generic < family < specific < reasoning`

并断言 `ignored` 不存在。该测试在旧实现上必须失败，因为旧逻辑只读取最长前缀。

- [ ] **Step 3：新增稳定排序边界测试**

覆盖相同长度匹配项时使用规范化前缀 / pattern 字典序作为 tie-break，确保 `read_dir` 和 `HashMap` 的非确定迭代顺序不会影响 prompt。

- [ ] **Step 4：运行目标测试确认 Red**

Run:

```bash
cargo test -p context --lib prompt::business::guidance::resolver::tests::test_resolve_guidance_combines_all_prefixes_general_to_specific -- --exact
```

Expected: FAIL，输出缺少通用和中间前缀内容。

---

## Task 2：锁定语言目录覆盖和文件读取边界

**Files:**
- Modify: `agent/features/context/src/prompt/business/guidance/resolver_tests.rs`

- [ ] **Step 1：新增语言目录覆盖失败测试**

根目录创建 `claude.md = root generic`、`claude-sonnet.md = root family`；`zh/` 创建 `claude.md = zh generic`。

调用 language `zh`，断言：

- `zh generic` 出现且仅出现一次；
- `root generic` 不出现；
- `root family` 仍出现。

语义：语言目录按同名 stem 覆盖根目录，而不是“语言目录只要命中一个文件就屏蔽全部根目录”。

- [ ] **Step 2：新增空文件与读取失败测试**

覆盖：

- 语言目录同名文件为空时回退根目录非空文件；
- 某候选读取失败时跳过该候选，后续更具体候选仍加载；
- 无模型前缀命中时仍保留 `_default` 和可选 `_reasoning`。

- [ ] **Step 3：运行 resolver 测试确认 Red**

Run:

```bash
cargo test -p context --lib prompt::business::guidance::resolver::tests
```

Expected: 至少语言覆盖与多前缀组合测试 FAIL。

---

## Task 3：锁定 config 补充组合与安全扫描语义

**Files:**
- Modify: `agent/features/context/src/prompt/business/guidance/resolver_tests.rs`

- [ ] **Step 1：新增 config 多匹配组合失败测试**

创建多个 config pattern 对应的临时文件，例如：

- `claude-*` → `config generic`
- `claude-sonnet-*` → `config family`
- `claude-sonnet-4*` → `config specific`

断言三个内容都出现，并按通用到具体排序。旧实现只读取一个匹配项，因此必须失败。

- [ ] **Step 2：新增文件 + config 同时生效失败测试**

目录创建匹配文件，config 同时提供匹配项；断言文件内容在前、config 内容在后，二者都存在。旧实现会在目录命中后提前返回，因此必须失败。

- [ ] **Step 3：新增 config 错误路径测试**

第一个匹配 config 路径不存在，后续匹配路径有效；断言有效内容仍加载，单个坏路径不阻断组合。

- [ ] **Step 4：保留并覆盖逐文件安全扫描**

使用包含现有 scanner 可识别模式的 config 文件，断言 warning 前缀与原内容一起进入对应片段；确保组合改造没有绕过安全扫描。

- [ ] **Step 5：运行 resolver 测试确认 Red**

Run:

```bash
cargo test -p context --lib prompt::business::guidance::resolver::tests
```

Expected: config 多匹配和文件 + config 组合测试 FAIL。

---

## Task 4：锁定 async hook 的完整性和顺序

**Files:**
- Modify: `agent/features/context/src/prompt/business/guidance/resolver_tests.rs`

- [ ] **Step 1：实现测试 HookRecorder**

实现 `InstructionsLoadedHook`，使用测试内可变容器记录 `(file_path, instruction_type)`；测试只验证实际文件，built-in default 不应伪造文件 hook。

- [ ] **Step 2：新增 async hook 失败测试**

创建 `_default`、三个匹配前缀、两个匹配 config、`_reasoning`，调用 `resolve_guidance_async`。

断言：

- 每个实际加载文件恰好记录一次；
- `instruction_type` 全部为 `guidance`；
- hook 路径顺序与最终内容顺序一致；
- 被语言目录覆盖的根目录同名文件不触发 hook；
- 不匹配文件不触发 hook。

旧实现只 hook 单个模型文件，且 config 文件不 hook，因此必须失败。

- [ ] **Step 3：运行 async 目标测试确认 Red**

Run:

```bash
cargo test -p context --lib prompt::business::guidance::resolver::tests::test_resolve_guidance_async_hooks_each_loaded_file_in_order -- --exact
```

Expected: FAIL，hook 数量与顺序不满足断言。

---

## Task 5：实现统一的 Guidance 候选模型

**Files:**
- Modify: `agent/features/context/src/prompt/business/guidance/resolver.rs`

- [ ] **Step 1：定义内部候选类型**

增加私有 `GuidanceSource` / `LoadedGuidance` 类型，至少承载：

- 稳定排序 key；
- 实际路径；
- 已读取内容。

类型保持私有，不扩大公开 API。

- [ ] **Step 2：实现前缀候选发现**

将旧 `load_prefix_matched_from_dir*` 的“best only”逻辑替换为：

1. 扫描根目录所有非 `_` 开头的 `.md`；
2. 扫描语言目录所有匹配 `.md`；
3. case-insensitive 判断 `model_id.starts_with(stem)`；
4. 以规范化 stem 为去重 key；
5. 语言文件非空且可读时覆盖根目录同名候选；为空或不可读时保留根目录候选；
6. 按 `(stem 长度升序, 规范化 stem 字典序, path 字典序)` 稳定排序。

- [ ] **Step 3：实现 config 候选发现**

保留现有 glob 兼容语义，但返回全部匹配：

- 通用 pattern 在前，具体 pattern 在后；
- 建议排序 key 为 `(非通配字面长度升序, wildcard 数量降序, pattern 字典序)`；
- 每个路径独立读取、独立安全扫描；
- 坏路径只记录 warning 并跳过，不阻断其它候选。

- [ ] **Step 4：实现单一文件读取函数**

统一处理：

- UTF-8 文本读取；
- 空白内容跳过；
- debug 日志；
- config 文件安全扫描；
- 返回实际路径和最终内容。

不得在候选发现阶段触发 hook，避免 sync/async 分叉。

- [ ] **Step 5：删除旧 best/fallback 辅助函数**

删除或替换：

- `load_prefix_matched_file_with_lang`
- `load_prefix_matched_from_dir`
- `load_prefix_matched_file_async_with_lang`
- `load_prefix_matched_from_dir_async`
- `find_matching_config_guidance` 的 Option/取首实现

确认没有死代码或保留两套排序规则。

---

## Task 6：统一 sync/async 组装路径

**Files:**
- Modify: `agent/features/context/src/prompt/business/guidance/resolver.rs`

- [ ] **Step 1：建立统一收集顺序**

内部收集函数产出以下顺序：

1. `_default`：语言文件优先，根文件回退，最后 built-in；
2. 全部文件前缀候选：短 → 长；
3. 全部 config 候选：通用 → 具体；
4. `_reasoning`：仅 reasoning=true，语言文件优先，根文件回退，最后 built-in。

- [ ] **Step 2：sync API 复用收集结果**

`resolve_guidance` 只负责调用统一收集函数并 join，不再持有独立 fallback/排序分支。

保留公开签名，避免本 Issue 扩大为 API 迁移。

- [ ] **Step 3：async API 复用相同收集结果**

`resolve_guidance_async` 使用同一收集结果，按顺序：

1. 对带实际路径的片段调用 hook；
2. 再组合内容。

built-in default/reasoning 没有实际文件路径，不调用 `InstructionsLoadedHook`。

- [ ] **Step 4：兼容 `resolve_model_guidance_async`**

保持现有公开导出可编译，但内部复用“模型文件 + config”收集逻辑并返回组合字符串；不得重新引入取首语义。

- [ ] **Step 5：更新函数注释与日志**

将 `longest match wins` / `fallback` 改为组合加载术语。debug 日志应能显示每个匹配 source，不新增 info/warn 噪音。

---

## Task 7：完成 Green 与局部重构

**Files:**
- Modify: `agent/features/context/src/prompt/business/guidance/resolver.rs`
- Modify: `agent/features/context/src/prompt/business/guidance/resolver_tests.rs`

- [ ] **Step 1：运行 resolver 单测**

Run:

```bash
cargo test -p context --lib prompt::business::guidance::resolver::tests
```

Expected: PASS。

- [ ] **Step 2：运行 guidance 模块测试**

Run:

```bash
cargo test -p context guidance
```

Expected: PASS。

- [ ] **Step 3：检查测试是否依赖非确定顺序**

将核心组合测试连续运行至少 3 次；每次输出顺序必须一致。

- [ ] **Step 4：局部重构**

检查并消除：

- sync/async 重复排序；
- 重复文件读取；
- 不必要 clone；
- 旧 longest/fallback 命名；
- 仅测试引用的生产辅助函数。

重构后重跑 Step 1–3。

---

## Task 8：运行 crate 与仓库验证门禁

**Files:**
- Verify only

- [ ] **Step 1：格式化**

Run:

```bash
cargo fmt --all
cargo fmt --all -- --check
```

Expected: PASS，第二条无 diff。

- [ ] **Step 2：context 全量测试**

Run:

```bash
cargo test -p context
```

Expected: PASS。

- [ ] **Step 3：context clippy**

Run:

```bash
cargo clippy -p context --all-targets -- -D warnings
```

Expected: PASS，无 warning。

- [ ] **Step 4：运行 workspace 相关验证**

由于 runtime 是 async Guidance 的生产调用方，运行：

```bash
cargo test -p runtime prompt
cargo check --workspace
```

Expected: PASS。

- [ ] **Step 5：检查变更范围和退役代码**

Run:

```bash
git diff --check
git status --short
git diff --stat
git diff -- agent/features/context/src/prompt/business/guidance/resolver.rs agent/features/context/src/prompt/business/guidance/resolver_tests.rs
```

确认：

- 没有 #828 / #829 范围变更；
- 没有新增文档或非必要文件；
- 旧 best/fallback 路径已删除；
- 没有 warning、dead code 或临时诊断日志。

---

## Task 9：完成前审查与 Issue 状态同步准备

**Files:**
- Review only

- [ ] **Step 1：请求代码审查**

调用 `superpowers:requesting-code-review`，重点审查：

- 组合优先级是否严格符合目标设计；
- language override 是否只覆盖同名 stem；
- config 排序是否确定且兼容旧 glob；
- hook 是否逐实际文件且不重复；
- 安全扫描是否覆盖每个 config 文件；
- sync/async 是否共享唯一规则。

- [ ] **Step 2：按审查意见修正并重跑门禁**

如有修改，至少重跑 Task 7 和 Task 8 的全部命令。

- [ ] **Step 3：准备 GitHub 状态更新**

在用户要求提交/创建 PR 后，再按仓库工作流：

1. 使用 `commit` skill 生成符合仓库风格的 commit；
2. `git pull origin release/v0.1.0` 并解决可能冲突；
3. 重跑完整验证；
4. push feature 分支并创建 base=`release/v0.1.0` 的 PR；
5. 不自动合并、不自动关闭 #827；
6. PR 合入后同步 #547 与 Release Gate #579，再由用户决定是否关闭 Issue。

---

## 验收矩阵

| 场景 | 预期 |
|---|---|
| `_default` 文件存在 | 最先加载实际文件 |
| `_default` 文件缺失/空 | 使用对应语言 built-in |
| 多个 model prefix 命中 | 全部加载，短前缀到长前缀 |
| prefix 大小写不同 | case-insensitive 命中 |
| language/root 同名 prefix | language 非空可读文件覆盖 root，仅加载一次 |
| language 同名为空/不可读 | 回退 root 同名文件 |
| language 缺少更具体 prefix | root 更具体 prefix 继续参与组合 |
| 多个 config pattern 命中 | 全部加载，通用到具体，稳定排序 |
| 文件与 config 同时命中 | 文件组合在前，config 组合在后 |
| 单个 config 路径坏 | 跳过，不阻断其它匹配 |
| config 内容触发安全扫描 | warning 前缀与原内容一并注入 |
| reasoning=false | 不加载 `_reasoning` |
| reasoning=true | `_reasoning` 最后加载 |
| async hook | 每个实际文件一次，顺序与注入一致 |
| built-in guidance | 无伪造路径，不触发 hook |
| 无 model/config 匹配 | 仍返回 default；reasoning=true 时追加 reasoning |
