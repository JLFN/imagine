---
project: imagegen
plan_start_commit: 7443adfcd22374ddaa3e2139287e83a78d431367
last_updated_commit: 3118732
branch: dev
work_branch: none in flight; PRs 5-8 merged this session and their branches deleted, feature/image-edit-probe (Unit 10) still outside dev
remote: origin (PRIVATE, github.com/JLFN/imagegen) - created 2026-09-22, dev pushed
handoff_written_at_context_usage: ~29% (signals.json, session 01a0e408)
handoff_written_at: 2026-09-27T19:07:00Z
session_id: 01a0e408-098e-7b42-b6e6-7b9d5daf9075
---
--- handoff probe (auto) ---
=== handoff probe (imagegen) ===
--- units completed since plan start (7443adfcd22374ddaa3e2139287e83a78d431367):
31c7ed2 feat(bridge): make turbo a runtime mode, with the base model as the default
d5fcfe0 feat(repo): measure the resolution headroom, port the verifier to Rust, bound the bridge
4e8f1a0 feat(bridge): pin Q6_K diffusion in VRAM and run the text encoder on CPU
9169fed feat(bridge): on-demand OpenAI image API bridge over sd-server
347ed52 docs(handoff): mark Unit 1 complete and plan Unit 2 (Rust bridge)
6b17eee feat(recipe): first real Qwen-Image-2.1 image on the workstation
--- progress diff stat:
.gitignore                                         |   13 +-
 AGENTS.md                                          |    5 +-
 Cargo.lock                                         | 1491 +++++++++++++++++++
 Cargo.toml                                         |    9 +
 crates/bridge/Cargo.toml                           |   16 +
 crates/bridge/README.md                            |  182 +++
 crates/bridge/src/main.rs                          | 1510 ++++++++++++++++++++
 crates/convrot-verify/Cargo.toml                   |   10 +
 crates/convrot-verify/src/main.rs                  |  343 +++++
 docs/architecture.md                               |  406 +++++-
 docs/convrot-encoder-experiment.md                 |  242 ++++
 docs/viggle-turbo-lora.md                          |  323 +++++
 imagegen-handoff.md                                |  317 ++++
 imagegen.toml                                      |  142 ++
 models/MANIFEST.md                                 |  160 +++
 packaging/hooks/ensure-imagegen-bridge             |  117 ++
 packaging/hooks/imagegen-turbo                     |   94 ++
 packaging/hooks/stop-imagegen-bridge               |  104 ++
 ...01-sd-cpp-llm-te-config-skip-weight-scale.patch |   53 +
 ...-cpp-dequantize-int8-tensorwise-embedding.patch |  172 +++
 qa-evidence/qa-base-1024-seed4242.png              |  Bin 0 -> 1932779 bytes
 qa-evidence/qa-gen-1024-28steps-seed42.png         |  Bin 0 -> 2463614 bytes
 qa-evidence/qa-gen-1152-28steps-seed5.png          |  Bin 0 -> 2826304 bytes
 qa-evidence/qa-gen-1152-8steps-seed5.png           |  Bin 0 -> 2297223 bytes
 qa-evidence/qa-gen-512-seed11.png                  |  Bin 0 -> 286535 bytes
 qa-evidence/qa-gen-512-seed3.png                   |  Bin 0 -> 280816 bytes
 qa-evidence/qa-report.md                           |  160 +++
 qa-evidence/qa-tiling-1024-untiled-vs-tiled.png    |  Bin 0 -> 2771580 bytes
 qa-evidence/qa-turbo-1024-seed4242.png             |  Bin 0 -> 2686314 bytes
 qa-evidence/qa-turbo-perrequest-512-seed7.png      |  Bin 0 -> 582913 bytes
 qa-evidence/unit6-qa-report.md                     |  218 +++
 qa-evidence/unit7-qa-report.md                     |  271 ++++
 tests/qa-gate.sh                                   |  109 ++
 tests/run.sh                                       |   34 +
 tools/convrot-te-experiment.sh                     |   72 +
 35 files changed, 6558 insertions(+), 15 deletions(-)
--- staleness (last_updated_commit 31c7ed2 .. HEAD):
bf50597 merge: bring the merged turbo-toggle unit (PR 5) into local dev
7b8d9e5 Merge pull request #5 from JLFN/feature/turbo-toggle
8ceda83 docs(handoff): record the manual-toggle decision
04d4ca6 docs(handoff): correct the PR state recorded in the Unit 13 plan
013912c docs(handoff): record Unit 12 (turbo toggle) and plan Unit 13
cca52dc docs(handoff): refresh the progress and staleness probe blocks
f8ac371 docs(handoff): record the turbo adapter units and plan Unit 12
a9d499a test(qa): self-verified QA pass over the bridge turbo LoRA unit
4df710e test(qa): self-verified QA pass over the Viggle turbo LoRA recipe unit
88ad5b4 docs(handoff): refresh the session probe block on dev
--- end handoff probe ---


Handoff + executable plan — imagegen (2026-09-26)

0. Rules for writing THIS document (read before editing, not just before reading)

- Only write or replace this file at a clean, verified, committed boundary.
- Draft from ~25% context usage; finalize before 40%.
- Every claim in "Current state" must be re-verified by running the command
  in the session that writes it, not copied from this file. Mark unverified
  anything that cannot be re-run.
- Hard caps: "Current state" max 15 lines. "Guardrails and open items" max
  15 lines. "Next unit plan" has no cap.
- Do NOT replay prior units' logs (git log is the record). When a unit
  completes, replace section 3 wholesale with the next unit's plan and fold
  the completed unit's outcome into section 2.

1. Session protocol

This project is a sequence of units. Units 1-6 are complete on dev. Units 7
and 8 are complete but still in flight as reviewed PRs (section 2). Unit 10
(the image-edit path) is in flight on a work branch and is BLOCKED by a
measured backend limit (section 3 opens with it).

On completing a unit: do not stop at the boundary. Read the session's
signals.json (sessions/<percent-encoded-cwd>/<session-id>/ under
~/.opengrok; cwd is %2Fdata%2Fimagegen, pick the newest session dir):
- If contextWindowUsage < 40: write the next unit's plan into section 3,
  execute it, verify, commit, push, re-check, repeat.
- At ~40%: do not pause or ask. Finish this handoff's update, launch a fresh
  session with the open-grok-handoff launcher, keep working.
The final commit of each unit carries the trailer "Unit: <N> complete" as
its last body line (git log --grep "Unit: .* complete").

BRANCH MODEL (rule 23): dev is the integration branch and the local working
line. Every unit is a short-lived work branch off the dev tip, pushed, and
merged through a REVIEWED PR on GitHub; only handoff and QA-evidence commits
go directly on dev. main does not exist yet (section 3).

PARALLEL WORK: a sibling repo /data/imagegen-rs contains a pure-Rust port of the
whole generation stack (candle: tokenizer, Qwen3-VL text tower, 32-block DiT, Wan
VAE, sampler, CLI, OpenAI server), replacing stable-diffusion.cpp. Read
/data/imagegen-rs/imagegen-rs-handoff.md and its docs/parity.md before changing
anything that affects the model stack.

2. Current state (2026-09-27 — the standing deployment is the UNCENSORED Q6_K; the bridge can layer adapters)

- dev is IN SYNC with origin/dev (4859061 as of the purge below); the earlier
  local-only handoff/docs commits were rewritten by the purge and landed in the
  same force-push, so the old "unpushed dev" problem is gone. Rule 23's
  "dev merges only via PR" still stands for everything that follows.
- HISTORY PURGED 2026-09-27, on the operator's explicit request: every
  qa-evidence/*.png was removed from the ENTIRE repository history with
  git filter-repo, and the rewritten branches were force-pushed (dev and
  feature/image-edit-probe; the four merged stale branches were deleted from the
  remote). Consequences a fresh session must know:
  * every commit hash from the first image onwards changed; the docs' commit
    references were remapped from .git/filter-repo/commit-map (saved at
    /data/imagegen-commit-map.txt), and the full pre-purge bundle of every ref is
    at /data/imagegen-pre-purge.bundle - keep it, it is the only copy of the old
    history;
  * the images themselves still exist locally under qa-evidence/ (gitignored,
    restored from the bundle) and inside the bundle;
  * LIMIT: GitHub still keeps the old commits reachable through the merged pull
    requests' pages (PRs 1-9) and its own object store until GC or a
    support-side purge; the branch refs themselves now carry no images.
- LANDED on origin/dev this session: PR 5 (turbo toggle), PR 6 (Q6_K
  deployment), PR 7 (the QUALITY/DRAFT/TURBO recipe), PR 8
  (feature/uncensored-model-trial, 2c96969): the standing model is now
  qwen-image-2.1-UC-Q6_K.gguf (base weights + the 33.6 MB uncensored LoRA
  merged; chosen for content behaviour, NOT quality - the A/B measured 9/10
  realism on both), and the bridge gained an optional EXTRA adapter layer
  (extra_lora) applied in every mode; PR 9 untracked the QA images. No PR is
  open.
- THE LAYER: `extra_lora` rides on base and turbo alike, so one process serves
  uncensored DRAFT and uncensored TURBO; a turbo request carries two adapters
  (verified in one run: 454/454 Viggle + 256/256 uncensored tensors), and the
  cache check verifies every configured adapter. Measured cost at 1024 square:
  7571 MiB peak against 7562 MiB without it - no measurable VRAM cost. With the
  layer configured the base path is no longer byte-identical to the pre-turbo
  request; the identity check holds with it unset.
- THE PROFILES: QUALITY (UC-Q6_K, standing, 183 s / 9200 MiB / 9 of 10 realism),
  DRAFT (Q4_K_M base + 16x16 tiles + the layer when configured, 141-187 s /
  7272-7571 MiB / 5-6 of 10), TURBO (that deployment + the Viggle adapter, 29-33 s
  / 7978-8230 MiB / 3 of 10). Recipe and switch procedure:
  docs/sampling-profiles.md; all five model files with verified checksums:
  models/MANIFEST.md.
- IDENTITY CHECKED: the pre-LoRA bridge (8c5b808) and the current code, both on
  Q4_K_M with default tiling and no extra layer, produced byte-identical images
  for one request (1024 square, seed 4242, sha256 436f3a96...); artifact
  qa-evidence/qa-q4km-identity-1024.png.
- LIVE SERVICE: UC-Q6_K, mode base, steps 28, cfg 6.0, turbo_available false;
  POST /v1/turbo answers 409 and imagegen-turbo on exits 1 with the reason.
  183 s per 1024-square image against open-grok's 240 s read timeout.
- Unit 10 (edits) unchanged at 2f07671 on feature/image-edit-probe, no PR yet.
- UNEXPLAINED, watch it: four tracked QA artifacts (qa-playground-turbo-1024,
  qa-q4km-identity-1024, qa-gen-1152-8steps-seed5, qa-gen-512-seed11) were found
  missing from disk once this session, along with the session images 1.jpg and
  2.jpg; the tracked ones were restored from git. Cause unknown - check
  `git status` before committing anything that touches qa-evidence/.
- FOUND 2026-09-27, NOT fixed: dev's bridge source lacks the on-demand lifecycle
  code (in_flight, idle_secs, idle_timeout_secs, the idle shutdown) that c018d2e
  added and that architecture.md and stop-imagegen-bridge still assume - the
  merge fc9b9f9 resolved main.rs to the turbo side, and guard 2 of the stop hook
  reads an in_flight field that no longer exists. Reconcile with
  feature/on-demand-lifecycle.

3. Next unit plan — Unit 16: restore the lifecycle code, then land the edit path

Task 1, restore the on-demand lifecycle on dev (see section 2's finding). Diff
feature/on-demand-lifecycle's crates/bridge/src/main.rs against dev's, and either
restore in_flight/idle_secs/idle_timeout_secs and the idle shutdown, or change
stop-imagegen-bridge's guard 2 and architecture.md to what the bridge actually
reports. A unit of its own on a branch off dev; the QA pass must exercise the
idle shutdown live (short IMAGEGEN_IDLE_TIMEOUT_SECS) and both stop-hook guards,
and it must not disturb the turbo toggle or the Q6_K deployment (the same file
and the same config carry them).

Task 2, land Unit 10 (the image-edit path). feature/image-edit-probe carries
three unpushed commits (tip 2f07671) and has no PR; push it and open one against
dev, or fold the unit into a fresh branch off the current dev tip. Then wire the
adapter into the edit route: the edit body is a multipart POST to the backend's
/v1/images/edits, that route is OpenAI-shaped (no lora field, prompt tags
ignored), so investigate whether sd_cpp_extra_args inside the prompt is honoured
there and where a structured lora entry could go, or whether the edit path
should move to the native surface. UNVERIFIED: no turbo edit has been run.

Task 3, the quality and measurement follow-ups from the Q6_K decision:
resolutions above 1024 square on Q6_K (1536 is untested; ~2.4 GiB of the card is
free at the 9200 MiB peak), and a written definition of acceptable image quality,
which is still missing and is why the three-way comparison had to be argued from
a critic pass.

Task 4, upstream candidates, all needing the operator's go-ahead before anything is
sent: the JSON body-size hang (bypassed, still unexplained), and the lazily built
LoRA cache that rejects a valid adapter path until an unrelated route has been
called (the bridge primes it, which is a workaround).

Task 5, wrap: fold the outcome into section 2, replace section 3, refresh the memory
entry, close with "Unit: 16 complete".

Still open from earlier units, none of it done: create main and protect dev and main
on GitHub; register or correct the ensure-localembed hook; the JSON body-size hang;
and the missing definition of acceptable image quality, which is why the
three-way deployment comparison had to be argued from a critic pass.


4. Project facts a fresh session cannot re-derive

- Path map: repo at /data/imagegen (dev branch, remote origin = private
  JLFN/imagegen). Handoff here. Models to /data/imagegen/models/ (gitignored),
  binaries to bin/, test images and probe harnesses to out/ (gitignored),
  deployment files to packaging/, QA evidence to qa-evidence/.
- sd.cpp sources and measurements live OUTSIDE the repo at /data/build-sd:
  stable-diffusion.cpp matches the installed sd-cli and sd-server (master
  6dcb5bb, SD_CUDA=ON, SD_USE_UPSTREAM_GGML OFF, i.e. the patched ggml that
  accepts int8 convrot files). Its docs/ (qwen_image_2.1.md, qwen_image_edit.md,
  backend.md, int8_convrot.md, and the server's own examples/server/api.md) are
  the authority for flags and the HTTP contract. Measurement logs and harnesses:
  /data/build-sd/te-split (prod/, q6/, unit7/). Bridge log:
  /data/build-sd/bridge-split.log.
- The backend contract for editing, verified in source: POST /v1/images/edits
  takes multipart (prompt, image[], image, mask, n, size, output_format) and
  dispatches uploads into gen_params.ref_images
  (examples/server/routes_openai.cpp:174); the native schema behind
  sdapi/sdcpp/OpenAI-extras accepts ref_images as an array of base64 strings or
  data URLs (examples/server/api.md, examples/common/common.cpp:2226);
  sd_cpp_extra_args may be embedded in a prompt and is extracted and removed
  before generation (routes_openai.cpp:66, routes_sdapi.cpp:144); and every
  image field accepts the same base64/data-URL forms.
- Vision weights: needed for any edit; the deployed convrot encoder supplies
  them itself (351 model.visual.* tensors), the GGUF encoder needs the deleted
  mmproj, and no diffusion GGUF carries vision.
- Probe harnesses in out/ (gitignored, reuse them): probe-matrix.sh (encoder
  comparisons), probe-budget.sh (placement and resolution latency),
  e2e-edit.sh (bridge end-to-end), iso-edit-path.sh and iso2-body-size.sh (the
  backend isolation that found the body-size blocker), direct-server-edit.sh.
  Reference images: out/first.png at 1024 square, out/e2e-edit-ref768.png at 768.
- Lifecycle: HOOK-ONLY, no systemd, nothing autostarts (operator decision
  2026-09-26). ~/.opengrok/hooks/imagegen-bridge.json registers a PreToolUse hook
  (^(image_gen|image_edit)$) running bin/ensure-imagegen-bridge, and a SessionEnd
  hook running bin/stop-imagegen-bridge; both script files live in the project at
  packaging/hooks/ and are symlinked from ~/.opengrok/hooks/bin/. The systemd unit
  is RETIRED (file moved to imagegen-bridge.service.retired-20260926, disabled,
  reloaded, LoadState not-found), so the ensure script's detached launch is the
  live path. Lifecycle log: ~/.local/state/ensure-imagegen-bridge.log. Never
  replace bin/imagegen-bridge while it runs: the canonical builder fails with
  "Text file busy" and then deletes target/.
- Rebuilding: bash /data/build/linux/build.sh -p /data/imagegen -b imagegen-bridge
  (and -b convrot-verify). Iterative dev may use cargo directly (rule 5's
  exception); cargo test -p imagegen-bridge runs the unit tests.
- Machine facts: x86-64, Intel i7-14700F, 31 GB RAM, RTX 4070 SUPER 12 GB
  (12,282 MiB), ~400 GB free. /data/ds4 benchmarks models on the SAME GPU:
  check nvidia-smi before any run and never run two sd-cli/sd-server processes
  at once. A secret-free local API key string is fine (endpoint is 127.0.0.1).
- open-grok's image clients: ImageGenClient and image_edit live in
  /data/open-grok/crates/codegen/xai-grok-tools/src/implementations/grok_build/.
  Generation posts to {base}/images/generations; editing posts JSON to
  {base}/images/edits with "image" or "images" data URLs plus aspect_ratio.
  Both share one HTTP client with a 300 s total and 240 s read timeout, and
  image_edit compresses each reference to at most 400 KiB and 768 px.

5. Guardrails and open items

- Global (read the global AGENTS.md): no emojis; plain text; conventional
  commits; source-of-truth verification; session handoff protocol; rule 23
  branch model (dev is the integration branch; units on work branches; PRs).
- Project (this repo's AGENTS.md): zero Python in the delivered repo; model
  files never committed; graphify-rs AST graph built at commit time; the qa
  model is a user-chosen parameter (the operator asked for self-verification by
  the primary agent for Units 6-10; independent passes used qwen38-flash-next-ds4
  earlier).
- DECISIONS taken 2026-09-26, do not silently reverse them:
  1. No GUI is to be added for now. The operator uses the image tools from chat
     and, when needed, curl against http://127.0.0.1:8787. The option to build
     upstream's own Vue frontend (examples/server/frontend, pnpm + Vite, compiled
     into the binary with SD_SERVER_BUILD_FRONTEND=ON; not built here, no pnpm
     installed) stays open but unrequested.
  2. The service stays LOOPBACK ONLY (listen_ip 127.0.0.1). There is no
     authentication and no TLS, so binding 0.0.0.0 would let anyone on the
     network occupy the GPU. Anything that needs remote access goes through a
     tunnel or a proxy with auth, never a direct bind.
  3. Hook-only start, no systemd, nothing autostarts (section 2 and 4).
  4. The turbo toggle stays MANUAL (decided 2026-09-27): no hook entry calls
     packaging/hooks/imagegen-turbo; the operator toggles with that script or
     curl when a session needs turbo. The script is installed and symlinked, so
     wiring a trigger later is a one-line hook addition, but it is not wanted
     now.
  5. The STANDING DEPLOYMENT is base-only Q6_K with default VAE tiling, and the
     turbo keys stay commented out (decided 2026-09-27 on the operator's pick
     from a measured three-way comparison). Turbo is refused with 409 by design;
     bringing it back is the three-line edit above `diffusion` in imagegen.toml
     plus a bridge restart. Do not "helpfully" re-enable the adapter.

- OPEN items:
  1. feature/image-edit-probe (Unit 10, tip 2f07671) carries three unpushed
     commits and has no PR; land it (section 3 task 2).
  2. No PR is open as of this handoff: PRs 2-6 are merged and PR 1 is closed.
  3. dev's bridge source lost the on-demand lifecycle code in the fc9b9f9 merge
     (in_flight, idle_secs, idle_timeout_secs, the idle shutdown); the stop hook
     and architecture.md still assume it. See section 3 task 1.
  4. The systemd path is dead on this machine but alive in the repo: the Unit 8
     branch still carries packaging/imagegen-bridge.service and the ensure
     script's systemd branch, and the unit it carried pointed at a deleted
     encoder. Remove both when that branch merges.
  5. main does not exist and neither dev nor main is protected on GitHub.
  6. The ensure-localembed hook described in the docs is not registered anywhere.
  7. patches/0001 and 0002 are not upstreamed (needs an operator go-ahead), and the
     JSON body-size hang this session bypassed looks like a third candidate.
  8. No formal quality standard is defined; the QUALITY/DRAFT/TURBO recipe
     (docs/sampling-profiles.md) gives named profiles with measured realism
     ratings, but those are one critic pass over one scene each, not a standard.
  9. SessionEnd does not fire in headless runs, so the stop hook covers
     interactive sessions and the idle timer covers headless ones - and the idle
     timer itself is one of the things item 3 says is missing on dev.
  10. Untested and open: resolutions above 1024 square on Q6_K (about 2.4 GiB of
     the card is free at the 9200 MiB peak); the step count and cfg for edits
     (sd.cpp's own examples use cfg 2.5, we inherit 6.0); whether the convrot
     encoder's built-in vision tower matches the separate mmproj in quality; and
     whether removing the VRAM oversubscription would let 1024 square fit inside
     the timeout. The turbo toggle is deliberately manual (decision 4), and the
     deployment is base-only (decision 5).

6. Progress and staleness check (run this first, every session)

Progress meter:
    cd /data/imagegen
    git log --oneline 7443adfcd22374ddaa3e2139287e83a78d431367..HEAD
    git diff --stat 7443adfcd22374ddaa3e2139287e83a78d431367..HEAD
    git log --grep "Unit: .* complete" 7443adfcd22374ddaa3e2139287e83a78d431367..HEAD

Staleness check:
    cd /data/imagegen && git log --oneline 6e699b1..HEAD
- Any output means this handoff predates newer work: read the log and diffs
  before trusting sections 2-5, then advance last_updated_commit.
- Branch or remote differing from the frontmatter: stop and reconcile.
- ALSO check for open work branches, which the marker cannot see:
    git branch -a && gh pr list --state open
- And check whether the previous session left a probe or a server running:
    pgrep -af 'sd-cli|sd-server|imagegen-bridge'; nvidia-smi --query-gpu=memory.used --format=csv

This document is COMPLETE and self-contained: a new session can execute the
project from this file alone, with the model manifest and architecture doc
for the pinned facts and the global rules for the guardrails.
