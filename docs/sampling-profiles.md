# Recipe: QUALITY, DRAFT and TURBO sampling profiles

Status: measured and verified 2026-09-27 through the bridge on this workstation.
QUALITY is the standing deployment; DRAFT and TURBO share one bridge process and
are switched at runtime with the turbo toggle. Every number below came from a run
in that session; the quality verdicts come from an independent critic pass
(agy/Gemini through rustcv-mcp describe_image), not from eyeballing alone.

## The three profiles

| Profile | Diffusion model | VAE tiles | Adapter | Steps | CFG | Wall (1024 square) | Peak VRAM | Critic realism |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| QUALITY | Q6_K | default | none | 28 | 6.0 | 183.3 s | 9200 MiB | 9/10 |
| DRAFT | Q4_K_M | 16x16 | none | 28 | 6.0 | 140.9 s | 7272 MiB | 6/10 |
| TURBO | Q4_K_M | 16x16 | Viggle r128 + its schedule | 6 | 1.0 | 29.3 s | 7978 MiB | 3/10 |

All three: 1024x1024, one prompt (the neighbourhood playground), one seed (4242),
through the bridge's OpenAI endpoint. Artifacts beside this recipe's QA evidence:
qa-evidence/qa-playground-q6k-1024.png (QUALITY),
qa-evidence/qa-playground-q4km-base-1024.png (DRAFT),
qa-evidence/qa-playground-turbo-1024.png (TURBO).

## What each one is for

- QUALITY: final stills. Q6_K is the better generator and the 183 s is inside
  open-grok's 240 s read timeout, with little margin. No adapter, so turbo is
  not available in this deployment (the bridge reports turbo_available false and
  refuses the toggle with 409, by design).
- DRAFT: iteration and preview. Same 28-step base path on the faster Q4_K_M with
  the small VAE tiles; about 40 s cheaper per image and visibly softer in fine
  mechanical detail.
- TURBO: speed only. Six-step Viggle distillation, about 6x faster than DRAFT,
  but the critic found melted structural geometry on mechanical subjects
  (realism 3/10 on the playground scene). Use it for composition checks, not for
  keepers.

## The identity guarantee (DRAFT is the pre-turbo behaviour)

The turbo work was required not to change the base path, and it did not: the
pre-LoRA bridge (commit 98ffc4a, the parent of the LoRA commit) and the current
code, both running Q4_K_M with default tiling and the same request (1024 square,
seed 4242), produced byte-identical images:

    sha256 436f3a964994d2cfa217cb4f56d60e83146e77dd7edae96d58f9711dcc231a03

pre-LoRA (98ffc4a, debug build) and current (dev HEAD, release build). The
artifact is qa-evidence/qa-q4km-identity-1024.png. The same check doubles as
evidence that this sampler is deterministic across separate processes for a fixed
seed, which is what the adapter-leak test in qa-evidence/qa-report.md relies on.

## How to switch

Two deployments, one at a time (the machine runs one sd-server on the GPU):

A. QUALITY (standing). imagegen.toml as committed: diffusion = Q6_K, backend_args
   without --vae-tiling, the four turbo keys commented out. Turbo is refused;
   this file's comments carry the same recipe.

B. DRAFT + TURBO. Three edits in imagegen.toml, listed above `diffusion`:
   diffusion = Q4_K_M, add --vae-tiling --vae-tile-size 16x16 to backend_args,
   and uncomment lora_dir / lora / lora_multiplier / sigmas. One process then
   serves both: the bridge starts in base mode (DRAFT), and
   `curl -s -X POST http://127.0.0.1:8787/v1/turbo -d '{"enabled":true}'`
   (or `packaging/hooks/imagegen-turbo on`) switches it to TURBO without a
   restart; a per-request `"turbo": true` pins one image.

Either direction takes effect on the next bridge start (about 35 s of weight
staging). The mode is process state and resets to imagegen.toml's `turbo` value.

## Honest limits

- The realism ratings come from one critic pass over one scene per profile;
  scene-specific, as any such verdict is. On soft subjects the TURBO gap is
  smaller (the fox QA image on DRAFT scored 6/10 with no structural complaint).
- 1024 square is the measured size. Above it, QUALITY's 9200 MiB peak leaves
  about 2.4 GiB of the 12 GB card, so 1152+ is untested; the bridge still
  enforces its 1536-edge ceiling.
- QUALITY's 183 s is close to open-grok's 240 s read timeout; a cold bridge adds
  the ~35 s weight staging, so the first image after a restart is the risky one.
