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
  `{"created":...,"data":[{"b64_json":"..."}]}`. An optional `"turbo": true|false`
  in the body pins this one request to a sampling mode.
- `GET /v1/turbo` — the live sampling mode and what the next request will use:
  `{"mode":"base"|"turbo","turbo":<bool>,"turbo_available":<bool>,"lora":<name>,"sigmas":<list>,"steps":<n>,"cfg_scale":<n>,"lora_visible":<bool|null>}`
- `POST /v1/turbo` — switch the mode: `{"enabled": true|false}` or
  `{"mode": "turbo"|"base"}`. Enabling turbo is refused (HTTP 409, mode
  unchanged) unless the adapter is configured, on disk, and confirmed by the
  backend. The mode is process state, not persisted.
- `GET /health` — `{"status":"ok","backend":<bool>,"backend_process_alive":<bool>,"backend_restarts":<n>,"backend_restart_attempts":<n>,"mode":"base"|"turbo","turbo_available":<bool>,"lora":<name>,"lora_visible":<bool|null>,"sigmas":<list>,"steps":<n>,"cfg_scale":<n>}`
  `mode`, `lora`, `sigmas`, `steps` and `cfg_scale` describe the LIVE mode and
  the next request; `lora_visible` answers whether the backend lists the
  configured adapter, and is null until the backend has been asked.
- Request limits: width and height must be divisible by 32 and at most 1536 on
  either edge. Larger sizes are rejected with HTTP 400 rather than forwarded,
  because they cannot fit this card and would abort the backend.

## Configuration (environment, with repo-defaults shown)

Model and binary paths have no value built into the binary: they come from
`imagegen.toml` (read from the working directory) or the environment, and a
missing one is a startup error. The table below gives the values this repository
ships plus the built-in fallbacks where one exists.

| Variable | Repo default (`imagegen.toml`) | Built-in fallback |
| --- | --- | --- |
| `IMAGEGEN_SD_SERVER` | `bin/sd-server` | none |
| `IMAGEGEN_DIFFUSION` | `models/diffusion_models/qwen-image-2.1-Q4_K_M.gguf` | none |
| `IMAGEGEN_VAE` | `models/vae/qwen_image_2.1_vae_bf16.safetensors` | none |
| `IMAGEGEN_LLM` | `models/text_encoders/qwen3vl_8b_int8_convrot.safetensors` | none |
| `IMAGEGEN_LISTEN_IP` | `127.0.0.1` | `127.0.0.1` |
| `IMAGEGEN_LISTEN_PORT` | `8787` | `8787` |
| `IMAGEGEN_BACKEND_PORT` | `8788` (internal sd-server listener) | `8788` |
| `IMAGEGEN_STEPS` | `28` | `28` |
| `IMAGEGEN_CFG_SCALE` | `6.0` | `6.0` |
| `IMAGEGEN_TURBO` | `false` (start in base mode) | `false` |
| `IMAGEGEN_TURBO_CFG_SCALE` | `1.0` | `1.0` |
| `IMAGEGEN_LORA_DIR` | `models/loras` | none; required when a LoRA is set |
| `IMAGEGEN_LORA` | the Viggle turbo rank-128 adapter | none (turbo unavailable) |
| `IMAGEGEN_LORA_MULTIPLIER` | `1.0` | `1.0` |
| `IMAGEGEN_SIGMAS` | `1.0,0.9375,0.875,0.75,0.5,0.25,0.0` | none (turbo unavailable) |

Run it from the repository root so the default model/sd-server paths resolve.
The diffusion model in the repository default is Q4_K_M rather than the
higher-quality Q6_K because of the adapter: with a runtime LoRA applied, Q6_K
cannot fit a 1024-square generation on this card (measured 2026-09-27). The mode
toggle does not swap models or tile sizes, so base mode runs on the turbo-tuned
setup as well; a base-only deployment swaps Q6_K back in and drops the small VAE
tiles, at the price of a restart to use turbo. See docs/viggle-turbo-lora.md.

The base model is the default and needs no environment juggling: the adapter and
its schedule are used only in turbo mode. To start a bridge in turbo mode set
`IMAGEGEN_TURBO=true`; to switch a running bridge use `POST /v1/turbo` (or the
`packaging/hooks/imagegen-turbo on|off|status` script).

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

## Sampling modes: base and turbo

One process serves two sampling profiles:

- BASE (the default) is the base model: no adapter, sd.cpp's own scheduler,
  `IMAGEGEN_STEPS` (28) and `IMAGEGEN_CFG_SCALE` (6.0).
- TURBO is the Viggle Qwen-Image-2.1 few-step distillation: the adapter, its
  published sigma schedule (which is also where its step count comes from) and
  `IMAGEGEN_TURBO_CFG_SCALE` (1.0).

The live mode is reported by `GET /v1/turbo` and `GET /health`, switched with
`POST /v1/turbo`, or pinned for one request with `"turbo": true` in the
generation body. `packaging/hooks/imagegen-turbo on|off|status` is the shell-side
wrapper, and it starts the bridge if it is not up.

Switching turbo ON is verified rather than assumed: the bridge asks the backend
for its adapter list, requires the configured adapter to be in it, and only then
flips the mode. A bridge that cannot confirm the adapter keeps serving base and
answers HTTP 409 with the reason. The mode is process state; a restart returns to
`IMAGEGEN_TURBO` from imagegen.toml.

One caveat stated plainly: the model file and the backend placement flags are
fixed when sd-server starts, so the toggle changes the sampling profile, not the
model. This repository's Q4_K_M plus 16x16-tile setup is tuned for turbo, and
base mode inherits it; a base-only deployment is described in imagegen.toml.

## Few-step turbo adapter (Viggle Qwen-Image-2.1 turbo)

Turbo mode is what uses the adapter and the schedule; a base request sends
neither. Recipe, file list and checksums: docs/viggle-turbo-lora.md.
Three details are not obvious:

1. The adapter travels as the structured `lora` field. sd-server deliberately
   ignores `<lora:...>` prompt tags; it accepts `lora[].path` and
   `lora[].multiplier`, and the path must match what its own scan of
   `--lora-model-dir` produced, which is a path relative to that directory. The
   bridge hands the backend `--lora-model-dir` and sends the relative name.
2. The schedule travels inside the prompt. The sdapi route has no sigma field, so
   the bridge appends
   `<sd_cpp_extra_args>{"sample_params":{"custom_sigmas":[...]}}</sd_cpp_extra_args>`;
   the server extracts the JSON, and removes the tag before generation. The step
   count then follows the list (sd.cpp derives steps = len - 1), so a `steps`
   value that disagrees is ignored and logged. The published Viggle schedule is
   six nodes, which is why the configured list has seven values: the trailing 0.0
   is what makes it six denoise steps instead of five.
3. The first request used to fail. sd-server builds its LoRA cache lazily and its
   sdapi generation route never refreshes it, so a cold backend answered
   `invalid lora path` until something asked for the adapter list. The bridge
   primes that cache when the backend becomes ready, and again before the first
   turbo generation (and before a turbo switch) if that has not happened yet, and
   reports the result as `lora_visible` in `/health`.

VRAM is the constraint. Adapters here can only be applied at runtime (merged mode
is refused because the int8 convrot text encoder rules it out for the model
group), and that costs about 2.4 GB of per-segment tables on the device, which is
the same budget the VAE decode needs. The repository configuration therefore uses
Q4_K_M rather than Q6_K and 16x16 VAE tiles. With those, a 1024-square turbo
generation completes with no memory warnings; see imagegen.toml for the
measurements of each rejected alternative.

Measured through this bridge on 2026-09-27 (RTX 4070 SUPER, a 3:2 request
resolved to 1024x672, cold start): sampling completed in 33.0 s at 1.51 s/it
after a 25.4 s first step, the whole request took 77.3 s, and the backend
reported `(454 / 454) LoRA tensors have been applied`.

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