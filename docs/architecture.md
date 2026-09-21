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

## Bridge design

- Endpoints:
  GET  /v1/models                        -> {"data":[{"id":"Qwen-Image-2.1"}]}
  POST /v1/images/generations            -> {"data":[{"b64_json": "..."}]}
- Request fields to support: prompt, size (e.g. "1024x1024"), n (1),
  num_inference_steps (default 40), seed (optional).
- Process model: no sd-cli running at idle. First POST spawns sd-cli, waits
  for the file, returns it, and starts an idle timer (default ~5 min). Any
  request resets the timer; on expiry the child is killed so the ~6-7 GB of
  model memory is released.
- Cold start cost: reading ~10 GB of weights + generation; the first request
  in a while is slower, subsequent ones (warm) are fast. Verify the
  open-grok image tool timeout tolerates the cold start; if not, either
  raise the client timeout or warm up a few seconds at session start.
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