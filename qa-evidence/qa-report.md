QA report for the uncensored-model unit: the standing QUALITY model moves to the
uncensored Q6_K build, and the bridge gains an optional extra adapter layer so
the DRAFT/TURBO deployment can carry the uncensored LoRA on the base weights.

qa-model: deepseek-flash (the primary agent, by operator request)
Date: 2026-09-27. Revision tested: the working tree on feature/uncensored-model-trial,
cut from origin/dev 7b8d9e5, with the bridge rebuilt by the canonical builder
(bash /data/build/linux/build.sh -p /data/imagegen -b imagegen-bridge, release).
The live bridge was restarted from the edited imagegen.toml, so every live claim
below is about the running service.

INDEPENDENCE DISCLOSURE, read this first
This pass was performed by the same agent that made the change, at the operator's
standing request, so it is self-verification and strictly weaker than an
independent QA model's pass. Quality verdicts quoted here come from an
independent vision route (agy/Gemini via rustcv-mcp describe_image); timings,
VRAM peaks, checksums and HTTP responses were measured live in this session.

Surfaces covered, each with the command and what was observed.

CONFIG KEY diffusion (the standing model becomes the uncensored Q6_K build)
  Live after the restart, three independent checks:
    /health -> {"status":"ok","backend":true,"mode":"base","turbo_available":false,
                "lora":"(none)","lora_visible":null,"steps":28,"cfg_scale":6.0}
    ps -C sd-server -> --diffusion-model models/diffusion_models/
      qwen-image-2.1-UC-Q6_K.gguf ...
    startup log -> effective configuration: diffusion=models/diffusion_models/
      qwen-image-2.1-UC-Q6_K.gguf ... extra_lora=(none) sigmas=(none)
      placement=--backend te=cpu,diffusion=cuda0,vae=cuda0 --params-backend
      te=cpu,diffusion=cuda0,vae=cpu
  Provenance, verified against the source repository's own SHA256SUMS:
    qwen-image-2.1-UC-Q6_K.gguf    e14bb312109333b3d73b92ad9b9ac8b29b51f2edca1e7b86d1af1981bf1c4ee3
    qwen-image-2.1-UC-Q4_K_M.gguf  e79c8a009f2ecbdb6c70fd663d9aea9ee304a0d91f347e4169a756b8ad141b41
    qwen-image-2.1-uncensored-lora.safetensors
                                   00ee2cb7f00a37b097640c1880ed0d17e368e30599f34bb348fee0e8c2cc4ca1
    (the LoRA hash is the repository's LFS object id; the SHA256SUMS file does
    not list it)
  Quality A/B, 1024 square, one prompt and seed, independent critic pass:
    base Q6_K (previous standing model) 183.3 s, 9200 MiB, 9/10, "yes" to DSLR
    UC-Q6_K (new standing model)        159.8 s, 9281 MiB, 9/10, "yes" to DSLR
  Artifact: qa-evidence/qa-playground-uncensored-q6k-1024.png (the base file's
  frame stays as qa-playground-q6k-1024.png). No quality difference measured;
  the change is content behaviour, stated as such in the config and docs.

BRIDGE FEATURE: the extra adapter layer (no gate surface; unit-tested and run live)
  cargo test -p imagegen-bridge: 10 passed, 0 failed, including the new
  the_extra_layer_rides_on_both_modes_and_after_the_turbo_adapter.
  Live, one bridge with both adapters configured (base Q4_K_M + 16x16 tiles +
  Viggle + the uncensored layer):
    /v1/turbo in base mode -> lora "qwen-image-2.1-uncensored-lora.safetensors",
      lora_visible true (the cache check verifies every configured adapter)
    /v1/turbo in turbo mode -> lora "Qwen-Image-2.1-viggle-turbo-v0.2.1-6step-
      lora-r128.safetensors,qwen-image-2.1-uncensored-lora.safetensors"
    DRAFT run (base mode, layer applied): HTTP 200 in 187.5 s, peak VRAM
      7571 MiB, backend "(256 / 256) LoRA tensors have been applied ... 
      uncensored-lora"
    TURBO run (both adapters): HTTP 200 in 32.7 s, peak VRAM 8230 MiB, backend
      "(454 / 454) LoRA tensors ... viggle-turbo" AND "(256 / 256) LoRA tensors
      ... uncensored-lora" in the same request
    toggle off -> mode base again
  Artifacts: qa-evidence/qa-playground-draft-uncensored-layer-1024.png and
  qa-evidence/qa-playground-turbo-both-adapters-1024.png.
  Cost of the layer, same prompt and seed at 1024 square: 7571 MiB peak against
  7562 MiB for the merged UC-Q4_K_M route, i.e. no measurable VRAM cost; the
  tensor application itself takes 0.03 s.

Docs updated in the same change, each checked against the live state above:
  imagegen.toml (standing model, the four-edit switch to DRAFT/TURBO, the layer
  note), models/MANIFEST.md (all five files with sizes, hashes, sources and the
  role each plays), docs/sampling-profiles.md (the profiles table, the
  uncensored-layer section, the identity-check caveat), crates/bridge/README.md
  (config table and the layer paragraph), docs/architecture.md (standing
  deployment and the layer).

NOT verified live, and why
- The uncensored behaviour itself (no refusals, no sanitising): NOT exercised.
  No restricted prompt was run, so the claim in the docs is attributed to the
  source repository, not measured here. What is measured is that the layer
  applies cleanly on top of the base weights and alongside the Viggle adapter.
- Quality of the layer on TURBO: the playground frame scored 3/10, the same
  class as TURBO without the layer; no broader scene set was run.
- Resolutions above 1024 square on UC-Q6_K: not run (about 2.4 GiB of the card
  free at the 9281 MiB peak).
- The identity check in docs/sampling-profiles.md holds with extra_lora unset;
  it was not re-run with the layer configured (the docs say so).

verdict: overall PASS
