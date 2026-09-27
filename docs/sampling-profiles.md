# Recipe: QUALITY, DRAFT and TURBO sampling profiles

Status: measured and verified 2026-09-27 through the bridge on this workstation.
QUALITY is the standing deployment; DRAFT and TURBO share one bridge process and
are switched at runtime with the turbo toggle. Every number below came from a run
in that session; the quality verdicts come from an independent critic pass
(agy/Gemini through rustcv-mcp describe_image), not from eyeballing alone.

## The three profiles

| Profile | Diffusion model | VAE tiles | Adapters | Steps | CFG | Wall (1024 square) | Peak VRAM | Critic realism |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| QUALITY | UC-Q6_K | default | none | 28 | 6.0 | 183.3 s | 9200 MiB | 9/10 |
| DRAFT | Q4_K_M | 16x16 | uncensored layer | 28 | 6.0 | 187.5 s | 7571 MiB | (see below) |
| TURBO | Q4_K_M | 16x16 | uncensored layer + Viggle r128 | 6 | 1.0 | 32.7 s | 8230 MiB | 3/10 |

All three: 1024x1024, one prompt (the neighbourhood playground), one seed (4242),
through the bridge's OpenAI endpoint. Artifacts beside this recipe's QA evidence:
qa-evidence/qa-playground-uncensored-q6k-1024.png (QUALITY, the standing model),
qa-evidence/qa-playground-q6k-1024.png (the base Q6_K, the A/B reference),
qa-evidence/qa-playground-q4km-base-1024.png (DRAFT without the layer),
qa-evidence/qa-playground-turbo-1024.png (TURBO without the layer).

## What each one is for

- QUALITY: final stills. The standing model is the UNCENSORED Q6_K build (base
  weights with the 33.6 MB uncensored LoRA merged): chosen on 2026-09-27 for
  content behaviour, because the base model refuses or sanitises some prompts
  while this one does not. The measured A/B on an ordinary scene found no
  quality difference (both 9/10 realism, 183 s, 9200 MiB). 183 s is inside
  open-grok's 240 s read timeout with little margin. No adapter, so turbo is not
  available in this deployment (the bridge reports turbo_available false and
  refuses the toggle with 409, by design).
- DRAFT: iteration and preview. The 28-step base path on the faster base Q4_K_M
  with the small VAE tiles, with the uncensored LoRA layered on top when the
  DRAFT deployment is configured that way; about 40 s cheaper per image than
  QUALITY and visibly softer in fine mechanical detail (6/10 realism without the
  layer; the layer measured 5/10 on the same scene, i.e. no gain either way).
- TURBO: speed only. Six-step Viggle distillation plus the layer, about 6x faster
  than DRAFT, but the critic found melted structural geometry on mechanical
  subjects (realism 3/10 on the playground scene). Use it for composition checks,
  not for keepers.

## The uncensored layer

The uncensored behaviour is available two ways, and the deployment uses one of
each: QUALITY ships the merged UC build, while the DRAFT/TURBO deployment layers
the separate 33.6 MB LoRA on the base weights through the bridge's `extra_lora`
key. The layer rides on every request in every mode, so one process gives
uncensored DRAFT and uncensored TURBO; a turbo request carries both adapters in
the server's structured lora list, verified in one run with "(454 / 454) LoRA
tensors ... viggle-turbo" and "(256 / 256) LoRA tensors ... uncensored-lora".

Measured cost of the layer, 1024 square, same prompt and seed: peak VRAM
7571 MiB against 7562 MiB without it, wall 187.5 s against 168.7 s (staging
noise; the tensor application itself takes 0.03 s). What it buys is content
behaviour, not quality: prompts the base model would refuse, soften or sanitise
render as asked. On ordinary scenes the two are equivalent.

Honest caveat: with the layer configured, the base path is no longer
byte-identical to the pre-turbo request. The identity check below holds with
`extra_lora` unset (which is how it was measured).

## The identity guarantee (DRAFT is the pre-turbo behaviour)

The turbo work was required not to change the base path, and it did not: the
pre-LoRA bridge (commit 8c5b808, the parent of the LoRA commit) and the current
code, both running Q4_K_M with default tiling and the same request (1024 square,
seed 4242), produced byte-identical images:

    sha256 436f3a964994d2cfa217cb4f56d60e83146e77dd7edae96d58f9711dcc231a03

pre-LoRA (8c5b808, debug build) and current (dev HEAD, release build). The
artifact is qa-evidence/qa-q4km-identity-1024.png. The same check doubles as
evidence that this sampler is deterministic across separate processes for a fixed
seed, which is what the adapter-leak test in qa-evidence/qa-report.md relies on.

## How to switch

Two deployments, one at a time (the machine runs one sd-server on the GPU):

A. QUALITY (standing). imagegen.toml as committed: diffusion =
   qwen-image-2.1-UC-Q6_K.gguf, backend_args without --vae-tiling, the turbo and
   extra-lora keys commented out. Turbo is refused; this file's comments carry
   the same recipe.

B. DRAFT + TURBO. Four edits in imagegen.toml, listed above `diffusion`:
   diffusion = qwen-image-2.1-Q4_K_M.gguf, add --vae-tiling --vae-tile-size 16x16
   to backend_args, uncomment lora_dir / lora / lora_multiplier / sigmas, and
   uncomment extra_lora / extra_lora_multiplier (the uncensored layer; drop this
   step for a plain base DRAFT). One process then serves both: the bridge starts
   in base mode (DRAFT), and
   `curl -s -X POST http://127.0.0.1:8787/v1/turbo -d '{"enabled":true}'`
   (or `packaging/hooks/imagegen-turbo on`) switches it to TURBO without a
   restart; a per-request `"turbo": true` pins one image.

Either direction takes effect on the next bridge start (about 35 s of weight
staging). The mode is process state and resets to imagegen.toml's `turbo` value.
The extra layer has no toggle of its own: it is on whenever it is configured.

## Honest limits

- The realism ratings come from one critic pass over one scene per profile;
  scene-specific, as any such verdict is. On soft subjects the TURBO gap is
  smaller (the fox QA image on DRAFT scored 6/10 with no structural complaint).
- 1024 square is the measured size. Above it, QUALITY's 9200 MiB peak leaves
  about 2.4 GiB of the 12 GB card, so 1152+ is untested; the bridge still
  enforces its 1536-edge ceiling.
- QUALITY's 183 s is close to open-grok's 240 s read timeout; a cold bridge adds
  the ~35 s weight staging, so the first image after a restart is the risky one.
