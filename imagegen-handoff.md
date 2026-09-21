---
project: imagegen
plan_start_commit: 7443adfcd22374ddaa3e2139287e83a78d431367
last_updated_commit: 7443adfcd22374ddaa3e2139287e83a78d431367
branch: dev
remote: none
handoff_written_at_context_usage: 0%
handoff_written_at: 2026-09-21T20:15:00Z
session_id: 01a0c4e0-21e1-7312-ac5d-48e842962cdf
---

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

2. Current state (2026-09-21 — COMPLETE, verified)

- Repo scaffolded and committed on dev at 7443adf (README, AGENTS.md,
  .gitignore, models/MANIFEST.md, docs/architecture.md). No remote, no code.
- Verified this session (source reads + live probes): ds4 cannot generate
  images (ds4_server.c: "DS4 doesn't register these tools" for
  image_generation_call); llama.cpp "diffusion" is text-diffusion only
  (LLaDA/Dream); the runner is stable-diffusion.cpp (official repo: day-0
  Qwen-Image-2.1, 2026-09-20).
- The encoder is fixed: Qwen3-VL-8B-Instruct. Reusing the Spark's Qwen3.8
  Flash Next as encoder is impossible (embedding-space mismatch; the runner
  takes local weights, and Qwen3.8 talks HTTP chat only). Use the official
  Q4_K_M GGUF, 5.03 GB.
- Structural decision made with the operator: run on THIS workstation, not
  the Spark. Specs verified: i7-14700F 28 threads, 31 GB RAM (~15 GB free),
  RTX 4070 SUPER 12 GB VRAM, 411 GB disk free. Model stack: Q4_K_M diff
  4.6 GiB (VRAM) + Q4_K_M encoder 5.03 GB (RAM, --offload-to-cpu) + VAE
  0.64 GiB; total ~10.3 GiB on disk, fits. Native 2K is tight; ~1024-1328
  is the default output.
- Wiring verified from open-grok source: [toolset.image_gen] activates only
  with non-empty base_url plus a resolvable api_key (or env_key);
  response_encoding "auto" accepts b64_json; never the session bearer.
- On-demand option verified: systemd user session is running; the bridge
  daemon (spawn sd-cli on demand, reap after idle) is the preferred design.

3. Next unit plan — Unit 1: runner binary, model files, first real image

Status: not yet started. Goal: a verified sd-cli that produces a real
Qwen-Image-2.1 image on this workstation from the three downloaded model
files. Ground truth: models/MANIFEST.md (pinned URLs, sizes, checksums) and
docs/architecture.md (invocation from the sd.cpp doc), plus the live
workstation (I have a shell here; the Spark is irrelevant for this unit).

- Phase 1, runner. Check stable-diffusion.cpp releases for a prebuilt Linux
  x86_64 CUDA binary first: https://github.com/leejet/stable-diffusion.cpp/
  releases. If a suitable prebuilt exists, download to bin/sd-cli (chmod
  +x). Else build: check nvcc presence (which nvcc, nvcc --version), then
  cmake build per the project's docs/build.md with CUDA enabled. Acceptance:
  bin/sd-cli runs and prints its version/help; --list-methods or the model
  loader accepts a QwenImage-2.1 architecture file (prove by Phase 3).
- Phase 2, model download. mkdir -p models; download the three files with
  the curl commands in models/MANIFEST.md into models/. Verify sizes with
  ls -l against the manifest, then verify SHA256SUMS against the source
  repo's published checksums; a mismatch means stop and re-download.
  Acceptance: sha256sum of each file matches the published checksum; total
  on disk about 10.3 GiB.
- Phase 3, CLI smoke (first real image). Run the text-to-image invocation
  from docs/architecture.md with the Q4_K_M files, a trivial prompt, 1024
  square output (divisible by 32), --cfg-scale 6.0, euler, -o out/first.png,
  --offload-to-cpu. Acceptance: exit 0, out/first.png exists and is a valid
  PNG of meaningful size (file out/first.png), and the image is visually
  coherent (view it per rule 21/agy path). Record: generation wall time,
  peak VRAM/RAM if measurable, and the command used, in the commit body.
- Verify, package, handoff: run the Phase acceptance checks; commit the
  three phases together on dev with a feat(recipe) commit whose body carries
  the measured numbers and the trailer "Unit: 1 complete". Do NOT commit
  model files, the sd-cli binary, or out/ (gitignored).
- Notes/decisions: the mirror choice (abenzerps vs KasugaiSakura) is a
  hash-verified decision, not a trust decision (SHA256SUMS); the
  KasugaiSakura repo is a fresh re-upload of the same bytes. Port for the
  bridge later: 8787 in the architecture doc. No Python anywhere in the
  deliverable (rule 14); the bridge is Rust (Unit 2).

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
  1. Decide the model mirror at download time: abenzerps/Qwen-Image-2.1-GGUF
     (recommended, 33k downloads) vs the operator's KasugaiSakura link; the
     checksums decide, not the name.
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