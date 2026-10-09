#!/usr/bin/env python3
"""Convert kev PointerHead head.pt -> raw f32 bins + head.json for the Rust spike.

Output (into spike-rlx/data/head/):
    q_weight.f32 [256*1024]  q_bias.f32 [256]
    k_weight.f32 [256*1024]  k_bias.f32 [256]
    head.json {q_weight, q_bias, k_weight, k_bias, proj, hidden, temperature}
"""
import json
import struct
import sys
from pathlib import Path

import torch

HEAD_PT = Path.home() / ".cache/huggingface/hub/models--jaredpalmer--kev-0.8b/snapshots" \
    / "bf75a6a8848ea6960ff2ed108d9ed44c2941174f/head.pt"

def main() -> None:
    out = Path(__file__).parent / "data" / "head"
    out.mkdir(parents=True, exist_ok=True)
    blob = torch.load(HEAD_PT, map_location="cpu", weights_only=False)
    head = blob["head"]
    q_w = head["q.weight"].float().contiguous()  # [256, 1024]
    q_b = head["q.bias"].float().contiguous()
    k_w = head["k.weight"].float().contiguous()
    k_b = head["k.bias"].float().contiguous()
    temp = float(blob["temperature"])
    for name, t in [("q_weight", q_w), ("q_bias", q_b), ("k_weight", k_w), ("k_bias", k_b)]:
        (out / f"{name}.f32").write_bytes(struct.pack(f"<{t.numel()}f", *t.flatten().tolist()))
    meta = {
        "q_weight": "q_weight.f32", "q_bias": "q_bias.f32",
        "k_weight": "k_weight.f32", "k_bias": "k_bias.f32",
        "proj": q_w.shape[0], "hidden": q_w.shape[1], "temperature": temp,
    }
    (out / "head.json").write_text(json.dumps(meta, indent=2) + "\n")
    print(f"wrote {out}: proj={meta['proj']} hidden={meta['hidden']} T={temp}")

if __name__ == "__main__":
    sys.exit(main())
