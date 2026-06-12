<!-- Migrated from: docs/feature/archived/071-stop-hook-project-dir-context.md -->
# #71 Stop hook 日志输出项目目录上下文

**状态**：已归档

**修复 commits**：
- 待补充：Stop hook 环境日志输出实现提交
- 归档提交：待提交

**背景 / 症状**：排查 Stop hook 在 main 与 git worktree 中的耗时时，需要明确 hook 实际使用的项目根目录，以及 Claude Code 兼容环境变量是否与 Aemeath 项目目录一致。此前 Stop hook 输出只显示检查结果，不直接打印 `AEMEATH_PROJECT_DIR` / `CLAUDE_PROJECT_DIR`，定位路径问题时需要额外手动执行命令。

**根因**：相关 Stop hook 脚本启动时只根据 `AEMEATH_PROJECT_DIR` 或脚本路径解析项目根目录，但没有把输入环境变量和解析后的目录写入日志；当 main checkout 与 git worktree 之间存在路径差异或环境变量陈旧时，缺少直接可见的上下文信息。

**实现 / 修复方案**：
1. `check-architecture-guards.sh` 启动时输出 `AEMEATH_PROJECT_DIR`、`CLAUDE_PROJECT_DIR` 与解析后的 `ROOT`。
2. `check-unit-tests.sh` 启动时输出 `AEMEATH_PROJECT_DIR`、`CLAUDE_PROJECT_DIR`、解析后的 `ROOT` 与 `PWD`。
3. `build_cli.sh` 启动时输出 `AEMEATH_PROJECT_DIR`、`CLAUDE_PROJECT_DIR` 与 `PWD`。
4. 输出格式统一为 `[hook-env] KEY=value`，未设置时显示 `<unset>`。
5. 不改变 hook 的检查语义、退出码和构建/测试目标目录策略。

**验证**：
- `AEMEATH_PROJECT_DIR="$PWD" CLAUDE_PROJECT_DIR="$PWD" .agents/hooks/check-architecture-guards.sh`
- `AEMEATH_PROJECT_DIR="$PWD" CLAUDE_PROJECT_DIR="$PWD" .agents/hooks/check-unit-tests.sh`
- `AEMEATH_PROJECT_DIR="$PWD" CLAUDE_PROJECT_DIR="$PWD" ./build_cli.sh`

**涉及路径**：
- `.agents/hooks/check-architecture-guards.sh`
- `.agents/hooks/check-unit-tests.sh`
- `build_cli.sh`
