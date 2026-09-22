# Architecture — local Qwen-Image-2.1 for open-grok

Design verified 2026-09-21. Details here are the pinned facts; anything not
in this file must be re-verified before being trusted.

## Components

1. Runner: stable-diffusion.cpp sd-cli binary (Linux, CUDA).
   - Source: github.com/leejet/stable-diffusion.cpp (day-0 Qwen-Image-2.1,
     2026-09-20). Prebuilt binaries on the releases page, or build from
     source with cmake (requires a CUDA toolkit for the CUDA backend).
   - Backends: CPU (AVX/AVX2/AVX512), CUDA, Vulkan, Metal, OpenCL, SYCL.
2. Model files (see models/MANIFEST.md): diffusion Q6_K GGUF 5.47 GiB
   (the deployed default; Q4_K_M at 4.29 GiB is also on disk), encoder
   Qwen3-VL-8B-Instruct Q4_K_M GGUF 4.68 GiB, VAE 0.63 GiB. Note: earlier
   revisions of this file and of MANIFEST.md quoted Q4_K_M as "4.6 GiB" and
   the encoder as "5.03 GB" - those are decimal GB, not GiB (4.60 GB and
   5.03 GB respectively).
3. Bridge (not yet written): a small Rust HTTP server that speaks the
   OpenAI image API and manages the sd-cli child process.
4. Wiring: [toolset.image_gen] in ~/.opengrok/config.toml.

## Runner invocation (from stable-diffusion.cpp docs/qwen_image_2.1.md)

Text-to-image (width/height divisible by 32; resolution-dependent flow
schedule is selected automatically):

  sd-cli \
    --diffusion-model models/diffusion_models/qwen-image-2.1-Q4_K_M.gguf \
    --vae models/vae/qwen_image_2.1_vae_bf16.safetensors \
    --llm models/text_encoders/Qwen3VL-8B-Instruct-Q4_K_M.gguf \
    -p "<prompt>" --cfg-scale 6.0 --sampling-method euler \
    -v --offload-to-cpu -o out/image.png

- --offload-to-cpu keeps the encoder in system RAM (recommended layout).
- Image editing (later phase, optional) adds -r <ref.png> for each reference
  image, plus --llm_vision <mmproj GGUF> when the text encoder is GGUF.

## Measured performance (Unit 1, 2026-09-21, this box)

Verified invocation: the text-to-image command above with -W 1024 -H 1024,
--steps 40, --cfg-scale 6.0, euler, --seed 42, --offload-to-cpu. GPU: RTX
4070 SUPER (compute 8.9). Runner: sd-cli built from source master-890-74988b2-
4-g6dcb5bb with -DSD_CUDA=ON (sm_89). CUDA toolkit 13.3.

- Total params 9337.96 MB, all in RAM (VRAM 0.00 MB at load): text encoder
  4302.33 MB, diffusion 4391.25 MB, VAE 644.38 MB.
- First real image (40 steps): generate_image completed in 216.61 s, of which
  sampling 199.49 s (~4.6-4.9 s/it after the first ~16.8 s warm step);
  process wall 3:38, max RSS ~10.04 GB. Output 1024x1024 RGBA, 1.87 MB.
- Reproducibility run (12 steps, seed 7): sampling 70.30 s, generate_image
  83.77 s, output 1.83 MB.
- Peak VRAM during 1024x1024 generation: 11,536 MiB of 12,282 (~94%).
  Weights are offloaded to RAM but the compute/attention buffers saturate the
  card at 1024 sq; higher resolutions or a larger batch would OOM (confirms
  "native 2K is tight"). Cold start (loading ~9.3 GB weights) is the dominant
  cost for an idle process; a warm process is ~4.6 s/it.
- Model load ~12.7 s (encoder), ~10.7 s (diffusion), ~1.2 s (VAE);
  condition forward ~13.9 s.

## Parameter placement (measured 2026-09-22)

sd.cpp's `--backend` selects the module that executes a module's graphs and
`--params-backend` selects where its parameters live (module names: `te`,
`diffusion`, `vae`). Measured on this box at 1024x1024 / 28 steps / seed 42,
same prompt, peak VRAM from a 1 s nvidia-smi poll, host RSS from
/usr/bin/time -v. Times are sd-cli `generate_image` (includes the load).

  layout                          sampling   VAE decode   total    peak VRAM   host RSS
  --offload-to-cpu, Q4_K_M        131.5 s    2.1 s        135.2 s  11,572 MiB  10.6 GiB
  DiT pinned, encoder CPU, Q4_K_M 132.9 s    4.4 s tiled  139.4 s   8,172 MiB   6.2 GiB
  all pinned, encoder on GPU      -          -            failed       -           -
  Q6_K pinned, encoder CPU, VAE
    parameters in RAM (deployed)  146.5 s    4.7 s tiled  153.9 s   9,426 MiB      -

What the measurements establish:

- Pinning the diffusion model costs nothing in sampling time at this
  resolution (131.5 s to 132.9 s). The staging it removes is hidden by
  prefetch once a run is long enough; at 4 steps the same change took a
  512-square run from 25.2 s to 10.7 s.
- The text encoder must not be pinned: with all three modules in VRAM the
  diffusion model fails during weight preparation at block 28 of 34 and the
  run dies. Its parameters in RAM with the graph on the CPU costs 0.5-1.1 s
  more conditioning (1.38-2.62 s, once per prompt).
- The VAE decode falls back to spatial tiling (9 tiles, ~2.4 s per tile) at
  1024 square when the DiT is pinned: 4.4-4.7 s instead of 2.1 s. This is
  resolution dependent - at 512 square the same layout decoded untiled in
  1.43 s with no tile passes - and it is the pinned DiT weights starving the
  decode, not the VAE's own parameters: moving those to RAM does not avoid
  the fallback. Budget ~2.5 s per 1024-square request for it, and treat the
  tiled output as visually unverified against the untiled decode.
- The bridge passes `--backend te=cpu,diffusion=cuda0,vae=cuda0
  --params-backend te=cpu,diffusion=cuda0,vae=cpu` (overridable with
  IMAGEGEN_BACKEND_ARGS). Through the bridge, a first 1024-square/28-step
  request returned HTTP 200 with a 1024x1024 PNG in 143.6 s, inside
  open-grok's 240 s read timeout.
- 1344 square does NOT fit under the pinned layout: the sampling graph fails
  during weight preparation, short by about 0.2 MB of its budget. 1152 square is
  the largest verified tier. The tiled decode is now quantified against the
  untiled one. See Resolution headroom and the VAE decode below.

Layout is not bit-reproducible: the same seed and prompt under two layouts
differed on 90% of channel bytes (mean absolute difference 6.07 of 255).
Structurally identical images, but do not expect byte equality across
placement changes.

## Resolution headroom and the VAE decode (measured 2026-09-22)

Two questions were open: does the pinned layout reach the 1344-square tier at
all, and is the tiled VAE decode it falls back to at 1024 indistinguishable from
the untiled decode. Both are now measured with sd-cli (the patched build),
prompt "a lovely cat holding a sign says 'qwen2.1.cpp'", seed 42, euler, cfg 6.0,
peak VRAM from a 1 s nvidia-smi poll and host RSS from /usr/bin/time -v - the same
method as the placement table above.

Resolution headroom, 1344x1344 / 40 steps, one variable per row:

  layout                          encoder  result  sampling  decode      total     peak VRAM  peak RSS
  1152 sq, pinned (1152x1152)     GGUF     ok      269.29 s  5.13 tiled  275.88 s  10,060 MiB  6.4 GiB
  1344 sq, pinned                 convrot  FAILED  -         -           exit 1    11,280 MiB  9.7 GiB
  1344 sq, pinned                 GGUF     FAILED  -         -           exit 1    10,756 MiB  5.1 GiB
  1344 sq, pinned, max-vram=11    GGUF     FAILED  -         -           exit 1    11,068 MiB  5.1 GiB
  1344 sq, --offload-to-cpu       convrot  ok      470.59 s  8.16 tiled  502.54 s  11,276 MiB  16.1 GiB

What that establishes:

- The pinned layout's ceiling sits between 1152 and 1344 square. The failure is
  not a slow path or a partial image: it happens during weight preparation of the
  sampling graph, before the first step runs, so the step count is irrelevant by
  construction (a 12-step attempt fails the same way).
- The failure is razor-thin and encoder-independent. Both encoders fail with the
  identical allocation demand (need 622.61 MB against 622.44 MB available, a
  0.17 MB shortfall), because the encoder's parameters live in RAM either way.
  Do not read the margin as "almost fits": the check leaves a 512 MiB scratch
  reserve, so the true deficit is larger than the printed one.
- An explicit --max-vram budget does not rescue it. --max-vram shapes the planner
  and auto-fit budgets, and an explicit --params-backend disables auto-fit, so
  the runner's segment-capacity check fails the same way.
- The documented fallback for 1344 square is the layout the upstream card itself
  uses: --offload-to-cpu, all 16.4 GB of parameters in RAM. It works and it is
  slow: 470.59 s of sampling (11.76 s per step) and 16.1 GiB of host RSS. This is
  a direct sd-cli job, NOT a bridge request: the pinned backend holds 5.6 GB of
  VRAM, the offload layout peaks at 11.3 GB, and the two cannot run at once.
- Requests at 1152 square do not fit open-grok's 240 s read timeout at 40 steps
  (275.88 s here); the practical on-demand tier inside open-grok remains
  1024 square at 28 steps, measured at 143.6 s through the bridge.

The VAE decode, tiled against untiled, at 1024x1024 / 28 steps / seed 42 under
--offload-to-cpu (the DiT must NOT be pinned, or the untiled decode cannot be
allocated at all, which is what made the earlier 768-square attempt inconclusive:
both sides tiled there, 3x3 by automatic retry against 2x2 forced):

  decode path                decode time  peak VRAM
  untiled (default)          2.09 s       11,558 MiB
  --vae-tiling forced (3x3)  4.04 s        8,692 MiB

  pixel differences, untiled against tiled, same seed:
    mean absolute difference  0.2847 of 255
    maximum absolute          25
    pixels differing at all   44.81 percent
    differing by more than 8  0.03 percent
    differing by more than 32 0.00 percent

The differences are tile-seam localised, which is what a spatially tiled decode
should produce: the mean absolute difference inside the 16-pixel bands around the
tile boundaries is 0.64-0.75 against 0.52 over the whole image, and the single
largest deviation (25) sits at the seam at column 334. So the tiled decode the
pinned layout falls back to at 1024 square is a faithful approximation of the
untiled one, with seam-localised deviations under one part in 250 of the range -
no longer merely "visually unverified" as the placement section said. It costs
roughly 2 s more per 1024-square request and saves about 2.9 GB of decode peak.

## Configuration and supervision (2026-09-22)

Model paths are not compiled into the bridge. They live in imagegen.toml at the
repository root, which the bridge reads from its working directory (point
IMAGEGEN_CONFIG elsewhere to override). A missing model path is a startup error
naming the file and the variable, never a built-in guess: silently loading a
different model is worse than refusing to start. Environment variables override
the file one value at a time, which is how a test or an operator changes a single
path without editing the file. The effective configuration is logged at startup.

Model files follow the upstream card's layout, one directory per component:
models/diffusion_models/ for the GGUF diffusion models, models/text_encoders/ for
the text encoders, models/vae/ for the VAE. The files themselves are unchanged and
hash-match the published checksums.

The default text encoder is the ComfyUI int8 convrot file the upstream card
recommends, which is why the deployed sd-server is the patched build from
patches/0001 and patches/0002: stock sd.cpp aborts on that file. The GGUF encoder
stays on disk at models/text_encoders/Qwen3VL-8B-Instruct-Q4_K_M.gguf and needs
no patches; set IMAGEGEN_LLM to it (or edit imagegen.toml) to go back. Its
measured cost against the GGUF encoder is about 5.8 GB more host RAM for the same
VRAM peak, a somewhat higher conditioning time, and a tie on image quality.

The bridge supervises its backend. It reaps the child, and if the process dies it
restarts it with exponential backoff and waits for it to answer before calling it
ready. /health reports backend (an HTTP probe of the backend), backend_process_alive
(the child's state), backend_restarts (healthy replacements) and
backend_restart_attempts (spawns attempted after a death). Before this, the child
was spawned once and never waited on: on 2026-09-22 the bridge ran 8h51m with a
zombie sd-server, a dead backend port and every image request failing silently.

The listener binds before the first readiness wait, so /health answers during a
startup where the backend never becomes ready, reporting backend false rather
than leaving the bridge unreachable. Two limits are worth knowing:

- Supervision keys on process liveness. A backend that stays ALIVE but never
  answers HTTP is visible in /health as backend false but is not auto-restarted.
  Restarting on a failed probe is future work.
- Readiness is an HTTP probe, so it passes as soon as sd-server binds its port,
  which happens before weights are loaded. The first request after a restart
  therefore pays the full model staging cost.

Requests are bounded: width and height must be divisible by 32, and no edge may
exceed max_edge (imagegen.toml, default 1152), checked on the final size from
BOTH request paths (an explicit size, and open-grok's aspect_ratio plus
resolution). A larger request receives HTTP 400 naming the size, the ceiling and
the reason, because it cannot fit under the configured layout and would otherwise
fail the generation after ~35 s with an opaque backend error.

The ceiling is a property of the layout in backend_args, not of the card, which
is why it is a configuration value rather than a compiled constant: it is set to
the measured pinned-layout ceiling (1152) and must be raised together with a
layout that can serve more. Before 2026-09-22 the bridge advertised 1536 while the
deployed layout could not serve anything above 1152, so a 1344-square request was
accepted and then failed inside the backend - the same class of defect as a
silently dead backend. Serving 1344 square through the bridge would additionally
require taking the warm backend down (see Resolution headroom above), so it stays
a direct sd-cli job.

## On-demand lifecycle (2026-09-22)

Nothing started or stopped the bridge before this: open-grok's image tool is a
pure HTTP client (there is no process spawning anywhere in its implementation,
and it carries a 300 s total / 240 s read timeout), so a request sent while the
bridge was down simply failed, and once started the deployment stayed resident
forever. Both halves are now hook-driven.

The hooks (~/.opengrok/hooks/imagegen-bridge.json, global scope, always trusted):

  PreToolUse, matcher ^(image_gen|image_edit)$  ->  bin/ensure-imagegen-bridge
  SessionEnd                                    ->  bin/stop-imagegen-bridge

ensure-imagegen-bridge is the start half. It probes /health, and if nothing
answers it starts the systemd user unit if that unit is loaded, otherwise it
launches the binary detached from the project root with the CUDA library path.
It waits a bounded 10 s in the foreground (a tool call must not stall), hands any
remaining wait to a detached waiter, fails open, and logs every decision to
~/.local/state/ensure-imagegen-bridge.log. The common path is one HTTP call.

stop-imagegen-bridge is the stop half, and it is guarded, because stopping the
wrong thing is worse than leaving it running:

  1. another live open-grok session  -> skip; sessions are long lived and share
     the bridge, so one session ending is not evidence nobody needs it
  2. /health reports in_flight > 0   -> skip; never interrupt a generation, which
     would throw away the whole run
  3. --force overrides both, for an operator who knows the state

The bridge is its own backstop: idle_timeout_secs (imagegen.toml, default 900 s,
0 disables) shuts the process down when no generation request has arrived for
that long. The timer never fires while a request is in flight, so a long first
request cannot be cut off by its own timeout, and it exits 0 so the unit's
Restart=on-failure does not bring it straight back. /health reports in_flight,
idle_secs and idle_timeout_secs so a stop decision can be made from data instead
of a guess.

What the lifecycle costs, measured on this box:

  state                          bridge RSS   backend RSS   VRAM      first request after
  started, no request yet          6 MiB        337 MiB      475 MiB   restages weights: 35-40 s
  after a generation             ~6 MiB        ~11 GB        ~5.6 GB   already warm

So the idle timer is what makes the on-demand model pay off: it releases roughly
11 GB of host RAM and 5.6 GB of pinned VRAM, and the hook restarts the bridge in
about a second when an image is next requested.

Verified live on 2026-09-22, end to end and by hand:

- With the bridge stopped and a SECOND session alive, a fresh headless session
  asked for an image. The PreToolUse hook started the service at 12:02:38, the
  request was served, and the session received a 1024x1024 PNG: "generating 1
  image(s) at 1024x1024 steps=28" at 12:02:39, "generated 1 image(s) in
  175.280632557s". No hand-holding: the hook is what brought the bridge up.
- With IMAGEGEN_IDLE_TIMEOUT_SECS=20 the bridge logged "idle for 30s (timeout
  20s), releasing the backend; the next request restages the model stack" and
  exited on its own after 48 s, leaving no sd-server behind. The 30 s interval is
  the poll period, so an idle exit lands within one poll of the timeout.
- With the systemd unit not installed, the same ensure path launched the binary
  detached, recorded the real pid in ~/.local/state/imagegen-bridge.pid, and
  became healthy, so the hook works on a machine without the unit too.
- A systemd stop on the new binary logged its own "SIGTERM received" and reaped
  sd-server, so a unit stop does not leak the backend.
- The stop hook's guards were observed doing their job: without --force it
  refused to stop while two open-grok sessions were alive, and skipped silently
  only when /health did not answer because the bridge was already down.

packaging/imagegen-bridge.service is the unit: Type=simple, WorkingDirectory the
repository root, Restart=on-failure, the same log file as the detached launcher,
and systemd's default SIGTERM stop signal, which the bridge now handles cleanly
(it reaps sd-server on either SIGINT or SIGTERM; before this, only SIGINT was
handled and a unit stop relied on systemd killing the whole cgroup). The unit is
deliberately not enabled: enabling it would start the bridge at every login,
which is exactly what the idle shutdown exists to avoid.

Two limits worth knowing. The session-end stop stays out of the way while any
other session is alive, so a long-lived session that never asks for images keeps
the bridge until the idle timer fires; that is the intended trade, not an
oversight. And SessionEnd does not fire in headless runs: measured with a
temporary probe hook (2026-09-22), a headless `open-grok -p` session emits
session_start and pre_tool_use and a stop with reason end_turn, but no
session_end, so the stop hook applies to interactive sessions only and the idle
timer is what covers headless ones. The start half fires in both modes, which is
what the end-to-end test above relies on. Socket activation is not used either:
the hook is the trigger, which keeps the start path debuggable and leaves the
bridge binding its own port.

## Quality and open-grok on-demand (Unit 2 findings)

- Step count matters more than any other single request field at this size, and
  the deployed default of 28 exists for that reason. Measured 2026-09-22 at 1152
  square, same prompt and seed 5, 8 steps against 28: the mean image gradient
  rises from 1.24 to 3.68 and the Laplacian mean from 3.75 to 9.25 (standard
  deviation 4.63 to 16.98), i.e. the 8-step image carries roughly a third of the
  high-frequency detail; 46.6 percent of pixels differ by more than 32 of 255. An
  independent visual read called the 8-step frame hazy and soft throughout, with
  an illegible sign, a placard detached from the cat and malformed paws, against
  a sharp 28-step frame with legible lettering. A short-step request is a fit
  probe, never a quality sample: qa-evidence/qa-gen-1152-8steps-seed5.png and
  qa-gen-1152-28steps-seed5.png are that pair. The bridge defaults to 28 steps
  (imagegen.toml) whenever the client does not send num_inference_steps.
- Quality on a 12 GB card is bounded by VRAM. Q4_K_M @ 1024 peaks ~11.5 GB.
  The practical higher-quality point is the Q6_K diffusion model (5.88 GB,
  near-lossless), which is the deployed default: parameters pinned in VRAM
  with the arrangement described under Parameter placement above. The
  figures in the next sentence were measured with --offload-to-cpu (weights
  RAM-resident, staged on demand): Q6_K @ 1344x1344, 40 steps fits (peak
  11.1 GB) at ~11.5 s/it (~8.3 min) and gives a clear quality step over
  1024/Q4. That 1344 tier is unverified under the pinned default.
- Pure CPU compute is a dead end for on-demand: it frees VRAM (~5 GB peak)
  but runs the 7B DiT at ~375 s/it, so higher resolution via full CPU is
  not usable.
- open-grok's image client (ImageGenClient) sends a specific payload and has
  a hard 300 s total / 240 s read timeout. For reliable on-demand, the
  request must finish in well under ~240 s, so the bridge defaults to 28
  steps (warm ~171 s). Request fields it honors: model, prompt, n,
  aspect_ratio, resolution, response_format (b64_json), num_inference_steps,
  seed, cfg_scale. Higher-res/longer generations are available outside
  open-grok by requesting more steps or a larger size directly.
- Wiring: [toolset.image_gen] in ~/.opengrok/config.toml -> base_url
  http://127.0.0.1:8787/v1, api_key "imagegen-local" (secret-free; the
  bridge ignores auth), model "Qwen-Image-2.1", response_encoding auto.

## Bridge design

- Endpoints:
  GET  /v1/models                        -> {"data":[{"id":"Qwen-Image-2.1"}]}
  POST /v1/images/generations            -> {"data":[{"b64_json": "..."}]}
- Request fields to support: prompt, size (e.g. "1024x1024"), n (1),
  num_inference_steps (default 40), seed (optional).
- Process model (as implemented): the bridge spawns sd-server once at
  startup (diffusion model, text encoder, VAE; --offload-to-cpu) and keeps
  it warm: the ~9.3 GB model stack stays resident, so every request after
  the initial load is warm. The bridge polls the backend until it answers
  (up to 180 s) before serving, and kills sd-server when the bridge is
  shut down (SIGINT / ctrl-c). There is no idle reaper; the backend lives
  as long as the bridge.
- Cold start cost: one-time at bridge startup (~35 s to load ~10 GB of
  weights); per-request cost is generation time only (~171 s at 28 steps
  and 1024 square warm; see Measured behavior). The open-grok image tool
  timeout (300 s total / 240 s read) tolerates warm requests comfortably.
- Alternative (zero resident processes): systemd user socket activation.
  The user systemd session is running (systemctl --user is-system-running
  returned running on 2026-09-21). Preferred: the bridge daemon, because it
  is simpler to control and debug.

## Wiring (verified against open-grok source)

The block that redirects the built-in image_gen/image_edit tools:

  [toolset.image_gen]
  enabled = true
  base_url = "http://127.0.0.1:8787/v1"
  api_key = "imagegen-local"
  model = "Qwen-Image-2.1"
  response_encoding = "auto"
  image_gen_enabled = true
  image_edit_enabled = true

Source-verified semantics:
- Active only when base_url is non-empty AND a key resolves (api_key field,
  or env_key naming an env var). The key is required even for a keyless
  local endpoint.
- The configured key, never the session bearer, authenticates the request.
- response_encoding "auto" accepts both b64_json and url responses; b64_json
  is what the bridge returns.
- model_override naming the endpoint's model id is optional; the example
  above sets model so open-grok names it.

## Constraints that shape everything

- ds4 (the text engine on the Spark) cannot generate images; llama.cpp
  cannot either. Only stable-diffusion.cpp (or a diffusers/ComfyUI stack,
  ruled out: no GUI desired) runs this model.
- The text encoder is fixed: Qwen3-VL-8B-Instruct. Not substitutable.
- Local is preferred over the Spark: open-grok runs here, and 12 GB VRAM +
  ~15 GB free RAM fits the model stack; no network hop to 192.168.1.91.