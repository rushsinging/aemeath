#!/usr/bin/env python3
"""候选集级 Memory recall 影子回放，不冒充全 corpus 端到端评测。

BM25 只在原事件候选上近似重算，无法据此测 corpus Recall@20；gold=-1
只表示原候选集无相关项。过滤后候选必须重新评分，不复用旧 softmax。
完整 Rust runtime 的双次评分、reminder pipeline 和模型收益由独立验证承担。
"""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
import math
from pathlib import Path
import sys
import urllib.error
import urllib.request

from memory_snapshot import is_injection_eligible, load_snapshot

EVAL_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(EVAL_ROOT.parent / "system-one" / "harness"))
import lexical_baseline as lexical  # noqa: E402

GATE_THRESHOLD = 0.5
PREVIEW_CHARS = 200
PREVIEW_BUDGET_CHARS = 1200
RECALL_TOP_K = 3
SCORING_TIMEOUT_SECONDS = 60
DEFAULT_DATASET = EVAL_ROOT.parent / "system-one/datasets/memory_recall_real.jsonl"


def classify_gate(gold: int, probabilities: list[float]) -> str:
    if not probabilities:
        return "correct_rejection" if gold == -1 else "missed_selection"
    top_index = max(range(len(probabilities)), key=probabilities.__getitem__)
    if probabilities[top_index] < GATE_THRESHOLD:
        return "correct_rejection" if gold == -1 else "missed_selection"
    if gold == -1:
        return "false_selection"
    return "correct_selection" if top_index == gold else "wrong_selection"


def simulate_preview(candidates: list[str], identities: list[str], probabilities: list[float]) -> dict:
    selected = []
    budget_used = 0
    if probabilities and max(probabilities) >= GATE_THRESHOLD:
        for index in sorted(range(len(probabilities)), key=lambda value: -probabilities[value])[:RECALL_TOP_K]:
            preview_chars = len(candidates[index][:PREVIEW_CHARS])
            estimated_chars = preview_chars + len(identities[index]) + 4
            if budget_used + estimated_chars > PREVIEW_BUDGET_CHARS:
                break
            budget_used += estimated_chars
            selected.append(index)
    return {"selected_indexes": selected, "selected_count": len(selected),
            "selected_content_chars": sum(len(candidates[index][:PREVIEW_CHARS]) for index in selected),
            "budget_accounting_chars": budget_used,
            "final_reminder_token_cost": None}


def score_candidates(context: str, candidates: list[str], instructions: str, url: str) -> list[float]:
    if not candidates:
        return []
    if len(candidates) == 1:
        state = f"{context}\n\n候选记忆：{candidates[0]}"
        question = {"type": "noul", "instructions": "Is the candidate memory entry      relevant to the user's current message and worth recalling into context?"}
    else:
        state = context
        question = {"type": "choice", "instructions": instructions,
                    "criteria": {str(index): text[:500] for index, text in enumerate(candidates)}}
    payload = {"state": state, "questions": {"q": question}, "model": "kev-latest"}
    request = urllib.request.Request(f"{url.rstrip('/')}/v1/systemone", data=json.dumps(payload).encode(),
                                     headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=SCORING_TIMEOUT_SECONDS) as response:
        answer = json.loads(response.read())["answers"]["q"]
    raw_probabilities = answer.get("probabilities")
    if len(candidates) == 1:
        probabilities = [answer["p_true"]]
    elif isinstance(raw_probabilities, dict):
        probabilities = [raw_probabilities[str(index)] for index in range(len(candidates))]
    else:
        probabilities = raw_probabilities
    if not isinstance(probabilities, list) or len(probabilities) != len(candidates):
        raise ValueError("probability count mismatch")
    if any(not isinstance(value, (int, float)) or isinstance(value, bool)
           or not math.isfinite(value) or not 0 <= value <= 1 for value in probabilities):
        raise ValueError("invalid probabilities")
    return probabilities


def match_candidates(candidates: list[str], entries: list[dict], now: int) -> list[dict]:
    matches = []
    for text in candidates:
        # 完整候选才可精确对齐；达到生产 500 字上限时保留前缀匹配的不确定性。
        matched = [entry for entry in entries if entry["content"] == text
                   or (len(text) == 500 and entry["content"].startswith(text))]
        if len(matched) != 1:
            matches.append({"state": "ambiguous" if matched else "unknown", "entry": None})
        else:
            entry = matched[0]
            matches.append({"state": "eligible" if is_injection_eligible(entry, now) else "ineligible",
                            "entry": entry})
    return matches


def summarize_cases(rows: list[dict]) -> dict:
    scored = [row for row in rows if row["status"] == "scored"]
    outcomes = Counter(row["gate_outcome"] for row in scored)
    return {"total_cases": len(rows), "scored_cases": len(scored),
            "scoring_errors": sum(row["status"] == "scoring_error" for row in rows),
            "status_counts": dict(Counter(row["status"] for row in rows)),
            "gate_outcomes": dict(sorted(outcomes.items())),
            "selected_cases": sum(row["preview"]["selected_count"] > 0 for row in scored),
            "selected_content_chars": sum(row["preview"]["selected_content_chars"] for row in scored),
            "corpus_recall_at_20": None, "full_injection_precision": None,
            "downstream_task_gain": None,
            "not_measured": ["corpus-level recall", "all selected memories relevance", "downstream task success"]}


def replay(cases: list[dict], snapshot: dict, labels: dict, url: str | None, responses: dict | None = None) -> dict:
    rows_by_phase = {"before_filter": [], "after_filter": []}
    now = snapshot["captured_at"]
    comparison_scope = Counter()
    for case in cases:
        candidates = case["answers"]
        matched = match_candidates(candidates, snapshot["entries"], now)
        states = [match["state"] for match in matched]
        known_stale = sum(state == "ineligible" for state in states)
        unknown = any(state in ("unknown", "ambiguous") for state in states)
        surviving = [index for index, state in enumerate(states) if state != "ineligible"]
        gold_removed = case["gold"] >= 0 and case["gold"] not in surviving
        comparison_scope["unknown_candidates_cases"] += unknown
        comparison_scope["gold_ineligible_cases"] += gold_removed
        for phase in rows_by_phase:
            indexes = list(range(len(candidates))) if phase == "before_filter" else surviving
            texts = [candidates[index] for index in indexes]
            source_entries = [matched[index]["entry"] for index in indexes]
            identities = [entry["id"] if entry else f"unknown-{index}" for index, entry in zip(indexes, source_entries)]
            metadata_complete = all(entry is not None for entry in source_entries)
            lexical_entries = source_entries if metadata_complete else [
                {"id": identity, "content": text, "tags": [], "pinned": False, "last_confirmed_at": 0}
                for identity, text in zip(identities, texts)]
            lexical_order = lexical.rank(lexical_entries, case["context"], now, len(texts))
            mapped_gold = indexes.index(case["gold"]) if case["gold"] in indexes else -1
            row = {"id": case["id"], "gold": case["gold"], "phase_gold": mapped_gold,
                   "confidence": labels.get(case["id"], {}).get("confidence", "unreviewed"),
                   "candidate_count": len(texts), "known_ineligible_candidates": known_stale,
                   "candidate_status": dict(Counter(states)),
                   "lexical_mode": "metadata_complete_subset" if metadata_complete else "text_only_subset_approximation",
                   "lexical_top1_matches_gold": bool(lexical_order) and mapped_gold >= 0 and lexical_order[0] == mapped_gold}
            if phase == "after_filter" and (unknown or gold_removed):
                row["status"] = "unresolved_snapshot_alignment" if unknown else "gold_ineligible_needs_relabel"
            elif responses is None and url is None:
                row["status"] = "scoring_skipped"
            else:
                try:
                    probabilities = (responses[phase][case["id"]] if responses is not None
                                     else score_candidates(case["context"], texts, case["question"], url))
                    if len(probabilities) != len(texts):
                        raise ValueError("cached probabilities do not match candidate set")
                    row.update(status="scored", probabilities=probabilities,
                               gate_outcome=classify_gate(mapped_gold, probabilities),
                               preview=simulate_preview(texts, identities, probabilities))
                    row["selected_known_ineligible"] = sum(
                        states[indexes[index]] == "ineligible" for index in row["preview"]["selected_indexes"])
                except (urllib.error.URLError, TimeoutError, ValueError, KeyError, TypeError, OSError) as error:
                    row.update(status="scoring_error", failure=f"{type(error).__name__}: {error}")
            rows_by_phase[phase].append(row)
    return {"scope": "candidate-set shadow replay; not production end-to-end", "snapshot_captured_at": now,
            "labels": dict(Counter(label.get("confidence", "unknown") for label in labels.values())),
            "comparison_scope": dict(comparison_scope),
            "scoring_backend": "cached HTTP kev results" if responses is not None else ("HTTP kev (not UDS)" if url else "disabled"),
            "phases": {phase: {"summary": summarize_cases(rows), "cases": rows} for phase, rows in rows_by_phase.items()}}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--snapshot", type=Path, required=True)
    parser.add_argument("--dataset", type=Path, default=DEFAULT_DATASET)
    parser.add_argument("--labels", type=Path, default=EVAL_ROOT.parent / "system-one/results/annotation/memory_recall_real_labels.jsonl")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--kev-url", help="显式启用 HTTP 评分；失败记 error，不当离线静默吞掉")
    parser.add_argument("--responses", type=Path, help="从同输入的已保存报告读取原始概率，离线重放")
    args = parser.parse_args()
    if args.kev_url and args.responses:
        parser.error("--kev-url 与 --responses 互斥")
    snapshot = load_snapshot(args.snapshot)
    cases = [json.loads(line) for line in args.dataset.read_text(encoding="utf-8").splitlines() if line.strip()]
    if any(case["gold"] < -1 or case["gold"] >= len(case["answers"]) for case in cases):
        raise ValueError("invalid or unannotated gold")
    labels = {label["id"]: label for label in map(json.loads, args.labels.read_text(encoding="utf-8").splitlines())}
    hashes = {"snapshot": hashlib.sha256(args.snapshot.read_bytes()).hexdigest(),
              "dataset": hashlib.sha256(args.dataset.read_bytes()).hexdigest(),
              "labels": hashlib.sha256(args.labels.read_bytes()).hexdigest()}
    responses = None
    if args.responses:
        saved = json.loads(args.responses.read_text(encoding="utf-8"))
        if saved.get("input_sha256") != hashes:
            raise ValueError("cached input fingerprints mismatch")
        responses = {phase: {row["id"]: row["probabilities"] for row in data["cases"] if row["status"] == "scored"}
                     for phase, data in saved["phases"].items()}
    report = replay(cases, snapshot, labels, args.kev_url, responses)
    report["input_sha256"] = hashes
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    for phase, data in report["phases"].items():
        print(phase, json.dumps(data["summary"], ensure_ascii=False))
    if any(data["summary"]["scoring_errors"] for data in report["phases"].values()):
        raise SystemExit(2)


if __name__ == "__main__":
    main()
