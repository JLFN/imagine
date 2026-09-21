# Architecture — local Qwen-Image-2.1 for open-grok

Design verified 2026-09-21. Details here are the pinned facts; anything not
in this file must be re-verified before being trusted.

## Components

1. Runner: stable-diffusion.cpp sd-cli binary (Linux, CUDA).
   - Source: github.com/leejet/stable-diffusion.cpp (day-0 Qwen-Image-2.1,
     2026-09-20). Prebuilt binaries on the releases page, or build from
     source with cmake (requires a CUDA toolkit for the CUDA backend).
   - Backends: CPU (AVX/AVX2/AVX512), CUDA, Vulkan, Metal, OpenCL, SYCL.
2. Model files (see models/MANIFEST.md): diffusion Q4_K_M GGUF 4.6 GiB,
   encoder Qwen3-VL-8B-Instruct Q4_K_M GGUF 5.03 GB, VAE 0.64 GiB.
3. Bridge (not yet written): a small Rust HTTP server that speaks the
   OpenAI image API and manages the sd-cli child process.
4. Wiring: [toolset.image_gen] in ~/.opengrok/config.toml.

## Runner invocation (from stable-diffusion.cpp docs/qwen_image_2.1.md)

Text-to-image (width/height divisible by 32; resolution-dependent flow
schedule is selected automatically):

  sd-cli \
    --diffusion-model models/qwen-image-2.1-Q4_K_M.gguf \
    --vae models/qwen_image_2.1_vae_bf16.safetensors \
    --llm models/Qwen3VL-8B-Instruct-Q4_K_M.gguf \
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

## Quality and open-grok on-demand (Unit 2 findings)

- Quality on a 12 GB card is bounded by VRAM. Q4_K_M @ 1024 peaks ~11.5 GB.
  The practical higher-quality point is the Q6_K diffusion model (5.88 GB,
  near-lossless) with GPU compute and weights RAM-resident
  (--offload-to-cpu): Q6_K @ 1344x1344, 40 steps fits (peak 11.1 GB) at
  ~11.5 s/it (~8.3 min) and gives a clear quality step over 1024/Q4.
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

## Constraints that shape everything

- ds4 (the text engine on the Spark) cannot generate images; llama.cpp
  cannot either. Only stable-diffusion.cpp (or a diffusers/ComfyUI stack,
  ruled out: no GUI desired) runs this model.
- The text encoder is fixed: Qwen3-VL-8B-Instruct. Not substitutable.
- Local is preferred over the Spark: open-grok runs here, and 12 GB VRAM +
  ~15 GB free RAM fits the model stack; no network hop to 192.168.1.91.