#!/usr/bin/env python3
"""kev GGUF（llama.cpp）与 MLX golden 的 46-case 数值对分。

三口径：
- golden：POST kev.serve /v1/systemone（MLX 后端，生产口径），直接出 probs
- torch：合并后 backbone（safetensors）forward 取 hidden states → PointerHead
- gguf：llama-server /embedding（逐 row token ids，pooling=none）取 hidden states → PointerHead

torch 与 gguf 共用同一 encode（kev.model.encode）+ PointerHead（head.pt）管线，
误差可分解：torch vs golden = LoRA 合并误差；gguf vs torch = GGUF 转换 + llama.cpp 前向误差。

用法：
    python3 parity_gguf.py --kev-repo ~/.cache/system-one-eval/kev \
        --backbone ~/.cache/system-one-eval/kev-merged-fp32-backbone \
        --datasets ../datasets/memory_rerank.jsonl [--datasets ...] \
        [--limit N] [--out results/parity_gguf.json]

前置：kev.serve（golden，:8009）与 llama-server --embedding --pooling none（:8019）已启动。
"""

from __future__ import annotations

import argparse
import json
import sys
import urllib.request
from pathlib import Path

import torch

HEAD_PT = Path.home() / ".cache/huggingface/hub/models--jaredpalmer--kev-0.8b/snapshots" \
    / "bf75a6a8848ea6960ff2ed108d9ed44c2941174f/head.pt"
GOLDEN_URL = "http://127.0.0.1:8009/v1/systemone"
GGUF_URL = "http://127.0.0.1:8019/embedding"
BASE = "Qwen/Qwen3.5-0.8B-Base"
BASE_REVISION = "dc7cdfe2ee4154fa7e30f5b51ca41bfa40174e68"


# ---------------------------------------------------------------------------
# PointerHead（kev/model.py:205 的独立复刻——head 外置正是内嵌化的前提）
# ---------------------------------------------------------------------------

class PointerHead:
    def __init__(self, head_pt: Path):
        blob = torch.load(head_pt, map_location="cpu", weights_only=False)
        head = blob["head"]
        self.q_weight = head["q.weight"].float()  # [256, 1024]
        self.q_bias = head["q.bias"].float()
        self.k_weight = head["k.weight"].float()
        self.k_bias = head["k.bias"].float()
        self.temperature = float(blob["temperature"])

    def logits(self, h_decide: torch.Tensor, h_opts: torch.Tensor) -> torch.Tensor:
        """h_decide [1024], h_opts [K, 1024] -> logits [K]（eval 口径：除以 temperature）。"""
        q = self.q_weight @ h_decide + self.q_bias
        k = h_opts @ self.k_weight.T + self.k_bias
        return (k @ q) / (self.q_weight.shape[0] ** 0.5) / self.temperature


# ---------------------------------------------------------------------------
# hidden states 来源
# ---------------------------------------------------------------------------

def http_json(url: str, payload: dict, timeout: float = 120.0) -> dict:
    req = urllib.request.Request(url, data=json.dumps(payload).encode(),
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read().decode())


class TorchHidden:
    """合并后 backbone 的 torch 前向（fp32 eager，kev torch 后端的精确口径）。"""

    def __init__(self, backbone_dir: str):
        from transformers import AutoModel
        self.lm = AutoModel.from_pretrained(backbone_dir, dtype=torch.float32,
                                            attn_implementation="eager")
        self.lm.eval()

    def hidden(self, ids: list[int]) -> torch.Tensor:
        with torch.no_grad():
            out = self.lm(input_ids=torch.tensor([ids]))
        return out.last_hidden_state[0].float()  # [L, 1024]


class GgufHidden:
    """llama-server embedding（pooling=none，逐 token hidden states）。"""

    def hidden(self, ids: list[int]) -> torch.Tensor:
        data = http_json(GGUF_URL, {"content": ids})
        emb = data[0]["embedding"]  # [L][1024]（pooling=none）
        return torch.tensor(emb, dtype=torch.float32)


# ---------------------------------------------------------------------------
# case 构造（与 run_eval.py 同一口径）与评分
# ---------------------------------------------------------------------------

def case_payload(case: dict) -> dict:
    """dataset jsonl case -> jev payload（复刻 run_eval.py 的构造分支）。"""
    scenario = case.get("scenario", "")
    if "stop_verify" in scenario:
        return {"state": case["state"],
                "questions": {"q": {"type": "noul", "instructions":
                                    ("Based on the task and the agent's actions described in the "
                                     "state, is the task fully complete? Answer true only if all "
                                     "requirements are met.")}}}
    if "permission" in scenario:
        return {"state": case["state"],
                "questions": {"q": {"type": "noul", "instructions":
                                    ("Does the requested operation have destructive or irreversible "
                                     "side effects?")}}}
    # rank 场景（memory_rerank / skill_match 及 *_real）→ choice 降级
    import os
    if os.environ.get("EVAL_GENERIC_INSTRUCT"):
        instr = "Which option is the most relevant answer to the question?"
    elif "memory_rerank" in scenario:
        instr = ("Given a user message from a coding-agent session, "
                 "retrieve the most relevant memory.")
    else:
        instr = "Which option is the most relevant answer to the question?"
    options = {str(i): text for i, text in enumerate(case["answers"])}
    return {"state": case["context"] + "\n" + case["question"],
            "questions": {"q": {"type": "choice", "instructions": instr, "criteria": options}}}


def payload_probs_golden(payload: dict) -> list[float]:
    data = http_json(GOLDEN_URL, payload)
    ans = data["answers"]["q"]
    if ans["type"] == "noul":
        return [1.0 - ans["noul"], ans["noul"]]
    probs = ans["probabilities"]
    return [probs[k] for k in sorted(probs, key=lambda x: (len(x), x))]


def payload_probs_local(payload: dict, kev_api, kev_model, tok, head: PointerHead, hidden_source) -> list[float]:
    """encode → rows → hidden → PointerHead → softmax probs（option 序）。"""
    req = kev_api.SystemOneRequest(**payload)
    rec, meta = kev_api.to_record(req)
    enc = kev_model.encode(tok, rec, max_state=kev_model.SERVE_MAX_STATE,
                         max_branch=kev_model.SERVE_MAX_BRANCH)  # 与 serve 同口径
    state_ids, _state_pos, rows = kev_model.rows_of(enc)
    all_probs = []
    for row in rows:
        row_ids = state_ids + row["ids"]
        h = hidden_source.hidden(row_ids)
        h_decide = h[len(state_ids) + row["decide"]]
        h_opts = torch.stack([h[len(state_ids) + o] for o in row["opts"]])
        logits = head.logits(h_decide, h_opts)
        all_probs.append(torch.softmax(logits, dim=-1).tolist())
    return all_probs[0]


def gold_index(case: dict) -> int:
    if "gold" in case:
        return case["gold"]
    # noul：label=True 对应 "true"（index 1）
    return 1 if case.get("label") else 0


# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------

def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--kev-repo", required=True)
    ap.add_argument("--backbone", required=True, help="合并后 backbone-only 目录（torch 口径）")
    ap.add_argument("--datasets", nargs="+", required=True)
    ap.add_argument("--limit", type=int, default=0, help="每数据集只跑前 N case（0=全量）")
    ap.add_argument("--skip-torch", action="store_true", help="跳过 torch 口径（只比 gguf vs golden）")
    ap.add_argument("--out", default="")
    args = ap.parse_args()

    sys.path.insert(0, args.kev_repo)
    import kev.api as kev_api  # noqa: E402
    import kev.model as kev_model  # noqa: E402

    tok = kev_model.load_tokenizer(BASE, revision=BASE_REVISION)
    head = PointerHead(HEAD_PT)
    print(f"PointerHead temperature={head.temperature:.6f}")

    torch_hidden = None if args.skip_torch else TorchHidden(args.backbone)
    gguf_hidden = GgufHidden()

    rows_out = []
    summary = {}
    for ds in args.datasets:
        cases = [json.loads(line) for line in open(ds) if line.strip()]
        if args.limit:
            cases = cases[: args.limit]
        name = Path(ds).stem
        stats = {"n": 0, "golden_vs_gguf_argmax_hit": 0, "golden_vs_torch_argmax_hit": 0,
                 "gguf_vs_torch_argmax_hit": 0, "max_dp_gguf": 0.0, "max_dp_torch": 0.0}
        for case in cases:
            payload = case_payload(case)
            p_golden = payload_probs_golden(payload)
            p_gguf = payload_probs_local(payload, kev_api, kev_model, tok, head, gguf_hidden)
            p_torch = payload_probs_local(payload, kev_api, kev_model, tok, head, torch_hidden) if torch_hidden else p_golden

            am = lambda p: max(range(len(p)), key=lambda i: p[i])
            n = len(p_golden)
            dp_gguf = max(abs(a - b) for a, b in zip(p_golden, p_gguf))
            dp_torch = max(abs(a - b) for a, b in zip(p_golden, p_torch))
            stats["n"] += 1
            stats["golden_vs_gguf_argmax_hit"] += int(am(p_golden) == am(p_gguf))
            stats["golden_vs_torch_argmax_hit"] += int(am(p_golden) == am(p_torch))
            stats["gguf_vs_torch_argmax_hit"] += int(am(p_gguf) == am(p_torch))
            stats["max_dp_gguf"] = max(stats["max_dp_gguf"], dp_gguf)
            stats["max_dp_torch"] = max(stats["max_dp_torch"], dp_torch)
            rows_out.append({"dataset": name, "id": case.get("id"), "gold": gold_index(case),
                             "argmax_golden": am(p_golden), "argmax_gguf": am(p_gguf),
                             "argmax_torch": am(p_torch),
                             "max_dp_golden_gguf": round(dp_gguf, 6),
                             "max_dp_golden_torch": round(dp_torch, 6),
                             "n_options": n})
            print(f"[{name}] {case.get('id')}: argmax golden/gguf/torch = "
                  f"{am(p_golden)}/{am(p_gguf)}/{am(p_torch)} (gold={gold_index(case)}) "
                  f"max|Δp| gguf={dp_gguf:.4f} torch={dp_torch:.4f}")
        summary[name] = stats

    print("\n=== 汇总 ===")
    for name, s in summary.items():
        n = s["n"]
        print(f"{name}: n={n} "
              f"argmax一致率 golden↔gguf={s['golden_vs_gguf_argmax_hit']}/{n} "
              f"golden↔torch={s['golden_vs_torch_argmax_hit']}/{n} "
              f"gguf↔torch={s['gguf_vs_torch_argmax_hit']}/{n} "
              f"max|Δp| gguf={s['max_dp_gguf']:.4f} torch={s['max_dp_torch']:.4f}")

    if args.out:
        Path(args.out).parent.mkdir(parents=True, exist_ok=True)
        Path(args.out).write_text(json.dumps({"summary": summary, "cases": rows_out}, indent=2, ensure_ascii=False) + "\n")
        print(f"写入 {args.out}")


if __name__ == "__main__":
    main()
