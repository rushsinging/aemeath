#!/usr/bin/env python3
"""System One 评测指标计算（#1751 阶段一）。

读取 results/<engine>/<scenario>.jsonl，输出：
- rank 场景（memory_rerank / skill_match）：R@1、MRR、order-flip 率、延迟 p50/p95
- noul 场景（stop_verify、permission_triage 的 p_true）：accuracy、F1、ECE(10桶)、Brier
- score 场景（permission_triage 的 risk_score）：MAE
- 汇总打印 Markdown 表 + 落盘 results/summary.json
"""
from __future__ import annotations

import argparse
import json
import math
import pathlib
import statistics
import sys

HERE = pathlib.Path(__file__).resolve().parent
EVAL_ROOT = HERE.parent
RESULTS = EVAL_ROOT / "results"
DATASETS = EVAL_ROOT / "datasets"

RANK_SCENARIOS = ("memory_rerank", "skill_match")
NOUL_SCENARIOS = ("stop_verify",)


def load_jsonl(path: pathlib.Path) -> list[dict]:
    if not path.exists():
        return []
    with path.open(encoding="utf-8") as fh:
        return [json.loads(line) for line in fh if line.strip()]


def percentile(values: list[float], pct: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    idx = min(len(ordered) - 1, max(0, math.ceil(pct / 100 * len(ordered)) - 1))
    return ordered[idx]


def ece(scores: list[tuple[float, int]], bins: int = 10) -> float | None:
    """scores: [(p_true, y)]。返回 Expected Calibration Error。"""
    if not scores:
        return None
    total = len(scores)
    err = 0.0
    for b in range(bins):
        lo, hi = b / bins, (b + 1) / bins
        bucket = [(p, y) for p, y in scores if lo <= p < hi or (b == bins - 1 and p == 1.0)]
        if not bucket:
            continue
        conf = sum(p for p, _ in bucket) / len(bucket)
        acc = sum(y for _, y in bucket) / len(bucket)
        err += len(bucket) / total * abs(acc - conf)
    return err


def is_chinese_case(case: dict) -> bool:
    """context/question 含 CJK 字符即计为中文 case（三项门禁之中文子集口径）。"""
    text = f"{case.get('context', '')}{case.get('question', '')}"
    return any("一" <= ch <= "鿿" for ch in text)


def eval_rank(records: list[dict], cases: dict[str, dict]) -> dict:
    forward = [r for r in records if r.get("order") == "forward" and "error" not in r]
    reversed_ = [r for r in records if r.get("order") == "reversed" and "error" not in r]
    errors = [r for r in records if "error" in r]

    def gold_of(rid: str) -> str:
        case = cases[rid]
        return case["answers"][case["gold"]]

    r_at_1 = (sum(1 for r in forward if r.get("top1") == gold_of(r["id"])) / len(forward)
              if forward else None)

    chinese_forward = [r for r in forward if is_chinese_case(cases[r["id"]])]
    chinese_r_at_1 = (sum(1 for r in chinese_forward if r.get("top1") == gold_of(r["id"]))
                      / len(chinese_forward) if chinese_forward else None)

    mrr_sum = 0.0
    for r in forward:
        gold = gold_of(r["id"])
        probs = r.get("probabilities") or {}
        ranked = sorted(probs, key=probs.get, reverse=True)
        try:
            mrr_sum += 1.0 / (ranked.index(gold) + 1)
        except ValueError:
            pass
    mrr = mrr_sum / len(forward) if forward else None

    flips = 0
    pairs = 0
    rev_by_id = {r["id"]: r for r in reversed_}
    for r in forward:
        rev = rev_by_id.get(r["id"])
        if rev and rev.get("top1") is not None and r.get("top1") is not None:
            pairs += 1
            if rev["top1"] != r["top1"]:
                flips += 1
    latencies = [r["latency_ms"] for r in records
                 if r.get("latency_ms") is not None]
    return {
        "n_forward": len(forward), "errors": len(errors),
        "R@1": r_at_1, "MRR": mrr,
        "chinese_r_at_1": chinese_r_at_1,
        "order_flip_rate": (flips / pairs) if pairs else None,
        "latency_p50_ms": percentile(latencies, 50),
        "latency_p95_ms": percentile(latencies, 95),
    }


def eval_noul(records: list[dict], cases: dict[str, dict]) -> dict:
    forward = [r for r in records if r.get("order") == "forward"
               and "error" not in r and r.get("p_true") is not None]
    errors = [r for r in records if "error" in r]
    pairs_scored = [(r["p_true"], 1 if cases[r["id"]]["label"] else 0) for r in forward]
    if not pairs_scored:
        return {"errors": len(errors)}

    preds = [(1 if p >= 0.5 else 0, y) for p, y in pairs_scored]
    tp = sum(1 for p, y in preds if p == 1 and y == 1)
    fp = sum(1 for p, y in preds if p == 1 and y == 0)
    fn = sum(1 for p, y in preds if p == 0 and y == 1)
    tn = sum(1 for p, y in preds if p == 0 and y == 0)
    acc = (tp + tn) / len(preds)
    precision = tp / (tp + fp) if tp + fp else 0.0
    recall = tp / (tp + fn) if tp + fn else 0.0
    f1 = (2 * precision * recall / (precision + recall)) if precision + recall else 0.0
    brier = sum((p - y) ** 2 for p, y in pairs_scored) / len(pairs_scored)

    reversed_ = [r for r in records if r.get("order") == "reversed"
                 and "error" not in r and r.get("p_true") is not None]
    rev_by_id = {r["id"]: r for r in reversed_}
    flips = pairs = 0
    for r in forward:
        rev = rev_by_id.get(r["id"])
        if rev:
            pairs += 1
            if (rev["p_true"] >= 0.5) != (r["p_true"] >= 0.5):
                flips += 1
    latencies = [r["latency_ms"] for r in records
                 if r.get("latency_ms") is not None]
    return {
        "n_forward": len(forward), "errors": len(errors),
        "accuracy": acc, "f1": f1, "brier": brier, "ece": ece(pairs_scored),
        "confusion": {"tp": tp, "fp": fp, "fn": fn, "tn": tn},
        "order_flip_rate": (flips / pairs) if pairs else None,
        "latency_p50_ms": percentile(latencies, 50),
        "latency_p95_ms": percentile(latencies, 95),
    }


def eval_permission(records: list[dict], cases: dict[str, dict]) -> dict:
    """permission_triage = noul(p_true) + score(risk_score) 双指标。"""
    out = eval_noul(records, cases)
    scored = [(r["risk_score"], cases[r["id"]]["risk"])
              for r in records
              if r.get("order") == "forward" and r.get("risk_score") is not None]
    if scored:
        out["risk_mae"] = sum(abs(p - y) for p, y in scored) / len(scored)
    return out


# ---------------------------------------------------------------------------
# 场景接入门禁（fail-closed）：阈值见各场景 issue 验收（#1834 memory_rerank）
# ---------------------------------------------------------------------------

GATES: dict[str, dict[str, tuple[str, float]]] = {
    "memory_rerank": {
        "R@1": (">=", 0.99),
        "MRR": (">=", 0.99),
        "order_flip_rate": ("<=", 0.02),
        "latency_p95_ms": ("<=", 2000.0),
        "chinese_r_at_1": ("==", 1.0),
    },
}

_OPS = {
    ">=": lambda v, t: v >= t,
    "<=": lambda v, t: v <= t,
    "==": lambda v, t: v == t,
}


def check_gate(scenario: str, metrics: dict) -> list[str]:
    """对照场景门禁阈值，返回违规描述列表（空 = 通过）。指标缺失 fail-closed。"""
    gate = GATES.get(scenario)
    if gate is None:
        return [f"no gate registered for scenario {scenario!r}"]
    violations = []
    for metric, (op, threshold) in gate.items():
        value = metrics.get(metric)
        if value is None:
            violations.append(f"{metric}: missing (fail-closed)")
            continue
        if not _OPS[op](value, threshold):
            violations.append(f"{metric}={value:.4g} violates {op} {threshold}")
    return violations


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gate", nargs=2, metavar=("ENGINE", "SCENARIO"),
                        help="门禁模式：对照场景阈值 fail-closed 检查，违规 exit 2")
    args = parser.parse_args()

    if args.gate:
        engine, scenario = args.gate
        records = load_jsonl(RESULTS / engine / f"{scenario}.jsonl")
        if not records:
            print(f"gate: {RESULTS / engine / f'{scenario}.jsonl'} 无结果", file=sys.stderr)
            sys.exit(2)
        cases = {c["id"]: c for c in load_jsonl(DATASETS / f"{scenario}.jsonl")}
        if scenario in RANK_SCENARIOS:
            metrics = eval_rank(records, cases)
        elif scenario == "permission_triage":
            metrics = eval_permission(records, cases)
        else:
            metrics = eval_noul(records, cases)
        violations = check_gate(scenario, metrics)
        print(f"gate[{engine}/{scenario}]: {json.dumps(metrics, ensure_ascii=False)}")
        if violations:
            for v in violations:
                print(f"  VIOLATION: {v}", file=sys.stderr)
            sys.exit(2)
        print("gate: PASS")
        sys.exit(0)

    engines = sorted(p.name for p in RESULTS.iterdir() if p.is_dir())
    if not engines:
        print("results/ 下无引擎结果", file=sys.stderr)
        sys.exit(1)

    cases_cache: dict[str, dict[str, dict]] = {}

    def cases_of(scenario: str) -> dict[str, dict]:
        if scenario not in cases_cache:
            cases_cache[scenario] = {c["id"]: c for c in load_jsonl(
                DATASETS / f"{scenario}.jsonl")}
        return cases_cache[scenario]

    summary: dict[str, dict[str, dict]] = {}
    for engine in engines:
        summary[engine] = {}
        for scenario in ("memory_rerank", "skill_match", "stop_verify",
                         "permission_triage"):
            records = load_jsonl(RESULTS / engine / f"{scenario}.jsonl")
            if not records:
                continue
            cases = cases_of(scenario)
            if scenario in RANK_SCENARIOS:
                summary[engine][scenario] = eval_rank(records, cases)
            elif scenario == "permission_triage":
                summary[engine][scenario] = eval_permission(records, cases)
            else:
                summary[engine][scenario] = eval_noul(records, cases)

    out_path = RESULTS / "summary.json"
    with out_path.open("w", encoding="utf-8") as fh:
        json.dump(summary, fh, ensure_ascii=False, indent=2)

    def fmt(v: float | None, pct: bool = True) -> str:
        if v is None:
            return "-"
        return f"{v * 100:.0f}%" if pct else f"{v:.2f}"

    def fmt_ms(v: float | None) -> str:
        return "-" if v is None else f"{v:.0f}"

    print(f"\n=== rank 场景（R@1 / MRR / flip / p50 / p95ms）===")
    print(f"{'engine':<10} {'场景':<16} {'R@1':>6} {'MRR':>6} {'flip':>6} {'p50':>7} {'p95':>7} {'err':>4}")
    for engine in engines:
        for scenario in RANK_SCENARIOS:
            m = summary[engine].get(scenario)
            if not m:
                continue
            print(f"{engine:<10} {scenario:<16} {fmt(m.get('R@1')):>6} "
                  f"{fmt(m.get('MRR')):>6} {fmt(m.get('order_flip_rate')):>6} "
                  f"{fmt_ms(m.get('latency_p50_ms')):>7} {fmt_ms(m.get('latency_p95_ms')):>7} "
                  f"{m.get('errors', 0):>4}")

    print(f"\n=== noul 场景（acc / F1 / ECE / Brier / flip / p50 / p95ms）===")
    print(f"{'engine':<10} {'场景':<18} {'acc':>6} {'F1':>6} {'ECE':>6} {'Brier':>6} {'flip':>6} {'p50':>7} {'p95':>7} {'err':>4}")
    for engine in engines:
        for scenario in ("stop_verify", "permission_triage"):
            m = summary[engine].get(scenario)
            if not m or "accuracy" not in m:
                continue
            extra = f" riskMAE={m['risk_mae']:.2f}" if m.get("risk_mae") is not None else ""
            print(f"{engine:<10} {scenario:<18} {fmt(m['accuracy']):>6} "
                  f"{fmt(m['f1']):>6} {fmt(m.get('ece')):>6} {fmt(m['brier']):>6} "
                  f"{fmt(m.get('order_flip_rate')):>6} "
                  f"{fmt_ms(m.get('latency_p50_ms')):>7} {fmt_ms(m.get('latency_p95_ms')):>7} "
                  f"{m.get('errors', 0):>4}{extra}")

    print(f"\nsummary -> {out_path}")


if __name__ == "__main__":
    main()
