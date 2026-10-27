# 设计图制作规范（archify v3）

本仓设计图统一用 [archify](https://github.com/tt-a1i/archify) v3 生成：声明式 JSON spec → 自包含 HTML（内联 SVG + 可注入交互面板）。

## 目录安置

每张图一个目录，置于 `docs/design/04-architecture/<模块>-<视图>/`：

```
docs/design/04-architecture/context-receipt-flow/   # 模块-视图 命名
├── spec.workflow.json    # archify spec：布局/节点/边的唯一真相源（类型=文件名第二段）
├── flow-data.json        # 节点详情数据：duty / io / defects / refs（人工维护字段）
├── inject-flow.js        # 详情面板注入脚本（点击节点 → 卡片）
├── index.html            # 构建产物（提交进 git，可直接 file:// 打开）
└── README.md             # 可选：图的说明与重建命令
```

## 构建流程

```bash
python3 tools/diagram/build_diagram.py docs/design/04-architecture/<图目录>
```

脚本做四件事：① spec 补 `meta.output` ② archify 渲染底图（校验失败即中止）
③ spec label 合入 data payload（label 以 spec 为真相源）④ 注入面板生成 index.html。

## 新图制作步骤

1. 按需求选图类型：`architecture`（系统/BC 分层）、`workflow`（泳道流程/数据链路）、
   `sequence`（调用时序）、`dataflow`（数据管道）、`lifecycle`（状态机）、
   另有 erd/timeline/tree/class/waterfall。
2. 读 `~/.agents/skills/archify/schemas/<type>.schema.json` 与一个 examples 对齐字段。
3. 写 `spec.<type>.json`（col/边布局先粗放，archify v3 自动路由能力较强；
   showcase 校验对交叉/贴边零容忍，跨泳道边优先走外侧通道 via）。
4. 写 `<图>-data.json`（每节点 id → 详情）与 `inject-*.js`（可复制现有图的改样式）。
5. 跑构建脚本，`open index.html` 实测点击交互（agent-browser DOM 断言）。

## 维护纪律

- 代码变更后同步更新 `*-data.json` 的事实字段（duty/缺陷状态/行号引用），
  再重跑构建；**禁止**只改 index.html。
- 缺陷修复后在 defect 文本标注【已修复（阶段）】而非删除条目（保留审计线索）。
- 图数据陈旧而代码已变（如符号退役）时，以 spec/根面为准清理 data 条目。
