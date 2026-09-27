# Experiment: the ComfyUI int8 convrot text encoder as the TE

Status: IMPLEMENTED AND RUNS on branch feat/convrot-encoder-te. Two patches were
needed. Correctness is verified numerically against BF16 ground truth, and the
acceptance criteria below still decide adoption; dev is untouched until then.

## Result (2026-09-22)

The patch fixed what it was written to fix. With it applied, the loader reads
the geometry correctly:

    [VERBOSE] llm.hpp:374 - llm: num_layers = 36, vocab_size = 151936,
                            hidden_size = 4096, intermediate_size = 12288

That matches the GGUF encoder exactly, so the config-inference diagnosis was
right. That fixed the geometry; the load then reached 643 of 649 tensors and
died on a second, independent cause:

    what(): type i8 unsupported for integer quantization: no dequantization available
    Aborted (core dumped), exit 134

Cause 2, the token embedding. int8_tensorwise weights are meant to be consumed in
place by the int8 matmul path, and GGML_TYPE_I8 has no to_float trait, so nothing
can dequantize one. The token embedding is a gather rather than a matmul, so
Embedding::init_params forces its table to F32 when the file stores I8, the
loader then asks for a conversion, and convert_tensor throws. Note the LLM's
linear layers were never the problem: they use the same int8-aware Linear block
the diffusion model uses, and no int8 path had to be written for them.

The fix (patches/0002-...) adds dequantize_int8_tensorwise to the loader. It
reads the companion row scales from the same file with the file handle the worker
already holds, applies one scale per output row, then undoes the offline convrot
rotation with the normalized regular Hadamard, copied verbatim from
ggml_regular_hadamard_group_f32 in ggml/src/ggml-cpu/ggml-cpu.c. That transform is
its own inverse, which is why applying it once more undoes the rotation.

The rotation must be undone here rather than left to the runtime. A downstream
quantized linear does rotate its input back, but a learned-weight RMSNorm sits
between the embedding and the first quantized linear, so the rotation would
survive it.

## Correctness verification (not merely "it ran")

Ground truth is the same model's weights in BF16 (Comfy-Org/Qwen-Image-2.1,
text_encoders/qwen3vl_8b_bf16.safetensors), fetched by HTTP byte range so only
the header and the tensors under test were downloaded. Reconstructing W from the
int8 file the way the patch does - per-row scale, then the rotation undo - gives:

    model.layers.0.self_attn.q_proj.weight  pearson 0.999963  rel_l2 0.0086
    model.embed_tokens.weight               pearson 0.999961  rel_l2 0.0089
    control, rotation NOT undone            rel_l2 1.3734

0.0086-0.0089 is int8 quantization noise, and the control shows the undo is doing
real work rather than being a no-op.

Reproduce with the Rust verifier in this repository, crates/convrot-verify:

    bin/convrot-verify [path/to/qwen3vl_8b_int8_convrot.safetensors]

It prints the same numbers for the same two modules, adds a verdict line, and
exits non-zero if a module disagrees with its BF16 ground truth. The experiment's
Python script was ported to it 1:1 and removed (global guardrail rule 14); the
port reproduces every printed digit above, including the wrong-rotation control.

End to end, CPU-only: the encoder loads all 649 of its tensors, conditioning
completes, and an image is produced. Conditioning cost on the same CPU footing and
the same prompt: int8 convrot 9.69 s against the Q4_K_M GGUF's 10.29 s.

Measured cost of the attempt, before the failure: the encoder's parameters
report as 10,104.77 MB in RAM, against the GGUF encoder's 4,302.33 MB. So even
if it ran, the int8 encoder would spend about 5.8 GB more host RAM than the
GGUF it would replace.

## The three encoder artifacts, characterized (2026-09-22)

Read from the safetensors headers over ranged HTTP fetches; hosts and shas
verified where downloaded.

- qwen3vl_8b_int8_convrot.safetensors, 8.71 GiB: 1258 tensors, I8 weights plus
  weight_scale plus comfy_quant. Needs the patch AND the missing TE int8 path.
  BLOCKED.
- qwen3vl_8b_w4a8.safetensors, 5.88 GiB: 1762 tensors, dtypes F32 506, BF16
  496, F8_E4M3 252, I8 254, U8 254, with 254 comfy_quant and 2 weight_scale.
  Hits BOTH walls (geometry and I8), plus F8_E4M3. BLOCKED.
- qwen3vl_8b_bf16.safetensors, 16.33 GiB: 750 tensors, every one BF16, zero
  weight_scale, zero comfy_quant. It hits NEITHER wall, so it should load
  against UNPATCHED sd.cpp, and it is the model's native precision. The cost is
  the reason it was never tried: roughly 16.3 GiB of host RAM for the encoder,
  about 12 GB more than the GGUF. Untested - it needs a 17.5 GB download.

That last one is the only remaining route to a higher-fidelity encoder in RAM,
and it is a memory-for-quality trade, not a saving. Deciding it needs a
download and a quality comparison against the current GGUF baseline.

## Verdict: measured as a loss, adopted by operator decision (2026-09-22)

The operator chose to run this encoder as the deployed default anyway, on the
grounds that it is the configuration the upstream model card recommends and that
it reconstructs the model's weights more faithfully than the GGUF encoder does.
That decision is recorded here as a decision, not as a measurement: the numbers
below stand, and they say the change costs memory without a quality gain that any
test here could see.

Measured on the GPU with the shipped placement, back to back in the same
session, 1024x1024 / 28 steps / seed 42, same prompt, only the encoder varying.
Both sides pin the diffusion model in VRAM, keep the encoder on the CPU and the
VAE parameters in RAM.

With the deployed Q6_K diffusion model:

    encoder            conditioning  sampling  decode  total    peak VRAM  host RSS
    Q4_K_M GGUF (now)      10.26 s    143.88 s  5.43 s  159.60 s   9,395 MiB  6.6 GB
    int8 convrot           22.66 s    141.83 s  4.11 s  168.64 s   9,397 MiB 11.7 GB

With their recommended Q4_K_M diffusion model:

    encoder            conditioning  sampling  decode  total    peak VRAM  host RSS
    Q4_K_M GGUF (now)      13.54 s    139.53 s  5.08 s  158.18 s   8,163 MiB  6.6 GB
    int8 convrot           16.71 s    130.08 s  4.12 s  150.93 s   8,174 MiB 11.7 GB

Against the pre-registered criteria:

1. Loads and completes: PASS.
2. Encoder parameters in RAM, not VRAM: PASS (10,104.77 MB RAM, 0 MB VRAM).
3. Peak VRAM no worse: PASS (identical within noise).
4. Total time within +5 percent: FAIL in the Q6_K pair (168.64 s against
   159.60 s, +5.7 percent) and inconclusive in the Q4_K_M pair, where the
   candidate came out 7.2 s faster on a sampling phase that must be identical
   because the diffusion model is the same - that difference is run-to-run
   variance, not an encoder effect. The conditioning deltas are also unstable
   (+12.4 s and +3.2 s in the two pairs, while the same GGUF encoder measured
   2.62 s, 10.26 s and 13.54 s across runs), so the honest reading is that the
   int8 encoder's conditioning is somewhat more expensive and the measurement
   cannot resolve it tightly.
5. Quality strictly better: FAIL, by the tie rule this document set. Both
   images render the requested literal text correctly. Graded blind by a
   separate vision model (the two outputs were presented as A and B with no
   indication of which encoder produced them), both transcribed qwen2.1.cpp
   fully legibly and both scored 10/10 on prompt adherence.

So the encoding is correct and the file runs, but it buys nothing that the
measurement can see while costing about 5.8 GB more encoder parameters (host RSS
6.6 GB to 11.7 GB) and an unresolved amount of extra conditioning time. It was the
deployed default from 2026-09-22 by operator decision, with the GGUF encoder kept
one line away in imagegen.toml; that decision was reversed and the file removed on
2026-09-27 - see "Reversal" below, which also states what the measurement above
does and does not cover. On the diffusion side, their recommended Q4_K_M
is a trade rather than a win on this box: 158.18 s and 8,163 MiB against Q6_K's
159.60 s and 9,395 MiB, i.e. the same speed, about 1.2 GiB less VRAM, and less
DiT precision. Q6_K fits comfortably at 9,395 of 11,893 MiB, so it stays.

## Reversal (2026-09-27): reverted to the GGUF encoder, convrot deleted

The operator reported visible artifacts in the generations of the deployment that
carried this encoder, and none after the GGUF encoder was put back, on the same
prompts. The change applied, all verified live rather than assumed:

    llm (imagegen.toml)   qwen3vl_8b_int8_convrot.safetensors
                          -> Qwen3VL-8B-Instruct-Q4_K_M.gguf
    diffusion             qwen-image-2.1-Q6_K.gguf (base weights)
    vae compute           CPU, in both --backend and --params-backend

The captured backend command line from the session that produced the retained
reference images (2026-09-26, session 01a0ddef) is the target configuration, and
the live process now prints it character for character: diffusion Q6_K, VAE
qwen_image_2.1_vae_bf16, `--llm .../Qwen3VL-8B-Instruct-Q4_K_M.gguf`, backend
`te=cpu,diffusion=cuda0,vae=cpu` and the same for params-backend.

What supports the attribution, and what does not:

- The diffusion GGUF, the VAE and the sd-server binary are unchanged since
  2026-09-21/22, so the encoder is the only component that differed between the
  artifacts and the clean images.
- The bridge binary cannot account for it: a controlled A/B with a fixed request
  (512 square, 8 steps, seed 4242) through today's build and through a build of
  the 2026-09-26 dev tip (a9e35b6) produced byte-identical output, sha256
  e9d27abcfd8e660d725ff95c75835e3c9741959874275edbc3ef55e9f95a614f on both.
- The GGUF encoder is the component models/MANIFEST.md marks fixed, with the
  standing warning that another encoder gives incoherent generations.
- LIMIT, stated plainly: this section does not overturn the 2026-09-22
  measurement above. That measurement recorded a quality tie, but its probe was a
  text-rendering prompt ("qwen2.1.cpp" transcribed correctly) scored for legibility,
  which is not the surface an artifact report is about. The artifact-bearing
  prompt was never run as a fixed-seed A/B across the two encoders, so the
  encoder defect is unproven; what is proven is that the report correlates with
  the encoder swap and that the swap is reverted.

The convrot file has been deleted from disk (9,350,798,360 bytes freed). Its
recovery path is its size and sha256, both recorded in models/MANIFEST.md, plus
the public sources named there (Comfy-Org/Qwen-Image-2.1 text_encoders/, mirrored
at KasugaiSakura/Qwen-Image-2.1-Uncensored-GGUF). The patches it requires
(patches/0001, patches/0002) stay in the repository, and
tools/convrot-te-experiment.sh remains the script that measures it.

If this is revisited, the cheap decisive test is the one that was not run: one
artifact-bearing prompt, one seed, both encoders, everything else fixed.

### The mmap gap, found only in production

sd-cli and sd-server do not read weights the same way. sd-cli opens a plain file;
sd-server mmaps them. The first version of patch 0002 implemented only the
plain-file branch, gated on `mmapped == nullptr`, so the embedding dequantization
silently did not engage under the server. The deployment test caught it exactly:
a 1024-square request returned HTTP 502, and the backend log ended with

    what(): type i8 unsupported for integer quantization: no dequantization available
    WARN sd-server backend exited (signal: 6 (SIGABRT) (core dumped))

The patch now reads the companion row scales from whichever source the weights
came from - mmap via MmapWrapper::copy_data, or a plain file via seek and read -
and the zip path logs an explicit error instead of falling through to a throw.
This is the difference between a change that works in a test harness and one that
works in the deployed service, and it is why the proof of a deployment has to be
a real request through the real process, not a CLI run.

The two patches remain worth having: patch 0001 fixes a real upstream defect that
misreads the geometry of any safetensors text encoder carrying a weight_scale
tensor, and the embedding work is what any weights-only text encoder would need.

## What is being tried

Using models/text_encoders/qwen3vl_8b_int8_convrot.safetensors (9,350,798,360
bytes, sha256 8bfd0f6e12abf2d2d697ecc888e5e90b0d6741d6708f05799f53afa560452e8f,
Comfy-Org/Qwen-Image-2.1) as the sd.cpp text encoder in place of the current
models/Qwen3VL-8B-Instruct-Q4_K_M.gguf, with its parameters in system RAM and
its graph executing on the CPU, exactly as the diffusion model is arranged
today (see docs/architecture.md, Parameter placement).

Why it might win: int8 weights are a finer quantization than the Q4_K_M GGUF
(4.68 GiB), so conditioning fidelity could improve (literal text in the image
and prompt adherence are the usual places this shows). Why it might not: it is
8.71 GiB instead of 4.68 GiB, so it costs about 4.03 GiB more host RAM for
possibly no measurable gain.

## The blocker and the fix

Without a code change the file cannot be loaded at all; sd.cpp aborts. The
cause is upstream, in src/model/te/llm.hpp: the config inference reads the
hidden and vocab dimensions from any tensor whose name CONTAINS
"embed_tokens.weight", which also matches the quantized companion
<module>.weight_scale. That companion sorts after the real weight, so it
overwrites the geometry with the scale tensor's shape, and every downstream
Linear is built wrong (GGML_ASSERT in ggml_block.hpp:173, exit 134).

patches/0001-sd-cpp-llm-te-config-skip-weight-scale.patch excludes the scale
companion. It cannot regress the GGUF encoder, whose quantized weights are
self-contained and carry no weight_scale tensor. The patch has NOT been sent
upstream.

## Acceptance criteria (pre-registered, all must hold)

Baseline is the shipped configuration measured 2026-09-22 on this box at
1024x1024 / 28 steps / seed 42: Q6_K pinned in VRAM, GGUF encoder on CPU, VAE
parameters in RAM. Baseline numbers: sampling 146.5 s, total 153.9 s, peak
VRAM 9,426 MiB, host RSS about 6.2 GiB.

1. HARD: the encoder loads and a 1024x1024 / 28 step generation completes with
   exit 0 and a written image.
2. HARD: the placement line reports the text encoder's parameters in RAM, not
   VRAM (the operator's stated constraint).
3. NO REGRESSION: peak VRAM at or below 9,426 MiB.
4. NO REGRESSION: total generate_image time at or below 161.6 s (baseline
   +5 percent), covering the extra conditioning cost of a larger encoder on
   the CPU.
5. THE WIN: better prompt and literal-text adherence than the baseline on the
   same prompt and seed, judged on the images themselves. A tie on quality is
   a LOSS, because the candidate's only cost justification is quality: it
   spends 4.03 GiB more host RAM and does not save VRAM.

If criterion 5 cannot be demonstrated, the branch is not merged.

## Cost if it wins

About 4.03 GiB more resident host memory (encoder 8.71 GiB against 4.68 GiB),
a patched sd.cpp that must be re-applied on every upstream update, and a
non-upstream code change in the delivery path.

## Reproduction

    # apply (in the sd.cpp tree at /data/build-sd/stable-diffusion.cpp)
    #   edit src/model/te/llm.hpp as in patches/0001-*.patch
    cd /data/build-sd/stable-diffusion.cpp/build
    cmake --build . --target sd-cli sd-server -j 12

    # measure candidate against baseline (stops nothing; see the harness)
    bash tools/convrot-te-experiment.sh

The harness needs exclusive use of the GPU: the production bridge pins 5.6 GB
in VRAM, so stop it first (kill -INT on bin/imagegen-bridge) and restart it
afterwards. It builds nothing and never touches /data/imagegen/bin.
