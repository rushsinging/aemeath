<!-- Migrated from: docs/feature/active.md#68 -->
### #68 项目指令搜索增强：全局 fallback ~/.claude/CLAUDE.md + 向上/向下 5 级目录探索

**状态**：已完成（4f2e5e1 + 2aecab7），待确认

**背景**：全局指令只读 `~/.agents/AGENTS.md`，不兼容 Claude Code 的 `~/.claude/CLAUDE.md`；项目指令只在 cwd 目录搜索，无法发现父目录或子目录中的指令文件。

**目标**：
1. 全局指令优先 `~/.agents/AGENTS.md`，不存在时 fallback `~/.claude/CLAUDE.md`
2. 项目指令从 cwd 向上 5 级祖先目录 + 向下 5 级子目录搜索，每层级 `CLAUDE.md` 优先于 `AGENTS.md`
3. 找到第一个存在的文件即停止（保持 break 语义）

**涉及路径**：
- `agent/shared/src/config/paths.rs`：新增 `INSTRUCTION_SEARCH_DEPTH` 常量
- `agent/features/runtime/src/business/prompt/build/prompt_build.rs`：`load_agents_md` 全局双路径 fallback + `project_instruction_walk` / `push_instruction_paths_for_dir` 函数
- `agent/features/runtime/src/business/prompt/build/prompt_build_tests.rs`：3 个 walk 测试
