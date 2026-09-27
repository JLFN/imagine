---
project: imagegen
plan_start_commit: 7443adfcd22374ddaa3e2139287e83a78d431367
last_updated_commit: 4ef1ac1
branch: dev
work_branch: none in flight on the local tree; PR 5 (feature/turbo-toggle, tip 4ef1ac1) is open, and PRs 1-4 carry the older branches
remote: origin (PRIVATE, github.com/JLFN/imagegen) - created 2026-09-22, dev pushed
handoff_written_at_context_usage: ~16% (signals.json, session 01a0e408)
handoff_written_at: 2026-09-27T18:30:00Z
session_id: 01a0e408-098e-7b42-b6e6-7b9d5daf9075
---
--- handoff probe (auto) ---
=== handoff probe (imagegen) ===
--- units completed since plan start (7443adfcd22374ddaa3e2139287e83a78d431367):
6e718d2 feat(repo): measure the resolution headroom, port the verifier to Rust, bound the bridge
4e8f1a0 feat(bridge): pin Q6_K diffusion in VRAM and run the text encoder on CPU
9169fed feat(bridge): on-demand OpenAI image API bridge over sd-server
347ed52 docs(handoff): mark Unit 1 complete and plan Unit 2 (Rust bridge)
6b17eee feat(recipe): first real Qwen-Image-2.1 image on the workstation
--- progress diff stat:
.gitignore                                         |   13 +-
 AGENTS.md                                          |    5 +-
 Cargo.lock                                         | 1491 ++++++++++++++++++++
 Cargo.toml                                         |    9 +
 crates/bridge/Cargo.toml                           |   16 +
 crates/bridge/README.md                            |  145 ++
 crates/bridge/src/main.rs                          | 1135 +++++++++++++++
 crates/convrot-verify/Cargo.toml                   |   10 +
 crates/convrot-verify/src/main.rs                  |  343 +++++
 docs/architecture.md                               |  386 ++++-
 docs/convrot-encoder-experiment.md                 |  242 ++++
 docs/viggle-turbo-lora.md                          |  312 ++++
 imagegen-handoff.md                                |  263 ++++
 imagegen.toml                                      |  114 ++
 models/MANIFEST.md                                 |  160 +++
 packaging/hooks/ensure-imagegen-bridge             |  117 ++
 packaging/hooks/stop-imagegen-bridge               |  104 ++
 ...01-sd-cpp-llm-te-config-skip-weight-scale.patch |   53 +
 ...-cpp-dequantize-int8-tensorwise-embedding.patch |  172 +++
 qa-evidence/qa-gen-1024-28steps-seed42.png         |  Bin 0 -> 2463614 bytes
 qa-evidence/qa-gen-1152-28steps-seed5.png          |  Bin 0 -> 2826304 bytes
 qa-evidence/qa-gen-1152-8steps-seed5.png           |  Bin 0 -> 2297223 bytes
 qa-evidence/qa-gen-512-seed11.png                  |  Bin 0 -> 286535 bytes
 qa-evidence/qa-gen-512-seed3.png                   |  Bin 0 -> 280816 bytes
 qa-evidence/qa-report.md                           |  133 ++
 qa-evidence/qa-tiling-1024-untiled-vs-tiled.png    |  Bin 0 -> 2771580 bytes
 qa-evidence/unit6-qa-report.md                     |  218 +++
 qa-evidence/unit7-qa-report.md                     |  271 ++++
 tests/qa-gate.sh                                   |  109 ++
 tests/run.sh                                       |   34 +
 tools/convrot-te-experiment.sh                     |   72 +
 31 files changed, 5912 insertions(+), 15 deletions(-)
--- staleness (last_updated_commit 90618e4 .. HEAD):
bf76419 docs(handoff): record the turbo adapter units and plan Unit 12
6aaa31f test(qa): self-verified QA pass over the bridge turbo LoRA unit
21e61a8 test(qa): self-verified QA pass over the Viggle turbo LoRA recipe unit
639a098 docs(handoff): refresh the session probe block on dev
86171d7 Merge pull request #2 from JLFN/feature/on-demand-lifecycle
af4e6c9 merge: bring the on-demand branch onto dev, keeping the turbo request path
00c257e Merge pull request #4 from JLFN/feature/viggle-turbo-lora
8a76b6b Merge pull request #3 from JLFN/feature/bridge-turbo-lora
f0d663c docs(recipe): record that the bridge now serves the turbo adapter
671d03a feat(bridge): apply the Viggle turbo LoRA and its sigma schedule
d3a1686 docs(recipe): add the Viggle turbo LoRA recipe and register the adapter
98ffc4a docs(handoff): record the no-GUI and loopback-only decisions
d607893 docs(handoff): record Unit 10 complete, the hook-only lifecycle and the new limits
4066f8e feat(lifecycle): start the bridge on demand from a hook and let it idle out
d36eb68 docs(qa): add a production-step artifact at the new ceiling and record the step-count effect
6e718d2 feat(repo): measure the resolution headroom, port the verifier to Rust, bound the bridge
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

2. Current state (2026-09-27 — turbo is a runtime mode; base is the default)

- dev LOCAL is 282d380, five commits ahead of origin/dev and unpushed (handoff
  probes and QA evidence only). Rule 23 forbids pushing dev directly.
- feature/turbo-toggle (4ef1ac1) is pushed and open as PR 5: the bridge starts in
  BASE mode (steps 28, cfg 6.0, no adapter) and switches without a restart via
  GET/POST /v1/turbo, packaging/hooks/imagegen-turbo on|off|status, or a
  per-request "turbo" flag. Enabling turbo is verified against the backend or
  refused with 409. QA evidence: qa-evidence/qa-report.md; gate PASS.
- Measured through the rebuilt bridge: base 1024 square seed 4242 169.5 s cold,
  7272 MiB peak; turbo same seed 26.6 s warm, 7978 MiB, "(454 / 454) LoRA
  tensors have been applied"; base after turbo byte-identical to base before it.
  Q4_K_M plus 16x16 VAE tiles are startup-fixed, so base mode inherits them.
- Unit 10 (edits) unchanged at 511c061 on feature/image-edit-probe, no PR yet.
  PR state checked live: PRs 2, 3 and 4 are merged, PR 1 is closed with its
  content in dev, and only PR 5 (this unit) is open.
- FOUND 2026-09-27, NOT fixed: dev's bridge source lacks the on-demand lifecycle
  code (in_flight, idle_secs, idle_timeout_secs, the idle shutdown) that 4066f8e
  added and that architecture.md and stop-imagegen-bridge still assume - the
  merge af4e6c9 resolved main.rs to the turbo side, and guard 2 of the stop hook
  reads an in_flight field that no longer exists. Reconcile with
  feature/on-demand-lifecycle.

3. Next unit plan — Unit 13: land the PR stack, then restore the lifecycle code

Task 1, land the remaining work. Review and merge PR 5
(feature/turbo-toggle); the rest of the stack is already in dev (PRs 2, 3 and 4
merged, PR 1 closed with its content in dev - verified with gh pr list --state
all and git branch --merged dev on 2026-09-27). What still sits outside dev is
feature/image-edit-probe (Unit 10, no PR, three commits unpushed) and dev's own
six local commits, which are handoff and QA docs only: dev is never pushed
directly, so either PR them or fold them into the next branch. Then create main
plus protection for dev and main on GitHub.

Task 2, restore the on-demand lifecycle on dev (see section 2's finding). Diff
feature/on-demand-lifecycle's crates/bridge/src/main.rs against dev's, and either
restore in_flight/idle_secs/idle_timeout_secs and the idle shutdown, or change
stop-imagegen-bridge's guard 2 and architecture.md to what the bridge actually
reports. A unit of its own on a branch off dev; the QA pass must exercise the
idle shutdown live (short IMAGEGEN_IDLE_TIMEOUT_SECS) and both stop-hook guards,
and it must not disturb the turbo toggle (its state and endpoints were added on
top of the same file).

Task 3, wire the adapter into the edit route (only after Unit 10 lands). The edit
body is a multipart POST to the backend's /v1/images/edits, and that route is
OpenAI-shaped: it takes no lora field and ignores prompt tags. Investigate first
whether it honours sd_cpp_extra_args inside the prompt (which is how the schedule
would travel) and where a structured lora entry could go, or whether the edit path
should move to the native surface instead. UNVERIFIED: no turbo edit has been run,
and the generation wiring gives no evidence about the edit route.

Task 4, the cheap measurements left open at 1024 square: VAE tile sizes between
16x16 and the 32x32 default (24x24 was never tried), flash attention as an
alternative way to recover the decode margin, and the cfg-isolation run (6 steps at
cfg 2.0) that would split the per-step gain between step count and guidance.

Task 5, upstream candidates, all needing the operator's go-ahead before anything is
sent: the JSON body-size hang (bypassed, still unexplained), and the lazily built
LoRA cache that rejects a valid adapter path until an unrelated route has been
called (the bridge primes it, which is a workaround).

Task 6, wrap: fold the outcome into section 2, replace section 3, refresh the memory
entry, close with "Unit: 13 complete".

Still open from earlier units, none of it done: create main and protect dev and main
on GitHub; register or correct the ensure-localembed hook; the JSON body-size hang;
and the missing definition of acceptable image quality, which is why Q4_K_M could
not be scored against Q6_K and why the turbo output has only been judged by eye.


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

- OPEN items:
  1. Three local commits on feature/image-edit-probe (8922ae2 is superseded by
     72a641f) are unpushed as of this handoff; push them with the Unit 10 PR.
  2. PR 5 (feature/turbo-toggle) is open and is the only open PR; PRs 2, 3 and 4
     are merged and PR 1 is closed. The only branch with unmerged work besides
     PR 5 is feature/image-edit-probe (Unit 10, no PR). Rule 23 wants one unit at
     a time: land PR 5 first.
  3. dev's bridge source lost the on-demand lifecycle code in the af4e6c9 merge
     (in_flight, idle_secs, idle_timeout_secs, the idle shutdown); the stop hook
     and architecture.md still assume it. See section 3 task 2.
  4. The systemd path is dead on this machine but alive in the repo: the Unit 8
     branch still carries packaging/imagegen-bridge.service and the ensure
     script's systemd branch, and the unit it carried pointed at a deleted
     encoder. Remove both when that branch merges.
  5. main does not exist and neither dev nor main is protected on GitHub.
  6. The ensure-localembed hook described in the docs is not registered anywhere.
  7. patches/0001 and 0002 are not upstreamed (needs an operator go-ahead), and the
     JSON body-size hang this session bypassed looks like a third candidate.
  8. No quality standard is defined anywhere in the project, which is why Q4_K_M
     could not be scored against Q6_K.
  9. SessionEnd does not fire in headless runs, so the stop hook covers
     interactive sessions and the idle timer covers headless ones - and the idle
     timer itself is one of the things item 3 says is missing on dev.
  10. Untested and open: the step count and cfg for edits (sd.cpp's own examples
     use cfg 2.5, we inherit 6.0); whether the convrot encoder's built-in vision
     tower matches the separate mmproj in quality; whether removing the VRAM
     oversubscription would let 1024 square fit inside the timeout; and whether
     the operator wants the turbo mode bound to an open-grok hook (the script
     packaging/hooks/imagegen-turbo is installed and symlinked, but no hook
     entry calls it yet - the trigger is the operator's call).

6. Progress and staleness check (run this first, every session)

Progress meter:
    cd /data/imagegen
    git log --oneline 7443adfcd22374ddaa3e2139287e83a78d431367..HEAD
    git diff --stat 7443adfcd22374ddaa3e2139287e83a78d431367..HEAD
    git log --grep "Unit: .* complete" 7443adfcd22374ddaa3e2139287e83a78d431367..HEAD

Staleness check:
    cd /data/imagegen && git log --oneline 3f3843e..HEAD
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
