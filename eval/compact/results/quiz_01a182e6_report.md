# 续接问答评测记录：01a182e6（compact 档位 × 续接质量）

- 日期：2026-10-29 / 30
- case：`01a182e6-0323-7f1c-96d6-1295a8529d76`（真实生产压缩会话）
- 规模：814,420 估算 token；2,770 条消息；被压缩 early 2,653 条；22 个 map 块
- 生产压缩：Bailian/deepseek-v4.1-flash-1017、Max 档 → **22 块降 10 块**
  （`output=16384 thinking=16384 text_deltas=0`，16K 输出预算被思考吃满）
- 评测模型：`Volcengine/deepseek-v4.1-flash`（三档压缩）；答题 `--reasoning max`；
  判卷 `OmniRoute/gpt-6.1-sol`（`--judge-reasoning max`）

## 一、compact 档位 → 压缩产物

| compact 档 | quality | 压缩耗时 | ms/1K token | summary chars |
|---|---|---|---|---|
| low | Llm | 150s | 184.8 | 130,587 |
| high | Llm | 217s | 266.5 | 135,876 |
| max | Llm | 405s | 496.7 | 150,748 |

> 三档均 `quality=Llm`（火山渠道无降级）；生产 Bailian Max 的 10/22 降级来自
> 「16K 输出预算 × 该渠道 thinking 行为」，与档位无关。

## 二、续接问答（60 题配额化题库，三段各 20 题）

| compact 档 | correct | early | mid | recent |
|---|---|---|---|---|
| low | 50/60（83%） | 0.85 | 0.81 | 0.84 |
| high | **54/60（90%）** | **0.95** | 0.86 | 0.89 |
| max | 53/60（88%） | 0.85 | 0.90 | 0.89 |

按 kind：

| kind | low | high | max |
|---|---|---|---|
| root_cause | 6/6 | 6/6 | 6/6 |
| constraint | 8/9 | 8/9 | 9/9 |
| decision | 20/22 | 21/22 | 20/22 |
| objective | 4/6 | 6/6 | 5/6 |
| tradeoff | 4/6 | 5/6 | 5/6 |
| state | 8/11 | 8/11 | 8/11 |

## 三、结论

1. **差距不大 → 默认档保持 `low`**（`context.compact_reasoning`）；
   high 略优（+4 题 / +7pp）但压缩成本 1.45×、max 成本 2.7× 且不优于 high。
2. **early 段（最老历史）仍然是最敏感信号**：high 0.95 vs low/max 0.85。
3. **无区分度的题目类型**（三档共同失败，后续出题应排除）：
   `全仓 pub trait 盘点结果`、`守卫登记表条目与分类`、`符号比例统计`、
   `38 处收敛结果` —— 统计/清单型，与「能否继续干活」无关。

## 四、方法学教训（三轮结论反转）

| 题库 | low | high | max | 当时的结论 |
|---|---|---|---|---|
| v1：19 题（含环境读取类） | 68% | 79% | 89% | “档位提升显著” → **误判** |
| v2：60 题（语义类，无配额） | 92% | 93% | 90% | “无差异” → 题目偏流程性 |
| v3：60 题（配额化） | 83% | 90% | 88% | 差距温和，保持 low |

关键：
- **环境读取类事实（路径/行号/分支名/commit/函数签名/命令输出）本来就该被摘要
  抽象掉**，用它出题等于测“摘要能否无损存盘”，会系统性放大档位差异；
- **统计/清单型题目**需要精确数字，任何档位都答不出，同样无区分度；
- 出题标准必须先于“结论”固定下来，否则样本量与题目类型会不断翻转结论。

## 五、数据文件

| 文件 | 内容 |
|---|---|
| `quiz_01a182e6_questions_tier60.json` | 60 题题库（含 epoch/kind/gold） |
| `quiz_01a182e6_compact-low_tier60.json` | low 档：答案/判定/证据/usage |
| `quiz_01a182e6_compact-high_tier60.json` | high 档 |
| `quiz_01a182e6_compact-max_tier60.json` | max 档 |

## 六、复现

```bash
# 1) 题库（固定后多档复用）
aemeath eval compact quiz generate \
  --cases /tmp/case-01a182e6 --model "Volcengine/deepseek-v4.1-flash" \
  --reasoning low --limit 1 --output quiz_questions.json

# 2) 三档压缩
for lv in low high max; do
  aemeath eval compact run --cases /tmp/case-01a182e6 --output-dir runs/$lv \
    --model "Volcengine/deepseek-v4.1-flash" --reasoning $lv
done

# 3) 同一题库 × 三份 summary（答题 max、判卷 gpt-6.1-sol max）
for lv in low high max; do
  aemeath eval compact quiz answer --cases /tmp/case-01a182e6 \
    --questions quiz_questions.json --summaries runs/$lv \
    --model "Volcengine/deepseek-v4.1-flash" \
    --judge "OmniRoute/gpt-6.1-sol" --judge-reasoning max \
    --output answer_$lv.json
done
```