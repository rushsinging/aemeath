# Issue #1085 测试补完计划

**目标：** 补齐 #853 Tool / Skill / Command 端口与能力边界的 L1-L4 测试证据，并回写完整性审查结论。

**范围：** #853 创建时的直接叶子 #908、#909、#910、#911、#912、#913、#914。MCP Ready、#879 progress/plan 收口、#947 TUI I/O Effect 化不在本 PR 实现范围。

## 任务

### 1. Command L1/L2
- 新增 `agent/features/tools/src/domain/command_pl_tests.rs`，覆盖 CommandName 规范化/非法输入、Descriptor alias 验证、ParsedArguments、错误展示与序列化。
- 新增 `agent/features/tools/src/adapters/command_tests.rs`，覆盖重复 name/alias、target mismatch、completion、参数 schema 与三种 route target。
- 在 `domain.rs`、`adapters.rs` 注册外置测试模块。

### 2. Command L3 与交付链
- 扩展 `agent/features/tools/tests/command_contract.rs`：验证完整 builtin descriptor 集、机制/target/schema、duplicate/mismatch/missing argument。
- 扩展 `agent/composition/tests/command_wiring.rs`：验证空 Skill、Skill 与 builtin/alias 冲突。
- 扩展 `apps/cli/src/command_contract_tests.rs`：验证 TUI/no-TUI 对 PromptInjection 与未知命令使用同一 Router。

### 3. Skill L1/L2/L3/L4
- 扩展 `skill_pl_tests.rs`：slash aliases、materialization error、query DTO、CacheHint。
- 扩展 `skill_filesystem_tests.rs`：global/extra 物化、跨来源覆盖物化、fallback primary 缺失。
- 复核现有 Context `SkillPromptSource` / isolated-context contract 与 Composition slash projection 的相邻链路：真实 filesystem adapter 的来源、优先级和物化归 Tools owning layer；跨 BC 仅通过 `SkillMaterializationPort` fake 验证，避免 Context 反向依赖具体 adapter。
- 保持 Composition slash projection 的相邻契约，并补多 Skill/冲突行为。

### 4. Catalog/Execution 与 Guard
- 扩展 `catalog_execution_contract_tests.rs`：真实 success content/data/metadata、failure retryable、dynamic membership 移除。
- 收紧 `check-tool-catalog-execution-boundary.sh`：Runtime 禁止穿透 Tools private adapter/backing；补 shell 负例。

### 5. 审查回写与验证
- 在 `docs/design/03-engineering/04-testing-and-coverage.md` 添加 #1085 L0-L5 行为—证据矩阵、覆盖率和 L5 不适用理由。
- 更新 #1085 与 #853 的验收状态。
- 运行定向测试、fmt、check、clippy、production reachability、完整 Guard、coverage；记录首次失败与最终结果。
