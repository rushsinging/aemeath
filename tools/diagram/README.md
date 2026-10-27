# 设计图制作规范（archify v3 · DDD 安置）

本仓设计图统一用 [archify](https://github.com/tt-a1i/archify) v3：声明式 JSON spec → 自包含 HTML（内联 SVG + 可注入交互面板）。

## 一、制品安置（与 DDD 文档体系对齐）

**图跟随它所描述的架构层级，源与产物同目录、按文件名区分**：

```
docs/design/
├── 01-system/diagrams/<视图>/            # 跨模块系统图（context map、依赖流）
├── 02-modules/<模块>/diagrams/<视图>/     # 模块图：与该模块 NN-*.md 编号文档同居
│   └── 例：02-modules/context-management/diagrams/published-language-map/
└── 03-engineering/diagrams/<视图>/       # 工程规则图（守卫、发版流程）
```

`04-architecture/` 保留其原有文档，不再新放图目录。

## 二、命名规则

| 对象 | 规则 | 示例 |
|---|---|---|
| 视图目录 | kebab-case 语义名，不用编号（区别于 NN-*.md 文档） | `published-language-map`、`receipt-flow` |
| spec 文件 | `spec.<archify图类型>.json` | `spec.workflow.json` |
| 详情数据 | `<视图短名>-data.json`（人工维护 duty/缺陷/引用） | `flow-data.json`、`symbol-data.json` |
| 面板注入 | `inject-<视图短名>.js` | `inject-flow.js` |
| 构建产物 | 固定 `index.html`（提交进 git，可 file:// 打开） | — |
| Mermaid 变体（可选） | `<视图>.mmd` / `<视图>-mermaid.svg`（PR 内嵌用） | `receipt-flow.mmd` |

## 三、构建

```bash
python3 tools/diagram/build_diagram.py docs/design/02-modules/<模块>/diagrams/<视图>
# 产物 <视图>/index.html；Mermaid 变体：mmdc -i <视图>.mmd -o <视图>-mermaid.svg -b transparent
```

脚本流程：spec 补 `meta.output` → archify 渲染（校验失败即止）→ spec label 合入
data payload → 面板注入 → index.html。

## 四、新图五步

1. 定层级：图属于系统 / 哪个模块 / 工程规则 → 选安置目录（上文第一节）。
2. 选图型：`architecture`（分层/BC）、`workflow`（泳道流程）、`sequence`（时序）、
   `dataflow`（管道）、`lifecycle`（状态机）、`timeline`（时间轴），另有 erd/tree/class 等。
3. 对齐 schema：`~/.agents/skills/archify/schemas/<type>.schema.json` + 一个 examples。
4. 写 spec（先让 v3 自动路由；showcase 对交叉/贴边零容忍，跨泳道边走外侧通道 via；
   phases 的 col 区间不可重叠）+ data + inject（可复制现有面板）。
5. 构建 → `open index.html` → agent-browser 实测点击交互。

## 五、维护纪律

- 代码变更后更新 `*-data.json` 事实字段再重建；**禁止只改 index.html**。
- 缺陷修复在 defect 文本标【已修复（阶段）】，保留审计线索不删条目。
- 符号退役/语义变更以代码为真相源清理 data；spec label 是节点显示名唯一真相源。
- 变体（Mermaid/C4）非必需不新增；C4 版已验证不适合链路叙事，勿复用。
