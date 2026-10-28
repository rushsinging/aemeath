#!/usr/bin/env python3
"""按 case 计算确定性指标（#2133 P1）。

执行面说明（MUST 如实呈现）：
- 本脚本不自己跑压缩：`--summary-dir` 提供「压缩后 summary + 保留 tail」的文本，
  文件名约定 `<case_id>.summary.txt`。生产路径应改由隐藏 CLI 子命令
  （`aemeath eval compact --case ... --model ...`）产生，直接走真实装配，
  避免被测试简化成假绿；该子命令尚未落地（见 issue #2133）。
- 未提供 summary 的 case 计入 skipped，不参与均值分母。
- 指标是覆盖率代理，不是端到端质量（详见 metrics.py 边界声明）。

用法：
  python3 harness/run_eval.py --cases /tmp/compact-cases \
      --summary-dir /tmp/compact-summaries \
      --output results/run_eval.json
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

from metrics import compression_ratio, must_keep_recall, structure_health, summarize_cases

EVAL_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT = EVAL_ROOT / "results" / "run_eval.json"


def load_cases(cases_dir: Path) -> list[dict]:
    index = cases_dir / "cases.jsonl"
    if not index.is_file():
        raise SystemExit(f"missing {index}; run export_cases.py first")
    return [json.loads(line) for line in index.read_text(encoding="utf-8").splitlines() if line]


def read_summary(summary_dir: Path | None, case: dict) -> str | None:
    if summary_dir is None:
        return None
    path = summary_dir / f"{case['case_id']}.summary.txt"
    if not path.is_file():
        return None
    return path.read_text(encoding="utf-8")


def evaluate_case(case: dict, summary: str | None) -> dict:
    result = {
        "case_id": case["case_id"],
        "message_count": case.get("message_count"),
        "estimated_tokens": case.get("estimated_tokens"),
        "must_keep_total": len(case.get("must_keep") or []),
    }
    if summary is None:
        result.update({"recall": None, "compression_ratio": None, "skipped_reason": "no_summary"})
        return result

    recall = must_keep_recall(case.get("must_keep") or [], summary)
    summary_tokens = int(len(summary) * 0.6) if summary else 0
    result.update(
        {
            "recall": recall["recall"],
            "missed": recall["missed"][:20],
            "compression_ratio": compression_ratio(case.get("estimated_tokens") or 0, summary_tokens),
            "summary_chars": len(summary),
            "structure_health": structure_health(case.get("structure_health") or {}),
        }
    )
    return result


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cases", type=Path, required=True)
    parser.add_argument("--summary-dir", type=Path, default=None)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    args = parser.parse_args()

    cases = load_cases(args.cases)
    evaluated = [evaluate_case(case, read_summary(args.summary_dir, case)) for case in cases]
    report = summarize_cases(evaluated)
    report["summary_source"] = str(args.summary_dir) if args.summary_dir else None
    report["cases"] = evaluated
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({k: v for k, v in report.items() if k != "cases"}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
