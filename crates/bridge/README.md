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
- `GET /health` — `{"status":"ok","backend":<bool>,"backend_process_alive":<bool>,"backend_restarts":<n>,"backend_restart_attempts":<n>,"lora":<name>,"lora_visible":<bool|null>,"sigmas":<list>,"steps":<n>,"cfg_scale":<n>}`
  `lora_visible` answers whether the backend lists the configured adapter; it is
  null until the backend has been asked.
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
| `IMAGEGEN_STEPS` | `6` | `28` |
| `IMAGEGEN_CFG_SCALE` | `1.0` | `6.0` |
| `IMAGEGEN_LORA_DIR` | `models/loras` | none; required when a LoRA is set |
| `IMAGEGEN_LORA` | the Viggle turbo rank-128 adapter | none (no adapter) |
| `IMAGEGEN_LORA_MULTIPLIER` | `1.0` | `1.0` |
| `IMAGEGEN_SIGMAS` | `1.0,0.9375,0.875,0.75,0.5,0.25,0.0` | none (sd.cpp's scheduler) |

Run it from the repository root so the default model/sd-server paths resolve.
The diffusion model in the repository default is Q4_K_M rather than the
higher-quality Q6_K because of the adapter: with a runtime LoRA applied, Q6_K
cannot fit a 1024-square generation on this card (measured 2026-09-27). Without
an adapter, Q6_K is the better generator; see docs/viggle-turbo-lora.md.

To run the base model without editing the file, set all four of
`IMAGEGEN_LORA=""`, `IMAGEGEN_SIGMAS=none`, `IMAGEGEN_STEPS=28` and
`IMAGEGEN_CFG_SCALE=6.0`. Clearing the adapter alone is not enough: the step and
CFG defaults in the file are the turbo pair, and the base model is not trained
for those.

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

## Few-step turbo adapter (Viggle Qwen-Image-2.1 turbo)

Setting `lora`, and normally `sigmas`, turns every generation into a few-step run
of a distilled adapter. Recipe, file list and checksums: docs/viggle-turbo-lora.md.
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
   generation if that has not happened yet, and reports the result as
   `lora_visible` in `/health`.

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