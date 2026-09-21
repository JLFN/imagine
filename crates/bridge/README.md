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
- `GET /health` — `{"status":"ok","backend":<bool>}`

## Configuration (environment, with repo-defaults shown)

| Variable | Default |
| --- | --- |
| `IMAGEGEN_SD_SERVER` | `bin/sd-server` |
| `IMAGEGEN_DIFFUSION` | `models/qwen-image-2.1-Q4_K_M.gguf` |
| `IMAGEGEN_VAE` | `models/qwen_image_2.1_vae_bf16.safetensors` |
| `IMAGEGEN_LLM` | `models/Qwen3VL-8B-Instruct-Q4_K_M.gguf` |
| `IMAGEGEN_LISTEN_IP` | `127.0.0.1` |
| `IMAGEGEN_LISTEN_PORT` | `8787` |
| `IMAGEGEN_BACKEND_PORT` | `8788` (internal sd-server listener) |
| `IMAGEGEN_STEPS` | `28` |
| `IMAGEGEN_CFG_SCALE` | `6.0` |

Run it from the repository root so the default model/sd-server paths resolve.
The `IMAGEGEN_DIFFUSION` default is `models/qwen-image-2.1-Q4_K_M.gguf`; set
it to `models/qwen-image-2.1-Q6_K.gguf` for the higher-fidelity option used
in the proof below.

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