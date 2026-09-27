QA report for imagegen Unit 12: the turbo toggle. The bridge serves two sampling
profiles from one process, starts in BASE mode (the adapter and its schedule are
no longer applied to every generation), and switches to TURBO without a restart
via POST /v1/turbo, the packaging/hooks/imagegen-turbo script, or a per-request
"turbo" flag in the generation body.

qa-model: deepseek-flash (the primary agent, by operator request)
Date: 2026-09-27. Revision tested: the working tree on feature/turbo-toggle, cut
from origin/dev 86171d7. Binary: bin/imagegen-bridge, rebuilt with the canonical
builder (bash /data/build/linux/build.sh -p /data/imagegen -b imagegen-bridge,
release). The bridge that had been left running by the previous session was
stopped first (packaging/hooks/stop-imagegen-bridge --force) and the rebuilt
binary restarted through packaging/hooks/ensure-imagegen-bridge.

INDEPENDENCE DISCLOSURE, read this first
This pass was performed by the same agent that wrote the code, at the operator's
request, so it is self-verification and strictly weaker than an independent QA
model's pass. Every claim below was exercised live - the real bridge, the real
backend, real generations through the OpenAI endpoint - and the output is quoted
verbatim. Nothing below is claimed from reading the code alone. The explicit
"NOT verified" list at the end is part of the evidence.

Surfaces covered, each with the command and what was observed.

ENDPOINT /v1/turbo (new: GET reports the live mode, POST switches it)
  Unit tests: cargo test -p imagegen-bridge
  observed: 9 passed; 0 failed, including the four new ones -
  the_base_profile_ignores_the_adapter_and_its_schedule,
  the_turbo_profile_carries_the_adapter_and_takes_steps_from_the_schedule,
  the_toggle_body_accepts_both_forms_and_rejects_anything_else,
  a_generation_body_may_pin_its_own_mode.

  Live, GET on the restarted bridge:
    {"cfg_scale":6.0,"lora":"(none)","lora_visible":true,"mode":"base",
     "sigmas":"(scheduler)","steps":28,"turbo":false,"turbo_available":true}

  POST {"enabled":true} -> 200, mode turbo, steps 6, cfg_scale 1.0, lora and
  sigmas populated; POST {"enabled":false} -> 200, mode base again. Both quoted
  in full in the sections below.

  Body validation, all on the live bridge, all HTTP 400 with an OpenAI-style
  error object:
    {}                -> "expected \"enabled\" or \"mode\" in the request body"
    {"mode":"fast"}   -> "mode 'fast' is neither turbo nor base"
    "not json"        -> "invalid request body: expected ident at line 1 column 2"
    (empty body)      -> "expected a JSON body: {\"enabled\": true} or
                          {\"mode\": \"turbo\"|\"base\"}"
  After all four, GET /v1/turbo still reported mode base: a rejected body does
  not move the mode.

CONFIG KEY turbo (new key, default false) and CONFIG KEY turbo_cfg_scale (1.0)
  The restarted bridge's own startup line proves the file default is base mode
  and that turbo is still available:
    effective configuration: ... mode=base turbo_available=true steps=28
    cfg_scale=6 lora=Qwen-Image-2.1-viggle-turbo-v0.2.1-6step-lora-r128.safetensors
    lora_multiplier=1 sigmas=1,0.9375,0.875,0.75,0.5,0.25,0 turbo_cfg_scale=1 ...

  A bridge started with IMAGEGEN_TURBO=true (fake backend so no second sd-server
  is spawned: IMAGEGEN_SD_SERVER=/bin/false, port 8797) reported
    {"mode":"turbo","steps":6,"cfg_scale":1.0,"lora":"...r128.safetensors",...}
  and POST {"enabled":false} answered 200 with {"mode":"base"}: the configured
  startup mode is honored and is itself switchable.

  Startup validation, three runs, each exits 2 with its reason and no backend
  spawned:
    IMAGEGEN_TURBO=true IMAGEGEN_LORA=          -> "IMAGEGEN_TURBO is set but no
      adapter is configured: set IMAGEGEN_LORA to the Viggle turbo adapter ..."
    IMAGEGEN_TURBO=true IMAGEGEN_SIGMAS=none    -> "IMAGEGEN_TURBO is set but no
      sigma schedule is configured: the turbo adapter ships with its own schedule,
      and the base scheduler is not what it was distilled for"
    IMAGEGEN_TURBO=true IMAGEGEN_LORA=nope...   -> "IMAGEGEN_TURBO is set but
      there is no adapter at models/loras/nope.safetensors: refusing to start
      rather than generate without the configured LoRA"

CONFIG KEY steps and CONFIG KEY cfg_scale (defaults flipped back to the base pair)
  The live base run's bridge log line and the request timing prove the defaults:
    generating 1 image(s) at 1024x1024 mode=base steps=28 cfg=6 seed=4242
    lora=(none) sigmas=(scheduler)
  observed: HTTP 200 in 169.5 s on a cold bridge (weights staged by this
  request), sampling 137.07 s in the backend; peak VRAM 7272 MiB (1 s poll).
  Artifact: qa-evidence/qa-base-1024-seed4242.png, sha256
  4ef0f68575a593aae156866e66105f588525f9a6c3cf1945d9fec2a39888bcbb.

  The turbo run proves the turbo pair travels with the mode instead:
    generating 1 image(s) at 1024x1024 mode=turbo steps=6 cfg=1 seed=4242
    lora=Qwen-Image-2.1-viggle-turbo-v0.2.1-6step-lora-r128.safetensors
    sigmas=1,0.9375,0.875,0.75,0.5,0.25,0
  observed: HTTP 200 in 26.6 s warm (18.39 s sampling), peak VRAM 7978 MiB
  (adapter tables cost about 700 MiB against the base run).
  Artifact: qa-evidence/qa-turbo-1024-seed4242.png, sha256
  1d73a9effcb1cbe9cb5916d868f14a3351b1cd32073e73dd74a84a6bcd4b05d3.

  The backend log for the turbo request shows the adapter being applied at
  runtime, and the base requests show no such lines at all:
    [INFO] diffusion_engine.cpp:1673 - apply lora at runtime
    [INFO] diffusion_engine.cpp:1786 - apply_loras completed, taking 0.03s
    [INFO] lora.hpp:982 - (454 / 454) LoRA tensors have been applied, ...
  and for the base request right after it: "get_sigmas with Flux scheduler", no
  lora load or apply lines.

ADAPTER-LEAK TEST (the one thing a mode switch could get silently wrong)
  Same prompt, same seed, 1024 square: base run BEFORE any turbo request, then a
  turbo request, then base run AGAIN with the live mode switched back.
  observed: the two base images are byte-identical -
    4ef0f68575a593aae156866e66105f588525f9a6c3cf1945d9fec2a39888bcbb  base run 1
    4ef0f68575a593aae156866e66105f588525f9a6c3cf1945d9fec2a39888bcbb  base run 2
  So the runtime adapter does not leak into later base requests on this sd-server
  build, and the base path is deterministic here (the comparison is meaningful).
  Base run 2 took 135.0 s warm; peak VRAM 7328 MiB.

REQUEST-LEVEL "turbo" FLAG (not a gate surface, but part of the unit)
  With the live mode at base, a single request carrying "turbo": true ran turbo
  and did not move the mode:
    generating 1 image(s) at 512x512 mode=turbo steps=6 cfg=1 seed=7 lora=...
  observed: HTTP 200 in 5.7 s; GET /v1/turbo afterwards still {"mode":"base"}.
  Artifact: qa-evidence/qa-turbo-perrequest-512-seed7.png.

TURBO-UNAVAILABLE PATH (second instance on port 8797, no adapter configured:
IMAGEGEN_LORA="" IMAGEGEN_SIGMAS=none, IMAGEGEN_SD_SERVER=/bin/false so no
second GPU process is started)
  GET /v1/turbo -> {"mode":"base","turbo":false,"turbo_available":false,...}
  POST {"enabled":true} -> HTTP 409 "turbo cannot be enabled: the adapter and its
  sigma schedule are not both configured (see /health turbo_available, and
  docs/viggle-turbo-lora.md)"
  POST a generation with "turbo":true -> HTTP 409 "turbo was requested but is not
  available: the adapter and its sigma schedule are not both configured ..."
  observed: the mode stayed base in both cases.

HOOK SCRIPT packaging/hooks/imagegen-turbo (new, symlinked at
~/.opengrok/hooks/bin/imagegen-turbo)
  bash -n: syntax ok.
  Live, against the running bridge:
    imagegen-turbo status -> mode base JSON, rc=0
    imagegen-turbo on     -> mode turbo JSON, rc=0
    imagegen-turbo off    -> mode base JSON, rc=0
    imagegen-turbo sideways -> "usage: imagegen-turbo [on|off|status]", rc=2
  Log lines land in ~/.local/state/imagegen-turbo.log with the verb and outcome.

IMAGE SANITY (independent vision route, not the QA agent's own eyes)
  Both 1024 artifacts were described through the agy/Gemini vision route
  (rustcv-mcp describe_image): each is "a coherent photo-like image depicting a
  red fox sitting upright in deep snow ... snow-covered forest", not noise and
  not a corrupted file. The two runs differ in composition, as a 28-step base
  generation and a 6-step turbo generation of the same seed should.

NOT verified live, and why
- The 409 branch "the backend did not confirm the adapter" (adapter on disk but
  not listed by the backend): reaching it needs a real sd-server whose LoRA cache
  lacks the adapter, and the machine rule allows one sd-server at a time, so no
  second backend was started. The branch calls the same prime_lora_cache that the
  request path exercises; its behavior was recorded as a code path, not a live
  result.
- The image-edit route: the bridge has no edit route yet (Unit 10 has not
  merged), so nothing in this unit touches it.
- Mode persistence: the mode is process state by design; no restart-survival test
  was run beyond the startup-mode test above, which covers the configured default.
- Quality scoring of base vs turbo images: this project still has no defined
  quality standard; the artifacts above are for the operator's own judgment.

verdict: overall PASS
