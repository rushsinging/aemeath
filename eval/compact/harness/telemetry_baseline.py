#!/usr/bin/env python3
"""压缩遥测基线（#2133 P0）：从真实日志出压缩画像。

评测边界（MUST 如实呈现）：
- 本脚本只读 `context.log`，度量的是「发生过什么」——频率、耗时、分块形状、
  模型标识与降级结果，不评价摘要内容质量。
- 触发来源（自动 / 手动）日志未记录，只能按 `early_tokens` 与阈值推断，
  因此不输出「自动压缩占比」。
- 2026-10-28 之前的历史日志没有 `model=` 字段，按 `unknown` 归桶，不与
  新日志混算同一模型。

指标：
  total_compactions        配对成功的压缩次数
  per_day                  按日分布
  duration_ms              p50 / p95 / max（开始→完成墙钟）
  by_model                 按 `model=` 分桶的持续时间与次数
  chunk_shape              分块数、波次、单块耗时 p50/p95
  quality                  质量/降级分布
  early_tokens             p50 / p95（触发时输入规模）

用法：
  python3 harness/telemetry_baseline.py \
      --log ~/.agents/logs/context.log \
      --since 2026-10-01 \
      --output results/telemetry_baseline.json
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import re
import statistics
from pathlib import Path

EVAL_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_LOG = Path.home() / ".agents/logs/context.log"
DEFAULT_OUTPUT = EVAL_ROOT / "results" / "telemetry_baseline.json"

START_RE = re.compile(
    r"\[compact\] 开始：messages=(?P<messages>\d+) early=(?P<early>\d+) "
    r"early_tokens=(?P<early_tokens>\d+) previous_summary=(?P<previous>\d+) chars "
    r"mode=(?P<mode>\w+)(?: model=(?P<model>\S+))?"
)
MAP_REDUCE_RE = re.compile(
    r"\[compact\] map-reduce：(?P<chunks>\d+) chunks.*并发上限 (?P<concurrency>\d+)"
    r"(?:，波次 (?P<waves>\d+))?(?: model=(?P<model>\S+))?"
)
MAP_DONE_RE = re.compile(
    r"\[compact\] map 完成：total_chunks=(?P<total>\d+) successful_chunks=(?P<successful>\d+) "
    r"degraded_chunks=(?P<degraded>\d+)(?: waves=(?P<waves>\d+))?"
    r"(?: chunk_ms_p50=(?P<p50>\d+) chunk_ms_p95=(?P<p95>\d+) chunk_ms_max=(?P<max>\d+))?"
    r"(?: elapsed_ms=(?P<elapsed>\d+))?"
)
REDUCE_DONE_RE = re.compile(
    r"\[compact\] reduce 合并完成：\d+ chars（预算 \d+ tokens）"
    r"(?: facts=(?P<facts>\d+))?(?: elapsed_ms=(?P<elapsed>\d+))?"
)
REFRESH_RE = re.compile(
    r"\[compact\] 汇总超预算，再压 round (?P<round>\d+)："
    r"(?P<before>\d+) -> (?P<after>\d+) tokens.*elapsed_ms=(?P<elapsed>\d+)"
)
DONE_RE = re.compile(
    r"\[compact\] 完成：summary=(?P<summary>\d+) chars recent_messages=(?P<recent>\d+)"
    r"(?: quality=(?P<quality>\S+))?(?: model=(?P<model>\S+))?"
    r"(?: elapsed_ms=(?P<elapsed>\d+))?"
)


def percentile(values: list[float], quantile: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    index = min(len(ordered) - 1, max(0, int(quantile * len(ordered))))
    return ordered[index]


def parse_log(path: Path, since: str | None) -> tuple[list[dict], dict]:
    """按 session 把开始/完成事件配对成一次压缩记录。"""
    pending: dict[str, dict] = {}
    records: list[dict] = []
    seen: dict[str, int] = {}
    with path.open("r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                entry = json.loads(line)
            except json.JSONDecodeError:
                continue
            ts = entry.get("ts") or ""
            if since and ts < since:
                continue
            session = entry.get("session") or ""
            message = entry.get("msg") or ""

            if start := START_RE.search(message):
                record = {
                    "session": session,
                    "started_at": ts,
                    "messages": int(start.group("messages")),
                    "early": int(start.group("early")),
                    "early_tokens": int(start.group("early_tokens")),
                    "mode": start.group("mode"),
                    "model": start.group("model"),
                    "chunks": None,
                    "concurrency": None,
                    "waves": None,
                    "chunk_ms_p50": None,
                    "chunk_ms_p95": None,
                    "chunk_ms_max": None,
                    "map_ms": None,
                    "reduce_ms": None,
                    "refresh_rounds": 0,
                    "refresh_ms": 0,
                    "degraded_chunks": None,
                    "quality": None,
                    "elapsed_ms": None,
                }
                # 同 session 可能连续压缩：旧未配对记录直接丢弃（日志截断/崩溃）。
                pending[session] = record
                continue

            if session not in pending:
                continue
            record = pending[session]

            if match := MAP_REDUCE_RE.search(message):
                record["chunks"] = int(match.group("chunks"))
                record["concurrency"] = int(match.group("concurrency"))
                waves = match.group("waves")
                record["waves"] = int(waves) if waves else None
                record["model"] = record["model"] or match.group("model")
            elif match := MAP_DONE_RE.search(message):
                record["chunks"] = int(match.group("total"))
                record["degraded_chunks"] = int(match.group("degraded"))
                waves = match.group("waves")
                record["waves"] = int(waves) if waves else record["waves"]
                if match.group("p50"):
                    record["chunk_ms_p50"] = int(match.group("p50"))
                    record["chunk_ms_p95"] = int(match.group("p95"))
                    record["chunk_ms_max"] = int(match.group("max"))
                if match.group("elapsed"):
                    record["map_ms"] = int(match.group("elapsed"))
            elif match := REDUCE_DONE_RE.search(message):
                if match.group("elapsed"):
                    record["reduce_ms"] = int(match.group("elapsed"))
            elif match := REFRESH_RE.search(message):
                record["refresh_rounds"] += 1
                record["refresh_ms"] += int(match.group("elapsed"))
            elif match := DONE_RE.search(message):
                record["quality"] = match.group("quality")
                record["finished_at"] = ts
                elapsed = match.group("elapsed")
                record["elapsed_ms"] = int(elapsed) if elapsed else None
                record["summary_chars"] = int(match.group("summary"))
                record["recent_messages"] = int(match.group("recent"))
                record["model"] = record["model"] or match.group("model")
                pending.pop(session, None)
                records.append(record)
                seen[session] = seen.get(session, 0) + 1

    unmatched = len(pending)
    return records, {"unmatched_sessions": unmatched}


def resolve_duration_ms(record: dict) -> int | None:
    """优先用日志里的 elapsed_ms；历史日志缺失时按开始/完成时间戳推算。"""
    if record.get("elapsed_ms") is not None:
        return record["elapsed_ms"]
    started, finished = record.get("started_at"), record.get("finished_at")
    if not started or not finished:
        return None
    try:
        delta = dt.datetime.fromisoformat(finished) - dt.datetime.fromisoformat(started)
    except ValueError:
        return None
    return int(delta.total_seconds() * 1000)


def summarize(records: list[dict]) -> dict:
    durations = [value for record in records if (value := resolve_duration_ms(record)) is not None]
    early = [r["early_tokens"] for r in records]
    chunks = [r["chunks"] for r in records if r["chunks"] is not None]

    by_day: dict[str, int] = {}
    by_model: dict[str, dict] = {}
    quality: dict[str, int] = {}
    degraded_total = 0
    for record in records:
        day = (record["started_at"] or "")[:10]
        if day:
            by_day[day] = by_day.get(day, 0) + 1
        model = record["model"] or "unknown"
        bucket = by_model.setdefault(model, {"count": 0, "durations": [], "early_tokens": []})
        bucket["count"] += 1
        bucket["early_tokens"].append(record["early_tokens"])
        duration = resolve_duration_ms(record)
        if duration is not None:
            bucket["durations"].append(duration)
        label = (record["quality"] or "unknown").split("{")[0].strip()
        quality[label] = quality.get(label, 0) + 1
        if record["degraded_chunks"]:
            degraded_total += record["degraded_chunks"]

    model_summary = {}
    for model, bucket in sorted(by_model.items(), key=lambda item: -item[1]["count"]):
        model_summary[model] = {
            "count": bucket["count"],
            "duration_ms_p50": percentile(bucket["durations"], 0.50),
            "duration_ms_p95": percentile(bucket["durations"], 0.95),
            "duration_ms_max": max(bucket["durations"]) if bucket["durations"] else None,
            "early_tokens_p50": percentile(bucket["early_tokens"], 0.50),
            "early_tokens_p95": percentile(bucket["early_tokens"], 0.95),
        }

    return {
        "total_compactions": len(records),
        "per_day": dict(sorted(by_day.items())),
        "duration_ms": {
            "p50": percentile(durations, 0.50),
            "p95": percentile(durations, 0.95),
            "max": max(durations) if durations else None,
        },
        "early_tokens": {
            "p50": percentile(early, 0.50),
            "p95": percentile(early, 0.95),
        },
        "by_model": model_summary,
        "chunk_shape": {
            "compactions_with_map": len(chunks),
            "chunks_p50": percentile(chunks, 0.50),
            "chunks_p95": percentile(chunks, 0.95),
            "chunks_max": max(chunks) if chunks else None,
            "chunk_ms_p50_p50": statistics.median(
                [r["chunk_ms_p50"] for r in records if r["chunk_ms_p50"] is not None]
            )
            if any(r["chunk_ms_p50"] is not None for r in records)
            else None,
            "map_ms_p95": percentile(
                [r["map_ms"] for r in records if r["map_ms"] is not None], 0.95
            ),
            "reduce_ms_p95": percentile(
                [r["reduce_ms"] for r in records if r["reduce_ms"] is not None], 0.95
            ),
            "refresh_compactions": sum(1 for r in records if r["refresh_rounds"]),
            "refresh_ms_max": max((r["refresh_ms"] for r in records), default=0) or None,
        },
        "quality": quality,
        "degraded_chunks_total": degraded_total,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--log", type=Path, default=DEFAULT_LOG)
    parser.add_argument("--since", default=None, help="ISO 日期前缀，如 2026-10-01")
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    args = parser.parse_args()

    records, notes = parse_log(args.log, args.since)
    report = summarize(records)
    report["source"] = {
        "log": str(args.log),
        "since": args.since,
        "generated_at": dt.datetime.now().isoformat(timespec="seconds"),
    }
    report["notes"] = notes
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
