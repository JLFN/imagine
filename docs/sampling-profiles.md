# Recipe: QUALITY, DRAFT and TURBO sampling profiles

Status: measured and verified 2026-09-27 through the bridge on this workstation.
QUALITY is the standing deployment; DRAFT and TURBO share one bridge process and
are switched at runtime with the turbo toggle. Every number below came from a run
in that session; the quality verdicts come from an independent critic pass
(agy/Gemini through rustcv-mcp describe_image), not from eyeballing alone.

## The three profiles

| Profile | Diffusion model | VAE tiles | Adapters | Steps | CFG | Wall (1024 square) | Peak VRAM | Critic realism |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| QUALITY | Q6_K | default | none | 28 | 6.0 | 183.3 s | 9200 MiB | 9/10 |
| DRAFT | Q4_K_M | 16x16 | uncensored layer | 28 | 6.0 | 187.5 s | 7571 MiB | (see below) |
| TURBO | Q4_K_M | 16x16 | uncensored layer + Viggle r128 | 6 | 1.0 | 32.7 s | 8230 MiB | 3/10 |

All three: 1024x1024, one prompt (the neighbourhood playground), one seed (4242),
through the bridge's OpenAI endpoint. Every profile conditions on the same text
encoder, the GGUF Qwen3VL-8B-Instruct-Q4_K_M file (models/MANIFEST.md section 2,
the fixed architectural component). The int8 convrot encoder that shipped with
the uncensored adoption was reverted and then deleted from disk on 2026-09-27;
see "The text encoder" below. Artifacts beside this recipe's QA evidence:
qa-evidence/qa-playground-uncensored-q6k-1024.png (the uncensored Q6_K, the
QUALITY measurement above), qa-evidence/qa-playground-q6k-1024.png (the base
Q6_K, the A/B reference),
qa-evidence/qa-playground-q4km-base-1024.png (DRAFT without the layer),
qa-evidence/qa-playground-turbo-1024.png (TURBO without the layer).

## What each one is for

- QUALITY: final stills. The standing model is the base Q6_K GGUF with the
  default VAE tiling and the GGUF text encoder (see "The text encoder" below).
  The uncensored Q6_K build (base weights with the 33.6 MB uncensored LoRA
  merged) held this slot for part of 2026-09-27 and was replaced the same day: it
  was adopted for content behaviour only - the measured A/B against the base was a
  quality tie, both 9/10 realism, 183 s, 9200 MiB - and the operator reported
  visible artifacts across that deployment's generations. 183 s is inside
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

## The text encoder

All three profiles condition on the GGUF Qwen3VL-8B-Instruct-Q4_K_M file, the
component models/MANIFEST.md has called fixed since 2026-09-21. This is a
correction of the 2026-09-27 uncensored adoption, which also switched the encoder
to the ComfyUI int8 convrot safetensors; the operator reported artifacts in every
generation of that deployment, and after the encoder was put back the artifacts
were gone on the same prompts.

The evidence for the attribution, and its limits: the diffusion and VAE files are
unchanged since 2026-09-21, the sampling parameters never moved, and a controlled
A/B proved the bridge binary cannot change a pixel (see "The identity guarantee"),
so the encoder is the only component that differed from the configuration
effectively serving the retained reference images - the backend captured on
2026-09-26 carried the GGUF encoder while imagegen.toml already named the convrot
file, so the process was stale relative to the config. The limit is that the
2026-09-22 comparison of the two encoders recorded a quality tie, so this is an
operator verdict on artifacts that the earlier measurement did not capture, not a
reproduced encoder defect. Details: docs/convrot-encoder-experiment.md.

The convrot file was deleted from disk on 2026-09-27. It is a public download if
it is ever wanted again: Comfy-Org/Qwen-Image-2.1 text_encoders/
qwen3vl_8b_int8_convrot.safetensors, sha256
8bfd0f6e12abf2d2d697ecc888e5e90b0d6741d6708f05799f53afa560452e8f (see
models/MANIFEST.md and tools/convrot-te-experiment.sh, which is the script that
measured it), and it needs the patched sd-server built from patches/0001 and
patches/0002.

The encoder choice does not affect the turbo path, and that was measured rather
than assumed on 2026-09-27, because the DRAFT and TURBO rows above were taken with
the convrot encoder. The toggle's gate is the adapter being configured, its
schedule having at least two nodes and the adapter file being on disk
(crates/bridge/src/main.rs); the encoder is not part of it. With the GGUF encoder
in place and the DRAFT deployment configured, POST /v1/turbo reported mode turbo,
steps 6, cfg_scale 1.0, the six-node schedule and both adapters listed by the
backend, and a full 672x1024 render completed. So the profiles are unchanged by
the revert: only what conditions them is.

## The uncensored layer

The uncensored behaviour is available as the separate 33.6 MB LoRA on the base
weights through the bridge's `extra_lora` key, which the DRAFT and TURBO
deployments use. The two merged uncensored diffusion GGUFs stay on disk but no
profile stands on them after 2026-09-27. The layer rides on every request in
every mode, so one process gives
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
   qwen-image-2.1-Q6_K.gguf, llm = Qwen3VL-8B-Instruct-Q4_K_M.gguf, backend_args
   without --vae-tiling, the turbo and extra-lora keys commented out. Turbo is
   refused; this file's comments carry the same recipe.

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
