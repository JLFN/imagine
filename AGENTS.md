# imagegen — agent instructions for work inside this repo

(Plain-text project instructions; the global AGENTS.md is the full rule set and always wins.)

## Read first

1. imagegen-handoff.md — the hook-managed handoff and executable plan. This is the source of truth for what to do next. Follow its section 1 session protocol: at every completed unit, read the session's signals.json and continue while below 40% context usage.
2. Project memory: ~/.opengrok/memory/imagegen-e61a3c5c/MEMORY.md
3. README.md and docs/architecture.md for verified constraints (re-verify anything you rely on: source-of-truth rule).

## Repo layout

- handoff at imagegen-handoff.md; docs commits land directly on dev (rule 23 exception).
- models/MANIFEST.md is the authoritative model registry. The model files themselves are gitignored and never committed.
- crates/bridge is the Rust bridge (not yet written). The final delivered repo contains zero Python source (rule 14).
- bin/sd-cli (runner binary), models/, out/, target/, graphify-rs-out/ are gitignored.

## Guardrails that always apply here

- No emojis and no markdown formatting in any output or file (rules 1-2).
- Conventional commits, ISO body (rule 3).
- Facts must trace to a checked source; never guess (rules 0 and 15).
- Rust deliverables build with the canonical builder /data/build (rule 5) and get an osv-mcp dependency scan before any push (rule 10).
- QA gate before a unit is declared done (rule 19): qa-model=<slug> is a parameter the user selects; ask if none is recorded.
- graphify-rs AST-only graph built at commit time (rule 12): graphify-rs build --path /data/imagegen --output graphify-rs-out --no-llm. Output gitignored.
- Back up ~/.opengrok after the config.toml wiring change (rule 7): run /home/leandro/.local/bin/backup-grok-restic at commit time; never commit config.toml.
- Branch model (rule 23): dev is the working branch; new code units go on feature/ branches off dev; main is production via PR only.

## Never

- Never commit model files, sd-cli binaries, or generated images.
- Never point image_gen at an endpoint with the session bearer; the configured key only.