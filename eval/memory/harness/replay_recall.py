#!/usr/bin/env python3
"""通道 B（per-message recall → 门控 → 注入）重放评测（#1998）。

口径：数据集内闭环——#1997 的 memory_recall_real.jsonl 自带当时候选全文
（生产词法 top-N 结果）与 gold，重放不依赖桶快照漂移；当前桶仅用于
标注候选的 stale 状态（outdated / superseded / drifted）。

重放链路（对齐生产 recall.rs / reminder_sources.rs）：
  消息 → BM25 词法序（lexical_baseline 同口径）→ [kev 在线] Choice 概率
  → 门控 0.5 → 注入决策四象限。

指标：
  lexical_top1_hit_rate  词法 top1 命中 gold 比例（有效 case；
                         与 #1997 kev R@1 对比回答「重排是否优于词法」）
  stale_in_candidates    候选中 outdated+superseded 条目总数（污染量化）
  stale_at_lexical_top1  词法 top1 是 stale 条目的 case 数
  gate_quadrant          correct_inject / wrong_inject / miss_inject /
                         correct_reject（kev 在线时）
kev 离线时跳过四象限，其余指标照常输出。

用法：python3 harness/replay_recall.py [--kev-url http://127.0.0.1:8009]
输出：results/replay_recall.json + stdout 摘要
"""
from __future__ import annotations

import argparse
import json
import pathlib
import sys
import urllib.request

HERE = pathlib.Path(__file__).resolve().parent
EVAL_ROOT = HERE.parent
sys.path.insert(0, str(EVAL_ROOT.parent / "system-one" / "harness"))
import lexical_baseline as lb  # noqa: E402

DATASET = (EVAL_ROOT.parent / "system-one" / "datasets"
           / "memory_recall_real.jsonl")
BUCKET_GLOB = "v2_2a2bdf23*"
MEMORY_ROOT = pathlib.Path.home() / ".agents" / "memory"
RESULTS_DIR = EVAL_ROOT / "results"
GATE_THRESHOLD = 0.5    # 生产 MEMORY_RECALL_THRESHOLD
GOLD_PREFIX_CHARS = 100  # 候选与当前桶条目的前缀对齐长度（容忍 reflect 改写尾部）
RERANK_INSTRUCTIONS = ("Given a user message from a coding-agent session, "
                       "retrieve the most relevant memory.")
NOW_TS = 1_792_200_000  # 固定评测时钟（recency tie-break 确定性）
KEV_TIMEOUT_SECS = 60


def load_active_entries() -> list[dict]:
    """读取 aemeath 项目桶最新 active member 的条目。"""
    bucket = sorted(MEMORY_ROOT.glob(BUCKET_GLOB))
    if not bucket:
        sys.exit(f"未找到记忆桶 {MEMORY_ROOT}/{BUCKET_GLOB}")
    active_files = [p for p in (bucket[-1] / "members").iterdir()
                    if json.loads(p.read_text()).get("location") == "active"]
    latest = max(active_files, key=lambda p: p.stat().st_mtime)
    return json.loads(latest.read_text())["entries"]


def make_stale_state(entries: list[dict]):
    """返回 stale_state(text)：候选在当前桶的 stale 状态。

    drifted = 前缀不可寻（多因 reflect 合并改写或跨桶候选）。
    """
    entries_by_prefix = {entry["content"][:GOLD_PREFIX_CHARS]: entry
                         for entry in entries}

    def stale_state(answer_text: str) -> str:
        matched = entries_by_prefix.get(answer_text[:GOLD_PREFIX_CHARS])
        if matched is None:
            return "drifted"
        if matched.get("superseded_by"):
            return "superseded"
        if matched.get("outdated"):
            return "outdated"
        return "eligible"

    return stale_state


def kev_choice_probabilities(query: str, candidate_texts: list[str],
                             kev_url: str) -> list[float] | None:
    """对候选发生产同构 Choice 请求，返回与候选对齐的概率列表。

    请求格式对齐 harness http_choice：questions 为 dict（q → question），
    criteria 为 {key: text} map；响应 answers.q.probabilities 与 key 序对齐。
    """
    payload = {
        "state": query,
        "questions": {"q": {
            "type": "choice",
            "instructions": RERANK_INSTRUCTIONS,
            "criteria": {str(i): text[:500]
                         for i, text in enumerate(candidate_texts)},
        }},
        "model": "kev-latest",
    }
    request = urllib.request.Request(
        f"{kev_url}/v1/systemone",
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=KEV_TIMEOUT_SECS) as response:
            body = json.loads(response.read())
    except Exception:
        return None
    answer = (body.get("answers") or {}).get("q") or {}
    raw = answer.get("probabilities")
    if isinstance(raw, dict):
        try:
            probabilities = [raw[str(i)] for i in range(len(raw))]
        except KeyError:
            return None
    else:
        probabilities = raw or []
    if len(probabilities) != len(candidate_texts):
        return None
    return probabilities


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--kev-url", default="http://127.0.0.1:8009")
    args = parser.parse_args()

    cases = [json.loads(line) for line in DATASET.open(encoding="utf-8")]
    entries = load_active_entries()
    stale_free = [e for e in entries if not e.get("outdated")
                  and not e.get("superseded_by")]
    stale_state = make_stale_state(entries)
    kev_online = (len(entries) >= 2 and
                  kev_choice_probabilities("ping",
                                           [e["content"] for e in entries[:2]],
                                           args.kev_url) is not None)

    report = {"bucket": {"total_active": len(entries),
                         "stale_free": len(stale_free),
                         "outdated_or_superseded": len(entries) - len(stale_free)},
              "kev_online": kev_online, "cases": []}
    counters = {"effective": 0, "lex_top1": 0,
                "stale_in_candidates": 0, "stale_top1": 0}
    quadrant = {"correct_inject": 0, "wrong_inject": 0,
                "miss_inject": 0, "correct_reject": 0}

    for case in cases:
        candidates = case["answers"]
        gold = case["gold"]
        fake_entries = [{"content": text, "tags": [], "pinned": False,
                         "last_confirmed_at": 0} for text in candidates]
        lexical_order = lb.rank(fake_entries, case["context"], NOW_TS,
                                len(candidates))
        lex_top1_is_gold = (gold != -1 and bool(lexical_order)
                            and lexical_order[0] == gold)
        states = [stale_state(text) for text in candidates]
        lex_top1_state = states[lexical_order[0]] if lexical_order else None

        row = {"id": case["id"], "gold": gold,
               "candidates": len(candidates),
               "gold_status": "no_relevant" if gold == -1 else "in_dataset",
               "lexical_top1_is_gold": lex_top1_is_gold,
               "candidate_states": {state: states.count(state)
                                    for state in sorted(set(states))},
               "lexical_top1_state": lex_top1_state}
        counters["effective"] += gold != -1
        counters["lex_top1"] += lex_top1_is_gold
        counters["stale_in_candidates"] += sum(
            1 for state in states if state in ("outdated", "superseded"))
        counters["stale_top1"] += lex_top1_state in ("outdated", "superseded")

        if kev_online and len(candidates) >= 2:
            probabilities = kev_choice_probabilities(
                case["context"], candidates, args.kev_url)
            if probabilities:
                top1 = max(range(len(probabilities)),
                           key=lambda i: probabilities[i])
                top1_p = probabilities[top1]
                top1_is_gold = gold != -1 and top1 == gold
                passed = top1_p >= GATE_THRESHOLD
                if gold == -1:
                    quadrant["correct_reject" if not passed else "wrong_inject"] += 1
                elif passed and top1_is_gold:
                    quadrant["correct_inject"] += 1
                elif passed and not top1_is_gold:
                    quadrant["wrong_inject"] += 1
                else:
                    quadrant["miss_inject"] += 1
                row["kev"] = {"top1_is_gold": top1_is_gold,
                              "top1_p": round(top1_p, 4),
                              "top1_state": states[top1]}
        report["cases"].append(row)

    effective = counters["effective"] or 1
    report["metrics"] = {
        "effective_cases": counters["effective"],
        "lexical_top1_hit_rate": counters["lex_top1"] / effective,
        "stale_in_candidates_total": counters["stale_in_candidates"],
        "stale_at_lexical_top1": counters["stale_top1"],
        "gate_quadrant": quadrant,
    }
    RESULTS_DIR.mkdir(parents=True, exist_ok=True)
    out_path = RESULTS_DIR / "replay_recall.json"
    out_path.write_text(json.dumps(report, ensure_ascii=False, indent=2))

    print(f"bucket: active={report['bucket']['total_active']} "
          f"stale_free={report['bucket']['stale_free']} "
          f"outdated_or_superseded={report['bucket']['outdated_or_superseded']}")
    print(f"effective={counters['effective']} "
          f"lexical_top1_hit={report['metrics']['lexical_top1_hit_rate']:.3f}")
    print(f"stale_in_candidates={counters['stale_in_candidates']} "
          f"stale_at_lexical_top1={counters['stale_top1']}")
    if kev_online:
        print(f"quadrant: {quadrant}")
    else:
        print("kev offline: gate quadrant skipped")
    print(f"-> {out_path}")


if __name__ == "__main__":
    main()
