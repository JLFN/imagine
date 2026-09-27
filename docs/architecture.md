# Architecture — local Qwen-Image-2.1 for open-grok

Design verified 2026-09-21. Details here are the pinned facts; anything not
in this file must be re-verified before being trusted.

## Components

1. Runner: stable-diffusion.cpp sd-cli binary (Linux, CUDA).
   - Source: github.com/leejet/stable-diffusion.cpp (day-0 Qwen-Image-2.1,
     2026-09-20). Prebuilt binaries on the releases page, or build from
     source with cmake (requires a CUDA toolkit for the CUDA backend).
   - Backends: CPU (AVX/AVX2/AVX512), CUDA, Vulkan, Metal, OpenCL, SYCL.
2. Model files (see models/MANIFEST.md): diffusion Q6_K GGUF 5.47 GiB
   (the deployed default until 2026-09-27, and still the better generator when
   no adapter is configured; Q4_K_M at 4.29 GiB is also on disk and is what the
   few-step turbo path deploys, see "Few-step turbo path" below), encoder
   Qwen3-VL-8B-Instruct Q4_K_M GGUF 4.68 GiB, VAE 0.63 GiB. Note: earlier
   revisions of this file and of MANIFEST.md quoted Q4_K_M as "4.6 GiB" and
   the encoder as "5.03 GB" - those are decimal GB, not GiB (4.60 GB and
   5.03 GB respectively).
3. Bridge (not yet written): a small Rust HTTP server that speaks the
   OpenAI image API and manages the sd-cli child process.
4. Wiring: [toolset.image_gen] in ~/.opengrok/config.toml.

## Runner invocation (from stable-diffusion.cpp docs/qwen_image_2.1.md)

Text-to-image (width/height divisible by 32; resolution-dependent flow
schedule is selected automatically):

  sd-cli \
    --diffusion-model models/diffusion_models/qwen-image-2.1-Q4_K_M.gguf \
    --vae models/vae/qwen_image_2.1_vae_bf16.safetensors \
    --llm models/text_encoders/Qwen3VL-8B-Instruct-Q4_K_M.gguf \
    -p "<prompt>" --cfg-scale 6.0 --sampling-method euler \
    -v --offload-to-cpu -o out/image.png

- --offload-to-cpu keeps the encoder in system RAM (recommended layout).
- Image editing (later phase, optional) adds -r <ref.png> for each reference
  image, plus --llm_vision <mmproj GGUF> when the text encoder is GGUF.

## Measured performance (Unit 1, 2026-09-21, this box)

Verified invocation: the text-to-image command above with -W 1024 -H 1024,
--steps 40, --cfg-scale 6.0, euler, --seed 42, --offload-to-cpu. GPU: RTX
4070 SUPER (compute 8.9). Runner: sd-cli built from source master-890-74988b2-
4-g6dcb5bb with -DSD_CUDA=ON (sm_89). CUDA toolkit 13.3.

- Total params 9337.96 MB, all in RAM (VRAM 0.00 MB at load): text encoder
  4302.33 MB, diffusion 4391.25 MB, VAE 644.38 MB.
- First real image (40 steps): generate_image completed in 216.61 s, of which
  sampling 199.49 s (~4.6-4.9 s/it after the first ~16.8 s warm step);
  process wall 3:38, max RSS ~10.04 GB. Output 1024x1024 RGBA, 1.87 MB.
- Reproducibility run (12 steps, seed 7): sampling 70.30 s, generate_image
  83.77 s, output 1.83 MB.
- Peak VRAM during 1024x1024 generation: 11,536 MiB of 12,282 (~94%).
  Weights are offloaded to RAM but the compute/attention buffers saturate the
  card at 1024 sq; higher resolutions or a larger batch would OOM (confirms
  "native 2K is tight"). Cold start (loading ~9.3 GB weights) is the dominant
  cost for an idle process; a warm process is ~4.6 s/it.
- Model load ~12.7 s (encoder), ~10.7 s (diffusion), ~1.2 s (VAE);
  condition forward ~13.9 s.

## Parameter placement (measured 2026-09-22)

sd.cpp's `--backend` selects the module that executes a module's graphs and
`--params-backend` selects where its parameters live (module names: `te`,
`diffusion`, `vae`). Measured on this box at 1024x1024 / 28 steps / seed 42,
same prompt, peak VRAM from a 1 s nvidia-smi poll, host RSS from
/usr/bin/time -v. Times are sd-cli `generate_image` (includes the load).

  layout                          sampling   VAE decode   total    peak VRAM   host RSS
  --offload-to-cpu, Q4_K_M        131.5 s    2.1 s        135.2 s  11,572 MiB  10.6 GiB
  DiT pinned, encoder CPU, Q4_K_M 132.9 s    4.4 s tiled  139.4 s   8,172 MiB   6.2 GiB
  all pinned, encoder on GPU      -          -            failed       -           -
  Q6_K pinned, encoder CPU, VAE
    parameters in RAM (base path) 146.5 s    4.7 s tiled  153.9 s   9,426 MiB      -

What the measurements establish:

- Pinning the diffusion model costs nothing in sampling time at this
  resolution (131.5 s to 132.9 s). The staging it removes is hidden by
  prefetch once a run is long enough; at 4 steps the same change took a
  512-square run from 25.2 s to 10.7 s.
- The text encoder must not be pinned: with all three modules in VRAM the
  diffusion model fails during weight preparation at block 28 of 34 and the
  run dies. Its parameters in RAM with the graph on the CPU costs 0.5-1.1 s
  more conditioning (1.38-2.62 s, once per prompt).
- The VAE decode falls back to spatial tiling (9 tiles, ~2.4 s per tile) at
  1024 square when the DiT is pinned: 4.4-4.7 s instead of 2.1 s. This is
  resolution dependent - at 512 square the same layout decoded untiled in
  1.43 s with no tile passes - and it is the pinned DiT weights starving the
  decode, not the VAE's own parameters: moving those to RAM does not avoid
  the fallback. Budget ~2.5 s per 1024-square request for it, and treat the
  tiled output as visually unverified against the untiled decode.
- The bridge passes `--backend te=cpu,diffusion=cuda0,vae=cuda0
  --params-backend te=cpu,diffusion=cuda0,vae=cpu` (overridable with
  IMAGEGEN_BACKEND_ARGS). Through the bridge, a first 1024-square/28-step
  request returned HTTP 200 with a 1024x1024 PNG in 143.6 s, inside
  open-grok's 240 s read timeout.
- 1344-square was measured earlier only with --offload-to-cpu (peak 11.1 GB).
  With the DiT pinned permanently the same tier is UNVERIFIED and expected to
  be too tight; measure before claiming it.

Layout is not bit-reproducible: the same seed and prompt under two layouts
differed on 90% of channel bytes (mean absolute difference 6.07 of 255).
Structurally identical images, but do not expect byte equality across
placement changes.

## Configuration and supervision (2026-09-22)

Model paths are not compiled into the bridge. They live in imagegen.toml at the
repository root, which the bridge reads from its working directory (point
IMAGEGEN_CONFIG elsewhere to override). A missing model path is a startup error
naming the file and the variable, never a built-in guess: silently loading a
different model is worse than refusing to start. Environment variables override
the file one value at a time, which is how a test or an operator changes a single
path without editing the file. The effective configuration is logged at startup.

Model files follow the upstream card's layout, one directory per component:
models/diffusion_models/ for the GGUF diffusion models, models/text_encoders/ for
the text encoders, models/vae/ for the VAE. The files themselves are unchanged and
hash-match the published checksums.

The default text encoder is the ComfyUI int8 convrot file the upstream card
recommends, which is why the deployed sd-server is the patched build from
patches/0001 and patches/0002: stock sd.cpp aborts on that file. The GGUF encoder
stays on disk at models/text_encoders/Qwen3VL-8B-Instruct-Q4_K_M.gguf and needs
no patches; set IMAGEGEN_LLM to it (or edit imagegen.toml) to go back. Its
measured cost against the GGUF encoder is about 5.8 GB more host RAM for the same
VRAM peak, a somewhat higher conditioning time, and a tie on image quality.

The bridge supervises its backend. It reaps the child, and if the process dies it
restarts it with exponential backoff and waits for it to answer before calling it
ready. /health reports backend (an HTTP probe of the backend), backend_process_alive
(the child's state), backend_restarts (healthy replacements) and
backend_restart_attempts (spawns attempted after a death). Before this, the child
was spawned once and never waited on: on 2026-09-22 the bridge ran 8h51m with a
zombie sd-server, a dead backend port and every image request failing silently.

The listener binds before the first readiness wait, so /health answers during a
startup where the backend never becomes ready, reporting backend false rather
than leaving the bridge unreachable. Two limits are worth knowing:

- Supervision keys on process liveness. A backend that stays ALIVE but never
  answers HTTP is visible in /health as backend false but is not auto-restarted.
  Restarting on a failed probe is future work.
- Readiness is an HTTP probe, so it passes as soon as sd-server binds its port,
  which happens before weights are loaded. The first request after a restart
  therefore pays the full model staging cost.

Requests are bounded: width and height must be divisible by 32 and at most 1536
on either edge, checked on the final size from BOTH request paths (an explicit
size, and open-grok's aspect_ratio plus resolution). A larger request receives
HTTP 400 naming the limit, because it cannot fit this card and would otherwise
fail the generation or abort the backend.

## Quality and open-grok on-demand (Unit 2 findings)

- Quality on a 12 GB card is bounded by VRAM. Q4_K_M @ 1024 peaks ~11.5 GB.
  The practical higher-quality point is the Q6_K diffusion model (5.88 GB,
  near-lossless): parameters pinned in VRAM
  with the arrangement described under Parameter placement above. It is the
  default for the base path, but not once an adapter is configured - see
  "Few-step turbo path" below. The
  figures in the next sentence were measured with --offload-to-cpu (weights
  RAM-resident, staged on demand): Q6_K @ 1344x1344, 40 steps fits (peak
  11.1 GB) at ~11.5 s/it (~8.3 min) and gives a clear quality step over
  1024/Q4. That 1344 tier is unverified under the pinned default.
- Pure CPU compute is a dead end for on-demand: it frees VRAM (~5 GB peak)
  but runs the 7B DiT at ~375 s/it, so higher resolution via full CPU is
  not usable.
- open-grok's image client (ImageGenClient) sends a specific payload and has
  a hard 300 s total / 240 s read timeout. For reliable on-demand, the
  request must finish in well under ~240 s, so the bridge defaults to 28
  steps (warm ~171 s). Request fields it honors: model, prompt, n,
  aspect_ratio, resolution, response_format (b64_json), num_inference_steps,
  seed, cfg_scale. Higher-res/longer generations are available outside
  open-grok by requesting more steps or a larger size directly.
- Wiring: [toolset.image_gen] in ~/.opengrok/config.toml -> base_url
  http://127.0.0.1:8787/v1, api_key "imagegen-local" (secret-free; the
  bridge ignores auth), model "Qwen-Image-2.1", response_encoding auto.

## Bridge design

- Endpoints:
  GET  /v1/models                        -> {"data":[{"id":"Qwen-Image-2.1"}]}
  POST /v1/images/generations            -> {"data":[{"b64_json": "..."}]}
- Request fields to support: prompt, size (e.g. "1024x1024"), n (1),
  num_inference_steps (default 40), seed (optional).
- Process model (as implemented): the bridge spawns sd-server once at
  startup (diffusion model, text encoder, VAE; --offload-to-cpu) and keeps
  it warm: the ~9.3 GB model stack stays resident, so every request after
  the initial load is warm. The bridge polls the backend until it answers
  (up to 180 s) before serving, and kills sd-server when the bridge is
  shut down (SIGINT / ctrl-c). There is no idle reaper; the backend lives
  as long as the bridge.
- Cold start cost: one-time at bridge startup (~35 s to load ~10 GB of
  weights); per-request cost is generation time only (~171 s at 28 steps
  and 1024 square warm; see Measured behavior). The open-grok image tool
  timeout (300 s total / 240 s read) tolerates warm requests comfortably.
- Alternative (zero resident processes): systemd user socket activation.
  The user systemd session is running (systemctl --user is-system-running
  returned running on 2026-09-21). Preferred: the bridge daemon, because it
  is simpler to control and debug.

## Wiring (verified against open-grok source)

The block that redirects the built-in image_gen/image_edit tools:

  [toolset.image_gen]
  enabled = true
  base_url = "http://127.0.0.1:8787/v1"
  api_key = "imagegen-local"
  model = "Qwen-Image-2.1"
  response_encoding = "auto"
  image_gen_enabled = true
  image_edit_enabled = true

Source-verified semantics:
- Active only when base_url is non-empty AND a key resolves (api_key field,
  or env_key naming an env var). The key is required even for a keyless
  local endpoint.
- The configured key, never the session bearer, authenticates the request.
- response_encoding "auto" accepts both b64_json and url responses; b64_json
  is what the bridge returns.
- model_override naming the endpoint's model id is optional; the example
  above sets model so open-grok names it.

## Few-step turbo path (2026-09-27)

The deployed generator is the Viggle Qwen-Image-2.1 turbo adapter, a DMD
distillation of the same base transformer applied at runtime, sampled with a
fixed six-node sigma schedule and no classifier-free guidance. Full recipe,
checksums and the file table: docs/viggle-turbo-lora.md.

What it changes in this deployment:

- Diffusion model Q4_K_M instead of Q6_K. The adapter is applied at runtime
  (merged mode is refused: "INT8 tensorwise weights do not support the
  immediately LoRA apply mode; using at_runtime instead", forced by the int8
  convrot text encoder), and the per-segment tables it needs cost about 2.4 GB
  of device memory. With Q6_K the sampling step itself fails: "model manager
  cannot make enough memory available on CUDA0: need 767.11 MB device /
  733.12 MB available", then segment 29/34 (transformer_blocks.27) fails during
  weight preparation.
- VAE tiles of 16x16 latent units. With the default 32x32 tiles the decode misses
  its budget by 3 MB (need 512.00 MB, 508.69 MB free) and the request ends in a
  500; smaller tiles are not slower here (about 32 s either way), because the
  pressure of the large tiles cost more than the extra tile count. Moving the VAE
  to the CPU also fits but costs about 116 s instead of 32 s.
- The bridge sends the adapter as a structured `lora` entry and the schedule as
  `sample_params.custom_sigmas` inside `sd_cpp_extra_args`, and primes the
  server's lazily built LoRA cache so the first request cannot fail with
  "invalid lora path". See crates/bridge/README.md.

Measured end to end through the bridge (1024x672 from a 3:2 request, cold start):
sampling 33.0 s at 1.51 s/it, whole request 77.3 s, `(454 / 454) LoRA tensors
have been applied`. The same prompt at 1024 square on the base 28-step path took
168.8 s of generation.

## Constraints that shape everything

- ds4 (the text engine on the Spark) cannot generate images; llama.cpp
  cannot either. Only stable-diffusion.cpp (or a diffusers/ComfyUI stack,
  ruled out: no GUI desired) runs this model.
- The text encoder is fixed: Qwen3-VL-8B-Instruct. Not substitutable.
- Local is preferred over the Spark: open-grok runs here, and 12 GB VRAM +
  ~15 GB free RAM fits the model stack; no network hop to 192.168.1.91.