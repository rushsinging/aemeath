#!/usr/bin/env python3
"""压缩评测的确定性指标（#2133 P1）。

评测边界（MUST 如实呈现）：
- 这里的 recall 是「可机械识别事实是否仍在 summary/tail 中」的覆盖率代理，
  不是内容正确性或续接可用性——语义等价、删繁就简但保留信息等判断需要
  真模型续接问答（P3）与人工 gold。
- must_keep 只收录两类可确定性抽取/声明的条目：typed id 与文件路径
  （自动抽取），以及 case 显式声明的关键字（人工/模型生成后冻结进数据集）。
- 指标不读压缩实现内部状态，只吃「压缩前输入 + 压缩后 summary + tail」。

对外函数：
  extract_identifiers(messages)     从消息文本抽取 typed id 与文件路径
  must_keep_recall(must_keep, text) 覆盖率与缺失清单
  compression_ratio(early_tokens, summary_tokens)
  summarize_cases(cases)            汇总为报告（分母只算可评估 case）
"""
from __future__ import annotations

import re

# typed id：run_/stp_/mem_/cht_/bgp_/irq_/ws_ 等前缀 + 足够长的随机段。
ID_PATTERN = re.compile(r"\b(?:run|stp|mem|cht|bgp|irq|ws|sess)_[A-Za-z0-9]{6,}\b")
# 文件路径：带已知扩展名，允许相对路径与行号后缀。
PATH_PATTERN = re.compile(
    r"(?:[\w.@-]+/)*[\w.@-]+\.(?:rs|py|md|json|jsonl|toml|yaml|yml|sh|ts|tsx|js|html|css)"
    r"(?::\d+)?"
)
# 噪声路径：评测脚本自身常出现，不代表上下文事实。
PATH_NOISE_PREFIXES = ("eval/compact/", "results/", "datasets/")


def extract_identifiers(texts: list[str]) -> list[str]:
    """从消息文本抽取 must_keep 候选：typed id 与文件路径（去重保序）。"""
    seen: dict[str, None] = {}
    for text in texts:
        for match in ID_PATTERN.finditer(text):
            seen.setdefault(match.group(0), None)
        for match in PATH_PATTERN.finditer(text):
            path = match.group(0)
            if len(path) < 5 or path.startswith(PATH_NOISE_PREFIXES):
                continue
            seen.setdefault(path, None)
    return list(seen)


def rank_identifiers(texts: list[str], limit: int = 200) -> list[str]:
    """按出现频次挑选 must_keep：高频条目更可能承载任务事实，低频多为噪声。"""
    counts: dict[str, int] = {}
    for item in extract_identifiers(texts):
        counts[item] = 0
    for text in texts:
        for match in ID_PATTERN.finditer(text):
            counts[match.group(0)] = counts.get(match.group(0), 0) + 1
        for match in PATH_PATTERN.finditer(text):
            path = match.group(0)
            if path in counts:
                counts[path] += 1
    ordered = sorted(counts.items(), key=lambda item: (-item[1], item[0]))
    return [item for item, _ in ordered[:limit]]


def must_keep_recall(must_keep: list[str], text: str) -> dict:
    """覆盖率：must_keep 中仍在 text 里出现的比例。空集合不算 0 分。"""
    if not must_keep:
        return {"total": 0, "hit": 0, "missed": [], "recall": None}
    missed = [item for item in must_keep if item not in text]
    hit = len(must_keep) - len(missed)
    return {
        "total": len(must_keep),
        "hit": hit,
        "missed": missed,
        "recall": hit / len(must_keep),
    }


def compression_ratio(early_tokens: int, summary_tokens: int) -> float | None:
    """摘要 token / 早期 token；输入为 0 时返回 None（不是 0 分）。"""
    if early_tokens <= 0:
        return None
    return summary_tokens / early_tokens


def structure_health(result: dict) -> dict:
    """结构健康：质量档位、降级块数、修复与重试次数。"""
    quality = result.get("quality") or "unknown"
    return {
        "quality": quality,
        "degraded_chunks": int(result.get("degraded_chunks") or 0),
        "repair_attempts": int(result.get("repair_attempts") or 0),
        "empty_retries": int(result.get("empty_retries") or 0),
        "failed_retries": int(result.get("failed_retries") or 0),
        "degraded": quality.startswith(("PartialMapFallback", "LocalFallback"))
        or int(result.get("degraded_chunks") or 0) > 0,
    }


def summarize_cases(cases: list[dict]) -> dict:
    """汇总确定性指标；不可评估的 case 进 skipped，不污染分母。"""
    evaluated = 0
    skipped = 0
    recalls: list[float] = []
    ratios: list[float] = []
    degraded = 0
    repair_total = 0
    quality_buckets: dict[str, int] = {}

    for case in cases:
        recall = case.get("recall")
        ratio = case.get("compression_ratio")
        if recall is None and ratio is None:
            skipped += 1
            continue
        evaluated += 1
        if recall is not None:
            recalls.append(recall)
        if ratio is not None:
            ratios.append(ratio)
        health = structure_health(case.get("structure_health") or {})
        if health["degraded"]:
            degraded += 1
        repair_total += health["repair_attempts"]
        quality = health["quality"]
        quality_buckets[quality] = quality_buckets.get(quality, 0) + 1

    def average(values: list[float]) -> float | None:
        return sum(values) / len(values) if values else None

    return {
        "total": len(cases),
        "evaluated": evaluated,
        "skipped": skipped,
        "must_keep_recall_mean": average(recalls),
        "compression_ratio_mean": average(ratios),
        "degraded_cases": degraded,
        "repair_attempts_total": repair_total,
        "quality": quality_buckets,
    }
