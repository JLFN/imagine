---
project: imagegen
plan_start_commit: 7443adfcd22374ddaa3e2139287e83a78d431367
last_updated_commit: 0b6f338
branch: dev
work_branch: feature/image-edit-probe (in flight, off origin/dev, commit 588ac82)
remote: origin (PRIVATE, github.com/JLFN/imagegen) - created 2026-09-22, dev pushed
handoff_written_at_context_usage: 40% (signals.json, session 01a0de48)
handoff_written_at: 2026-09-26T17:55:00Z
session_id: 01a0de48-f809-7353-a458-31b3f19d943f
---
--- handoff probe (auto) ---
=== handoff probe (imagegen) ===
--- units completed since plan start (7443adfcd22374ddaa3e2139287e83a78d431367):
4e8f1a0 feat(bridge): pin Q6_K diffusion in VRAM and run the text encoder on CPU
9169fed feat(bridge): on-demand OpenAI image API bridge over sd-server
347ed52 docs(handoff): mark Unit 1 complete and plan Unit 2 (Rust bridge)
6b17eee feat(recipe): first real Qwen-Image-2.1 image on the workstation
--- progress diff stat:
.gitignore                                         |   13 +-
 Cargo.lock                                        | 1228 ++++++++++++++++++++
 Cargo.toml                                        |    7 +
 crates/bridge/Cargo.toml                          |   16 +
 crates/bridge/README.md                           |   87 ++
 crates/bridge/src/main.rs                         |  777 +++++++++++++
 docs/architecture.md                              |  173 ++-
 docs/convrot-encoder-experiment.md                |  234 ++++
 imagegen-handoff.md                               |  216 ++++
 imagegen.toml                                     |   57 +
 models/MANIFEST.md                                |  115 ++
 ...01-sd-cpp-llm-te-config-skip-weight-scale.patch |   53 +
 ...-cpp-dequantize-int8-tensorwise-embedding.patch |  172 +++
 qa-evidence/qa-gen-1024-28steps-seed42.png         |  Bin 0 -> 2463614 bytes
 qa-evidence/qa-gen-512-seed11.png                  |  Bin 0 -> 286535 bytes
 qa-evidence/qa-gen-512-seed3.png                   |  Bin 0 -> 280816 bytes
 qa-evidence/qa-report.md                           |  218 ++++
 tools/convrot-te-experiment.sh                     |   72 ++
 tools/verify-convrot-dequant.py                    |  105 ++
 19 files changed, 3529 insertions(+), 14 deletions(-)
--- staleness (last_updated_commit e7c478f .. HEAD):
0b6f338 docs(handoff): record Units 7 and 8 as open PRs and plan Unit 9
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
(the image-edit path) is in flight as a probe branch (section 2, section 3).

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

2. Current state (2026-09-26 — edit path measured; Units 7 and 8 still open PRs)

- dev is at 0b6f338, pushed to the private remote JLFN/imagegen. Units 7 and 8
  are STILL NOT on dev: verified 2026-09-26 with gh pr list --state open, which
  shows PR 1 (feature/unit7-resolution-and-port -> dev) and PR 2
  (feature/on-demand-lifecycle, stacked on PR 1). Nothing new may merge until
  PR 1 lands.
- In flight: feature/image-edit-probe, off origin/dev, commit 588ac82
  (docs-only). It carries the measured image-edit findings and NO code yet.
- Image editing WORKS end to end through the runner; the bridge has no route
  for it. Backend contract and measurements are in docs/architecture.md,
  "Image editing (measured 2026-09-26)"; model facts in models/MANIFEST.md.
- Vision weights: the deployed int8 convrot encoder is a COMBINED encoder
  (1,258 tensors, 351 of them model.visual.*), so it edits with --llm alone.
  The GGUF encoder is LLM-only and needs the mmproj projector; that file was
  downloaded, measured, then deleted at the operator's request (re-download is
  a 30 s hf call; the manifest records its checksum).
- Measured, 1024 square / 28 steps / seed 42 / DiT pinned / VAE parameters in
  RAM, one run each: GGUF Q4_K_M plus mmproj 334.26 s, peak VRAM 9,207 MiB,
  peak RSS 7.4 GiB; convrot alone 426.24 s, peak VRAM 9,304 MiB, peak RSS
  12.8 GiB. Both exceed open-grok's 240 s read timeout; sampling rates match
  (7.05 against 7.10 s/it), so the whole gap is conditioning.
- Machine state verified this session: no bridge and no sd-server running, GPU
  idle at 331 MiB, no stray probe processes or sampler loops.
- The rule 18 precondition was met for the memory work: localembed healthy at
  127.0.0.1:8765 (multilingual-e5-small, 384 dims, loaded true).

3. Next unit plan — Unit 10: serve image editing through the bridge

Prerequisite (operator action, unchanged from Unit 9): land the open PR stack —
review and merge PR 1 into dev, retarget PR 2's base to dev (gh pr edit 2
--base dev), refresh it with dev, confirm its diff is only Unit 8, then merge.
Rule 23 allows one in-flight unit, so settle the stack before pushing Unit 10.

- Phase 1, decide the edit encoder and the latency budget. The measured
  configurations both miss the 240 s open-grok read timeout, so the budget must
  be found before the route is written. Candidates, all UNMEASURED: fewer steps
  (16 steps at 7.05 s/it samples ~113 s, total ~250 s, still marginal); a
  smaller edit resolution (fewer image tokens, cheaper ViT forward); moving the
  text-encoder GRAPH to the GPU while its parameters stay in RAM (--backend
  te=cuda0 with --params-backend te=cpu), which risks OOM on a 12 GB card that
  already holds the pinned DiT. Also decide the encoder: GGUF plus mmproj is
  92 s faster per edit but needs the 752 MB file re-downloaded; convrot needs
  no extra file and is the deployed default. Acceptance: a measured edit under
  240 s, end to end, with the chosen settings written into imagegen.toml or the
  bridge defaults.
- Phase 2, the bridge route. Add POST /v1/images/edits to
  crates/bridge/src/main.rs (today it registers only /v1/models,
  /v1/images/generations, /health — lines 753-755). open-grok's client posts
  JSON to {base_url}/images/edits with model, prompt, n, resolution,
  response_format, and "image" (one data URL) or "images" (array) plus
  aspect_ratio; the backend wants multipart (prompt, image[], image, mask, n,
  size, output_format). So the route must decode base64 data URLs and re-encode
  multipart toward sd-server, which needs reqwest's multipart feature in
  crates/bridge/Cargo.toml. Acceptance: a real request from open-grok's own
  client shape reaches the backend and returns a PNG; a unit test covers the
  data-URL to multipart translation.
- Phase 3, the guardrails the edit path currently bypasses: the max_edge
  ceiling (default 1152) and the generation defaults (28 steps, cfg 6.0) are
  applied on the generation path only. Note that sd.cpp's own edit examples use
  cfg-scale 2.5 (docs/qwen_image_edit.md), so edit defaults must be chosen
  deliberately, not inherited. Acceptance: an over-limit or malformed edit
  request gets a 400 naming the limit, exactly as generation does.
- Phase 4, verification and QA under rule 19: run the QA gate, record an edit
  run in qa-evidence/, write the edit section of crates/bridge/README.md, and
  rebuild the graphify graph at commit time. Then the PR into dev.
- Phase 5, wrap: fold the probe outcome into section 2, replace section 3,
  refresh the memory entry (imagegen-fadeb80d — the LIVE workspace key; the
  older imagegen-e61a3c5c is orphaned by the 2026-09-22 remote creation, and
  its content was migrated into the live key on 2026-09-26), and close with
  "Unit: 10 complete".

Still open from Unit 9, not yet done anywhere: create main and protect dev and
main on GitHub; register or correct the ensure-localembed hook (the docs
describe it, no hooks json registers it); fix crates/bridge/README.md's
configuration table, which names an IMAGEGEN_DIFFUSION default the binary does
not have; upstream patches/0001 and 0002 to leejet/stable-diffusion.cpp (an
EXTERNAL action, needs the operator's go-ahead); define what acceptable image
quality means (no quality standard exists in this project).

4. Project facts a fresh session cannot re-derive

- Path map: repo at /data/imagegen (dev branch, remote origin = private
  JLFN/imagegen). Handoff here. Model files go to /data/imagegen/models/
  (gitignored). Binaries to /data/imagegen/bin/ (gitignored). Test images to
  /data/imagegen/out/ (gitignored). Deployment files to /data/imagegen/packaging/.
- sd.cpp sources and measurements live OUTSIDE the repo, at /data/build-sd:
  stable-diffusion.cpp is the source tree matching the installed sd-cli and
  sd-server (upstream master 6dcb5bb, built with SD_CUDA=ON, SD_USE_UPSTREAM_GGML
  OFF, i.e. the patched ggml that accepts int8 convrot files). Its docs/
  (qwen_image_2.1.md, qwen_image_edit.md, backend.md, int8_convrot.md) are the
  authority for flag semantics. Measurement logs and harnesses: /data/build-sd/te-split
  (prod/, q6/, refined/, unit7/ with the Unit 7 measurement set and logs,
  measure-unit7.sh, and e2e-hook-test.sh for the Unit 8 hook test). The bridge's
  own log is /data/build-sd/bridge-split.log.
- Image-edit facts (measured 2026-09-26, full detail in docs/architecture.md):
  the backend already serves POST /v1/images/edits as multipart and dispatches
  the uploads into gen_params.ref_images; vision weights are required and the
  deployed convrot encoder supplies them; both measured configurations missed
  the 240 s timeout. Probe artefacts are gitignored under out/:
  out/probe-edit.* (GGUF plus mmproj), out/probe-a2.* (convrot alone),
  out/probe-matrix.sh (the harness), out/first.png (the reference image).
- Lifecycle (Unit 8): the hook registration is global
  (~/.opengrok/hooks/imagegen-bridge.json, events PreToolUse ^(image_gen|image_edit)$
  and SessionEnd); the scripts it runs are versioned in the repo under
  packaging/hooks/ and symlinked from ~/.opengrok/hooks/bin/; the systemd user unit
  is packaging/imagegen-bridge.service, installed to ~/.config/systemd/user/ and
  deliberately NOT enabled; the lifecycle log is
  ~/.local/state/ensure-imagegen-bridge.log. Start by hand:
  systemctl --user start imagegen-bridge (or the ensure script). Stop: the stop
  script, or systemctl --user stop imagegen-bridge --force. Never replace the
  binary while it runs: the canonical builder fails with "Text file busy" and then
  deletes target/, silently discarding the build.
- Rebuilding: bash /data/build/linux/build.sh -p /data/imagegen -b imagegen-bridge
  (and -b convrot-verify for the verifier). The builder deletes target/ each time
  and copies into bin/.
- Machine facts: x86-64, Intel i7-14700F, 31 GB RAM, RTX 4070 SUPER 12 GB
  (12,282 MiB), ~400 GB free on /. A secret-free local API key string is fine for
  the bridge (endpoint is 127.0.0.1). The NVIDIA Spark (192.168.1.91) is NOT
  involved in image generation. Another project (/data/ds4) benchmarks models on
  the same GPU: check nvidia-smi before any generation and never run two sd-cli
  processes at once.
- Stray model downloads: an hf download run once dropped an identical 9.35 GB copy
  of the int8 convrot encoder at /data/imagegen/text_encoders/. It is gitignored
  and was left for the operator to remove; check before deleting anything.
- Model inventory on disk (models/ is gitignored; see models/MANIFEST.md for
  checksums): Q6_K (deployed diffusion default) and Q4_K_M diffusion GGUFs, the
  UC uncensored Q4_K_M finetune, the GGUF Q4_K_M encoder, the int8 convrot
  encoder (deployed default), the VAE. No mmproj (deleted 2026-09-26).
- open-grok's image tool: ImageGenClient in
  /data/open-grok/crates/codegen/xai-grok-tools/src/implementations/grok_build/image_gen/mod.rs,
  a pure HTTP client (no process spawning) with a 300 s total and 240 s read
  timeout; image_edit/mod.rs is the edit client that posts JSON to
  {base_url}/images/edits. That is why the lifecycle is hook-driven rather than
  tool-driven.

5. Guardrails and open items

- Global (read the global AGENTS.md): no emojis; plain text; conventional
  commits; source-of-truth verification; session handoff protocol; rule 23
  branch model (dev is the integration branch; units on work branches; PRs).
- Project (this repo's AGENTS.md): zero Python in the delivered repo; model
  files never committed; graphify-rs AST graph built at commit time; the qa
  model is a user-chosen parameter (the operator has asked for self-verification
  by the primary agent for Units 6-8; an independent model was used earlier).
- OPEN items:
  1. PR 1 then PR 2 await review and merge (section 3 prerequisite).
  2. main does not exist and neither dev nor main is protected on GitHub yet.
  3. The ensure-localembed hook described in the docs is not registered anywhere.
  4. patches/0001 and 0002 are not upstreamed (needs an operator go-ahead).
  5. No quality standard is defined anywhere in the project.
  6. SessionEnd does not fire in headless open-grok runs (measured), so the stop
     hook covers interactive sessions and the idle timer covers headless ones.
  7. The edit path misses open-grok's 240 s read timeout at 1024 square and 28
     steps; the budget work is Unit 10 phase 1 and is not yet done.
  8. The convrot encoder's built-in vision tower is used by editing, but whether
     it carries the same quality as the separate mmproj is unmeasured.

6. Progress and staleness check (run this first, every session)

Progress meter:
    cd /data/imagegen
    git log --oneline 7443adfcd22374ddaa3e2139287e83a78d431367..HEAD
    git diff --stat 7443adfcd22374ddaa3e2139287e83a78d431367..HEAD
    git log --grep "Unit: .* complete" 7443adfcd22374ddaa3e2139287e83a78d431367..HEAD

Staleness check:
    cd /data/imagegen && git log --oneline 0b6f338..HEAD
- Any output means this handoff predates newer work: read the log and diffs
  before trusting sections 2-5, then advance last_updated_commit.
- Branch or remote differing from the frontmatter: stop and reconcile.
- ALSO check for open work branches, which the marker cannot see:
    git branch -a && gh pr list --state open

This document is COMPLETE and self-contained: a new session can execute the
project from this file alone, with the model manifest and architecture doc
for the pinned facts and the global rules for the guardrails.
