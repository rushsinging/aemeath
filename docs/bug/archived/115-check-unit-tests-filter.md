# Bug #115：check-unit-tests 测试过滤参数误用导致误报失败

| 字段 | 值 |
|------|-----|
| 优先级 | 低 |
| 发现日期 | 2026-06 |
| 归档日期 | 2026-06-04 |
| 状态 | 已确认修复 |
| 修复 | 05de0ec |

## 症状

`cargo test` 使用短测试名配合 `--exact` 参数过滤时，匹配到 0 个测试，被误报为测试失败。

## 根因

测试过滤参数误用：短名 + `--exact` 导致 cargo test 过滤出 0 个测试，工具将 0 个测试执行结果误判为失败。

## 修复

补充完整路径测试名，避免 `--exact` 过滤参数与短名组合导致 0 匹配。

## 验证

- 用户确认修复。

## 涉及路径

- `agent/features/runtime/`（reasoning_config 测试路径）

## 关联提交

- `05de0ec fix(runtime): 记录 reasoning_config 精确测试路径 (refs #115)`
