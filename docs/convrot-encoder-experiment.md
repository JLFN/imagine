# Experiment: the ComfyUI int8 convrot text encoder as the TE

Status: CLOSED, RESULT LOSS. Not merged; dev is untouched. The patch here is
the prerequisite for any safetensors TE that carries weight_scale, but it was
not sufficient to make the int8 encoder run, and the remaining gap is a
missing feature rather than a configuration. The branch is kept as the record.

## Result (2026-09-22)

The patch fixed what it was written to fix. With it applied, the loader reads
the geometry correctly:

    [VERBOSE] llm.hpp:374 - llm: num_layers = 36, vocab_size = 151936,
                            hidden_size = 4096, intermediate_size = 12288

That matches the GGUF encoder exactly, so the config-inference diagnosis was
right. The load then proceeds to 643 of 649 tensors and dies on a second,
different wall:

    what(): type i8 unsupported for integer quantization: no dequantization available
    Aborted (core dumped), exit 134

Why that is not fixable by configuration: src/model_loader.cpp:137 throws from
convert_tensor when the destination type differs from GGML_TYPE_I8, because
int8 tensorwise weights are meant to be consumed in place with per-row scales
and GGML_TYPE_I8 has no to_float trait. That conversion is attempted because
the TE module has no int8 path: the int8_tensorwise and convrot handling lives
in src/model/common/ggml_block.hpp (the DiT's Linear block),
src/model_io/safetensors_io.cpp, src/model_io/tensor_storage.h and
src/model_manager_files.cpp, and appears nowhere in src/model/te/llm.hpp. The
DIFFUSION model runs int8 convrot because its Linear block implements the
rotation, the activation quantization and the int8 GEMM; the LLM's own linear
blocks do not.

So this file cannot be used as the text encoder without implementing the int8
tensorwise path for the LLM blocks - a real feature (the kernels exist for the
other module, so it is wiring plus correctness work in the LLM graph, and it
belongs upstream), not a flag.

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
