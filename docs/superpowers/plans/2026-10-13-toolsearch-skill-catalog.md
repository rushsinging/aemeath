# ToolSearch catalog + skill 搜索 + skill 目录瘦身（#1955）

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/1955
> Milestone: v0.2.0

## Goal

1. 生产 Main/Sub 为 ToolSearch 注入工具 catalog（修复常无结果）。
2. ToolSearch 词法搜索覆盖 tools + skills（无 systemone）。
3. Available Skills 目录块只保留 skill 名。
4. STATIC_SYSTEM_PROMPT 增加 ToolSearch 指引。
5. 非 systemone 召回率测试。

## Decisions

- Skill prompt：`- {name}` only（无 description / Usage）。
- ToolSearch 结果：`kind: tool|skill`；tool 保留 input_schema；skill 用 name+description。
- 空 query：只列 tools，不倾倒全部 skills。
- 无 scoring：纯词法；有 scoring 且低置信：对候选（含 skill）语义重排（保持现有 gate）。

## Tasks

### Task 1: 失败测试 — skill prompt 只留名
- 改 `agent/features/context/src/integration_tests/skill_prompt_pipeline.rs`
- 断言块内容为 `- name` 行，不含 description

### Task 2: 实现 C — render_entry 只留名
- 改 `agent/features/context/src/adapters/skill_prompt_source.rs`
- 跑 `cargo test -p context skill_prompt`

### Task 3: 失败测试 — ToolSearch 无 catalog / 有 catalog / skill 命中
- 改 `agent/features/tools/src/adapters/tool_search_tests.rs`
- 新增非 systemone 召回用例（中英）

### Task 4: 实现 B — ToolSearch 类型 + adapter
- `domain/types/tool_search.rs` 加 kind
- adapter 合并 skill catalog
- sdk tool_result 同步（若有独立类型）

### Task 5: 实现 A — runtime with_catalog
- `main_run_port.rs` / `derived/setup.rs`
- 注入当前 scope 的 ToolCatalogSnapshot

### Task 6: Prompt 指引
- `agent/shared/src/i18n/prompt/constants.rs` STATIC_SYSTEM_PROMPT_EN/ZH

### Task 7: TUI + specs
- ToolSearch display 显示 kind
- `specs/3.3` / `3.5` / `3.7`

### Task 8: 验证
- `cargo test -p tools tool_search`
- `cargo test -p context skill_prompt`
- 相关 share/cli 测试
