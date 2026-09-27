QA report for the bridge turbo LoRA unit: the adapter and sigma schedule wired
into imagegen-bridge (crates/bridge/src/main.rs, revision 07e7195 on
feature/bridge-turbo-lora), with its configuration in imagegen.toml and the
documentation that describes it.

qa-model: self-verification by the primary agent, chosen by the operator in this
session (the choice was made one unit earlier, for the recipe unit, and is
carried forward here rather than defaulted). The project's earlier independent
passes used qwen38-flash-next-ds4; this pass carries no independent-model weight.
Date: 2026-09-27. Unit type: code, with one live end-to-end path.

INDEPENDENCE DISCLOSURE, read this first
This pass was performed by the same agent that wrote the code and ran the
generations, at the operator's request. It is self-verification and strictly
weaker than an independent-model pass. Every check below was run live against the
deployed binary and quotes its output; the list of things NOT verified is
explicit at the end.

Surfaces covered

FUNCTION parse_sigmas, sigmas_arg, prompt_with_sigmas, steps_for (new, pure)
  command: cargo test -p imagegen-bridge
  observed: 5 passed, 0 failed. The tests cover the rejections that matter: a
  non-numeric entry ("1.0,abc") and a single-value list are errors, "none" and
  "" both mean "no schedule", the seven-value turbo list parses to 6 steps, and
  the prompt keeps its own text while gaining the sd_cpp_extra_args payload.

FUNCTION prime_lora_cache (new, live)
  command: start the bridge cold, then GET /health every 2 s
  observed: within about 6 s of start, /health reported
  "lora_visible":true and the log line "backend lora cache primed; configured
  adapter Qwen-Image-2.1-viggle-turbo-v0.2.1-6step-lora-r128.safetensors is
  visible". This is the fix for the observed failure that came first: a request
  against an unprimed cache answered 400 "invalid lora path", because sd-server
  builds that cache lazily and its generation route never refreshes it. The
  generation-time guard was also exercised by the race (the readiness prime and
  the first request landed in the same second and the guard primed a second
  time), which is why the log shows the line twice.

ENDPOINT POST /v1/images/generations (changed)
  Live, turbo, through the real tool: image_gen from the chat, bridge started by
  the PreToolUse hook, a 3:2 request resolved to 1024x672.
  observed, from the bridge log:
    effective configuration: ... steps=6 cfg_scale=1 lora=Qwen-Image-2.1-viggle-turbo-v0.2.1-6step-lora-r128.safetensors lora_multiplier=1 sigmas=1,0.9375,0.875,0.75,0.5,0.25,0 placement=... --vae-tiling --vae-tile-size 16x16
    generating 1 image(s) at 1024x672 steps=6 cfg=1 seed=-1 lora=...
    | 1/6 - 25.43s/it ... 6/6 - 1.51s/it
    sampling completed, taking 32.97s
    (454 / 454) LoRA tensors have been applied, lora_file_path = models/loras/Qwen-Image-2.1-viggle-turbo-v0.2.1-6step-lora-r128.safetensors
    generate_image completed in 77.25s
  No memory warning appears in that run. The adapter is therefore applied on the
  tool path, not merely accepted by the bridge.
  Live, turbo, direct: a 1024x1024 request with seed 42 returned HTTP 200 with a
  2.9 MB PNG, sampling 42.91 s at 2.72 s/it.
  Live, base path regression: with IMAGEGEN_LORA="", IMAGEGEN_SIGMAS=none,
  IMAGEGEN_STEPS=28 and IMAGEGEN_CFG_SCALE=6.0, a 512x512 request returned HTTP
  200 in 64.2 s (439,697-byte PNG), and /health reported lora "(none)", sigmas
  "(scheduler)", steps 28, cfg_scale 6.0. The change is additive for the path
  that existed before it.

ENDPOINT GET /health (changed)
  observed: the JSON now carries lora, lora_visible, sigmas, steps and cfg_scale
  beside the existing fields. lora_visible is null when no adapter is configured
  or before the backend has been asked, true when the backend lists it.

Startup validation (new, all three exercised live)
  command: run the binary with each broken setting
  observed: a missing adapter file -> "no adapter at models/loras/no-such-adapter.safetensors
  (lora_dir = models/loras): refusing to start rather than generate without the
  configured LoRA", exit code 2. An adapter with no directory -> "IMAGEGEN_LORA is
  set without IMAGEGEN_LORA_DIR: ...", exit 2. A non-numeric sigma -> "sigma
  list: 'oops' is not a number", exit 2. In each case the process exits before
  spawning a backend.

Which configurations were rejected, and the numbers
  These are measurements, not preferences, and each one is recorded in
  imagegen.toml next to the setting it forced.
- Q6_K diffusion model plus the adapter, 1024 square: sampling fails. The backend
  reports "model manager cannot make enough memory available on CUDA0: need
  767.11 MB device / 733.12 MB budget, available 733.12 MB", then "qwen_image_2_1
  segment 29/34 (qwen_image_2_1.transformer_blocks.27) failed during weight
  preparation", and the request ends as a 500.
- Merged LoRA application, which would avoid the runtime tables: refused by
  sd.cpp itself - "INT8 tensorwise weights do not support the immediately LoRA
  apply mode; using at_runtime instead".
- Q4_K_M plus the adapter with the default VAE tiling, 1024 square: sampling
  succeeds (42.91 s) and the decode fails 3 MB short - "need 512.00 MB device /
  2450.50 MB budget, available 508.69 MB", "vae decode compute failed while
  processing a tile".
- Q4_K_M plus the adapter with the VAE on the CPU: completes, but the decode
  costs about 116 s (generate_image 147.28 s against 62.91 s for the GPU path
  below).
- Q4_K_M plus the adapter with 16x16 latent tiles: completes with no memory
  warning of any kind, decode about 32 s, generate_image 62.91 s. This is the
  shipped configuration.
- Quality check on that tiled output: an independent vision pass reports no tile
  seams, no repeated texture blocks and no brightness steps, and reads the frame
  as one coherent photograph. Small tiles are not visibly free of cost; they are
  at least not obviously visible here.

Gates
- Rule 10, osv-mcp: recorded below at push time for the current lockfile.
- Rule 12, graphify-rs: AST-only rebuild at commit time; output stays gitignored.
- Rule 6, visibility: gh repo view JLFN/imagegen --json isPrivate,visibility
  reports PRIVATE, checked immediately before the push.

NOT verified
1. The image-edit route. It does not exist on this branch (Unit 10 lives on
   feature/image-edit-probe), so the adapter is wired into generation only. The
   same two fields must be added to the edit body when that route lands.
2. The rank-256 adapter: not downloaded, not compared; r128 is what is configured.
3. Resolutions above 1024 with the adapter, and the 1536 edit ceiling.
4. Sustained behavior: every measurement here is a single run. Host CPU load
   moved the fixed costs by roughly a factor of two in the earlier unit's runs,
   and nothing here establishes a steady-state figure.
5. Whether the same requests would pass with a larger VAE tile (24x24 was not
   tried) or with flash attention enabled: both are plausible ways to recover the
   margin, neither was measured.
6. The task's own guidance to prefer the tool path was followed, but only one
   tool-driven image was generated; the direct-curl checks cover the rest.

Open items recorded, not defects of this revision
- The lazily built LoRA cache in sd-server looks like an upstream defect: a valid
  adapter path is rejected until an unrelated route has been called. The bridge
  primes it, which is a workaround, not an explanation. It is a candidate for the
  same upstream report as the JSON body-size hang.
- Q4_K_M replaces Q6_K on the deployed path. The trade is capacity, not a measured
  quality preference: no quality standard exists in this project, which is
  already an open item.
- The docs/viggle-turbo-lora.md recipe on feature/viggle-turbo-lora says the
  bridge does not do this wiring yet. That sentence is now false and needs a
  one-line correction when that branch merges.

verdict: overall PASS
