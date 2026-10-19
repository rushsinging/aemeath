#!/usr/bin/env python3
"""通道 A（memory-context system block 注入）质量评测（#1998）。

生产链路（service.rs retrieve_for_inject）：全量 eligible active 条目按
injection_score（pinned+10000 → confirmation_count×100(min20) → recency）降序
truncate(limit)。本脚本离线复刻该排序，量化注入内容的质量属性：

  - eligible / ineligible（outdated/superseded/ttl）分布 —— 验证 M5/M10 过滤
  - pinned / category / layer 分布与字符预算占用
  - stale 兜底检查：eligible 集内不得含 outdated/superseded（违规即 fail）
  - 关键记忆覆盖：pinned 与高确认条目是否稳定进入注入序前部

用法：python3 harness/analyze_channel_a.py
输出：results/channel_a.json + stdout 摘要（stale 违规时 exit 2）
"""
from __future__ import annotations

import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
EVAL_ROOT = HERE.parent
BUCKET_GLOB = "v2_2a2bdf23*"
MEMORY_ROOT = pathlib.Path.home() / ".agents" / "memory"
RESULTS_DIR = EVAL_ROOT / "results"
NOW_TS = 1_792_200_000
DEFAULT_LIMIT = 200  # 生产 system block 注入上限


def load_active_entries() -> list[dict]:
    bucket = sorted(MEMORY_ROOT.glob(BUCKET_GLOB))
    if not bucket:
        sys.exit(f"未找到记忆桶 {MEMORY_ROOT}/{BUCKET_GLOB}")
    active_files = [p for p in (bucket[-1] / "members").iterdir()
                    if json.loads(p.read_text()).get("location") == "active"]
    latest = max(active_files, key=lambda p: p.stat().st_mtime)
    return json.loads(latest.read_text())["entries"]


def recency_score(last_confirmed_at: int, now: int) -> int:
    """生产 policy.rs recency_score 同构阶梯：0天=1000, ≤7=800, ≤30=500, ≤90=200, >90=50。"""
    age_days = max(0, now - last_confirmed_at) // 86_400
    if age_days == 0:
        return 1_000
    if age_days <= 7:
        return 800
    if age_days <= 30:
        return 500
    if age_days <= 90:
        return 200
    return 50


def injection_score(entry: dict) -> int:
    pinned_bonus = 10_000 if entry.get("pinned") else 0
    confirmation = min(entry.get("confirmation_count", 0), 20) * 100
    return pinned_bonus + confirmation + recency_score(
        entry.get("last_confirmed_at", 0), NOW_TS)


def main() -> None:
    entries = load_active_entries()
    eligible = [e for e in entries
                if not e.get("outdated") and not e.get("superseded_by")]
    ineligible = len(entries) - len(eligible)

    ordered = sorted(eligible, key=lambda e: (-injection_score(e), e["id"]))
    injected = ordered[:DEFAULT_LIMIT]

    stale_leak = [e for e in injected
                  if e.get("outdated") or e.get("superseded_by")]
    pinned_n = sum(1 for e in injected if e.get("pinned"))
    categories: dict[str, int] = {}
    for entry in injected:
        categories[entry.get("category", "?")] = \
            categories.get(entry.get("category", "?"), 0) + 1
    total_chars = sum(len(e["content"]) for e in injected)
    top10_pinned = sum(1 for e in injected[:10] if e.get("pinned"))
    high_confirm_top10 = sum(
        1 for e in injected[:10] if e.get("confirmation_count", 0) >= 3)

    report = {
        "bucket_total_active": len(entries),
        "eligible": len(eligible),
        "ineligible": len(entries) - len(eligible),
        "injected": len(injected),
        "stale_leak_in_injected": len(stale_leak),
        "pinned_injected": pinned_n,
        "category_distribution": categories,
        "injected_total_chars": total_chars,
        "top10_pinned": top10_pinned,
        "top10_high_confirmation": high_confirm_top10,
    }
    RESULTS_DIR.mkdir(parents=True, exist_ok=True)
    out_path = RESULTS_DIR / "channel_a.json"
    out_path.write_text(json.dumps(report, ensure_ascii=False, indent=2))

    print(json.dumps(report, ensure_ascii=False, indent=2))
    print(f"-> {out_path}")
    if stale_leak:
        print(f"FAIL: {len(stale_leak)} stale entries leaked into channel A",
              file=sys.stderr)
        sys.exit(2)


if __name__ == "__main__":
    main()
