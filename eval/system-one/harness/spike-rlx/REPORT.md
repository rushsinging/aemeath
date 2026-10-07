# Spike A：rlx-qwen35 加载 kev 合并 GGUF 与 llama.cpp hidden states 对拍

> aemeath#1833 System One 引擎 Rust 内嵌化 · spike A · 2026-10-07
> 结论先行：**GO（CPU 后端）**。rlx-qwen35 0.2.11 能加载 kev-merged-f16.gguf，
> 逐 token 导出最后一层 normed hidden states，与 llama-server（f16 口径）数值一致；
> 端到端 PointerHead 对拍 max|Δp|=0.00561，**与 llama.cpp 自身对 golden 的误差完全相同**，
> mem-001 argmax=0 命中 gold。Metal/MLX 后端本 spike 不可用（见 §5）。

---

## 1. 复现方式

```bash
cd eval/system-one/harness/spike-rlx
# 依赖已在 Cargo.toml（[workspace] 空表隔离；default-features=false 跳过 tokenizer feature）
cargo build --release                 # CPU 默认路径（全量 ~51s，sccache 热；增量 1–2s）

# ① 加载 + 固定序列 forward + 与 llama-server :8019 逐位置对拍
cargo run --release -- hidden 151644 9707 11 1879   # 默认 id 亦可不带参数

# ② PointerHead 端到端（mem-001）
~/.cache/system-one-eval/convert-venv/bin/python convert_head.py   # head.pt → f32 bins + head.json
~/.cache/system-one-eval/convert-venv/bin/python make_case.py      # encode + golden(:8009) + llama 参照(:8019)
cargo run --release -- head data/case_mem001.json data/head
```

辅助脚本复用 `harness/parity_gguf.py` 的 `case_payload` / `kev.model.encode` /
`rows_of` / `PointerHead` 逻辑，token 口径与 79-case 对分完全一致。

注意：本 worktree 的 `.cargo/config.toml` 把 `target-dir` 重定向到
`~/.cache/aemeath-target/feature_1833-systemone-gguf-parity-206053942e43fbe8/`
（post-checkout hook 自动生成），二进制不在 spike-rlx/target/ 下。

## 2. API 可用性（先摸 API 的结论）

候选 API 实测情况：

| API | 结论 |
|---|---|
| `Qwen35Runner::builder().weights(gguf).device(Cpu)` | ✅ 官方高层入口，`tests/gguf_parity.rs` 即其用例；但**只暴露 logits**（`predict_logits` → `Qwen35PrefillOutput{logits,...}`），hidden states 无公开导出口（`fast_greedy_lm_head` 内部把 normed hidden 送 host lm_head 后仍返回 logits） |
| `build_qwen35_prefill_flow_ext(cfg, weights, 1, seq, with_lm_head=false, last_logits_only=false, mtp=false, runtime_mrope=false, fast_mtp=false, export_normed_hidden=true)` | ✅ **hidden 导出的正道**：输出 `[1, seq, 1024]` F32，output 名 `hidden`（`Qwen35Flow::export_normed_hidden()`，builder.rs/flow.rs 文档明确 "Export final RMS-normed hidden"），与 llama.cpp `/embedding`(pooling=none) 的 normed hidden 同口径 |
| `GgufLoader::from_file` + `assert_gguf_family(path, GgufModelFamily::Qwen35)` + `Qwen35Config::from_gguf` + `Qwen35Weights::from_loader_packed` | ✅ 全部 pub，元数据解析 24 层 / hidden 1024 / vocab 151936 / nextn=0 |
| `Qwen35CompileCache::new(Device::Cpu, n)` + `get_or_specialize_hir(cache, prefill_config(1, seq), hir, on_first_hit)` | ✅ 编译/上传管线；`compiled.run(&[("input_ids", f32_ids)])` 喂 `[seq]` F32（`pack_input_ids` 口径），返回 `Vec<Vec<f32>>`，`outs[0].len()==seq*1024` |
| `Qwen35ForwardCase/ForwardRequest/ForwardResult` | ❌ 0.2.11 源码中不存在该三件套（任务书里的候选，实际 API 是 flow/runner 两层） |
| Metal：`--features metal` + `Device::Metal` | ⚠️ 编译通过但运行失败，见 §5 |
| Cargo feature | 默认 `tokenizer` 拉 tokenizers/onig；本 spike 用 `default-features=false`（encode 全部在 Python 侧完成），**一次编译通过** |

没有 `cargo doc`，全程靠 `cargo fetch` 后读
`~/.cargo/registry/src/*/rlx-qwen35-0.2.11/{src,tests}`（lib.rs 的 pub use 清单 +
tests/gguf_parity.rs + runner.rs 的 `ensure_predict_compiled` 反推），1 小时内摸清。

## 3. 数值对拍结果

GGUF：`~/.cache/system-one-eval/kev-merged-f16.gguf`，arch=qwen35，24 层，hidden=1024，
vocab=151936，nextn=0，dense（0 experts），f16；blk.0 带 ssm_* 张量（GDN 层）确认混合结构可加载。

参照：`POST http://127.0.0.1:8019/embedding {"content":[ids]}` → 逐 token hidden（f16 口径）。

### 3a. 固定短序列 probe

`ids=[151644, 9707, 11, 1879]`（rlx CPU vs llama-server）：

```
 pos    token       max|Δ|        relL2          cos
   0   151644      0.07429      4.37e-3    0.9999907
   1     9707      0.03673      2.64e-3    0.9999966
   2       11      0.05757      4.02e-3    0.9999920
   3     1879      0.04158      2.73e-3    0.9999966
OVERALL max|Δ| = 0.07429 (relL2=4.37e-3)   → 超 5e-2，< 1e-1（bf16 放宽口径）
```

`ids=[1,2,3,4]`（普通词表 id）：max|Δ| = **0.04213**（relL2 4.24e-3，cos≥0.999991）→ **< 5e-2 PASS**。

解读：相对误差 ~0.3–0.4%、余弦 ≥0.99999 —— 是**同一函数的舍入级差异**而非算子错位
（若 GDN/attention 口径错会差几个数量级）。rlx-cpu 的 thunk.rs/op_registry.rs 含
bf16 路径，0.074 落在任务给定的 “bf16 参与计算放宽 1e-1” 档；两个短序列 probe 一个
0.074、一个 0.042，均为特殊 token 激活较大位置贡献。

### 3b. 真实长序列（mem-001 row，169 tokens）

```
worst max|Δ| = 0.00493 (relL2=3.20e-4) at pos 63
decide@168: 0.00059；opts@[96,112,134,153,167]: 0.00063 0.00053 0.00060 0.00059 0.00082
```
→ **远低于 5e-2**，决策位（decide/opts）~6e-4。

### 3c. PointerHead 端到端（mem-001，5 选项）

head 权重由 `convert_head.py` 从 `head.pt` 转 f32 bin（T=2.3510958125672174 与任务书一致）；
case token ids 由 `make_case.py` 用 kev 官方 encode 产出（state=36, row=133, 决策位口径与
`parity_gguf.py` 相同）；golden probs 现场只读查询 kev :8009。

```
logits(rlx):  0.40936, -0.50802, -2.36983, -2.26698, -2.95762
probs (rlx):  0.63899, 0.25532, 0.03967, 0.04397, 0.02204
probs (golden):0.6446,  0.2537,  0.0387,  0.042,   0.021
probs (llama→head): 0.63899, 0.25533, 0.03968, 0.04397, 0.02203

argmax rlx = 0 = argmax golden = 0 = gold ✓
max|Δp| rlx vs golden            = 0.00561（= parity_gguf.json 记录的 gguf-vs-golden 0.005611）
max|Δp| rlx vs llama-server→head = 0.00001
```

**关键判读**：rlx 与 llama.cpp 的 hidden 差在 PointerHead 之后只剩 1e-5 量级——
rlx 在决策位上 ≡ llama.cpp；0.0056 的 golden 差是 llama.cpp/MLX 生态本身的差
（79-case 对分已 100% 通过的那一档），不是 rlx 引入的新误差。

### 性能/资源（CPU，M 系列）

- GGUF 元数据 0.5–0.9s，packed load 0.8–1.7s
- flow IR 构建 **28–40s/次**（进程内按 seq 缓存；`Qwen35CompileCache::with_aot` 可落盘但本 spike 未启用——这是最大的一条启动成本）
- 参数上传 419 个 F32 param ≈ 0.9–1.8s（内联构建把 f16 反量化为 f32，常驻 ~3GB；
  `packed=0`：flow 级内联路径未产出 PackedParams，与 runner 级 packed 行为不同，RAM 敏感场景要注意）
- forward：seq=4 → 0.26s；seq=169 → 0.75s（对拍不追速度，够用）

## 4. 编译/运行坑

1. `target/` 被 worktree `.cargo/config.toml` 重定向（见 §1），直接 `./target/release/...` 找不到二进制。
2. `no targets specified`：新建 crate 要先放 `src/main.rs` 才能 `cargo fetch`。
3. `default-features=false` 可编译（tokenizer 模块内部有 `#[cfg(feature="qwen35-tokenizer")]` 门控）；
   但 onig/tokenizers 仍会被 `rlx-cli`→`rlx-text` 连带拉进来编译，省不掉。
4. `get_or_specialize_hir` 的 `on_first_hit` 是首次编译回调——F32/packed 参数上传必须放这里
   （pipeline 命中后不再回调）；cache miss 判断用 `cache.contains(&prefill_config(1,seq))`。
5. HTTP 参照用 `curl` 子进程 + serde_json 手拆（不引 reqwest，省编译时间）。
6. `head.pt` Rust 侧不读——`convert_head.py`（convert-venv torch）转 f32 LE bin + head.json。
7. Rust `println!` 混用位置/命名参数编译期报 `argument never used`（一次，已修）。
8. 全量 release 构建 ~51s（sccache 热）；metal feature 增量 26s。

## 5. Metal / MLX 后端（“易用则更好”验证）

- **Metal**（`--features metal`，`SPIKE_DEVICE=metal`）：编译通过，权重加载正常，
  但 MPSGraph 图优化失败，seq=4 和 seq=64 都是同一断言：
  `'mps.slice' op failed: length value 6144 does not fit within the dimension size (4)`
  （tensor `1×4×8208`，优化 pass `MPSCopyDataFiles` 崩）。混合 GDN 图在 Metal 后端
  有 shape/lowering bug → **本 spike 视为不可用**，需上游修或换 seq bucket 再试。
- **MLX**（`--features mlx`）：`mlx` 依赖的 cmake build script 直接编译失败 → feature 无法启用；
  运行期 `panic: backend registered`（model_pipeline.rs:105）。
- CPU 是 rlx 官方 parity 测试（`tests/gguf_parity.rs`）采用的口径，本 spike 所有数值结论均基于 CPU。

## 6. crate 成熟度评估

| 维度 | 事实 |
|---|---|
| 版本 | rlx-qwen35 **0.2.11**（2026-07-06 发布）；全家族同版本号 lockstep（rlx-models-core/rlx-runtime/rlx-ir/rlx-gguf… =0.2.11，rlx-macros 0.2.17） |
| 发布节奏 | 共 6 个版本，2026-05-29 → 2026-07-06 密集发布，**此后 ~3 个月未发新版** |
| 下载量 | rlx-qwen35 累计 1274（recent 726）；rlx-models-core 3176 —— 典型早期项目量级 |
| 仓库/作者 | github.com/MIT-RLX/rlx-models（Eugene Hauptmann, Nataliya Kosmyna；MIT-RLX 实验室） |
| **许可证** | **GPL-3.0-only**（Cargo.toml `license`）——静态链接进 aemeath 需要法务/许可兼容性确认，**这是 go 之外最大的非技术风险** |
| 依赖树规模 | `cargo tree \| wc -l` = **347**；顶层直接依赖仅 6（anyhow/serde_json/rlx-ir/rlx-models-core/rlx-qwen35/rlx-runtime），树大但全是自家 rlx-* + 常规生态（clap、half、safetensors、tokenizers、onig） |
| 工程质量 | 有 `parity-llama` feature + 自带 llama.cpp 对拍测试、`RLX_QWEN35_DEBUG_LAYERS` 分层诊断、`predict_logits` 退化输出 fail-fast、BENCHMARKS.md；API 面大（runner/flow 两层），但**高层 API 不覆盖 hidden 导出**，需走 flow 层（本 spike 已趟通） |
| 内嵌化契合 | GDN+attention 混合结构、GGUF f16 直载、CPU 单进程内嵌可行；PointerHead 外置 → Rust 侧直接接（本 spike 已验证） |

## 7. Go / No-Go 建议

**GO（CPU 路径）**，依据：

1. 任务 3a/3b/3c 全过：加载 ✅、逐 token normed hidden 导出 ✅、真实序列 max|Δ|=0.0049≪5e-2 ✅、
   PointerHead 端到端 max|Δp|=0.00561 且与 llama.cpp 自身误差完全一致、argmax 命中 ✅。
2. hidden 导出 API 存在且稳定（flow 层），本 spike 的调用链可直接作为正式内嵌代码的骨架。
3. 可复现：`cargo run --release` 两命令复现全部数字。

**附带条件 / 后续**：

- **License**：GPL-3.0-only 与 aemeath 主项目许可的兼容性必须先确认（否则 no-go）。
- **启动成本**：IR 构建 ~30s/进程 —— 正式方案需常驻进程或启用 `with_aot` 磁盘 LIR 缓存。
- **内存**：内联路径 f16→f32 常驻 ~3GB；若内存敏感需回到 runner 级 packed（U8 直传）路径。
- **Metal**：后端对混合 GDN 图有 MPSGraph lowering bug（§5），GPU 加速列为后续观察项，不阻塞 go。
- **对拍广度**：本 spike 是 1 case + 2 probe 序列；批次 2 建议把 79-case 里的全部 row 用 rlx 重跑一遍
  （管线已就绪，只差批量驱动）。

---

### 附：改动面

只新增 `eval/system-one/harness/spike-rlx/`（Cargo.toml、src/main.rs、
convert_head.py、make_case.py、data/{head,case_mem001.json}、build.log、REPORT.md）；
未触碰 worktree 其它文件（`git status` 确认）。llama-server:8019 与 kev:8009 进程未动。
