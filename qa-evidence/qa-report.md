QA EVIDENCE REPORT - imagegen bridge VRAM-split change (IMAGEGEN_BACKEND_ARGS)

qa-model: qwen38-flash-next-ds4
date: 2026-09-22T05:27Z (UTC) / 07:27 CEST
workspace: /data/imagegen
branch under test: feature/bridge-vram-split, HEAD 341bc8d
  "fix(bridge): reject n>1 and non-div-32 sizes with OpenAI errors"
  plus the uncommitted working-tree change under test
  (git status: M crates/bridge/src/main.rs, M docs/architecture.md,
   M imagegen-handoff.md)
target under test: http://127.0.0.1:8787 (production imagegen-bridge),
  backend sd-server on 127.0.0.1:8788
throwaway instances used for config tests: bridge 8789 and 8791, backend
  port 8790, with IMAGEGEN_SD_SERVER pointing at a stub script that records
  argv and serves a minimal /v1/models response. No model was loaded by any
  throwaway instance.

SCOPE AND SAFETY
The change under test is: (1) new env var IMAGEGEN_BACKEND_ARGS carrying the
sd-server placement flags with the default "--backend
te=cpu,diffusion=cuda0,vae=cuda0 --params-backend
te=cpu,diffusion=cuda0,vae=cpu", replacing the hardcoded --offload-to-cpu;
(2) IMAGEGEN_DIFFUSION default moved from models/qwen-image-2.1-Q4_K_M.gguf
to models/qwen-image-2.1-Q6_K.gguf; (3) intended live placement is diffusion
parameters pinned in VRAM, text encoder executing and living in RAM, VAE
parameters in RAM with compute on the GPU.
Every check below ran live. No production process was stopped, killed or
restarted. Process ids 2124252 (bridge) and 2124282 (sd-server) are the same
pair before and after the whole QA run, and both were still alive and
listening at the end.

PRE-FLIGHT: the tested binary is the one containing the change

  command: stat -c '%n %y' crates/bridge/src/main.rs bin/imagegen-bridge
  observed:
    /data/imagegen/crates/bridge/src/main.rs 2026-09-22 07:18:26
    /data/imagegen/bin/imagegen-bridge       2026-09-22 07:19:14
  The deployed binary is newer than the edited source, so the running process
  does contain the change under test.

  command: strings -n 8 bin/imagegen-bridge | grep -E 'qwen-image|params-backend|offload-to-cpu'
  observed (excerpt):
    IMAGEGEN_DIFFUSION models/qwen-image-2.1-Q6_K.gguf
    IMAGEGEN_BACKEND_ARGS
    --backend te=cpu,diffusion=cuda0,vae=cuda0 --params-backend te=cpu,diffusion=cuda0,vae=cpu
  The string --offload-to-cpu is absent from the binary (grep found no
  occurrence), confirming the hardcoded flag is gone.

  Production process pair (ps -eo pid,ppid,etime,args):
    2124252  1439  ./bin/imagegen-bridge
    2124282  2124252  bin/sd-server --diffusion-model
      models/qwen-image-2.1-Q6_K.gguf --vae
      models/qwen_image_2.1_vae_bf16.safetensors --llm
      models/Qwen3VL-8B-Instruct-Q4_K_M.gguf --listen-ip 127.0.0.1
      --listen-port 8788 --backend te=cpu,diffusion=cuda0,vae=cuda0
      --params-backend te=cpu,diffusion=cuda0,vae=cpu
  /proc/2124252/cwd -> /data/imagegen, /proc/2124252/exe ->
  /data/imagegen/bin/imagegen-bridge.
  /proc/2124252/environ contains no IMAGEGEN_* variable, so the production
  backend argv is produced entirely by the code defaults. This is what makes
  the live argv a test of the default and not of an ambient env var.

SURFACES COVERED

CONFIG IMAGEGEN_BACKEND_ARGS (default reaches spawned sd-server argv, live process)
CONFIG IMAGEGEN_BACKEND_ARGS (default reaches argv of a fresh spawn, stub backend)
CONFIG IMAGEGEN_BACKEND_ARGS (explicit override changes the argv, stub backend)
CONFIG IMAGEGEN_DIFFUSION (default is the Q6_K file, live process argv)
CONFIG IMAGEGEN_DIFFUSION (env override reaches argv, stub backend)
MODEL models/qwen-image-2.1-Q6_K.gguf (size and sha256 vs models/MANIFEST.md)
LIVE PLACEMENT /data/build-sd/bridge-split.log (diffusion params in VRAM, text encoder and VAE params in RAM)
ENDPOINT GET /health
ENDPOINT GET /v1/models
ENDPOINT POST /v1/images/generations (happy path, real generation, PNG validated)
ENDPOINT POST /v1/images/generations (error path n = 2)
ENDPOINT POST /v1/images/generations (error path n = 3)
ENDPOINT POST /v1/images/generations (error path n = 0)
ENDPOINT POST /v1/images/generations (error path malformed json body)
ENDPOINT POST /v1/images/generations (error path empty prompt)
ENDPOINT POST /v1/images/generations (error path size not divisible by 32)
FUNCTION parse_size (non-div-32 rejection, live through the endpoint)
FUNCTION create_generation n guard (no backend call, proven by stub request log)
FUNCTION spawn_backend (arg construction, observed through recorded stub argv)
FUNCTION health (backend liveness probe, live)
FUNCTION get_models (model id, live)

EVIDENCE

1. CONFIG IMAGEGEN_BACKEND_ARGS - default reaches the live sd-server argv

  command: tr '\0' ' ' < /proc/2124282/cmdline
  observed:
    bin/sd-server --diffusion-model models/qwen-image-2.1-Q6_K.gguf --vae
    models/qwen_image_2.1_vae_bf16.safetensors --llm
    models/Qwen3VL-8B-Instruct-Q4_K_M.gguf --listen-ip 127.0.0.1
    --listen-port 8788 --backend te=cpu,diffusion=cuda0,vae=cuda0
    --params-backend te=cpu,diffusion=cuda0,vae=cpu
  Token-by-token this equals the source default (main.rs:123-127): the flag
  "--backend" and its value "te=cpu,diffusion=cuda0,vae=cuda0" are two
  separate argv entries, and "--params-backend" with
  "te=cpu,diffusion=cuda0,vae=cpu" are two more. The split_whitespace
  tokenizer in env_args (main.rs:77-82) keeps each token as one entry, as
  its doc comment claims. Also note the absence of --offload-to-cpu.
  PASS.

2. CONFIG IMAGEGEN_BACKEND_ARGS - default on a fresh spawn, stub backend

  command: IMAGEGEN_SD_SERVER=/tmp/qa-stub/fake-sd-server
    IMAGEGEN_LISTEN_PORT=8789 IMAGEGEN_BACKEND_PORT=8790
    ./bin/imagegen-bridge
    (stub writes its argv to /tmp/qa-stub/argv.txt then sleeps)
  observed, /tmp/qa-stub/argv.txt in order:
    --diffusion-model
    models/qwen-image-2.1-Q6_K.gguf
    --vae
    models/qwen_image_2.1_vae_bf16.safetensors
    --llm
    models/Qwen3VL-8B-Instruct-Q4_K_M.gguf
    --listen-ip
    127.0.0.1
    --listen-port
    8790
    --backend
    te=cpu,diffusion=cuda0,vae=cuda0
    --params-backend
    te=cpu,diffusion=cuda0,vae=cpu
  The default is not specific to the long-running process; a fresh bridge
  with no IMAGEGEN_BACKEND_ARGS set produces the same placement flags.
  PASS.

3. CONFIG IMAGEGEN_BACKEND_ARGS - explicit override changes the argv

  command: IMAGEGEN_BACKEND_ARGS="--backend
    te=cpu,diffusion=cpu,vae=cuda0 --params-backend
    te=cpu,diffusion=cpu,vae=cpu --extra-token qa42"
    IMAGEGEN_SD_SERVER=/tmp/qa-stub/fake-sd-server
    IMAGEGEN_LISTEN_PORT=8789 IMAGEGEN_BACKEND_PORT=8790
    ./bin/imagegen-bridge
  observed, stub-recorded argv tail:
    --backend
    te=cpu,diffusion=cpu,vae=cuda0
    --params-backend
    te=cpu,diffusion=cpu,vae=cpu
    --extra-token
    qa42
  and the bridge log line:
    2026-09-22T05:22:00Z INFO imagegen_bridge: backend placement args:
    --backend te=cpu,diffusion=cpu,vae=cuda0 --params-backend
    te=cpu,diffusion=cpu,vae=cpu --extra-token qa42
  The env var fully replaces the default (no default flags leaked into the
  argv), arbitrary extra tokens pass through, and no shell quoting is
  involved. PASS.

4. CONFIG IMAGEGEN_DIFFUSION - default and override

  Default, live process (command: tr '\0' ' ' < /proc/2124282/cmdline):
    ... --diffusion-model models/qwen-image-2.1-Q6_K.gguf ...
  Default, fresh spawn (item 2 above): --diffusion-model
    models/qwen-image-2.1-Q6_K.gguf
  Default, bridge log at startup:
    2026-09-22T05:19:14Z INFO imagegen_bridge: starting sd-server backend on
    127.0.0.1:8788 (models/qwen-image-2.1-Q6_K.gguf /
    models/Qwen3VL-8B-Instruct-Q4_K_M.gguf /
    models/qwen_image_2.1_vae_bf16.safetensors)
  Override still works (command: IMAGEGEN_DIFFUSION=models/custom-test.gguf
    IMAGEGEN_SD_SERVER=/tmp/qa-stub2/fake-sd-server
    IMAGEGEN_LISTEN_PORT=8789 IMAGEGEN_BACKEND_PORT=8790
    ./bin/imagegen-bridge):
    --diffusion-model
    models/custom-test.gguf
  The Q4_K_M default is gone from the binary strings (item PRE-FLIGHT).
  PASS.

5. MODEL models/qwen-image-2.1-Q6_K.gguf against models/MANIFEST.md

  command: stat -c '%n %s bytes' models/qwen-image-2.1-Q6_K.gguf
  observed: /data/imagegen/models/qwen-image-2.1-Q6_K.gguf 5876556448 bytes
  manifest (models/MANIFEST.md lines 81-83):
    - models/qwen-image-2.1-Q6_K.gguf   (higher-quality diffusion option)
      size    5876556448
      sha256  a3a0d39bb03cda26302fc048b49d019baaea1c381cbda3994f6f2a7826344fb9
  command: sha256sum models/qwen-image-2.1-Q6_K.gguf   (elapsed 7.6 s)
  observed: a3a0d39bb03cda26302fc048b49d019baaea1c381cbda3994f6f2a7826344fb9
  Size matches the manifest exactly and the file is byte-identical to the
  manifest hash, so the pinned-in-VRAM diffusion model is the registered
  artifact and not a partial or corrupted download. PASS.

6. LIVE PLACEMENT - backend startup log /data/build-sd/bridge-split.log

  The log is the live one: mtime 2026-09-22 07:25:18 (it grew during this QA
  run when the generation below ran), and its first lines are the bridge
  spawn for pids 2124252 / 2124282.
  command: head -20 /data/build-sd/bridge-split.log
  observed (key lines, quoted verbatim):
    2026-09-22T05:19:14.285620Z  INFO imagegen_bridge: backend placement
      args: --backend te=cpu,diffusion=cuda0,vae=cuda0 --params-backend
      te=cpu,diffusion=cuda0,vae=cpu
    ggml_cuda_init: found 1 CUDA devices (Total VRAM: 11893 MiB):
      Device 0: NVIDIA GeForce RTX 4070 SUPER, compute capability 8.9,
      VMM: yes, VRAM: 11893 MiB
    [INFO   ] diffusion_engine.cpp:709  - loading diffusion model from
      'models/qwen-image-2.1-Q6_K.gguf'
    [INFO   ] diffusion_engine.cpp:1486 - Diffusion model weight type stat:
      q6_K: 229  |    bf16: 68
    [INFO   ] diffusion_engine.cpp:1294 - total params memory size =
      10551.03MB (VRAM 5604.32MB, RAM 4946.71MB): text_encoders
      4302.33MB(RAM), diffusion_model 5604.32MB(VRAM), vae 644.38MB(RAM),
      controlnet 0.00MB(N/A), extensions 0.00MB(N/A)
    [INFO   ] main.cpp:149  - listening on: http://127.0.0.1:8788
  Reading of the placement line against the intended arrangement:
    diffusion_model 5604.32MB (VRAM)  -> diffusion parameters pinned in VRAM: matches intent
    text_encoders 4302.33MB (RAM)     -> text encoder parameters live in RAM: matches intent
    vae 644.38MB (RAM)                -> VAE parameters live in RAM: matches intent
    --backend vae=cuda0               -> VAE compute on the GPU: matches intent
    --backend diffusion=cuda0         -> diffusion compute on the GPU: matches intent
    --backend te=cpu                  -> text encoder compute on the CPU: matches intent
  Corroboration from the live GPU state, after the generation in item 9 had
  loaded the text-encoder and VAE tensors:
    command: nvidia-smi --query-compute-apps=pid,used_memory --format=csv
    observed: 2124282, 5980 MiB   (sd-server, close to the 5604 MB of pinned
              diffusion params plus activations)
  One caveat, stated as a limitation rather than a failure: the log line
  proves where the parameters live, and the --backend flags state the
  intended compute module, but the log does not itself print a per-op
  compute-device trace. The VAE-on-GPU claim additionally rests on the
  observed decode behaviour in item 10 below. PASS on the parameter
  placement as the surface was specified; the compute-device split is
  evidenced indirectly (flags plus timing), not by a device-per-op log.

7. ENDPOINT GET /health

  command: curl -sS http://127.0.0.1:8787/health
  observed: {"backend":true,"status":"ok"}   (0.006 s)
  The backend:true is a live probe of the backend (health() GETs
  /v1/models on 8788, main.rs:273-283); confirmed independently:
    command: curl -sS -o /dev/null -w '%{http_code}' http://127.0.0.1:8788/v1/models
    observed: 200
  Negative control for the same function: on the stub-backed throwaway
  bridge the endpoint answered {"backend":true,"status":"ok"} while the
  stub answered /v1/models, and during the first throwaway attempt (a stub
  that never answered) /health refused to answer at all because the bridge
  had not yet become ready. PASS.

8. ENDPOINT GET /v1/models

  command: curl -sS http://127.0.0.1:8787/v1/models
  observed: {"data":[{"id":"Qwen-Image-2.1","object":"model","owned_by":"local"}]}
  Model id is Qwen-Image-2.1 as required. PASS.

9. ENDPOINT POST /v1/images/generations - real generation through production

  command: curl -sS -o /tmp/qa-gen-resp.json
    -w 'HTTP_STATUS:%{http_code} CURL_TIME:%{time_total}s'
    -X POST http://127.0.0.1:8787/v1/images/generations
    -H 'Content-Type: application/json'
    -d '{"prompt":"a red cube on a white table","size":"512x512","num_inference_steps":4,"seed":1}'
  observed: HTTP_STATUS:200 CURL_TIME:20.426540s
    wall time measured around the curl (date +%s.%N): 20.434 s
    peak VRAM during the request (1 s nvidia-smi poll): 9337 MiB of 12282 MiB
    response: {"created":1790054718,"data":[{"b64_json":"iVBORw0KGgoAAAANSUhEUgAA...
  PNG validation of the decoded b64_json (python, byte level):
    decoded bytes: 256908
    png signature ok: True
    IHDR width x height: 512 x 512
    bit depth / colortype: 8 6
    chunks: [('IHDR', 13), ('tEXt', 1142), ('IDAT', 255697), ('IEND', 0)]
    all CRCs valid, ended at IEND: True
    file: /tmp/qa-gen.png: PNG image data, 512 x 512, 8-bit/color RGBA,
      non-interlaced
    sha256: 90f027108da9b4c5405ff52aa11eb9681c1912142d9242df64b3e2b7bcf80a08
  Content sanity (PIL, 32x32 downsample): 9.7 percent of pixels are
  red-ish (R above 120, G and B below 90) and 82.7 percent are light, which
  is consistent with a red object on a light table. It is not a blank or
  garbage buffer.
  Shape check against the open-grok client contract
  (/data/open-grok/crates/codegen/xai-grok-tools/src/implementations/
  grok_build/image_gen/mod.rs:559-580, ImageGenResponse reads
  data[0].b64_json): the response carries created plus data[0].b64_json and
  no url key, so the client takes the b64 path.
  Backend-side timing from /data/build-sd/bridge-split.log for this request:
    [INFO   ] image.cpp:803  - generate_image 512x512
    [INFO   ] image.cpp:524  - get_learned_condition completed, taking 2.48s
    [INFO   ] image.cpp:888  - sampling completed, taking 16.49s   (4 steps)
    [INFO   ] image.cpp:619  - decode_first_stage completed, taking 1.43s
    [INFO   ] image.cpp:1039 - generate_image completed in 20.40s
    2026-09-22T05:25:18.895640Z  INFO imagegen_bridge: generated 1 image(s)
      in 20.426295725s
  Wall time for the requested generation: 20.43 seconds. PASS.

10. VAE compute observed on the GPU at 512x512 (supporting evidence, and a
    documentation discrepancy)

  At 512x512 the backend decoded without tiling:
    [INFO   ] image.cpp:549  - decoding 1 latents
    [INFO   ] ggml_graph_cut.cpp:1032 - wan_vae build cached graph cut plan done
    [INFO   ] image.cpp:615  - latent 1 decoded, taking 1.43s
  grep -ic tile over the whole log returns 0, so no spatial tiling kicked in
  at this size, and a 1.43 s decode is GPU-speed, not CPU-speed. That
  supports --backend vae=cuda0 actually executing on the GPU.
  Discrepancy worth recording: docs/architecture.md (the change under test)
  states the VAE decode "falls back to spatial tiling (9 tiles, ~2.4 s per
  tile) when the DiT is pinned: 4.4-4.7 s instead of 2.1 s", and main.rs
  lines 36-40 repeat it. Those figures were measured at 1024x1024 (stated in
  both places). At 512x512 under the same pinned arrangement there is no
  tiling and the decode is 1.43 s. Not a functional defect: the doc
  statements are scoped to 1024, and the bridge's own default resolution for
  open-grok is 1024-square (dims_from_aspect, main.rs:240-264), so the tiled
  path is the common one in production. Recorded as a documentation
  precision note, not a failure.

11. ERROR PATH - unsupported n must be rejected and must not reach the backend

  Test rig: throwaway bridge on 127.0.0.1:8791 with the stub backend on
  8790. The stub appends one line per HTTP request it receives to
  /tmp/qa-err2/hits.txt, so the file is a proof of what reached the
  backend.
  state before the requests (readiness probes only):
    GET /v1/models HTTP/1.1   (x4)
  requests and responses:
    command: curl -X POST http://127.0.0.1:8791/v1/images/generations
      -H 'Content-Type: application/json'
      -d '{"prompt":"a red cube on a white table","n":2,"size":"512x512","num_inference_steps":4,"seed":1}'
    observed: HTTP/1.1 400 Bad Request, content-type: application/json
      body: {"error":{"code":null,"message":"n must be 1 (multi-image
      batches are not supported)","param":null,"type":"invalid_request_error"}}
      time 0.000357 s
    n = 3: same status 400 and same body.
    n = 0: same status 400 and same body (n defaults only when omitted).
  state after the requests: /tmp/qa-err2/hits.txt still contains exactly the
  four GET /v1/models lines and no POST /sdapi/v1/txt2img. The rejected
  requests never reached the backend, and the 0.35 ms latency confirms they
  were short-circuited before any network call.
  The error object is OpenAI-shaped: error.message, error.type
  "invalid_request_error", error.param, error.code. PASS.

12. ERROR PATH - other validations, live on production 8787 (no generation)

  command: curl -X POST http://127.0.0.1:8787/v1/images/generations -d '{"prompt":'
  observed: 400 {"error":{"code":null,"message":"invalid request body: EOF
    while parsing a value at line 1 column 10","param":null,
    "type":"invalid_request_error"}}
  command: curl ... -d '{"prompt":"   ","n":1}'
  observed: 400 {"error":{... "message":"prompt is required" ...}}
  command: curl ... -d '{"prompt":"a red cube","size":"900x100"}'
  observed: 400 {"error":{... "message":"size '900x100' is not supported:
    width and height must be divisible by 32" ...}}
  All three are JSON with the OpenAI error envelope, not axum plain text.
  Production health re-checked after them: {"backend":true,"status":"ok"}.
  PASS.

13. Not verified live, with reasons

  a. A second real sd-server with an alternative IMAGEGEN_BACKEND_ARGS value
     (for example diffusion=cpu) to demonstrate a measurable VRAM delta
     between placements. Reason: the task constraint forbids a second
     model-loading backend, and the 12 GB card cannot host two copies of the
     5.6 GB pinned diffusion stack alongside the live one. The override path
     is instead proven at the argv level (item 3), which is the surface the
     change actually implements.
  b. The exact VRAM figures claimed in docs/architecture.md (peak 9,426 MiB
     at 1024x1024 / 28 steps). Reason: reproducing them costs roughly 2.5
     minutes of GPU time per configuration and re-measures a documented
     benchmark rather than a newly added surface. What was measured here
     instead: peak 9,337 MiB during a 512x512 / 4-step request under the
     pinned layout, which is consistent with the pinned-diffusion claim.
  c. Tiled VAE behaviour at 1024x1024. Reason: not a surface of this change
     and a 1024 request is roughly two minutes; also explicitly scoped in the
     docs. At 512 the untiled GPU decode was observed (item 10).
  d. End-to-end image_gen from inside an open-grok tool call (bridge plus
     the client in one live request). Reason: the client contract was
     verified by reading its parser source and matching the response shape
     against it (item 9), and the endpoint was exercised over real HTTP with
     the exact client-side field names; a real tool call would additionally
     depend on an interactive session's config.
  e. Behaviour when sd-server fails to start with bad placement flags (for
     example an invalid module name). Reason: only the bridge-side arg
     passing is in scope, and the bridge's existing wait_backend_ready
     timeout path (180 s, main.rs:161-175) was observed in its retry form
     during the throwaway runs:
       WARN imagegen_bridge: backend not reachable (...), retrying

14. Environment integrity after the run

  command: ps -o pid,stat,etime -p 2124252,2124282
  observed: 2124252 Ssl and 2124282 Sl, elapsed 05:06, i.e. the original
    production pair, never restarted.
  command: ss -tlnp | grep -E '878[78]'
  observed:
    LISTEN 127.0.0.1:8787 users:(("imagegen-bridge",pid=2124252,fd=12))
    LISTEN 127.0.0.1:8788 users:(("sd-server",pid=2124282,fd=18))
  command: curl -sS http://127.0.0.1:8787/health
  observed: {"backend":true,"status":"ok"}
  All throwaway bridges, stub scripts and stub listeners were killed by
  explicit pid; ports 8790 and 8791 are free again and no stub process
  remains. No repository file was edited and nothing was committed by this
  QA run; the only file written under /data/imagegen is this report.

KNOWN LIMITATIONS AND NOTES FOR THE DEVELOPER

  - docs/architecture.md and the main.rs header comment describe the tiled
    VAE decode without stating that tiling is resolution dependent; at 512
    the same layout decodes untiled in 1.43 s. Consider scoping that
    sentence to 1024 and above (item 10).
  - models/MANIFEST.md lines 85-89 still describe the Q6_K default as
    "weights stay in system RAM via --offload-to-cpu", which the change under
    test removes. The manifest is the authoritative model registry per
    AGENTS.md, so this is stale documentation in a file the change did not
    touch. It is a docs fix, not a functional defect.
  - IMAGEGEN_BACKEND_ARGS is whitespace-split with no quoting support, so a
    future flag value containing a space cannot be expressed. Acceptable for
    the current flag set; worth a comment in the README.
  - The bridge does not restart sd-server if the backend dies; health then
    reports backend:false and generations return 502. Out of scope for this
    change and unchanged by it.

verdict: overall PASS
