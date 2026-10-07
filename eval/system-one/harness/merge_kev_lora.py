#!/usr/bin/env python3
"""合并 kev LoRA adapter 到底座并导出全量 safetensors 模型。

kev-0.8b = Qwen/Qwen3.5-0.8B-Base（冻结底座）+ LoRA（r=16, alpha=32）+ 外置 PointerHead。
PointerHead（head.pt）不参与合并——它在推理侧外置前向（hidden states -> q/k 投影 -> 点积）。

用法：
    python3 merge_kev_lora.py [--out DIR] [--base-revision REV] [--adapter REV]

产物：--out 目录下的全量模型（safetensors + tokenizer + config），供 convert_hf_to_gguf.py 转换。
数值口径：全程 fp32 加载与合并，保存 bf16 由调用方决定（默认 fp32 保存，转换阶段再量化）。
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import torch
from peft import PeftModel
from transformers import AutoModelForCausalLM, AutoTokenizer

BASE = "Qwen/Qwen3.5-0.8B-Base"
BASE_REVISION = "dc7cdfe2ee4154fa7e30f5b51ca41bfa40174e68"  # kev adapter 的 base_revision
ADAPTER = "jaredpalmer/kev-0.8b"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", required=True, help="合并产物输出目录")
    parser.add_argument("--base", default=BASE)
    parser.add_argument("--base-revision", default=BASE_REVISION)
    parser.add_argument("--adapter", default=ADAPTER)
    parser.add_argument(
        "--dtype",
        choices=["fp32", "bf16", "fp16"],
        default="fp32",
        help="保存精度（默认 fp32 保数值，量化留给 GGUF 转换阶段）",
    )
    args = parser.parse_args()

    save_dtype = {"fp32": torch.float32, "bf16": torch.bfloat16, "fp16": torch.float16}[args.dtype]

    print(f"加载底座 {args.base}@{args.base_revision}（fp32）...")
    model = AutoModelForCausalLM.from_pretrained(args.base, revision=args.base_revision, dtype=torch.float32)

    # kev adapter 的 key 是 base_model.model.layers.N.*（对 backbone 套 peft），
    # 必须对 .model（TextModel backbone）加载，对 ForCausalLM 整体加载会多一层前缀导致全部 missing。
    print(f"加载 LoRA adapter {args.adapter}...")
    backbone = PeftModel.from_pretrained(model.model, args.adapter)

    print("合并 LoRA 权重（merge_and_unload）...")
    model.model = backbone.merge_and_unload()
    model = model.to(save_dtype)

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    print(f"保存到 {out}（{args.dtype}）...")
    model.save_pretrained(out)

    tok = AutoTokenizer.from_pretrained(args.base, revision=args.base_revision)
    tok.save_pretrained(out)

    manifest = {
        "base": args.base,
        "base_revision": args.base_revision,
        "adapter": args.adapter,
        "dtype": args.dtype,
        "hidden_size": model.config.hidden_size,
        "num_hidden_layers": model.config.num_hidden_layers,
        "model_type": model.config.model_type,
    }
    (out / "merge_manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
