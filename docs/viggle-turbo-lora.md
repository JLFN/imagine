# Recipe: 6-step generation with the Viggle turbo LoRA (Qwen-Image 2.1)

Status: VERIFIED on this workstation 2026-09-27 for text-to-image at 1024 square,
through sd-cli, on the Q4_K_M diffusion model. The editing path, other
resolutions and the sd-server route are marked UNVERIFIED where they were not
run. Every number below came from a command in this session; nothing is copied
from an upstream page without saying so.

## 1. What it is

Viggle's Qwen-Image-2.1-viggle-turbo is a DMD (distribution matching
distillation) student of the Qwen-Image 2.1 transformer, shipped as a LoRA
adapter that is loaded on top of the unmodified base transformer at runtime.
It samples in 6 transformer passes with no classifier-free guidance instead of
the base model's 40, and is the fastest route to a good image on this box.

It is an ADAPTER, not a model. The base transformer, the Qwen3-VL text encoder,
the VAE and the processor are still required and are NOT redistributed with it;
they come from the base stack pinned in models/MANIFEST.md exactly as before.

Sources

- Adapter repository (weights, README, ComfyUI nodes):
  https://huggingface.co/Viggle/Qwen-Image-2.1-viggle-turbo
- Announcement (v0.2, 2026-09-23):
  https://comfyui-wiki.com/en/news/2026-09-23-viggle-qwen-image-turbo
- Base model, not redistributed here: https://huggingface.co/Qwen/Qwen-Image-2.1

License: the adapter ships under the Qwen RESEARCH LICENSE AGREEMENT in the
repository (non-commercial: research or evaluation only, a commercial licence is
available separately from the licensor). The base stack carries its own licences
in models/MANIFEST.md. Read both before any commercial use.

Two releases exist in the repository and both are LoRAs on the same base:

- v0.2.1 (2026-09-24, the current one): 6-step schedule, sharper than v0.2 and
  the variant the upstream demo Space runs.
- v0.2 (2026-09-23, the one the news article describes): 5-step launch schedule,
  kept for reproducibility. Upstream says there is no reason to prefer it.

## 2. What to download

Get the LoRA into models/loras/ (that directory is gitignored: .gitignore has
`models/*` with `!models/MANIFEST.md`, so nothing under models/loras/ can be
committed). Sizes and SHA256 values below are the ones Hugging Face serves for
the files; the SHA256 is the LFS object id, which is what `sha256sum` of a
correct download must print.

| File in Viggle/Qwen-Image-2.1-viggle-turbo | Bytes | SHA256 | Role |
| --- | --- | --- | --- |
| Qwen-Image-2.1-viggle-turbo-v0.2.1-6step-lora-r256.safetensors | 1,359,147,904 | 2a0148f5c73abbed5f97da5ea356e439318aadb281d01fce4af39cdf43728803 | Upstream's recommended adapter (rank 256, alpha 256, bf16) |
| Qwen-Image-2.1-viggle-turbo-v0.2.1-6step-lora-r128.safetensors | 679,604,800 | bafb91d0047df3f9b8a5a850b0c967f051164314d8aad778dfa34d9c24ec345b | The same adapter cut to rank 128, half the size; the one verified here |
| Qwen-Image-2.1-viggle-turbo-v0.2-5step-lora-r256.safetensors | 1,359,147,904 | 7fd196e14629160c9d1470a698fbb65ccb95f725b72397ef554648ce468ad825 | The v0.2 release, optional |
| scheduler/scheduler_config.json | 484 | 0f289c902df312761c091522b66b6d6d2933d8c1 | Diffusion-only; sd.cpp never reads it (see below) |

Both rank-128 and rank-256 adapters target the same 10 modules per block
(to_q, to_k, to_v, to_out.0, img_mlp.proj, img_mlp.gate_layer, img_mlp.out,
modulation.1, timestep_embedder.linear_1, timestep_embedder.linear_2) over 32
transformer blocks, plus the shared modulation and timestep-embedder tensors, for
454 tensors in the rank-128 file (read from the file's own safetensors header and
tensor count in this session).

Not needed for this stack: `comfyui/` (custom nodes and workflows for ComfyUI),
`peft/`, `peft_v0.2/` and `peft_v0.2.1/` (the same adapter in PEFT key format),
and `transformer/` (the v0.1 full fine-tune).

Download with the hf CLI (resumable), the same tool the rest of the manifest
uses:

    hf download Viggle/Qwen-Image-2.1-viggle-turbo \
      Qwen-Image-2.1-viggle-turbo-v0.2.1-6step-lora-r128.safetensors \
      --local-dir models/loras

curl fallback (identical bytes):

    mkdir -p models/loras
    curl -L -o models/loras/Qwen-Image-2.1-viggle-turbo-v0.2.1-6step-lora-r128.safetensors \
      https://huggingface.co/Viggle/Qwen-Image-2.1-viggle-turbo/resolve/main/Qwen-Image-2.1-viggle-turbo-v0.2.1-6step-lora-r128.safetensors

Verify before first use, always:

    sha256sum models/loras/*.safetensors
    # compare with the table above; a mismatch means re-download, never trust size alone

## 3. Running it through sd-cli

The verified command (this is exactly what produced the images in section 4):

    ./bin/sd-cli \
      --diffusion-model models/diffusion_models/qwen-image-2.1-Q4_K_M.gguf \
      --vae models/vae/qwen_image_2.1_vae_bf16.safetensors \
      --llm models/text_encoders/qwen3vl_8b_int8_convrot.safetensors \
      --lora-model-dir models/loras \
      -p "a capybara sitting in a hot spring, soft morning light, highly detailed<lora:Qwen-Image-2.1-viggle-turbo-v0.2.1-6step-lora-r128:1>" \
      --steps 6 \
      --sigmas "1.0,0.9375,0.875,0.75,0.5,0.25,0.0" \
      --cfg-scale 1.0 \
      -W 1024 -H 1024 --seed 42 -o out/turbo-1024-seed42.png \
      --backend te=cpu,diffusion=cuda0,vae=cuda0 \
      --params-backend te=cpu,diffusion=cuda0,vae=cpu

The four knobs that decide whether it works, and why:

1. `--lora-model-dir models/loras` plus the `<lora:NAME:WEIGHT>` prompt tag. NAME
   is the file name without the .safetensors extension and WEIGHT is the
   multiplier; keep it at 1 (in this adapter alpha equals rank, so the correct
   scale is exactly 1.0). This prompt-tag form is sd-cli only; the HTTP server
   routes reject it, see section 6.
2. `--sigmas` carries the schedule. Upstream publishes six sigma nodes
   (1.0, 0.9375, 0.875, 0.75, 0.5, 0.25 for v0.2.1) and six transformer passes.
   sd.cpp derives its step count as `sigmas.size() - 1`, so the list is the six
   published nodes PLUS a trailing 0.0: seven entries, six steps. Passing the six
   nodes alone would run five steps and silently drop the last denoise.
3. `--cfg-scale 1.0` with no negative prompt. The student was trained without
   classifier-free guidance, and CFG is also what makes the base model's steps
   cost roughly double (see section 4).
4. `--lora-apply-mode` may be left at its default `auto`. On a quantized GGUF the
   auto rule selects `at_runtime`, which is what ran here; the log line is
   `apply lora at runtime`, and the adapter applied in full: the run ended with
   `(454 / 454) LoRA tensors have been applied`.

Two lines that look alarming and are not. A custom sigma list always logs

    [WARN] total_steps != custom_sigmas_count - 1, set total_steps to 6
    [WARN] total_steps != custom_sigmas_count - 1, set sample_steps to 6

because the pipeline sets the step count from the list and logs the setting
unconditionally; with seven sigmas and six steps they agree, and the sample used
all six. The per-step line then reads `2/6 - 2.58s/it`.

What must NOT be copied across:

- The base model's scheduler setting `shift_terminal` does not apply here.
  sd.cpp is not the diffusers pipeline: `--sigmas` replaces the schedule
  outright, and the shipped scheduler_config.json (which sets shift_terminal to
  null and dynamic shifting) is never read. Upstream's warning about
  shift_terminal 0.02 wrecking the final step is therefore moot for this route.
  sd.cpp does still apply its own fixed flow shift when it turns a sigma into the
  model's timestep input (its Qwen-Image default is 3.0); whether that matches the
  intended schedule better than `--flow-shift 1.0` is UNVERIFIED.
- The adapter's own alpha/rank metadata is enough; no `.alpha` tensor is in the
  file, and none is needed at scale 1.

## 4. What it costs, measured

Same workstation (RTX 4070 SUPER 12 GB), Q4_K_M diffusion model, 1024x1024,
text-to-image, one image per run. "generate" is the figure sd-cli reports for
the whole generation; "step 1" is slow because the graph is built there.

| Run | Steps | CFG | Per step after step 1 | Step 1 | generate | Wall |
| --- | --- | --- | --- | --- | --- | --- |
| Turbo LoRA r128 | 6 (7 sigmas) | 1.0 | 2.58 s/it | 17.0 s | 60.56 s | 61.63 s |
| Base model | 28 | 6.0 | 4.40 s/it | 17.7 s | 168.83 s | not captured |

That is 2.8x faster end to end and about 4.3x faster in the sampling loop
(6 x 2.58 s against 28 x 4.40 s). Part of the per-step difference is the step
count and part is CFG: with guidance off there is one denoiser pass per step
instead of two. The split between the two causes was NOT isolated by a
measurement, so treat the attribution as an inference, not a result.

The fixed costs around the loop vary more than the loop itself, because the text
encoder runs on the CPU:

| Phase | Turbo run | Volvo run A | Volvo run B |
| --- | --- | --- | --- |
| Text encoder weights load | 21.77 s | (not captured) | 24.81 s |
| Diffusion weights load | 12.56 s | 12.65 s | (not captured) |
| Conditioning (vision + text encode) | 24.98 s | 27.28 s | 44.61 s |
| Step 1 | 17.00 s | 17.18 s | 30.50 s |
| Steps 2-6 | 2.58 s/it | 2.73 s/it | 2.58 s/it |
| VAE decode (9 tiles x about 2.2 s/tile) | about 20 s | about 20 s | about 20 s |
| generate, total | 60.56 s | 63.74 s | 93.65 s |

The per-step cost is stable (2.58-2.73 s/it); everything else moved, and run B's
conditioning and first step are roughly double run A's with no model change in
between. Host CPU load during a run is therefore part of the measurement, and a
timing quoted from a single run is not a sustained figure.

Quality, on the evidence rather than on the upstream claim:

- 1024 square capybara, seed 42, same prompt on both schedules: the turbo image
  and the 28-step base image were both judged, by an independent vision pass, as
  coherent photograph-like renders with no duplicated limbs, melted anatomy or
  ghosting.
- The one visible defect matched upstream's own stated weakness: on a classic
  Volvo render the badge lettering came out as invented nonsense
  ("TV50"/"IY5D") instead of a real wordmark. Small dense text is where the
  40-step teacher is still ahead.

## 5. Where the files landed here

    models/loras/Qwen-Image-2.1-viggle-turbo-v0.2.1-6step-lora-r128.safetensors   (680 MB, verified)
    out/turbo-1024-seed42.png        turbo, 6 steps, seed 42 (capybara)
    out/base-1024-seed42.png         base, 28 steps, seed 42 (same prompt)
    out/volvo-turbo-240-seed7.png    turbo, classic 240-style wagon in autumn forest
    out/volvo-turbo-studio-seed11.png turbo, modern estate in a studio

models/ and out/ are both gitignored; the images are working evidence, not
deliverables.

## 6. Through the bridge and the image tools (IMPLEMENTED; turbo is a mode, not the default)

sd-cli takes the `<lora:...>` prompt tag, but the server deliberately does not.
Verified in the server's own documentation and source at /data/build-sd:

- `/v1/images/generations`, `/v1/images/edits`, `/sdapi/v1/txt2img` and
  `/sdcpp/v1/img_gen` all state that prompt-embedded `<lora:...>` tags are
  intentionally unsupported.
- The native routes carry a structured LoRA list instead:
  `lora[].path`, `lora[].multiplier`, `lora[].is_high_noise`, and a sampling
  block with `sample_params.custom_sigmas`, `sample_params.sample_steps`,
  `sample_params.flow_shift` and `sample_params.guidance.txt_cfg`.
- `GET /sdapi/v1/loras` lists the adapters the server can see; the LoRA directory
  is passed to the server with `--lora-model-dir`, and `lora[].path` must equal
  the path the server's own directory scan produced (a path relative to that
  directory), not an absolute path.
- The sdapi generation route has no sigma field at all, so the schedule can only
  travel as the native JSON embedded in the prompt:
  `prompt<sd_cpp_extra_args>{"sample_params":{"custom_sigmas":[...]}}</sd_cpp_extra_args>`.

The bridge does all of that (commit 671d03a on feature/bridge-turbo-lora, with
its QA evidence on dev): `imagegen.toml` carries `lora_dir`, `lora`,
`lora_multiplier` and `sigmas`, the bridge sends the structured entry and the
schedule, derives the step count from the schedule, and primes the server's
lazily built LoRA cache so the first request cannot fail with "invalid lora
path".

Since the turbo-toggle unit, that adapter is no longer applied to every
generation. The bridge serves two profiles and starts in base mode (`turbo =
false` in imagegen.toml): base sends neither the adapter nor the schedule and
uses `steps`/`cfg_scale`, while turbo sends both and uses the schedule's step
count with `turbo_cfg_scale`. The live mode is reported by `GET /v1/turbo` and
switched without a restart by `POST /v1/turbo` (or for one request with
`"turbo": true` in the generation body), and `packaging/hooks/imagegen-turbo
on|off|status` wraps that endpoint for shell and hook use. Enabling turbo is
verified before it takes effect: the adapter must be on disk and listed by the
backend, otherwise the switch is refused with HTTP 409 and the mode is left
alone.

DEPLOYMENT STATE (2026-09-27, after the Q6_K decision): the standing deployment
is base-only. The adapter keys in imagegen.toml are commented out, the diffusion
model is Q6_K with default VAE tiling, and the bridge reports `turbo_available:
false` and refuses the toggle with 409 - the adapter cannot run on Q6_K at 1024
square on this card, and Q6_K was chosen for stills quality (9 of 10 realism
against 6 of 10 for Q4_K_M base and 3 of 10 for turbo, measured at 1024 square
with an independent critic pass). Turbo remains one three-line edit away: the
Q4_K_M model, the 16x16 VAE tiles, and the four uncommented keys, followed by a
bridge restart. Everything below in this document remains the verified recipe
for that turbo deployment.

Two consequences of doing it through the bridge rather than sd-cli:

- The adapter must be applied at runtime, and merged application is refused
  outright: sd.cpp reports "INT8 tensorwise weights do not support the
  immediately LoRA apply mode; using at_runtime instead", forced by the int8
  convrot text encoder. Runtime tables cost about 2.4 GB of device memory, which
  is why the deployed bridge config pairs the adapter with Q4_K_M instead of
  Q6_K (Q6_K plus the adapter cannot sample at 1024 square) and with 16x16 VAE
  tiles (the default 32x32 tiles miss the decode budget by 3 MB). Both are
  measured and recorded in imagegen.toml.
- Measured through the bridge on the real tool path (a 3:2 request resolved to
  1024x672, cold start): sampling 32.97 s at 1.51 s/it after a 25.43 s first
  step, the whole request 77.25 s, and `(454 / 454) LoRA tensors have been
  applied`. At 1024 square the same configuration took 42.91 s of sampling and
  62.91 s for the whole image.

The image-edit route is not on that branch (Unit 10 has not merged), so the
adapter is wired into generation only; the same two fields belong in the edit
body when it lands.

## 7. Prerequisites and rebuild notes

- The base stack from models/MANIFEST.md: the diffusion GGUF (Q4_K_M or Q6_K),
  the Qwen3-VL text encoder and the Qwen-Image 2.1 VAE. The adapter needs all
  three; vision weights are not part of it.
- The LoRA needs no patch to sd.cpp. It is a plain diffusers-format adapter, and
  sd.cpp's name conversion already maps `.lora_A.weight` to its internal
  `lora_down` and `.lora_B.weight` to `lora_up` (src/name_conversion.cpp, the
  LoRA suffix map), which is why all 454 tensors matched with no "unused lora
  tensor" warning. The int8 convrot text encoder still needs patches/0001 and
  0002 as documented in models/MANIFEST.md; that is unrelated to the adapter.
- sd.cpp revision in use here: master 6dcb5bb (matches the installed sd-cli and
  sd-server, SD_CUDA=ON, upstream GGML off).
- Rebuild note: the deployable number for this route is the whole-run figure in
  section 4, so a warm-service run (bridge already up, weights already loaded)
  should be substantially cheaper than 61 s. That has not been measured.

## 8. Limits and open items

Upstream's own stated limits for v0.2.1, plus what is unmeasured here.

From the adapter's README (upstream, 2026-09-24):

- It is still a preview. Complicated edits - multi-reference composition, face
  swaps, identity-preserving edits, instructions with several constraints - can
  still fall short of the 40-step base model.
- Small or long rendered text garbles more often than at 40 steps; 8 steps
  narrows the gap.
- 2K output is not validated against the teacher. RGBA output, more than three
  reference images, and mask-based local editing are untested upstream.
- Text-to-image was trained at 1024 and 2048 area; editing at 1024 and 1536 area,
  with up to 3 reference images whose order fixes what `<image1>`, `<image2>` and
  `<image3>` mean in the prompt.

Unmeasured here, in the order they would matter:

1. Editing with the turbo LoRA (the image-to-image route, `ref_images`): not run.
2. The rank-256 adapter: not downloaded, so its checksum is from Hugging Face's
   own metadata and its output has not been compared with rank 128.
3. Resolution 1536 and above, and the 2K training area: not run.
4. `--flow-shift` (sd.cpp's own default 3.0 against 1.0): not compared.
5. Peak VRAM for the 6-step run: not measured, so whether the turbo route frees
   headroom at 1024 square compared with the base model is unknown.
6. The cfg-isolation test (6 steps at cfg 2.0 to confirm that guidance accounts
   for the per-step difference): not run.
7. VAE tile sizes between 16x16 and the 32x32 default (24x24 was not tried), and
   whether flash attention would recover the decode margin instead: not run.

The bridge wiring that used to be item 7 here is now implemented and measured on
feature/bridge-turbo-lora; section 6 describes it.

## 9. Sources used for this document

- https://huggingface.co/Viggle/Qwen-Image-2.1-viggle-turbo - README, LICENSE,
  file list, per-file sizes, LFS SHA256 values (read 2026-09-27; the repository
  was last modified 2026-09-25).
- https://comfyui-wiki.com/en/news/2026-09-23-viggle-qwen-image-turbo - the
  announcement the request came from, covering v0.2 and the ComfyUI situation.
- The local sd.cpp checkout at /data/build-sd/stable-diffusion.cpp - the sigma
  handling (src/pipeline/request.cpp, src/pipeline/diffusion_engine.cpp), the
  LoRA name mapping (src/name_conversion.cpp) and the server contract
  (examples/server/api.md).
- Runs in this session: the four images named in section 5, plus the base-model
  comparison at 28 steps, all on 2026-09-27.
