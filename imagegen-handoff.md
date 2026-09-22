---
project: imagegen
plan_start_commit: 7443adfcd22374ddaa3e2139287e83a78d431367
last_updated_commit: e7c478f
branch: dev
remote: origin (PRIVATE, github.com/JLFN/imagegen) - created 2026-09-22, dev pushed
handoff_written_at_context_usage: ~35% (signals.json, session 01a0c84e)
handoff_written_at: 2026-09-22T10:20:00Z
session_id: 01a0c84e-dd01-7cc1-a95c-b8c755cc7353
---

Handoff + executable plan — imagegen (2026-09-22)

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

This project is a sequence of units. Units 1-6 are complete on dev. Units 7 and 8
are complete but in flight as reviewed PRs (see section 2).

On completing a unit: do not stop at the boundary. Read the session's
signals.json (sessions/<percent-encoded-cwd>/<session-id>/ under
~/.opengrok; cwd is %2Fdata%2Fimagegen, pick the newest session dir):
- If contextWindowUsage < 40: write the next unit's plan into section 3,
  execute it, verify, commit, push, re-check, repeat.
- At ~40%: do not pause or ask. Finish this handoff's update, launch a fresh
  session with the open-grok-handoff launcher, keep working.
The final commit of each unit carries the trailer "Unit: <N> complete" as
its last body line (git log --grep "Unit: .* complete").

BRANCH MODEL (rule 23, now that a remote exists): dev is the integration
branch and the local working line. Every unit is a short-lived work branch off
the dev tip, pushed, and merged through a REVIEWED PR on GitHub; only handoff
and QA-evidence commits go directly on dev. main does not exist yet (section 3).

PARALLEL WORK: a sibling repo /data/imagegen-rs contains a pure-Rust port of the
whole generation stack (candle: tokenizer, Qwen3-VL text tower, 32-block DiT, Wan
VAE, sampler, CLI, OpenAI server), replacing stable-diffusion.cpp. Read
/data/imagegen-rs/imagegen-rs-handoff.md and its docs/parity.md before changing
anything that affects the model stack.

2. Current state (2026-09-22 — Units 7 and 8 done, open PRs awaiting review)

- dev is at e7c478f and pushed to the private remote JLFN/imagegen. Unit 7 and
  Unit 8 are NOT on dev yet; they are open PRs, which is the point of the review
  flow the operator chose.
- PR 1, branch feature/unit7-resolution-and-port (2 commits): 1344 square measured
  (does NOT fit the pinned layout; --offload-to-cpu serves it at 502.54 s and
  16.1 GiB), the tiled-versus-untiled VAE decode isolated at 1024 square (mean
  0.2847 of 255, deviations at the seams), the rule 14 Python verifier ported to
  crates/convrot-verify and deleted, the bridge request ceiling added as max_edge
  (default 1152, the measured value; it used to advertise 1536 while the layout
  could not serve above 1152), and the project's first test suite (tests/run.sh
  plus the rule 19 mount tests/qa-gate.sh).
- PR 2, branch feature/on-demand-lifecycle (STACKED on PR 1, 1 commit):
  hook-driven lifecycle. PreToolUse on the image tools runs
  packaging/hooks/ensure-imagegen-bridge (health probe, systemd start, detached
  fallback, fail-open, logged); SessionEnd runs packaging/hooks/stop-imagegen-bridge
  (refuses while another session is alive or in_flight > 0, --force overrides);
  the bridge stops itself after idle_timeout_secs (900 default) and reports
  in_flight / idle_secs / idle_timeout_secs in /health; it now handles SIGTERM so a
  systemd stop reaps sd-server; packaging/imagegen-bridge.service is the unit
  (installed, not enabled).
- Verified live today: 1152 works pinned (peak 10,060 MiB), 1344 fails pinned for
  both encoders, offload 1344 works; tiled decode measured against untiled; the
  Python port reproduces the Python baseline to every printed digit; a fresh
  headless session started the bridge through the hook and got a 1024x1024 PNG in
  175.28 s; the idle timer exited at 48 s with a 20 s timeout, leaving no backend.
- Machine paths for the lifecycle: hook registration ~/.opengrok/hooks/imagegen-bridge.json;
  scripts symlinked from ~/.opengrok/hooks/bin into /data/imagegen/packaging/hooks/;
  unit ~/.config/systemd/user/imagegen-bridge.service; lifecycle log
  ~/.local/state/ensure-imagegen-bridge.log; bridge log /data/build-sd/bridge-split.log.
- The bridge is currently unit-managed and up (this session restarted it through the
  ensure hook, so it is the systemd instance, not a detached one).

3. Next unit plan — Unit 9: land the two PRs, then close the gaps they exposed

Status: two PRs are open and green (QA gate passed on both branches, osv clean at
155 packages, 0 vulnerable). Nothing new may start until PR 1 merges: rule 23
allows one in-flight unit at a time, and PR 2 is stacked on PR 1.

- Phase 1, land the stack (operator action, then mine). Review and merge PR 1 into
  dev; then retarget PR 2's base to dev (gh pr edit 2 --base dev), refresh it with
  dev (git fetch && git checkout feature/on-demand-lifecycle && git merge origin/dev),
  confirm its diff is now only Unit 8, self-review the QA gate output, and merge.
  Acceptance: dev contains Units 7 and 8; git log --oneline origin/dev shows the
  two unit commits; both branches deleted after merge (git branch -d and
  git push origin --delete).

- Phase 2, main and protection (rule 23). Create main at the post-merge dev state
  (or at the last release point the operator prefers), push it, protect dev and
  main on GitHub (require PR review, block force-push), and record the promotion
  flow in this handoff. Acceptance: gh api shows the protection rules; a direct
  push to dev/main is refused.

- Phase 3, register or correct the localembed ensure hook. Found 2026-09-22:
  ~/.opengrok/memory/MEMORY.md and global rule 18 both describe
  hooks/bin/ensure-localembed as a SessionStart auto-start hook, but NO hooks json
  registers it (the three/four registered files are callgraph, handoff,
  imagegen-bridge, skill-allowlist), and it is absent from a sweep of the project
  hook dirs. Either register it (mirroring imagegen-bridge.json) or correct the
  memory and rule text. Acceptance: the docs and the machine agree, verified by
  listing the registered hooks and by a probe session.

- Phase 4, the docs inaccuracies the review may not catch. crates/bridge/README.md's
  configuration table lists an IMAGEGEN_DIFFUSION default of Q4_K_M, its own
  following paragraph says Q6_K, and the binary has no model default at all (a
  missing path is a startup error, exit 2, verified live in Unit 6). Fix the table
  to say the value comes from imagegen.toml. Acceptance: the README makes one
  claim about the default and it is the true one.

- Phase 5, upstream the sd.cpp patches (still open from Unit 7). patches/0001
  fixes a genuine sd.cpp defect (a safetensors text encoder carrying a weight_scale
  tensor gets its geometry misread) and 0002 is the int8 embedding
  dequantization. Both need re-applying on every upstream update otherwise. This
  is an EXTERNAL, public action: get the operator's go-ahead before opening a PR
  against leejet/stable-diffusion.cpp.

- Phase 6, optional quality work (operator asked about image quality 2026-09-22).
  No quality standard exists in this project: every quality statement is relative
  (precision tiers measured a tie, step count measured dominant). Options: name
  qa-evidence/qa-gen-1024-28steps-seed42.png as the reference artifact and define
  what acceptable means; or measure 1024/28 against 1152/28 at the same seed and
  prompt. Decide with the operator before building anything.

- Phase 7, wrap. Fold Units 7 and 8 into section 2, replace section 3, refresh the
  memory entry (imagegen-e61a3c5c), and close with "Unit: 9 complete".

4. Project facts a fresh session cannot re-derive

- Path map: repo at /data/imagegen (dev branch, remote origin = private
  JLFN/imagegen). Handoff here. Model files go to /data/imagegen/models/
  (gitignored). Binaries to /data/imagegen/bin/ (gitignored). Test images to
  /data/imagegen/out/ (gitignored). Deployment files to /data/imagegen/packaging/.
- sd.cpp sources and measurements live OUTSIDE the repo, at /data/build-sd:
  stable-diffusion.cpp is the source tree matching the installed sd-cli and
  sd-server (upstream master 6dcb5bb, built with SD_CUDA=ON, SD_USE_UPSTREAM_GGML
  OFF, i.e. the patched ggml that accepts int8 convrot files). Its docs/
  (qwen_image_2.1.md, backend.md, int8_convrot.md) are the authority for flag
  semantics. Measurement logs and harnesses: /data/build-sd/te-split (prod/, q6/,
  refined/, unit7/ with the Unit 7 measurement set and logs, measure-unit7.sh, and
  e2e-hook-test.sh for the Unit 8 hook test). The bridge's own log is
  /data/build-sd/bridge-split.log.
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
- Machine facts: x86-64, Intel i7-14700F, 31 GB RAM, RTX 4070 SUPER 12 GB, ~400 GB
  free on /. A secret-free local API key string is fine for the bridge (endpoint is
  127.0.0.1). The NVIDIA Spark (192.168.1.91) is NOT involved in image generation.
- Stray model downloads: an hf download run once dropped an identical 9.35 GB copy
  of the int8 convrot encoder at /data/imagegen/text_encoders/. It is gitignored
  and was left for the operator to remove; check before deleting anything.
- open-grok's image tool: ImageGenClient in
  /data/open-grok/crates/codegen/xai-grok-tools/src/implementations/grok_build/image_gen/mod.rs,
  a pure HTTP client (no process spawning) with a 300 s total and 240 s read
  timeout. That is why the lifecycle is hook-driven rather than tool-driven.

5. Guardrails and open items

- Global (read the global AGENTS.md): no emojis; plain text; conventional
  commits; source-of-truth verification; session handoff protocol; rule 23
  branch model (dev is the integration branch; units on work branches; PRs).
- Project (this repo's AGENTS.md): zero Python in the delivered repo; model
  files never committed; graphify-rs AST graph built at commit time; the qa
  model is a user-chosen parameter (the operator has asked for self-verification
  by the primary agent for Units 6-8; an independent model was used earlier).
- OPEN items:
  1. PR 1 then PR 2 await review and merge (section 3 phase 1).
  2. main does not exist and neither dev nor main is protected on GitHub yet.
  3. The ensure-localembed hook described in the docs is not registered anywhere.
  4. patches/0001 and 0002 are not upstreamed (needs an operator go-ahead).
  5. No quality standard is defined anywhere in the project (section 3 phase 6).
  6. SessionEnd does not fire in headless open-grok runs (measured), so the stop
     hook covers interactive sessions and the idle timer covers headless ones.

6. Progress and staleness check (run this first, every session)

Progress meter:
    cd /data/imagegen
    git log --oneline 7443adfcd22374ddaa3e2139287e83a78d431367..HEAD
    git diff --stat 7443adfcd22374ddaa3e2139287e83a78d431367..HEAD
    git log --grep "Unit: .* complete" 7443adfcd22374ddaa3e2139287e83a78d431367..HEAD

Staleness check:
    cd /data/imagegen && git log --oneline e7c478f..HEAD
- Any output means this handoff predates newer work: read the log and diffs
  before trusting sections 2-5, then advance last_updated_commit.
- Branch or remote differing from the frontmatter: stop and reconcile.
- ALSO check for open work branches, which the marker cannot see:
    git branch -a && gh pr list --state open

This document is COMPLETE and self-contained: a new session can execute the
project from this file alone, with the model manifest and architecture doc
for the pinned facts and the global rules for the guardrails.
