# TODO

## 高优先级
- [ ] markdown.rs 中 dead code 清理（inline_markdown_spans、flush_plain、scan_until 函数未使用）
- [ ] 未提交的改动需要 review 后提交（completion/sessions.rs、markdown.rs 等新文件）
- [ ] provider.rs / guidance.rs 的修改需要验证

## 功能
- [ ] Session 管理功能完善（list/resume/delete）
- [ ] 上下文压缩策略可配置化
- [ ] 多模型 pool 支持优化

## 技术债务
- [ ] TODO.md 命令本身的单元测试
- [ ] 错误处理完善——部分 unwrap() 需要替换为 proper error handling
- [ ] CLI 参数文档化

## 低优先级
- [ ] TUI 模式下更多快捷键支持
- [ ] /usage 命令的 usage tracking 实际实现
