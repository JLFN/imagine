# imagegen-bridge

A thin Rust HTTP server that exposes an OpenAI image API and supervises
`stable-diffusion.cpp`'s `sd-server` backend. It is the process open-grok's
[`[toolset.image_gen]`] points at.

## Why it exists

`sd-server` already implements the OpenAI `images/generations` API with
`b64_json` responses and runs warm (the ~9.3 GB Qwen-Image-2.1 model stack
stays resident, so later requests avoid the ~35 s cold start). The bridge
adds:

- Process supervision: it spawns `sd-server` once with the model arguments,
  waits for the backend to be reachable, and kills it on shutdown.
- Field translation: the OpenAI endpoint accepts only `prompt`/`n`/`size`;
  the fields open-grok actually sends are honored by translating the request
  into `sd-server`'s explicit `/sdapi/v1/txt2img` body. `aspect_ratio`
  (`1:1`, `16:9`, ...) and `resolution` (`1k`) map to a WIDTHxHEIGHT
  divisible by 32; `num_inference_steps`, `cfg_scale`, and `seed` map to
  steps/cfg/seed; `n` maps to batch_size.
- Clean errors: malformed input returns an OpenAI-style `error` object with
  a non-2xx status instead of a backend failure leaking through.

## Endpoints

- `GET /v1/models` — `{"data":[{"id":"Qwen-Image-2.1",...}]}`
- `POST /v1/images/generations` — OpenAI shape, returns
  `{"created":...,"data":[{"b64_json":"..."}]}`
- `GET /health` — `{"status":"ok","backend":<bool>,"backend_process_alive":<bool>,"backend_restarts":<n>,"backend_restart_attempts":<n>,"in_flight":<n>,"idle_secs":<n>,"idle_timeout_secs":<n>}`
  The last three exist for the on-demand lifecycle: `in_flight` is what stops a
  stop hook from interrupting a generation, and `idle_secs` against
  `idle_timeout_secs` shows how close the bridge is to releasing itself.
- On-demand lifecycle: the bridge is started when an image is requested and
  releases itself when the requests stop. An open-grok PreToolUse hook on the
  image tools runs `~/.opengrok/hooks/bin/ensure-imagegen-bridge`; the bridge
  exits by itself after `idle_timeout_secs` without a request (`0` disables that,
  for a permanently resident deployment), and a SessionEnd hook runs
  `stop-imagegen-bridge`, which refuses to stop while another session is alive or
  a request is in flight unless given `--force`. `packaging/imagegen-bridge.service`
  is the optional systemd user unit (not enabled on purpose). See
  docs/architecture.md, "On-demand lifecycle".
- Request limits: width and height must be divisible by 32, and no edge may
  exceed `max_edge` (default 1152, the measured ceiling of the pinned layout).
  Larger sizes are rejected with HTTP 400 naming the ceiling and the reason
  rather than forwarded, because the pinned layout cannot allocate its sampling
  graph above 1152 square and the request would otherwise fail inside the backend
  after about 35 s. 1344 square runs only under `--offload-to-cpu`, which cannot
  coexist with the warm pinned backend, so it stays a direct sd-cli job. See
  docs/architecture.md.
## Configuration (environment, with repo-defaults shown)

| Variable | Default |
| --- | --- |
| `IMAGEGEN_SD_SERVER` | `bin/sd-server` |
| `IMAGEGEN_DIFFUSION` | `models/diffusion_models/qwen-image-2.1-Q4_K_M.gguf` |
| `IMAGEGEN_VAE` | `models/vae/qwen_image_2.1_vae_bf16.safetensors` |
| `IMAGEGEN_LLM` | `models/text_encoders/qwen3vl_8b_int8_convrot.safetensors` |
| `IMAGEGEN_LISTEN_IP` | `127.0.0.1` |
| `IMAGEGEN_LISTEN_PORT` | `8787` |
| `IMAGEGEN_BACKEND_PORT` | `8788` (internal sd-server listener) |
| `IMAGEGEN_STEPS` | `28` |
| `IMAGEGEN_CFG_SCALE` | `6.0` |
| `IMAGEGEN_MAX_EDGE` | `1152` (measured ceiling of the pinned layout) |
| `IMAGEGEN_IDLE_TIMEOUT_SECS` | `900` (seconds without a request before the bridge exits; `0` disables) |
| `IMAGEGEN_BACKEND_ARGS` | the pinned placement, see imagegen.toml |

Run it from the repository root so the default model/sd-server paths resolve.
The `IMAGEGEN_DIFFUSION` default is `models/diffusion_models/qwen-image-2.1-Q6_K.gguf`;
`models/diffusion_models/qwen-image-2.1-Q4_K_M.gguf` is the smaller, lower-precision
alternative recommended by the upstream model card.

Model files follow the upstream card's layout, one directory per component:

    models/diffusion_models/   the GGUF diffusion models
    models/text_encoders/      the int8 convrot safetensors encoder (default) and the
                               Q4_K_M GGUF encoder, which needs no patched sd.cpp
    models/vae/                qwen_image_2.1_vae_bf16.safetensors

The default text encoder is the ComfyUI int8 convrot file. It needs the patched
sd-server from patches/0001 and patches/0002 in this repository: stock sd.cpp
aborts on that file, because its config inference misreads the geometry and its
token-embedding loader cannot dequantize int8 weights. Against the GGUF encoder
it replaces, the measured cost is about 5.8 GB more host RAM (10,104.77 MB of
parameters against 4,302.33 MB) for the same VRAM peak, a somewhat higher
conditioning time, and a tie on image quality. Set `IMAGEGEN_LLM` to
`models/text_encoders/Qwen3VL-8B-Instruct-Q4_K_M.gguf` to use the GGUF encoder
with a stock sd-server.

The bridge reaps and restarts its sd-server backend if it dies. `backend_restarts`
in `/health` counts those restarts, so a restarted backend is visible rather than
silent.

## Measured behavior (2026-09-21, RTX 4070 SUPER)

- Default `IMAGEGEN_STEPS=28` is tuned so a warm request completes in ~171 s,
  inside open-grok's 300 s total / 240 s read timeout (verified with the
  exact payload open-grok's ImageGenClient sends). A 40-step default (~235 s)
  sits too close to the 240 s read limit and times out intermittently.
- open-grok on-demand, warm, Q6_K, resolution "1k" (1024x1024), aspect
  "1:1", 28 steps: returned a valid 1024x1024 RGBA PNG in 171 s.
- Higher quality is available outside open-grok's timeout by raising steps
  or resolution directly: Q6_K at 1344x1344, 40 steps fits 12 GB VRAM
  (peak 11.1 GB) at ~11.5 s/it (~8.3 min total). Pure CPU compute is not
  usable (frees VRAM but ~375 s/it).
- The configured key for `[toolset.image_gen]` is a secret-free local string
  (`api_key = "imagegen-local"`); the bridge does not inspect credentials.