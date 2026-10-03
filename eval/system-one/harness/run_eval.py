#!/usr/bin/env python3
"""System One 候选引擎统一评测 harness（#1751 阶段一）。

用法：
    python3 harness/run_eval.py --engine <name> [--scenario <name>] [--repeat N]

引擎（端点见 ENGINES）：
    clm / jevos / kev / rsi-jev / laya   —— HTTP /v1/systemone（+ clm 有 /v1/rank）
    semif                                —— CLI semif-score（JSONL，仅 choice 形态）
    anyjev                               —— Python 库内调用（HFBackend）

规则：
- 每个 case 跑两遍：候选正序 + 反序（order-flip 检测）。
- 每引擎先 2 条 warmup 不计入结果。
- 原始结果落盘 results/<engine>/<scenario>.jsonl，含完整响应与延迟。
- rank 场景：引擎无 /v1/rank 时降级为 choice 题型（候选 key→原文），取 probabilities。
"""
from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import sys
import tempfile
import time
import urllib.request
import urllib.error

HERE = pathlib.Path(__file__).resolve().parent
EVAL_ROOT = HERE.parent
DATASETS = EVAL_ROOT / "datasets"
RESULTS = EVAL_ROOT / "results"
RUNTIME = EVAL_ROOT / "runtime"

ENGINES = {
    "clm": {"kind": "http", "systemone": "http://127.0.0.1:8700/v1/systemone",
            "rank": "http://127.0.0.1:8700/v1/rank", "model": "clm-latest"},
    "jevos": {"kind": "http", "systemone": "http://127.0.0.1:8017/v1/systemone",
              "rank": None, "model": "jev-latest"},
    "kev": {"kind": "http", "systemone": "http://127.0.0.1:8009/v1/systemone",
            "rank": None, "model": "kev-latest"},
    "rsi-jev": {"kind": "http", "systemone": "http://127.0.0.1:8200/v1/systemone",
                "rank": None, "model": "jev-latest"},
    "laya": {"kind": "http", "systemone": "http://127.0.0.1:8000/v1/systemone",
             "rank": None, "model": None},  # laya 不要求 model 字段
    "semif": {"kind": "semif-cli"},
    "anyjev": {"kind": "anyjev-lib"},
}

HTTP_TIMEOUT = 120


# ---------------------------------------------------------------------------
# HTTP 引擎（Jev 兼容线格式）
# ---------------------------------------------------------------------------

def http_post(url: str, payload: dict) -> tuple[dict, float]:
    body = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(url, data=body,
                                 headers={"Content-Type": "application/json"})
    start = time.monotonic()
    with urllib.request.urlopen(req, timeout=HTTP_TIMEOUT) as resp:
        data = json.loads(resp.read().decode("utf-8"))
    return data, (time.monotonic() - start) * 1000.0


def http_noul(engine_cfg: dict, state: str, instructions: str) -> dict:
    payload = {"state": state,
               "questions": {"q": {"type": "noul", "instructions": instructions}}}
    if engine_cfg.get("model"):
        payload["model"] = engine_cfg["model"]
    data, latency = http_post(engine_cfg["systemone"], payload)
    ans = data["answers"]["q"]
    return {"p_true": ans.get("noul"), "raw": data, "latency_ms": latency}


def http_choice(engine_cfg: dict, state: str, instructions: str,
                options: dict[str, str]) -> dict:
    payload = {"state": state,
               "questions": {"q": {"type": "choice", "instructions": instructions,
                                   "criteria": options}}}
    if engine_cfg.get("model"):
        payload["model"] = engine_cfg["model"]
    data, latency = http_post(engine_cfg["systemone"], payload)
    ans = data["answers"]["q"]
    return {"choice": ans.get("choice"), "probabilities": ans.get("probabilities"),
            "raw": data, "latency_ms": latency}


def http_score(engine_cfg: dict, state: str, instructions: str,
               levels: list[str]) -> dict:
    payload = {"state": state,
               "questions": {"q": {"type": "score", "instructions": instructions,
                                   "criteria": levels}}}
    if engine_cfg.get("model"):
        payload["model"] = engine_cfg["model"]
    data, latency = http_post(engine_cfg["systemone"], payload)
    ans = data["answers"]["q"]
    return {"score": ans.get("score"), "probabilities": ans.get("probabilities"),
            "raw": data, "latency_ms": latency}


def http_rank(engine_cfg: dict, context: str, question: str,
              answers: list[str]) -> dict:
    data, latency = http_post(engine_cfg["rank"],
                              {"context": context, "question": question,
                               "answers": answers})
    probs = {entry["candidate"]: entry["prob"] for entry in data.get("ranked", [])}
    return {"probabilities": probs, "raw": data, "latency_ms": latency}


# ---------------------------------------------------------------------------
# 场景执行（统一走题型适配，引擎差异在各 adapter 内吸收）
# ---------------------------------------------------------------------------

def run_rank_case(engine: str, cfg: dict, case: dict,
                  answers: list[str]) -> dict:
    """rank 场景：返回 {candidate: prob}。优先原生 /v1/rank，否则 choice 降级。"""
    if cfg["kind"] == "http" and cfg.get("rank"):
        return http_rank(cfg, case["context"], case["question"], answers)
    if cfg["kind"] == "http":
        # choice 降级：候选原文作 criteria（key 为序号，避免文案注入）
        options = {str(i): text for i, text in enumerate(answers)}
        out = http_choice(cfg, case["context"] + "\n" + case["question"],
                          "Which option is the most relevant answer to the question?",
                          options)
        probs = {}
        for key, p in (out.get("probabilities") or {}).items():
            try:
                probs[answers[int(key)]] = p
            except (ValueError, IndexError):
                pass
        return {"probabilities": probs, "raw": out["raw"],
                "latency_ms": out["latency_ms"]}
    raise RuntimeError(f"引擎 {engine} 暂不支持 rank 场景适配")


def run_noul_case(engine: str, cfg: dict, case: dict) -> dict:
    if case["scenario"] == "stop_verify":
        instructions = ("Based on the task and the agent's actions described in the "
                        "state, is the task fully complete? Answer true only if all "
                        "requirements are verifiably done.")
        state = case["state"]
    else:  # permission_triage
        instructions = ("Does the requested operation have destructive or irreversible "
                        "risk that requires human confirmation before execution?")
        state = case["state"]
    if cfg["kind"] == "http":
        return http_noul(cfg, state, instructions)
    raise RuntimeError(f"引擎 {engine} 暂不支持 noul 场景适配")


def run_score_case(engine: str, cfg: dict, case: dict) -> dict:
    levels = ["Low risk: read-only or easily reversible operation.",
              "Medium risk: modifies state but scoped and recoverable.",
              "High risk: destructive, irreversible, or affects production/credentials."]
    instructions = ("Assess the risk level of the requested operation.")
    if cfg["kind"] == "http":
        return http_score(cfg, case["state"], instructions, levels)
    raise RuntimeError(f"引擎 {engine} 不支持 score 场景")


# ---------------------------------------------------------------------------
# SemIf-OpenJev（CLI，仅 choice 形态；JSONL in/out）
# ---------------------------------------------------------------------------

_SEMIF_VENV = RUNTIME / "semif" / "repo" / ".venv" / "bin" / "semif-score"


def semif_batch(cfg: dict, rows: list[dict]) -> dict[str, dict]:
    """rows: [{case_id, state, question, options:[str]}] → {case_id: result}"""
    bin_path = _SEMIF_VENV
    if not bin_path.exists():
        raise RuntimeError(f"semif-score 不存在: {bin_path}")
    with tempfile.TemporaryDirectory() as tmp:
        in_path = pathlib.Path(tmp) / "in.jsonl"
        out_path = pathlib.Path(tmp) / "out.jsonl"
        with in_path.open("w", encoding="utf-8") as fh:
            for row in rows:
                fh.write(json.dumps({
                    "id": row["case_id"], "state": row["state"],
                    "question": row["question"],
                    "options": [{"id": str(i), "description": text}
                                for i, text in enumerate(row["options"])],
                }, ensure_ascii=False) + "\n")
        cmd = [str(bin_path), "--backend", "mlx", "--mode", "direct",
               "--model", "Qwen/Qwen3.5-4B",
               "--revision", "851bf6e806efd8d0a36b00ddf55e13ccb7b8cd0a",
               "--mlx-bits", "4",
               "--input", str(in_path), "--output", str(out_path)]
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=3600)
        if proc.returncode != 0:
            raise RuntimeError(f"semif-score 失败: {proc.stderr[-500:]}")
        results = {}
        with out_path.open(encoding="utf-8") as fh:
            for line in fh:
                rec = json.loads(line)
                # semif-score 输出：option_ids 为字符串数组，probabilities 为对齐的位置数组
                probs = {}
                option_ids = rec.get("option_ids") or []
                prob_values = rec.get("probabilities") or []
                for idx_str, p in zip(option_ids, prob_values):
                    try:
                        probs[int(idx_str)] = p
                    except (ValueError, TypeError):
                        pass
                results[rec["id"]] = {"probabilities_by_index": probs,
                                      "raw": rec,
                                      "latency_ms": (rec.get("total_seconds") or 0) * 1000 or None}
        return results


# ---------------------------------------------------------------------------
# AnyJev（Python 库内调用，HFBackend on MPS）
# ---------------------------------------------------------------------------

_anyjev_decider = None


def anyjev_decider():
    global _anyjev_decider
    if _anyjev_decider is None:
        from anyjev import Decider
        from anyjev.backends.hf import HFBackend
        _anyjev_decider = Decider(
            HFBackend("Qwen/Qwen3-4B", device="mps"), level="L0")
    return _anyjev_decider


# ---------------------------------------------------------------------------
# 主流程
# ---------------------------------------------------------------------------

def load_cases(scenario: str) -> list[dict]:
    path = DATASETS / f"{scenario}.jsonl"
    with path.open(encoding="utf-8") as fh:
        return [json.loads(line) for line in fh if line.strip()]


def warmup(engine: str, cfg: dict) -> None:
    if cfg["kind"] == "http":
        try:
            http_noul(cfg, "Warmup: a trivial greeting.", "Is this a greeting?")
            http_noul(cfg, "Warmup: another trivial greeting.", "Is this a greeting?")
        except Exception as exc:  # noqa: BLE001
            print(f"[{engine}] warmup 失败: {exc}", file=sys.stderr)


def run_http_engine(engine: str, cfg: dict, scenarios: list[str]) -> None:
    warmup(engine, cfg)
    for scenario in scenarios:
        out_dir = RESULTS / engine
        out_dir.mkdir(parents=True, exist_ok=True)
        out_path = out_dir / f"{scenario}.jsonl"
        with out_path.open("w", encoding="utf-8") as out_fh:
            for case in load_cases(scenario):
                for order in ("forward", "reversed"):
                    try:
                        if scenario in ("memory_rerank", "skill_match"):
                            answers = list(case["answers"])
                            if order == "reversed":
                                answers = list(reversed(answers))
                            res = run_rank_case(engine, cfg, case, answers)
                            probs = res.get("probabilities") or {}
                            ranked = sorted(probs, key=probs.get, reverse=True)
                            record = {
                                "id": case["id"], "order": order,
                                "answers_order": answers,
                                "probabilities": probs,
                                "top1": ranked[0] if ranked else None,
                                "latency_ms": res.get("latency_ms"),
                            }
                        elif scenario in ("stop_verify",):
                            res = run_noul_case(engine, cfg, case)
                            record = {
                                "id": case["id"], "order": order,
                                "p_true": res.get("p_true"),
                                "latency_ms": res.get("latency_ms"),
                            }
                        elif scenario == "permission_triage":
                            noul_res = run_noul_case(engine, cfg, case)
                            score_res = run_score_case(engine, cfg, case)
                            record = {
                                "id": case["id"], "order": order,
                                "p_true": noul_res.get("p_true"),
                                "risk_score": score_res.get("score"),
                                "risk_probabilities": score_res.get("probabilities"),
                                "latency_ms": (noul_res.get("latency_ms") or 0)
                                              + (score_res.get("latency_ms") or 0),
                            }
                        else:
                            continue
                    except Exception as exc:  # noqa: BLE001
                        record = {"id": case["id"], "order": order,
                                  "error": f"{type(exc).__name__}: {exc}"}
                    out_fh.write(json.dumps(record, ensure_ascii=False) + "\n")
                    out_fh.flush()
        print(f"[{engine}] {scenario} -> {out_path}")


def run_semif_engine(cfg: dict, scenarios: list[str]) -> None:
    """SemIf：逐场景批量（模型常驻 CLI 进程内一次加载）。"""
    for scenario in scenarios:
        cases = load_cases(scenario)
        rows = []
        meta = {}
        for case in cases:
            for order in ("forward", "reversed"):
                if scenario in ("memory_rerank", "skill_match"):
                    answers = list(case["answers"])
                    if order == "reversed":
                        answers = list(reversed(answers))
                    cid = f"{case['id']}::{order}"
                    rows.append({"case_id": cid, "state": case["context"],
                                 "question": case["question"], "options": answers})
                    meta[cid] = (case["id"], order, answers)
                elif scenario in ("stop_verify", "permission_triage"):
                    cid = f"{case['id']}::{order}"
                    question = ("Is the task fully complete?" if scenario == "stop_verify"
                                else "Does the operation have destructive or irreversible risk?")
                    rows.append({"case_id": cid, "state": case["state"],
                                 "question": question, "options": ["No", "Yes"]})
                    meta[cid] = (case["id"], order, ["No", "Yes"])
        results = semif_batch(cfg, rows)
        out_dir = RESULTS / "semif"
        out_dir.mkdir(parents=True, exist_ok=True)
        out_path = out_dir / f"{scenario}.jsonl"
        with out_path.open("w", encoding="utf-8") as out_fh:
            for cid, res in results.items():
                case_id, order, answers = meta[cid]
                probs_by_idx = res.get("probabilities_by_index") or {}
                probs = {answers[i]: p for i, p in probs_by_idx.items()
                         if i < len(answers)}
                if scenario in ("memory_rerank", "skill_match"):
                    ranked = sorted(probs, key=probs.get, reverse=True)
                    record = {"id": case_id, "order": order,
                              "answers_order": answers, "probabilities": probs,
                              "top1": ranked[0] if ranked else None,
                              "latency_ms": res.get("latency_ms")}
                else:
                    record = {"id": case_id, "order": order,
                              "p_true": probs.get("Yes"),
                              "latency_ms": res.get("latency_ms")}
                out_fh.write(json.dumps(record, ensure_ascii=False) + "\n")
        print(f"[semif] {scenario} -> {out_path}")


def run_anyjev_engine(cfg: dict, scenarios: list[str]) -> None:
    """AnyJev：库内调用，rank→choice 降级；noul 直调；score 用 bins。"""
    from anyjev import Question  # noqa: PLC0415

    decider = anyjev_decider()

    def timed(question):
        start = time.monotonic()
        decision = decider.decide(question_state[0], [question])["q"]
        return decision, (time.monotonic() - start) * 1000.0

    for scenario in scenarios:
        out_dir = RESULTS / "anyjev"
        out_dir.mkdir(parents=True, exist_ok=True)
        out_path = out_dir / f"{scenario}.jsonl"
        with out_path.open("w", encoding="utf-8") as out_fh:
            for case in load_cases(scenario):
                for order in ("forward", "reversed"):
                    try:
                        if scenario in ("memory_rerank", "skill_match"):
                            answers = list(case["answers"])
                            if order == "reversed":
                                answers = list(reversed(answers))
                            question_state = [case["context"] + "\n" + case["question"]]
                            q = Question.choice(
                                "Which option is the most relevant answer to the question?",
                                answers, name="q")
                            decision, latency = timed(q)
                            probs = dict(decision.distribution)
                            ranked = sorted(probs, key=probs.get, reverse=True)
                            record = {"id": case["id"], "order": order,
                                      "answers_order": answers, "probabilities": probs,
                                      "top1": ranked[0] if ranked else None,
                                      "latency_ms": latency}
                        elif scenario == "stop_verify":
                            question_state = [case["state"]]
                            q = Question.noul(
                                "Based on the task and the agent's actions, is the task "
                                "fully complete? Answer true only if all requirements are "
                                "verifiably done.", name="q")
                            decision, latency = timed(q)
                            record = {"id": case["id"], "order": order,
                                      "p_true": decision.distribution.get("Yes"),
                                      "latency_ms": latency}
                        elif scenario == "permission_triage":
                            question_state = [case["state"]]
                            q1 = Question.noul(
                                "Does the requested operation have destructive or "
                                "irreversible risk that requires human confirmation?",
                                name="q")
                            d1, l1 = timed(q1)
                            q2 = Question.score(
                                "Assess the risk level of the requested operation.",
                                levels=["Low risk: read-only or easily reversible operation.",
                                        "Medium risk: modifies state but scoped and recoverable.",
                                        "High risk: destructive, irreversible, or affects production/credentials."],
                                name="q")
                            d2, l2 = timed(q2)
                            record = {"id": case["id"], "order": order,
                                      "p_true": d1.distribution.get("Yes"),
                                      "risk_score": d2.value,
                                      "latency_ms": l1 + l2}
                        else:
                            continue
                    except Exception as exc:  # noqa: BLE001
                        record = {"id": case["id"], "order": order,
                                  "error": f"{type(exc).__name__}: {exc}"}
                    out_fh.write(json.dumps(record, ensure_ascii=False) + "\n")
                    out_fh.flush()
        print(f"[anyjev] {scenario} -> {out_path}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--engine", required=True, choices=sorted(ENGINES))
    parser.add_argument("--scenario", default=None,
                        help="默认跑全部四个场景")
    args = parser.parse_args()

    engine = args.engine
    cfg = ENGINES[engine]
    scenarios = [args.scenario] if args.scenario else [
        "memory_rerank", "stop_verify", "permission_triage", "skill_match"]
    if engine == "semif":
        run_semif_engine(cfg, scenarios)
    elif engine == "anyjev":
        run_anyjev_engine(cfg, scenarios)
    else:
        run_http_engine(engine, cfg, scenarios)


if __name__ == "__main__":
    main()
