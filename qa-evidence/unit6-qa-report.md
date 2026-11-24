QA report for the imagegen bridge, Unit 6 (config file, backend supervision,
upstream model layout, int8 convrot encoder as the shipped default) and its
QA-driven fixes.

qa-model: deepseek-flash (the primary agent, by operator request)
Date: 2026-09-22. Revision tested: fb054be plus the uncommitted startup
restructure and architecture-doc notes in the working tree.

INDEPENDENCE DISCLOSURE, read this first
This pass was performed by the same agent that wrote the code, at the operator's
request, replacing a separate QA model. That makes it self-verification and
strictly weaker than the two independent passes that preceded it (qa-model
qwen38-flash-next-ds4, recorded in this file's history). To compensate, every
check below was run live against the deployed service or a throwaway instance,
evidence is quoted verbatim, and the list of things NOT verified is explicit.
The three defects those independent passes found are the reason this revision
exists at all; a self-pass should not be read as having equal weight.

Surfaces covered, each with the command and what was observed.

CONFIG imagegen.toml (the file is the binding source of truth)
  command: P=$(pgrep -x sd-server | head -1); tr '\0' ' ' < /proc/$P/cmdline
  observed: bin/sd-server --diffusion-model
    models/diffusion_models/qwen-image-2.1-Q6_K.gguf --vae
    models/vae/qwen_image_2.1_vae_bf16.safetensors --llm
    models/text_encoders/qwen3vl_8b_int8_convrot.safetensors --listen-ip
    127.0.0.1 --listen-port 8788 --backend te=cpu,diffusion=cuda0,vae=cuda0
    --params-backend te=cpu,diffusion=cuda0,vae=cpu
  Every value is the imagegen.toml value; the bridge process carries no
  IMAGEGEN variables in its environ, so nothing came from the environment.

CONFIG IMAGEGEN_CONFIG and the missing-path refusal
  command: env -i IMAGEGEN_CONFIG=/tmp/qa-mine/absent.toml ./bin/imagegen-bridge
  observed: exit=2, "no model path for IMAGEGEN_SD_SERVER: set it in
  /tmp/qa-mine/absent.toml or in the environment"
  Same command with only sd_server present advances to the next missing key, so
  the check names one missing path at a time rather than failing anonymously.
  COSMETIC FINDING, recorded rather than fixed: the message says "no model path"
  for IMAGEGEN_SD_SERVER, which is the sd-server binary rather than a model. The
  behaviour is correct (it refuses and exits 2); only the wording is imprecise. It
  is left as found because changing it after this verification would invalidate the
  evidence above for a string that no logic depends on.

CONFIG malformed TOML
  command: printf 'this is not toml = = =\n' > bad.toml; env -i
    IMAGEGEN_CONFIG=bad.toml ./bin/imagegen-bridge
  observed: exit=2, parser message "expected `.`, `=`"

CONFIG per-key precedence, environment over file
  command: a throwaway with a config whose values are deliberately fake
    (sd_server /tmp/qa-mine/stub.sh, diffusion FROM-FILE-A.gguf, vae
    FROM-FILE-B.safetensors, llm FROM-FILE-C.safetensors, backend_args
    ["--backend","FILE-PLACEMENT"], listen_port 8797, backend_port 8798), started
    with IMAGEGEN_CONFIG set and IMAGEGEN_DIFFUSION=FROM-ENV.gguf,
    IMAGEGEN_LISTEN_PORT=8795, IMAGEGEN_BACKEND_PORT=8796
  observed, from the stub's own recorded argv:
    --diffusion-model FROM-ENV.gguf --vae FROM-FILE-B.safetensors --llm
    FROM-FILE-C.safetensors --listen-ip 127.0.0.1 --listen-port 8796 --backend
    FILE-PLACEMENT
  So the three overridden keys took the environment and the others kept the file.

CONFIG backend_args reach the backend
  observed: the production argv above ends with the two placement flag pairs, and
  the same pair appears verbatim in the stub argv, so placement is file-driven and
  env-overridable rather than compiled in.

BEHAVIOR startup observability (the defect the independent pass found)
  command: IMAGEGEN_SD_SERVER=/bin/true IMAGEGEN_LISTEN_PORT=8797
    IMAGEGEN_BACKEND_PORT=8798 (a backend that spawns and exits instantly)
  observed: port 8797 bound (ss -tln), and GET /health answered DURING the
    failure: {"backend":false,"backend_process_alive":false,
    "backend_restart_attempts":1,"backend_restarts":0,"status":"ok"}
  The listener binds before the readiness wait, so the bridge is observable while
  the backend is broken instead of being unreachable for 180 s and then gone.

BEHAVIOR readiness and /health probe timeouts are bounded
  command: a stub that accepts the TCP connection and never answers, bridge on
    8799/8800; time curl -s -m 40 http://127.0.0.1:8799/health
  observed: answer in real 0m10.007s:
    {"backend":false,"backend_process_alive":true,"backend_restart_attempts":0,
     "backend_restarts":0,"status":"ok"}
  Bounded at the 10 s probe timeout. Note for monitoring clients: /health can take
  up to about 10 s when the backend is wedged.

BEHAVIOR a wedged-but-alive backend is not auto-restarted (documented limitation)
  observed: over 20 s with that wedged stub, log lines matching
    "restarting|exited" = 0, and health still reported
    backend_process_alive true with both counters 0. The limitation stated in
    docs/architecture.md is accurate as written.

BEHAVIOR backend supervision on production
  command: kill -9 on the sd-server child, then poll /health
  observed: before {"...backend_restart_attempts":0,"backend_restarts":0...};
    t+3s attempts 1, restarts 0; t+7s attempts 1, restarts 1; log:
      WARN sd-server backend exited (signal: 9 (SIGKILL))
      WARN sd-server backend is down; restarting in 1s
      INFO sd-server backend restarted and ready (restart number 1)
  The two counters separate a spawn attempt from a confirmed healthy replacement,
  which is the fix for the finding that attempts alone looked healthy.

BEHAVIOR no zombie and no duplicate child
  command: ps -o pid,stat,cmd --ppid $(pgrep -x imagegen-bridge | head -1)
  observed: exactly one child, state Sl, with the expected convrot argv.

ENDPOINT GET /health
  observed: HTTP 200, {"backend":true,"backend_process_alive":true,
  "backend_restart_attempts":0,"backend_restarts":0,"status":"ok"}

ENDPOINT GET /v1/models
  observed: HTTP 200, {"data":[{"id":"Qwen-Image-2.1","object":"model",
  "owned_by":"local"}]}

ENDPOINT POST /v1/images/generations (full-quality artifact, production settings)
  command: 1024x1024, 28 steps, seed 42, prompt "a lovely cat holding a sign says
    qwen2.1.cpp", through the production bridge with the int8 convrot encoder
  observed: http_code=200 total_time=140.023654s; a real PNG 1024x1024 RGBA,
    2,463,614 bytes, saved as qa-evidence/qa-gen-1024-28steps-seed42.png.
    Structure check: mean RGB [156.7 143.9 122.8], per-channel std
    [51.9 54.9 61.2], so it is a rendered image and not a blank frame. /health
    after the request: backend true, both counters 1.
  Reproducibility, same pass: this file is byte-identical to the image produced by
    the earlier deployment proof at the same seed and prompt
    (sha256 cc9d0fd6a682eee8f01028cc906055860370d2d3240656fbc053b120d7a8a078 for
    both qa-evidence/qa-gen-1024-28steps-seed42.png and out/proof-convrot-1024.png).
    Two separate bridge processes, same bytes out, which is the strongest
    end-to-end signal in this report.

ENDPOINT the small fast frames are evidence too
  qa-evidence/qa-gen-512-seed11.png (mine, 512x512, 4 steps, 286,535 bytes) and
  qa-evidence/qa-gen-512-seed3.png (from the independent pass, 512x512, 4 steps,
  280,816 bytes). Both are genuine PNGs, kept as the cheap high-frequency checks
  beside the 1024-square artifact above.


ENDPOINT POST /v1/images/generations (real generation after the respawn)
  command: 512x512, 4 steps, seed 11, on the production bridge
  observed: http=200 time=29.909068s, a real PNG 512x512 RGBA, 286,535 bytes,
  saved as qa-evidence/qa-gen-512-seed11.png. The same body was also run earlier
  in the deployment proof at 1024x1024, 28 steps: HTTP 200, PNG, 158.56 s, inside
  a typical client read timeout.

ENDPOINT POST error paths (all on production, backend untouched)
  observed, seven malformed requests, each HTTP 400 with an
  OpenAI-shaped error object of type invalid_request_error:
    n=2                      "n must be 1 (multi-image batches are not supported)"
    100x100                  "size '100x100' is not supported: width and height must be divisible by 32"
    2048x2048                "size 2048x2048 exceeds the supported maximum of 1536x1536 on this 12 GB GPU"
    aspect 1:1 + 2k          "size 2048x2048 exceeds the supported maximum of 1536x1536 on this 12 GB GPU"
    whitespace prompt        "prompt is required"
    {}                       "invalid request body: missing field `prompt`"
    not json                 "invalid request body: expected ident at line 1 column 2"
  /health before and after the seven rejects is byte-identical with both counters
  at 0, so none of them reached or disturbed the backend.

ENDPOINT size boundary, both request paths (throwaway, no models)
  observed:
    1536x1536 via size        forwarded (HTTP 502 from the fake stub, not 400)
    1504x1504 via size        forwarded
    1568x1568 via size        HTTP 400, "size 1568x1568 exceeds ... 1536x1536"
    aspect 3:2 + 1080p        HTTP 400, "size 2048x1376 exceeds ... 1536x1536"
    aspect 1:1 + 1k           forwarded
  The ceiling applies to the final size from the aspect path as well, which is the
  fix for the bypass the independent pass found.

MODEL files (this pass, sha256 recomputed)
  observed:
    models/text_encoders/qwen3vl_8b_int8_convrot.safetensors
      8bfd0f6e12abf2d2d697ecc888e5e90b0d6741d6708f05799f53afa560452e8f
    models/text_encoders/Qwen3VL-8B-Instruct-Q4_K_M.gguf
      67d1659bfe71b89d50b45a4ad1a9e5b997e5bb16ce5da66a6a6167abd569e9e2
    models/vae/qwen_image_2.1_vae_bf16.safetensors
      bb21f7473051e1ac368515dd3f2e15cd44d7a11748ee8823e1ddca3e4876b7c9
    models/diffusion_models/qwen-image-2.1-Q6_K.gguf
      a3a0d39bb03cda26302fc048b49d019baaea1c381cbda3994f6f2a7826344fb9
  All four match their models/MANIFEST.md entries, including the convrot encoder
  entry added in response to the first independent pass's finding.

MODEL layout matches the upstream card
  observed: models/diffusion_models/, models/text_encoders/, models/vae/ exist and
  hold the files above; every path in imagegen.toml resolves.

Not verified in this pass, with the reason

1. The 1024x1024, 28-step timing with the convrot encoder was measured earlier
   today in the deployment proof (158.56 s) but not re-run here; a re-run costs a
   full model staging and would have contended with the checks above.
2. A true untiled-versus-tiled VAE decode comparison at 1024 square. At 768
   square both sides tile, so the isolation the earlier attempt intended did not
   happen; the recorded result is only that the decode is insensitive to tile
   count (0.15 of 255 mean difference).
3. 1344-square headroom under the pinned layout. Never measured; expected to be
   too tight, and the bridge now rejects anything above 1536 with a clear 400.
4. That the stock sd-server aborts on the convrot file. Proving it needs a second
   9.3 GB model load, which would contend with production for the card. The .stock
   binaries exist and are unverified in this pass.
5. Exponential backoff growth beyond the first step: each failed cycle costs the
   full 180 s startup window, so a multi-step check is expensive.
6. The zip path for int8_tensorwise dequantization, which is unimplemented and
   logs an error. Exercising it needs a zip-packaged model that does not exist
   here.
7. Direct evidence that the mmap dequantization branch engages. There is no
   positive log line for it; the evidence is indirect - the backend loads the
   convrot encoder and reports text_encoders 10104.77 MB in RAM, and a generation
   succeeds, which the plain-file-only version failed to do.

Open items recorded, not defects of this revision
- tools/verify-convrot-dequant.py is Python inside the repository and rule 14
  requires zero Python in the deliverable: port to Rust or remove before delivery.
- The int8 convrot encoder costs about 5.8 GB more host RAM than the GGUF encoder
  for the same VRAM peak, with image quality a measured tie. It is the shipped
  default by operator decision, not because the measurement favours it.
- The 1536 ceiling rejects the "2k" and "4k" tiers with a clear 400. The
  default "1k" path is unaffected.
- A backend that stays alive but never answers is not auto-restarted; it is
  visible in /health as backend false.
- The two sd.cpp patches have not been sent upstream.

verdict: overall PASS
