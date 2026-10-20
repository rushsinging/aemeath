#!/usr/bin/env python3
"""冻结反思历史 record 为质量评测数据集（#1999）。

来源：~/.agents/memory/*/reflection-history/*.jsonl（append-only，按 id 折叠
取最新）。输出统一数据集：每条含 trigger/status/token/duration、建议全集
（supersedes/synthesizes/outdated 引用）与 apply_result 实际计数；
新容量字段（active_after/capacity_target/stale_swept）在旧 record 上缺失，
如实记 null，不得伪造。

用法：python3 harness/build_reflection_dataset.py [--output PATH]
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

MEMORY_ROOT = Path.home() / ".agents" / "memory"


def load_latest_records() -> dict[str, dict]:
    """按 record id 折叠：append-only 流中同 id 取最后一行。"""
    latest: dict[str, dict] = {}
    for history_dir in sorted(MEMBER for MEMBER in MEMORY_ROOT.iterdir()
                              if MEMBER.is_dir()):
        for segment in sorted((history_dir / "reflection-history").glob("*.jsonl")):
            for line in segment.open(encoding="utf-8", errors="replace"):
                line = line.strip()
                if not line:
                    continue
                try:
                    record = json.loads(line)
                except json.JSONDecodeError:
                    continue
                record_id = record.get("id")
                if record_id:
                    latest[record_id] = record
    return latest


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output", type=Path, required=True)
    args = parser.parse_args()
    records = load_latest_records()
    cases = []
    for record_id in sorted(records):
        record = records[record_id]
        output = record.get("output") or {}
        suggestions = output.get("suggested_memories") or []
        cases.append({
            "id": record_id,
            "timestamp": record.get("timestamp"),
            "trigger": record.get("trigger"),
            "status": record.get("status"),
            "duration_ms": record.get("duration_ms"),
            "token_usage": record.get("token_usage"),
            "error_category": record.get("error_category"),
            "suggestions": [
                {
                    "supersedes": suggestion.get("supersedes") or [],
                    "synthesizes": suggestion.get("synthesizes") or [],
                    "layer": suggestion.get("layer"),
                    "category": suggestion.get("category"),
                    "content_chars": len(suggestion.get("content") or ""),
                }
                for suggestion in suggestions
            ],
            "outdated_memories": output.get("outdated_memories") or [],
            "apply_result": record.get("apply_result"),
            # 新可观测字段：旧 record 缺失记 null。
            "active_after": (record.get("apply_result") or {}).get("active_after"),
            "capacity_target": (record.get("apply_result") or {}).get("capacity_target"),
            "stale_swept": record.get("stale_swept"),
        })
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w", encoding="utf-8") as stream:
        for case in cases:
            stream.write(json.dumps(case, ensure_ascii=False) + "\n")
    print(f"records folded: {len(cases)} -> {args.output}")


if __name__ == "__main__":
    main()
