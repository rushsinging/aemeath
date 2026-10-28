#!/usr/bin/env python3
"""从真实 session dataset 导出压缩评测 case（#2133 P1）。

每个 case = 一次「压缩前输入」快照：
  messages.jsonl   按提交顺序摊平的消息（逐条 JSON Lines）
  case.json        元数据：规模、估算 token、previous_summary、must_keep

must_keep 来源分两类，写入时显式标注：
  auto_identifiers  从消息文本确定性抽取的 typed id / 文件路径
  declared          人工或模型补充声明（后续阶段追加，本脚本不改写）

评测边界：case 只是输入快照，不含压缩输出；指标由 run_eval / metrics 计算。

用法：
  python3 harness/export_cases.py --root ~/.agents/session --limit 8 \
      --output-dir /tmp/compact-cases
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

from compact_snapshot import (
    dataset_size_hint,
    estimate_tokens,
    flatten_messages,
    iter_datasets,
    load_session,
    message_text,
)
from metrics import rank_identifiers

DEFAULT_ROOT = Path.home() / ".agents/session"


def build_case(dataset: Path, session: dict, output_dir: Path, max_must_keep: int) -> dict | None:
    messages = flatten_messages(session)
    if len(messages) < 20:
        return None
    texts = [message_text(message) for message in messages]
    joined = "\n".join(texts)
    case_id = session["session_id"]
    case_dir = output_dir / case_id
    case_dir.mkdir(parents=True, exist_ok=True)

    with (case_dir / "messages.jsonl").open("w", encoding="utf-8") as handle:
        for message in messages:
            handle.write(json.dumps(message, ensure_ascii=False) + "\n")

    case = {
        "case_id": case_id,
        "session_id": case_id,
        "model": session.get("model"),
        "source_dataset": str(dataset),
        "committed_step_count": session["committed_step_count"],
        "message_count": len(messages),
        "estimated_tokens": estimate_tokens(joined),
        "previous_summary_chars": len(session.get("previous_summary") or ""),
        "must_keep": rank_identifiers(texts, max_must_keep),
        "must_keep_source": "auto_identifiers",
        "messages_path": f"{case_id}/messages.jsonl",
    }
    (case_dir / "case.json").write_text(
        json.dumps(case, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    return case


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--limit", type=int, default=8)
    parser.add_argument("--min-steps", type=int, default=40)
    parser.add_argument("--max-member-bytes", type=int, default=80_000_000)
    parser.add_argument("--max-must-keep", type=int, default=200)
    args = parser.parse_args()

    args.output_dir.mkdir(parents=True, exist_ok=True)
    index_path = args.output_dir / "cases.jsonl"
    exported = 0
    with index_path.open("w", encoding="utf-8") as index:
        for dataset in iter_datasets(args.root):
            if exported >= args.limit:
                break
            if dataset_size_hint(dataset) > args.max_member_bytes:
                continue
            try:
                session = load_session(dataset)
            except (KeyError, json.JSONDecodeError, OSError):
                continue
            if session["committed_step_count"] < args.min_steps:
                continue
            case = build_case(dataset, session, args.output_dir, args.max_must_keep)
            if case is None:
                continue
            index.write(json.dumps(case, ensure_ascii=False) + "\n")
            exported += 1
            print(f"[export] {case['case_id']} messages={case['message_count']} "
                  f"tokens≈{case['estimated_tokens']} must_keep={len(case['must_keep'])}")
    print(f"exported {exported} case(s) → {args.output_dir}")


if __name__ == "__main__":
    main()
