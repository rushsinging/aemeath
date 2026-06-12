<!-- Migrated from: docs/feature/archived/060-auto-compact-llm-summary.md -->
# Feature #60：Auto-compact LLM 语义化压缩

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 归档日期 | 2026-06-02 |
| 状态 | 已确认完成 |
| 实现 | a0bcdc2 |

## 目标

将 auto-compact 的本地文本摘要替换为 LLM 语义化压缩，生成结构化摘要，并在失败时回退到本地摘要。

## 完成内容

1. 新增 LLM compact request 与 compact prompt。
2. 支持生成包含目标、进度、关键决策、相关文件、当前状态、下一步的结构化摘要。
3. auto-compact 接入 LLM 语义化压缩路径。
4. LLM 压缩失败时回退到本地摘要，保证功能可用性。

## 验证

- 用户确认完成。
