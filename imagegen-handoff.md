---
project: imagegen
plan_start_commit: 7443adfcd22374ddaa3e2139287e83a78d431367
last_updated_commit: 92c7e02
branch: dev
remote: none
handoff_written_at_context_usage: 59% (estimated: chat_history bytes / 4; this session had no signals.json)
handoff_written_at: 2026-09-22T05:36:00Z
session_id: 01a0c775-65e6-7e73-8df2-6220db99459f
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
 Cargo.lock                                         | 1228 ++++++++++++++++++++
 Cargo.toml                                         |    7 +
 crates/bridge/Cargo.toml                           |   16 +
 crates/bridge/README.md                            |   87 ++
 crates/bridge/src/main.rs                          |  777 +++++++++++++
 docs/architecture.md                               |  173 ++-
 docs/convrot-encoder-experiment.md                 |  234 ++++
 imagegen-handoff.md                                |  229 ++++
 imagegen.toml                                      |   57 +
 models/MANIFEST.md                                 |  115 ++
 ...01-sd-cpp-llm-te-config-skip-weight-scale.patch |   53 +
 ...-cpp-dequantize-int8-tensorwise-embedding.patch |  172 +++
 qa-evidence/qa-gen-1024-28steps-seed42.png         |  Bin 0 -> 2463614 bytes
 qa-evidence/qa-gen-512-seed11.png                  |  Bin 0 -> 286535 bytes
 qa-evidence/qa-gen-512-seed3.png                   |  Bin 0 -> 280816 bytes
 qa-evidence/qa-report.md                           |  218 ++++
 tools/convrot-te-experiment.sh                     |   72 ++
 tools/verify-convrot-dequant.py                    |  105 ++
 19 files changed, 3542 insertions(+), 14 deletions(-)
--- staleness (last_updated_commit 92c7e02 .. HEAD):
a3c2a97 docs(handoff): record Unit 6, its QA rounds and the Unit 7 plan
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

2. Current state (2026-09-22 — Unit 6 complete on its branch, not yet merged)

- Unit 6 (e0e9030 on feat/supervision-and-layout): the bridge is configured from
  imagegen.toml — no model path is compiled in, and a missing path is a startup
  error (exit 2); it supervises its sd-server backend, reaping it and restarting
  it with backoff, and /health reports backend_process_alive and
  backend_restarts; the model files follow the upstream card's layout
  (models/diffusion_models/, models/text_encoders/, models/vae/); and the
  deployed text encoder is the int8 convrot file, which needs the patched
  sd-server now installed as bin/sd-server (rollback: bin/sd-server.stock,
  bin/sd-cli.stock).
- Proven live: 1024x1024/28 steps with the convrot encoder returned a PNG in
  158.6 s (open-grok read limit 240 s); kill -9 on the sd-server child produced a
  restart with backend_restarts 0 to 1, and the next request returned a PNG in
  26.9 s.
- patches/0002 now reads the int8 embedding's row scales through mmap as well as
  a plain file. sd-server mmaps its weights and sd-cli does not, so the first
  version worked under sd-cli and aborted the server with "type i8 unsupported
  for integer quantization" on a real request. A CLI run is not proof that a
  deployed service works.
- Known costs, unchanged by the decision: the convrot encoder is about 5.8 GB
  more host RAM than the GGUF encoder for the same VRAM peak, image quality a
  measured tie, conditioning somewhat slower; the VAE decode still tiles at 1024
  square (+2.5 to 3 s); 1344 square is unverified.
- Open: rule 14 requires tools/verify-convrot-dequant.py (Python) to be ported to
  Rust or removed before delivery.
- Unit 6 is QA-gated: qa-evidence/qa-report.md, verdict overall PASS, from three
  independent-capable passes (the first two by qwen38-flash-next-ds4, the third
  self-verified by the primary agent deepseek-flash at operator request). The
  gates found five defects, all fixed: the manifest lacked the shipped encoder,
  a bad sd_server path panicked with exit 101, the size ceiling was bypassable
  through the aspect path, the restart counter counted attempts rather than
  healthy replacements, and a startup that waited for the backend before binding
  made /health unreachable exactly when it was needed. The evidence includes a
  production-quality 1024-square artifact that is byte-identical to the earlier
  deployment proof at the same seed.

3. Next unit plan — Unit 7: resolution headroom, delivery, and the Python port

Status: Unit 6 is complete on its branch (section 2). This unit is small and
mostly verification. Measurements need the bridge STOPPED first, because it pins
5.6 GB of weights in VRAM and a second run at 1024 square or above fights it for
the card; code phases need a feature branch off dev (rule 23).

- Phase 1, resolution headroom (measurement). 1344 square is the quality tier the
  project used before the pinned layout, and it is unverified under it:
    cd /data/imagegen && kill -INT <bridge pid> && \
    ./bin/sd-cli --diffusion-model models/diffusion_models/qwen-image-2.1-Q6_K.gguf \
      --vae models/vae/qwen_image_2.1_vae_bf16.safetensors \
      --llm models/text_encoders/Qwen3VL-8B-Instruct-Q4_K_M.gguf \
      -p "<prompt>" --cfg-scale 6.0 --sampling-method euler -v \
      --backend te=cpu,diffusion=cuda0,vae=cuda0 \
      --params-backend te=cpu,diffusion=cuda0,vae=cpu \
      -W 1344 -H 1344 --steps 40 --seed 42 -o out/res1344.png
  then restart the bridge. Acceptance: a measured number in docs/architecture.md
  (Parameter placement), with the mitigation if it fails (a --max-vram budget, or
  --offload-to-cpu above 1024).

- Phase 2, the true tiling comparison (measurement). The 768-square run showed
  the decode is insensitive to tile count (0.15 of 255 mean difference), but both
  sides tiled there. Compare 1024 square with --vae-tiling forced against an
  untiled decode of the same seed, which needs the DiT not pinned for the untiled
  side. Acceptance: a recorded mean and max pixel difference, or an explicit
  statement that it cannot be isolated on this card.

- Phase 3, rule 14 (code, feature branch). Port tools/verify-convrot-dequant.py to
  Rust — ranged HTTP fetch of the BF16 ground truth, safetensors header parse, the
  same dequantize plus rotation-undo, and the correlation check — or remove it and
  inline the method in docs/convrot-encoder-experiment.md. Acceptance: no Python
  source left in the repository.

- Phase 4, delivery and the remote decision (still open). No remote exists. If one
  is wanted: pick host and visibility, run
    gh repo view OWNER/REPO --json isPrivate,visibility   (rule 6)
  before any push, re-run the osv scan (rule 10; last clean 2026-09-22, 115
  packages), then push dev; main stays behind a reviewed PR (rule 23). If the user
  declines, record that and the repo is deliverable on dev alone.

- Phase 5, upstream the patches. Patch 0001 fixes a genuine sd.cpp defect: any
  safetensors text encoder carrying a weight_scale tensor gets its geometry
  misread. Patch 0002 is the int8 embedding dequantization. Both are on their own
  branch today and would need re-applying on every upstream update otherwise.

- Phase 6, wrap. Fold this unit into section 2, replace section 3, refresh the
  memory entry (imagegen-e61a3c5c), and close with "Unit: 7 complete".

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
- Stray model downloads: on 2026-09-22 an hf download run with the repo root as
  its working directory dropped an identical 9.35 GB copy of the int8 convrot
  encoder at /data/imagegen/text_encoders/ (plus ./.cache/huggingface). Its
  origin is unattributed - it is not this session's download (mine landed in
  models/) and not the QA subagent's. It was left in place for the operator to
  remove; .gitignore now covers *.safetensors, *.gguf, *.ckpt and .cache/, so
  no model file can be staged from anywhere in the tree.
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
