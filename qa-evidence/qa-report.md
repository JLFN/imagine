QA report for the imagegen deployment change: the standing configuration moves
to Q6_K base-only (turbo keys commented out), decided by the operator on
2026-09-27 after a measured three-way quality comparison.

qa-model: deepseek-flash (the primary agent, by operator request)
Date: 2026-09-27. Revision tested: the working tree on chore/deploy-q6k-base,
cut from origin/dev cf99ac9. No code changed in this unit: imagegen.toml, the
three docs and one message in packaging/hooks/imagegen-turbo. The live bridge was
restarted from the edited file, so every claim below is about the running
service, not about the file alone.

INDEPENDENCE DISCLOSURE, read this first
This pass was performed by the same agent that made the change, at the operator's
standing request, so it is self-verification and strictly weaker than an
independent QA model's pass. The three quality verdicts quoted here come from an
independent vision route (agy/Gemini via rustcv-mcp describe_image), not from the
QA agent's own eyes alone; the timings, VRAM peaks and HTTP responses were
measured live in this session.

Surfaces covered, each with the command and what was observed.

CONFIG KEY diffusion (Q4_K_M -> Q6_K, the deployment's core change)
  The restart was performed with the repo's own hook:
    bash packaging/hooks/stop-imagegen-bridge --force
    bash packaging/hooks/ensure-imagegen-bridge
  observed, from the new process:
    /health -> {"status":"ok","backend":true,"mode":"base",
                "turbo_available":false,"lora":"(none)","steps":28,
                "cfg_scale":6.0}
    startup log -> effective configuration: diffusion=models/diffusion_models/
      qwen-image-2.1-Q6_K.gguf ... mode=base turbo_available=false steps=28
      cfg_scale=6 lora=(none) sigmas=(none) placement=--backend
      te=cpu,diffusion=cuda0,vae=cuda0 --params-backend
      te=cpu,diffusion=cuda0,vae=cpu
    ps -C sd-server -> bin/sd-server --diffusion-model
      models/diffusion_models/qwen-image-2.1-Q6_K.gguf ... (no
      --lora-model-dir, no --vae-tiling)
  Live generation through the restarted service (the serving path, not just
  health):
    curl -X POST /v1/images/generations -d '{"prompt":"a single ripe red apple
    on a wooden table, window light","size":"512x512","seed":11}'
    observed: HTTP 200 in 64.7 s, PNG 512x512, bridge log line
    "generating 1 image(s) at 512x512 mode=base steps=28 cfg=6 seed=11
    lora=(none) sigmas=(scheduler)".

  The measured basis for the change, all at 1024 square, one prompt and seed
  ("A quiet neighbourhood playground on an ordinary weekday afternoon ..."), each
  run through a bridge on this machine and judged by the independent vision
  route:
    Q6_K, default VAE tiling:    183.3 s, peak VRAM 9200 MiB, critic realism
      9/10, sharpness 9/10, "Yes ... looks like a real DSLR photo"
    Q4_K_M, 16x16 VAE tiles:     140.9 s, peak VRAM 7272 MiB, critic realism
      6/10, sharpness 8/10, structural mush in the playground hardware
    turbo (Q4_K_M + adapter):     29.3 s, peak VRAM 7978 MiB, critic realism
      3/10, sharpness 6/10, "melted" metalwork
  Artifacts committed beside this report: qa-playground-q6k-1024.png,
  qa-playground-q4km-base-1024.png, qa-playground-turbo-1024.png.

  Related config change in the same commit: backend_args drops
  --vae-tiling --vae-tile-size 16x16 (default tiling), which is what the 9200 MiB
  peak above was measured with, and the four turbo keys are commented out.

TURBO UNAVAILABLE, BY DESIGN (reported through the surfaces the unit keeps)
  packaging/hooks/imagegen-turbo on
    observed: HTTP 409 with "turbo cannot be enabled: the adapter and its sigma
    schedule are not both configured (see /health turbo_available, and
    docs/viggle-turbo-lora.md)"; the script printed that reason and exited 1
    (its refusal message was improved in this commit to print the bridge's own
    reason instead of "mode is unknown").
  GET /v1/turbo -> {"mode":"base","turbo":false,"turbo_available":false,...}
  The turbo recipe itself is unchanged and remains one three-line edit away
  (Q4_K_M model, 16x16 tiles, four uncommented keys, restart); that path is
  documented in imagegen.toml and docs/viggle-turbo-lora.md and was QA'd in the
  Unit 12 report (qa-evidence/unit12-qa-report.md).

Docs updated in the same commit, each checked against the live state above:
  crates/bridge/README.md (config table: Q6_K default, turbo keys commented;
  the sampling-modes caveat now states the one-deployment tradeoff),
  docs/architecture.md (the standing-deployment paragraph with the measured
  numbers), docs/viggle-turbo-lora.md (a DEPLOYMENT STATE block pointing at the
  three-line switch back).

NOT verified live, and why
- Turbo ON on this deployment: impossible by construction (409 by design); the
  turbo path itself is verified in unit12-qa-report.md against the Q4_K_M
  deployment.
- Resolutions above 1024 square on Q6_K (1536 etc.): not run; the 9200 MiB peak
  at 1024 leaves about 2.4 GiB of the 12 GiB card, so larger sizes are untested
  and the bridge's own 1536 edge ceiling still applies.
- Quality on prompts other than the playground and fox scenes: the verdicts are
  scene-specific, as any critic pass is.

verdict: overall PASS
