---
project: imagegen
plan_start_commit: 7443adfcd22374ddaa3e2139287e83a78d431367
last_updated_commit: 6b17eee28922021763223e78c780bd07e3931831
branch: dev
remote: none
handoff_written_at_context_usage: 0%
handoff_written_at: 2026-09-21T20:15:00Z
session_id: 01a0c4e0-21e1-7312-ac5d-48e842962cdf
---
--- handoff probe (auto) ---
=== handoff probe (imagegen) ===
--- units completed since plan start (7443adfcd22374ddaa3e2139287e83a78d431367):
  (none)
--- progress diff stat:
imagegen-handoff.md | 162 ++++++++++++++++++++++++++++++++++++++++++++++++++++
 1 file changed, 162 insertions(+)
--- staleness (last_updated_commit 7443adfcd22374ddaa3e2139287e83a78d431367 .. HEAD):
f0b85a0 docs(handoff): record the imagegen plan and launch point
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

2. Current state (2026-09-21 — Unit 1 complete, verified)

- Unit 1 landed on dev at 6b17eee. sd-cli built from source (CUDA 13.3,
  -DSD_CUDA=ON, sm_89); the three model files are downloaded and sha256-
  verified (diffusion 833439e9..., encoder 67d1659b..., vae bb21f747...);
  the first 1024x1024 image generated coherently, exit 0. Measured numbers
  are in that commit body and docs/architecture.md (Measured performance).
- Runner installed at bin/sd-cli (gitignored); source build area
  /data/build-sd (sd-server is also built there). nvcc is NOT on PATH —
  it is /usr/local/cuda/bin/nvcc; set CUDACXX or CMAKE_CUDA_COMPILER.
- Machine re-verified: RTX 4070 SUPER 12 GB (peak VRAM 11.5 GB at 1024 sq),
  31 GB RAM, 411 GB free. Wiring/runner constraints unchanged.
- Current unit: Unit 2 (Rust bridge), plan in section 3.

3. Next unit plan — Unit 2: Rust bridge (OpenAI image API)

Status: not yet started. Goal: a small Rust HTTP server on 127.0.0.1:8787
that speaks the OpenAI image API and manages the sd-cli child process, so
open-grok's [toolset.image_gen] can call it. Ground truth:
docs/architecture.md (Bridge design), the built bin/sd-cli, and Unit 1's
measured numbers. Branch: feature/bridge off dev (rule 23); canonical
builder /data/build (rule 5); osv scan before push (rule 10); qa-model is a
user parameter, ask before the QA gate (rule 19).

- Phase 1, design confirm. Confirm sd-cli is one-shot (generates one image
  and exits) vs persistent; check whether sd-server already serves
  /v1/images/generations with b64 (reference-implementation rule). Fix the
  process model: likely per-request spawn, so each request pays cold start
  (~30 s load + generation) unless a persistent mode is available; carry
  the measured peak-VRAM/load costs from Unit 1 into the decision.
- Phase 2, implement the bridge. Rust HTTP server exposing:
    GET  /v1/models              -> {"data":[{"id":"Qwen-Image-2.1"}]}
    POST /v1/images/generations  -> {"data":[{"b64_json": "..."}]}
  Request fields: prompt, size ("1024x1024" -> -W -H, divisible by 32),
  n (1), num_inference_steps (default 40 -> --steps), seed (optional ->
  --seed), cfg_scale (default 6.0). Write sd-cli output to a temp path
  under out/, read it, base64 it into the response, delete the temp. Map
  badly-formed size / n>1 to an OpenAI-style error body + non-2xx status
  (or support n via --batch-count). Use the measured invocation verbatim
  as the spawn arguments.
- Phase 3, verify end-to-end. With the bridge listening on 127.0.0.1:8787,
  curl POST /v1/images/generations {"prompt":...,"size":"1024x1024"} and
  confirm exit 0, a b64_json that decodes to a valid PNG, and the writer
  file exists; curl GET /v1/models. Measure a request wall time. Run the
  QA gate with a user-chosen qa-model before the commit.
- Verify, package, handoff: run acceptance checks; osv-mcp scan the new
  crate; commit on feature/bridge with a conventional message carrying the
  measured request time and the trailer "Unit: 2 complete"; merge to dev;
  graphify AST graph rebuilt at commit time (rule 12); fold outcome here.
- Open items carried forward: cold-start vs the open-grok image-tool client
  timeout (telemeter at wiring, Unit 3); bridge port 8787 free (confirm ss
  -tlnp at wiring); remote/push still pending a visibility check.

4. Project facts a fresh session cannot re-derive

- Path map: repo at /data/imagegen (dev branch, no remote). Handoff here.
  Model files go to /data/imagegen/models/ (gitignored). Runner to
  /data/imagegen/bin/sd-cli (gitignored). Test images to /data/imagegen/out/
  (gitignored).
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
