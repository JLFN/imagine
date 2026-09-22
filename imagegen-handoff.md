---
project: imagegen
plan_start_commit: 7443adfcd22374ddaa3e2139287e83a78d431367
last_updated_commit: 1fbe512555466f2e70a89341309276c8ff330ee1
branch: dev
remote: none
handoff_written_at_context_usage: 59% (estimated: chat_history bytes / 4; this session had no signals.json)
handoff_written_at: 2026-09-22T05:36:00Z
session_id: 01a0c775-65e6-7e73-8df2-6220db99459f
---
--- handoff probe (auto) ---
=== handoff probe (imagegen) ===
--- units completed since plan start (7443adfcd22374ddaa3e2139287e83a78d431367):
9169fed feat(bridge): on-demand OpenAI image API bridge over sd-server
347ed52 docs(handoff): mark Unit 1 complete and plan Unit 2 (Rust bridge)
6b17eee feat(recipe): first real Qwen-Image-2.1 image on the workstation
--- progress diff stat:
.gitignore                |    4 +-
 Cargo.lock                | 1146 +++++++++++++++++++++++++++++++++++++++++++++
 Cargo.toml                |    7 +
 crates/bridge/Cargo.toml  |   15 +
 crates/bridge/README.md   |   64 +++
 crates/bridge/src/main.rs |  443 ++++++++++++++++++
 docs/architecture.md      |   62 ++-
 imagegen-handoff.md       |  162 +++++++
 models/MANIFEST.md        |   94 ++++
 9 files changed, 1988 insertions(+), 9 deletions(-)
--- staleness (last_updated_commit 341bc8d4bb3a4c7bb61c28806e51ac77002e73a8e .. HEAD):
  (clean: handoff is current)
--- end handoff probe ---


Handoff + executable plan — imagegen (2026-09-21)

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

This project is a sequence of units (Unit 1: runner + models + CLI smoke;
Unit 2: Rust bridge; Unit 3: open-grok wiring + end-to-end; Unit 4: QA,
graph, push). Each unit ends with a verified commit set on dev.

On completing a unit: do not stop at the boundary. Read the session's
signals.json (sessions/<percent-encoded-cwd>/<session-id>/ under
~/.opengrok; cwd is %2Fdata%2Fimagegen, pick the newest session dir):
- If contextWindowUsage < 40: write the next unit's plan into section 3,
  execute it, verify, commit, push (when a remote exists), re-check, repeat.
- At ~40%: do not pause or ask. Finish this handoff's update, launch a fresh
  session with the open-grok-handoff launcher, keep working.
The final commit of each unit carries the trailer "Unit: <N> complete" as
its last body line (git log --grep "Unit: .* complete").

PARALLEL WORK (2026-09-21): a sibling repo /data/imagegen-rs now contains a
pure-Rust port of the whole generation stack (candle: tokenizer, Qwen3-VL text
tower, 32-block DiT, Wan VAE, sampler, CLI, OpenAI server), replacing
stable-diffusion.cpp entirely. Verified against this repo's reference at
1024x1024 / 28 steps: identical noise and token ids, conditioning correlation
0.995, PSNR 26.87 dB against the sd.cpp image. Read
/data/imagegen-rs/imagegen-rs-handoff.md and its docs/parity.md before
changing anything that affects the model stack.

2. Current state (2026-09-22 — Units 1-5 complete; QA passed on Unit 5)

- Unit 5 (4e8f1a0, merged c3b8b26): the bridge starts sd-server with an explicit
  placement instead of --offload-to-cpu — diffusion parameters pinned in VRAM,
  encoder executing on CPU with parameters in RAM, VAE parameters in RAM with GPU
  compute — and defaults to Q6_K; override with IMAGEGEN_BACKEND_ARGS. Measured
  table and caveats in docs/architecture.md (Parameter placement).
- RUNNING: bridge pid 2124252 on 127.0.0.1:8787, sd-server 2124282; start with
  cd /data/imagegen && ./bin/imagegen-bridge. /health reports backend true; a
  1024x1024/28-step request returned a PNG in 143.6 s (open-grok limit 240 s).
  config.toml wiring unchanged.
- Known cost: with the DiT pinned the VAE decode OOMs at 1024 square and tiles
  (4.4-4.7 s against 2.1 s); 1344 square is unverified under this layout.
- The recommended int8 convrot encoder (models/text_encoders/
  qwen3vl_8b_int8_convrot.safetensors) CANNOT load as --llm: the loader misreads
  its geometry and aborts on a GGML_ASSERT. Only the GGUF encoder runs.
- Verified: QA PASS (qwen38-flash-next-ds4, 21 surfaces, evidence at 1fbe512),
  osv clean (115 packages, 0 vulnerabilities), graphify-rs graph rebuilt.

3. Next unit plan — Unit 6: resolution headroom, backend supervision, delivery

Status: Units 1-5 are complete and verified (section 2); the QA gate passed and
the osv scan is clean. Three items are open, in priority order. dev is the
working branch; every code phase below needs a feature/ branch off dev (rule
23), and every measurement needs the bridge STOPPED first, because the bridge
holds 5.6 GB of Q6_K weights pinned in VRAM and a second sd-cli run at 1024
square or above will fight it for the card.

- Phase 1, resolution headroom (code, feature branch). The pinned layout was
  measured only at 512 and 1024 square; the Q6_K quality tier the project used
  before is 1344 square and is now unverified. Measure it:
    cd /data/imagegen && kill -INT <bridge pid> && \
    ./bin/sd-cli --diffusion-model models/qwen-image-2.1-Q6_K.gguf \
      --vae models/qwen_image_2.1_vae_bf16.safetensors \
      --llm models/Qwen3VL-8B-Instruct-Q4_K_M.gguf \
      -p "<prompt>" --cfg-scale 6.0 --sampling-method euler -v \
      --backend te=cpu,diffusion=cuda0,vae=cuda0 \
      --params-backend te=cpu,diffusion=cuda0,vae=cpu \
      -W 1344 -H 1344 --steps 40 --seed 42 -o out/res1344.png
  then restart the bridge. If it OOMs, choose between a --max-vram budget,
  keeping --offload-to-cpu for sizes above 1024, or documenting the cap.
  Acceptance: a measured 1344-square number recorded in docs/architecture.md
  (Parameter placement), with the mitigation if it fails.

- Phase 2, backend supervision (code, feature branch; the user has not asked
  for this yet, so confirm before spending the unit). The bridge spawns
  sd-server once, never waits on it and never respawns it: observed
  2026-09-22 after 8h51m of uptime with a zombie sd-server child, port 8788
  dead, /health reporting backend false, so every image request failed until
  the bridge was restarted by hand. Reproduce by killing the sd-server child
  and watching the bridge keep answering /v1/models. Fix by reaping the child
  and respawning a backend that died, or by failing loudly.
  Acceptance: killing the backend produces a respawn or a clear error,
  verified live, with the QA gate re-run.

- Phase 3, delivery state and the remote decision (carried over from the old
  Unit 4 plan, still open). No remote exists. If the user wants one: pick host
  and visibility first, run
    gh repo view OWNER/REPO --json isPrivate,visibility   (rule 6)
  before any push, re-run the osv scan (rule 10; last clean 2026-09-22, 115
  packages), then git remote add origin <url> and push dev. main stays behind
  a reviewed PR (rule 23). If the user declines a remote, record the decision
  here: the repo is deliverable on dev alone.

- Phase 4, wrap. Fold this unit's outcome into section 2, replace section 3
  wholesale with the next plan, refresh the memory entry
  (imagegen-e61a3c5c), and close with the trailer "Unit: 6 complete".

4. Project facts a fresh session cannot re-derive

- Path map: repo at /data/imagegen (dev branch, no remote). Handoff here.
  Model files go to /data/imagegen/models/ (gitignored). Runner to
  /data/imagegen/bin/sd-cli (gitignored). Test images to /data/imagegen/out/
  (gitignored).
- sd.cpp sources and measurements live OUTSIDE the repo, at /data/build-sd:
  stable-diffusion.cpp is the source tree matching the installed sd-cli and
  sd-server (upstream master 6dcb5bb, built with SD_CUDA=ON, SD_USE_UPSTREAM_GGML
  OFF, i.e. the patched ggml that accepts int8 convrot files). Its docs/
  (qwen_image_2.1.md, backend.md, int8_convrot.md) are the authority for flag
  semantics. The layout measurement logs and scripts are under
  /data/build-sd/te-split (prod/ has the 1024 runs A/B/C, q6/ the pinned Q6_K
  run, refined/ the VAE-in-RAM variant, and measure*.sh the harnesses).
- Machine facts: this workstation (where open-grok runs) is x86-64, Intel
  i7-14700F, 31 GB RAM, RTX 4070 SUPER 12 GB, ~411 GB free on /. The NVIDIA
  driver works (nvidia-smi reports the GPU). A secret-free local API key
  string is fine for the bridge (endpoint is 127.0.0.1).
- The Spark: gx10-e1d2 at 192.168.1.91, ARM64. It serves Qwen3.8 Flash
  Next via ds4-server (start-qwen38.sh) on port 8003; DeepSeek (:8000) and
  Laguna (:8002) are idle. SSH to it is refused; file access is via the
  gio sftp mount, re-established with: gio mount sftp://leandro@192.168.1.91/
  (mounts at /run/user/1000/gvfs/sftp:host=192.168.1.91,user=leandro).
- Skills that apply: hf-cli (on the box for Hugging Face download), imagine
  (image tool usage guidance), rust-build (rule 5 builder), osv-mcp, and the
  user-guide docs for the [toolset.image_gen] block.

5. Guardrails and open items

- Global (read the global AGENTS.md): no emojis; plain text; conventional
  commits; source-of-truth verification; session handoff protocol; rule 23
  branch model (dev working line; code units on feature/ branches; handoff/
  docs commits direct on dev).
- Project (this repo's AGENTS.md): zero Python in the delivered repo; model
  files never committed; graphify-rs AST graph built at commit time; qa
  model is a user-chosen parameter (ask the user for qa-model=<slug> before
  the QA gate).
- OPEN items:
  1. RESOLVED in Unit 1: the model mirror is abenzerps/Qwen-Image-2.1-GGUF
     (SHA256SUMS verified; the KasugaiSakura link was not needed).
  2. Confirm the open-grok image tool's client timeout tolerates the cold
     start (load ~10 GB + generation); if not, adjust at wiring time.
  3. Bridge port 8787 is proposed but unverified free; check ss -tlnp at
     wiring time.
  4. Remote: none yet. Pushing to GitHub later requires a visibility check
     first (rule 6) and an osv scan (rule 10).

6. Progress and staleness check (run this first, every session)

Progress meter:
    cd /data/imagegen
    git log --oneline 7443adfcd22374ddaa3e2139287e83a78d431367..HEAD
    git diff --stat 7443adfcd22374ddaa3e2139287e83a78d431367..HEAD
    git log --grep "Unit: .* complete" 7443adfcd22374ddaa3e2139287e83a78d431367..HEAD

Staleness check:
    cd /data/imagegen && git log --oneline 7443adfcd22374ddaa3e2139287e83a78d431367..HEAD
- Any output means this handoff predates newer work: read the log and diffs
  before trusting sections 2-5, then advance last_updated_commit.
- Branch or remote differing from the frontmatter: stop and reconcile.

This document is COMPLETE and self-contained: a new session can execute the
project from this file alone, with the model manifest and architecture doc
for the pinned facts and the global rules for the guardrails.
