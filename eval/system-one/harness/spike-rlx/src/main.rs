//! Spike A (aemeath#1833): rlx-qwen35 vs llama.cpp hidden-state parity probe.
//!
//! Modes:
//!   cargo run --release -- hidden [token ids...]   # forward + compare vs llama-server :8019
//!   cargo run --release -- head <case.json> <head_dir>  # PointerHead end-to-end vs golden
//!
//! `hidden` default ids: 151644 9707 11 1879 (fixed short probe sequence).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use rlx_core::gguf_support::{GgufModelFamily, assert_gguf_family};
use rlx_core::weight_loader::GgufLoader;
use rlx_ir::DType;
use rlx_qwen35::{
    Qwen35CompileCache, Qwen35Config, Qwen35Weights, build_qwen35_prefill_flow_ext,
    get_or_specialize_hir, prefill_config,
};
use rlx_runtime::Device;

const GGUF: &str = "/Users/guoyuqi/.cache/system-one-eval/kev-merged-f16.gguf";
const LLAMA_EMBED_URL: &str = "http://127.0.0.1:8019/embedding";

struct Ctx {
    cfg: Qwen35Config,
    weights: Qwen35Weights,
    loader: GgufLoader,
    cache: Qwen35CompileCache,
    n_forwards: usize,
}

/// CPU by default; `SPIKE_DEVICE=metal` selects Metal (requires `--features metal`).
fn pick_device() -> Device {
    match std::env::var("SPIKE_DEVICE").as_deref() {
        Ok("metal") => Device::Metal,
        Ok("mlx") => Device::Mlx,
        _ => Device::Cpu,
    }
}

impl Ctx {
    fn open(gguf: &Path) -> Result<Self> {
        let device = pick_device();
        let t = Instant::now();
        let raw = assert_gguf_family(gguf, GgufModelFamily::Qwen35)?;
        let cfg = Qwen35Config::from_gguf(&raw)?;
        drop(raw);
        println!(
            "[cfg] arch=qwen35 layers={} hidden={} vocab={} nextn={} moe_experts={} eps={} (in {:.2?})",
            cfg.num_hidden_layers, cfg.hidden_size, cfg.vocab_size,
            cfg.nextn_predict_layers, cfg.num_experts, cfg.rms_norm_eps, t.elapsed()
        );
        let t = Instant::now();
        let mut loader = GgufLoader::from_file(
            gguf.to_str().ok_or_else(|| anyhow!("non-utf8 gguf path"))?,
        )?;
        loader.include_mtp(true);
        let weights = Qwen35Weights::from_loader_packed(&mut loader, &cfg)?;
        println!("[weights] packed load done in {:.2?}", t.elapsed());
        println!("[device] {device:?}");
        let cache = Qwen35CompileCache::new(device, 4);
        Ok(Self { cfg, weights, loader, cache, n_forwards: 0 })
    }

    /// Prefill forward → last-layer normed hidden, row-major `[seq, hidden]`.
    fn hidden(&mut self, ids: &[u32]) -> Result<Vec<f32>> {
        let seq = ids.len();
        let hidden = self.cfg.hidden_size;
        let config = prefill_config(1, seq);
        // Compile-cache lookup first: rebuild + re-upload only on miss.
        let built = if self.cache.contains(&config) {
            None
        } else {
            let t = Instant::now();
            let b = build_qwen35_prefill_flow_ext(
                &self.cfg,
                &self.weights,
                1,
                seq,
                /*with_lm_head=*/ false,
                /*last_logits_only=*/ false,
                /*enable_mtp_head=*/ false,
                /*runtime_mrope=*/ false,
                /*fast_mtp=*/ false,
                /*export_normed_hidden=*/ true,
            )?;
            println!("[flow] built hidden prefill IR seq={seq} in {:.2?}", t.elapsed());
            Some(b)
        };
        let compiled = match built {
            Some((hir, params, packed)) => {
                let loader = &self.loader;
                let t = Instant::now();
                let n_params = params.len();
                let n_packed = packed.len();
                let c = get_or_specialize_hir(&mut self.cache, &config, || hir, move |g| {
                    for (name, data) in &params {
                        g.set_param(name, data);
                    }
                    for (name, (key, _scheme, _shape)) in &packed {
                        let bytes = loader
                            .tensor_bytes_borrowed(key)
                            .ok_or_else(|| anyhow!("packed bytes missing: {key}"))?;
                        g.set_param_typed(name, bytes, DType::U8);
                    }
                    g.finalize_params();
                    Ok(())
                })?;
                println!(
                    "[compile] uploaded {n_params} f32 + {n_packed} packed params in {:.2?}",
                    t.elapsed()
                );
                c
            }
            None => get_or_specialize_hir(&mut self.cache, &config, || {
                panic!("graph reported cached but HIR rebuild requested")
            }, |_| Ok(()))?,
        };

        let feed: Vec<f32> = ids.iter().map(|&t| t as f32).collect();
        let t = Instant::now();
        let outs = compiled.run(&[("input_ids", feed.as_slice())]);
        println!(
            "[run#{}] forward seq={seq} in {:.2?} ({} outputs, out[0].len()={})",
            self.n_forwards + 1,
            t.elapsed(),
            outs.len(),
            outs.first().map(Vec::len).unwrap_or(0)
        );
        self.n_forwards += 1;
        let out = outs.into_iter().next().ok_or_else(|| anyhow!("no graph outputs"))?;
        let expect = seq * hidden;
        if out.len() != expect {
            bail!("hidden output len {} != seq*hidden {expect} ({seq}x{hidden})", out.len());
        }
        Ok(out)
    }
}

/// Reference hidden states from llama-server (pooling=none, f16 weights).
fn llama_hidden(ids: &[u32]) -> Result<Vec<f32>> {
    let payload = format!(
        "{{\"content\":[{}]}}",
        ids.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(",")
    );
    let out = Command::new("curl")
        .args(["-s", "-m", "300", "-X", "POST", LLAMA_EMBED_URL,
               "-H", "Content-Type: application/json", "-d", &payload])
        .output()
        .context("spawn curl")?;
    if !out.status.success() {
        bail!("curl failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)
        .with_context(|| format!("parse llama-server response: {}", String::from_utf8_lossy(&out.stdout).chars().take(200).collect::<String>()))?;
    let rows = v[0]["embedding"].as_array().ok_or_else(|| anyhow!("no embedding: {v}"))?;
    let mut flat = Vec::with_capacity(rows.len() * 1024);
    for row in rows {
        for x in row.as_array().ok_or_else(|| anyhow!("ragged embedding"))? {
            flat.push(x.as_f64().ok_or_else(|| anyhow!("non-float"))? as f32);
        }
    }
    Ok(flat)
}

fn max_abs(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max)
}

/// Mode 3b: forward a fixed sequence, compare per-position vs llama-server.
fn mode_hidden(ids: &[u32]) -> Result<()> {
    let mut ctx = Ctx::open(Path::new(GGUF))?;
    let h = ctx.hidden(ids)?;
    println!("[llama] fetching reference hidden for {} tokens...", ids.len());
    let t = Instant::now();
    let r = llama_hidden(ids)?;
    println!("[llama] reference ready in {:.2?} (len={})", t.elapsed(), r.len());
    if r.len() != h.len() {
        bail!("shape mismatch: rlx {} vs llama {}", h.len(), r.len());
    }
    let hidden = 1024usize;
    let seq = ids.len();
    println!("\n== per-position rlx vs llama-server (f16 ref) ==");
    println!("{:>4} {:>8} {:>12} {:>12} {:>12}", "pos", "token", "max|Δ|", "relL2", "cos");
    let mut worst = 0.0f32;
    let mut worst_pos = 0usize;
    let mut worst_rel = 0.0f32;
    for i in 0..seq {
        let sl = &h[i * hidden..(i + 1) * hidden];
        let sr = &r[i * hidden..(i + 1) * hidden];
        let d = max_abs(sl, sr);
        let sq: f64 = sl.iter().map(|x| (*x as f64).powi(2)).sum();
        let sq2: f64 = sr.iter().map(|x| (*x as f64).powi(2)).sum();
        let dot: f64 = sl.iter().zip(sr).map(|(a, b)| (*a as f64) * (*b as f64)).sum();
        let rel = (sq + sq2 - 2.0 * dot).sqrt().max(0.0) / sq2.sqrt().max(1e-12);
        let cos = dot / (sq.sqrt() * sq2.sqrt()).max(1e-12);
        if d > worst {
            worst = d;
            worst_pos = i;
            worst_rel = rel as f32;
        }
        println!("{i:>4} {:>8} {:>12.5} {:>12.2e} {:>12.7}", ids[i], d, rel, cos);
    }
    println!(
        "\nOVERALL max|Δ| = {:.5} at pos {worst_pos} (relL2={worst_rel:.2e}); tolerance: 5e-2 f16 / 1e-1 bf16-compute",
        worst
    );
    if worst < 5e-2 {
        println!("RESULT: PASS (< 5e-2)");
    } else if worst < 1e-1 {
        println!("RESULT: BORDERLINE (< 1e-1, bf16-compute allowance)");
    } else {
        println!("RESULT: FAIL");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Mode 4: PointerHead end-to-end on memory_rerank mem-001
// ---------------------------------------------------------------------------

fn read_f32_bin(path: &Path) -> Result<Vec<f32>> {
    let bytes = std::fs::read(path).with_context(|| format!("read {path:?}"))?;
    if bytes.len() % 4 != 0 {
        bail!("{path:?}: len {} not multiple of 4", bytes.len());
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

struct PointerHead {
    q_w: Vec<f32>, // [256, 1024] row-major (PyTorch layout)
    q_b: Vec<f32>, // [256]
    k_w: Vec<f32>, // [256, 1024]
    k_b: Vec<f32>, // [256]
    temperature: f64,
}

impl PointerHead {
    fn load(dir: &Path) -> Result<Self> {
        let meta: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("head.json"))?)?;
        let get = |k: &str| -> Result<PathBuf> {
            Ok(dir.join(meta[k].as_str().ok_or_else(|| anyhow!("head.json: {k}"))?))
        };
        let head = PointerHead {
            q_w: read_f32_bin(&get("q_weight")?)?,
            q_b: read_f32_bin(&get("q_bias")?)?,
            k_w: read_f32_bin(&get("k_weight")?)?,
            k_b: read_f32_bin(&get("k_bias")?)?,
            temperature: meta["temperature"].as_f64().ok_or_else(|| anyhow!("temperature"))?,
        };
        let d_model = meta["hidden"].as_u64().ok_or_else(|| anyhow!("hidden"))? as usize;
        let d_proj = meta["proj"].as_u64().ok_or_else(|| anyhow!("proj"))? as usize;
        if head.q_w.len() != d_proj * d_model || head.k_w.len() != d_proj * d_model {
            bail!("head weight size mismatch");
        }
        println!("[head] loaded proj={d_proj} hidden={d_model} T={}", head.temperature);
        Ok(head)
    }

    /// h [1024] → q/k projection.
    fn project(w: &[f32], b: &[f32], h: &[f32], d_model: usize) -> Vec<f32> {
        let d_proj = b.len();
        (0..d_proj)
            .map(|i| {
                let row = &w[i * d_model..(i + 1) * d_model];
                let dot: f32 = row.iter().zip(h).map(|(a, x)| a * x).sum();
                dot + b[i]
            })
            .collect()
    }

    /// logits[k] = (k_k · q) / sqrt(d_proj) / T   (kev eval 口径).
    fn logits(&self, h_decide: &[f32], h_opts: &[Vec<f32>], d_model: usize) -> Vec<f32> {
        let q = Self::project(&self.q_w, &self.q_b, h_decide, d_model);
        let d_proj = self.q_b.len() as f32;
        h_opts
            .iter()
            .map(|h| {
                let k = Self::project(&self.k_w, &self.k_b, h, d_model);
                let dot: f32 = k.iter().zip(&q).map(|(a, b)| a * b).sum();
                dot / d_proj.sqrt() / self.temperature as f32
            })
            .collect()
    }
}

fn softmax(logits: &[f32]) -> Vec<f32> {
    let m = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let ex: Vec<f32> = logits.iter().map(|x| (x - m).exp()).collect();
    let s: f32 = ex.iter().sum();
    ex.iter().map(|x| x / s).collect()
}

fn mode_head(case_path: &Path, head_dir: &Path) -> Result<()> {
    let case: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(case_path)?)?;
    let id = case["id"].as_str().unwrap_or("?").to_string();
    let row_ids: Vec<u32> = case["row_ids"]
        .as_array()
        .ok_or_else(|| anyhow!("row_ids"))?
        .iter()
        .map(|x| x.as_u64().unwrap() as u32)
        .collect();
    let decide = case["decide"].as_u64().ok_or_else(|| anyhow!("decide"))? as usize;
    let opts: Vec<usize> = case["opts"]
        .as_array()
        .ok_or_else(|| anyhow!("opts"))?
        .iter()
        .map(|x| x.as_u64().unwrap() as usize)
        .collect();
    let golden: Vec<f32> = case["golden_probs"].as_array().map(|a| {
        a.iter().map(|x| x.as_f64().unwrap() as f32).collect()
    }).unwrap_or_default();
    let llama: Vec<f32> = case["llama_probs"].as_array().map(|a| {
        a.iter().map(|x| x.as_f64().unwrap() as f32).collect()
    }).unwrap_or_default();
    println!(
        "[case] {id}: row_len={} decide={} opts={:?} golden_len={} llama_len={}",
        row_ids.len(), decide, opts, golden.len(), llama.len()
    );

    let mut ctx = Ctx::open(Path::new(GGUF))?;
    let h = ctx.hidden(&row_ids)?;
    let d_model = ctx.cfg.hidden_size;
    let at = |i: usize| -> &[f32] { &h[i * d_model..(i + 1) * d_model] };
    let head = PointerHead::load(head_dir)?;
    let logits = head.logits(at(decide), &opts.iter().map(|&o| at(o).to_vec()).collect::<Vec<_>>(), d_model);
    let probs = softmax(&logits);
    println!("\n== mem-001 PointerHead probs (rlx hidden) ==");
    println!("logits: {}", logits.iter().map(|x| format!("{x:.5}")).collect::<Vec<_>>().join(", "));
    println!("probs : {}", probs.iter().map(|x| format!("{x:.5}")).collect::<Vec<_>>().join(", "));
    let am = |p: &[f32]| p.iter().enumerate().max_by(|a, b| a.1.partial_cmp(b.1).unwrap()).map(|(i, _)| i);
    println!("argmax rlx = {:?}", am(&probs));
    let mut out = String::new();
    if !golden.is_empty() {
        let d = max_abs(&probs, &golden);
        out.push_str(&format!(
            "\nvs golden(kev :8009): argmax = {:?}, max|Δp| = {:.5} (parity_gguf.json recorded gguf-vs-golden ≤ 0.0056 on mem-001)",
            am(&golden), d
        ));
    }
    if !llama.is_empty() {
        let d = max_abs(&probs, &llama);
        out.push_str(&format!("\nvs llama-server hidden→head: argmax = {:?}, max|Δp| = {:.5}", am(&llama), d));
    }
    println!("{out}");

    // Extra evidence: per-position hidden parity on the real 169-token row.
    match llama_hidden(&row_ids) {
        Ok(r) if r.len() == h.len() => {
            let mut worst = 0.0f32;
            let mut worst_pos = 0usize;
            let mut worst_rel = 0.0f32;
            for i in 0..row_ids.len() {
                let sl = &h[i * d_model..(i + 1) * d_model];
                let sr = &r[i * d_model..(i + 1) * d_model];
                let d = max_abs(sl, sr);
                if d > worst {
                    let sq: f64 = sl.iter().map(|x| (*x as f64).powi(2)).sum();
                    let sq2: f64 = sr.iter().map(|x| (*x as f64).powi(2)).sum();
                    let dot: f64 = sl.iter().zip(sr).map(|(a, b)| (*a as f64) * (*b as f64)).sum();
                    worst_rel = ((sq + sq2 - 2.0 * dot).sqrt().max(0.0) / sq2.sqrt().max(1e-12)) as f32;
                    worst = d;
                    worst_pos = i;
                }
            }
            let at_pos = |i: usize| -> f32 {
                max_abs(&h[i * d_model..(i + 1) * d_model], &r[i * d_model..(i + 1) * d_model])
            };
            println!(
                "\n== hidden parity on row ({n} tokens) rlx vs llama-server ==\n\
                 worst max|Δ| = {worst:.5} (relL2={worst_rel:.2e}) at pos {worst_pos}; \
                 decide@{decide}: {dd:.5}; opts@{opts:?}: {od:?}",
                n = row_ids.len(),
                worst = worst,
                worst_rel = worst_rel,
                worst_pos = worst_pos,
                decide = decide,
                dd = at_pos(decide),
                opts = opts,
                od = opts.iter().map(|&o| format!("{:.5}", at_pos(o))).collect::<Vec<_>>(),
            );
        }
        Ok(_) => println!("\n[warn] llama row hidden shape mismatch"),
        Err(e) => println!("\n[warn] llama row hidden fetch failed: {e}"),
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("hidden") => {
            let ids: Vec<u32> = if args.len() > 2 {
                args[2..].iter().map(|s| s.parse().expect("token id")).collect()
            } else {
                vec![151644, 9707, 11, 1879]
            };
            mode_hidden(&ids)
        }
        Some("head") => {
            let case = args.get(2).ok_or_else(|| anyhow!("usage: head <case.json> <head_dir>"))?;
            let head = args.get(3).ok_or_else(|| anyhow!("usage: head <case.json> <head_dir>"))?;
            mode_head(Path::new(case), Path::new(head))
        }
        _ => {
            eprintln!("usage: spike-rlx [hidden [ids...] | head <case.json> <head_dir>]");
            Ok(())
        }
    }
}
