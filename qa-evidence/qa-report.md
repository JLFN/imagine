QA report for imagegen Unit 7: the 1344-square resolution-headroom measurement,
the true tiled-versus-untiled VAE decode comparison at 1024 square, the rule 14
port of the convrot verifier from Python to Rust, and the bridge request ceiling
that the headroom measurement made necessary.

qa-model: deepseek-flash (the primary agent, by operator request)
Date: 2026-09-22. Revision tested: the working tree on
feature/unit7-resolution-and-port, on top of dev e7c478f, with the bridge
running the newly built bin/imagegen-bridge (5,311,720 bytes).

INDEPENDENCE DISCLOSURE, read this first
This pass was performed by the same agent that wrote the code, at the operator's
request, so it is self-verification and strictly weaker than an independent QA
model's pass. To compensate, every check below was run live - against the
deployed bridge, against a throwaway bridge instance on separate ports, or
against the actual measurement logs - and the evidence is quoted verbatim. The
list of things NOT verified is explicit at the end. Two defects were found and
fixed during this pass (section 5); a self-pass that found nothing should be read
with suspicion.

Surfaces covered, each with the command and what was observed.

BINARY convrot-verify (the Python-to-Rust port required by rule 14)
  command: bash /data/build/linux/build.sh -p /data/imagegen -b convrot-verify;
           bin/convrot-verify
  observed:
    int8 tensors: 1258  bf16 tensors: 750

    model.layers.0.self_attn.q_proj.weight
      shape [4096, 4096] scales 4096 rows checked 8
      pearson=0.999963 rel_l2=0.0086 max_abs=0.0006
      (control, rotation NOT undone: rel_l2=1.3734)

    model.embed_tokens.weight
      shape [151936, 4096] scales 151936 rows checked 4
      pearson=0.999961 rel_l2=0.0089 max_abs=0.0010
      (control, rotation NOT undone: rel_l2=1.3746)

    verdict: PASS (pearson >= 0.99, rel_l2 <= 0.02 on every tensor checked)
    exit=0
  The baseline this is compared against is the Python script's own output, run in
  this session BEFORE the script was deleted, on the same file and the same two
  modules:
    q_proj        pearson=0.999963 rel_l2=0.0086 max_abs=0.0006  control 1.3734
    embed_tokens  pearson=0.999961 rel_l2=0.0089 max_abs=0.0010  control 1.3746
  Every printed digit matches. The port reproduces the Python baseline exactly.

BINARY convrot-verify, negative path (the verdict must fail a wrong file)
  command: bin/convrot-verify /tmp/fake-weights.safetensors
    (a 639,731,670-byte synthetic safetensors with the same header shape as the
    real encoder but random I8 weights and random F32 scales)
  observed:
    q_proj        pearson=0.001856 rel_l2=318.6653 max_abs=26.6902
    embed_tokens  pearson=0.012937 rel_l2=119.1506 max_abs=22.6287
    verdict: FAIL (a tensor disagreed with its BF16 ground truth)
    exit=1
  The measurement therefore discriminates: 0.999963 against 0.001856.
  Additional error paths, both exit 1 with a clear message and no partial
  verdict:
    bin/convrot-verify /tmp/does-not-exist.safetensors
      -> error: open /tmp/does-not-exist.safetensors: No such file or directory
         (os error 2)
    bin/convrot-verify /tmp/fake-weights.safetensors   (first, malformed
      generator run, whose declared tensor sizes exceeded the data written)
      -> error: read model.embed_tokens.weight_scale from
         /tmp/fake-weights.safetensors: failed to fill whole buffer

Rule 14 - no Python left in the repository
  commands: git ls-files | grep -E '\.py$'          -> (no output)
            find . -name '*.py' -not -path './target/*' -not -path './.git/*'
                                                     -> (no output)
  observed: the only tracked Python file, tools/verify-convrot-dequant.py, is
  removed in this revision (git rm) and its replacement is the Rust binary above.
  docs/convrot-encoder-experiment.md now points at bin/convrot-verify.

CONFIG KEY max_edge (imagegen.toml; the bridge's new request ceiling)
  Unit tests, command: cargo test -p imagegen-bridge
  observed: 5 passed; 0 failed - ceiling_allows_the_measured_working_size,
  ceiling_rejects_the_size_the_layout_cannot_serve,
  ceiling_is_symmetric_and_configurable, size_parsing_rejects_non_multiples_of_32,
  aspect_path_stays_inside_the_ceiling.

  Live, the deployed instance was restarted on the newly built binary and the
  effective configuration logged:
    effective configuration: diffusion=models/diffusion_models/qwen-image-2.1-Q6_K.gguf
      llm=models/text_encoders/qwen3vl_8b_int8_convrot.safetensors
      vae=models/vae/qwen_image_2.1_vae_bf16.safetensors listen=127.0.0.1:8787
      backend_port=8788 steps=28 cfg_scale=6 max_edge=1152
      placement=--backend te=cpu,diffusion=cuda0,vae=cuda0
      --params-backend te=cpu,diffusion=cuda0,vae=cpu

  ENDPOINT POST /v1/images/generations, rejection path
  command: curl -X POST http://127.0.0.1:8787/v1/images/generations -d
           '{"prompt":"x","size":"1344x1344"}'
  observed: http_code=400, total_time=0.00029s, message:
    "size 1344x1344 exceeds the maximum 1152x1152 supported by the configured
     layout (diffusion parameters pinned in VRAM): at larger sizes the sampling
     graph cannot be allocated on this 12 GB GPU and the request fails during
     weight preparation. Raise max_edge in the configuration only together with a
     layout that can serve it; for a one-off large image run sd-cli directly with
     --offload-to-cpu. See docs/architecture.md."
  The rejection is 0.3 ms: it happens before any backend work. Before this
  revision the same request was accepted and failed inside the backend after
  about 35 s with an opaque error.

  ENDPOINT POST /v1/images/generations, the aspect path
  command: curl -X POST ... -d '{"prompt":"x","aspect_ratio":"1:1","resolution":"2k"}'
  observed: http_code=400 with the same message for size 2048x2048. Both request
  paths are bounded, which was a defect fixed in Unit 6 and is still enforced.

  ENDPOINT POST /v1/images/generations, acceptance at the ceiling
  command: curl -X POST ... -d '{"prompt":"a lovely cat holding a sign says
           qwen2.1.cpp","size":"1152x1152","num_inference_steps":8,"seed":5}'
  observed: http_code=200 total_time=72.48s; out/unit7-verify-1152.png PNG
  (1152, 1152) RGBA 2,297,223 bytes; bridge log "generating 1 image(s) at
  1152x1152 steps=8 cfg=6 seed=5" then "generated 1 image(s) in
  72.478421964s". 1152 square is accepted AND served end to end.

  That acceptance run used 8 steps deliberately, to keep the check short, and it
  is a FIT probe rather than a quality sample. Following an operator question
  about the image's appearance, the same request was repeated at the production
  step count for a like-for-like artifact, same prompt / size / seed 5:
  command: curl -X POST ... -d '{...,"size":"1152x1152",
           "num_inference_steps":28,"seed":5}'
  observed: http_code=200 total_time=198.35s; PNG (1152, 1152) RGBA 2,826,304
  bytes, committed as qa-gen-1152-28steps-seed5.png. Measured difference between
  the two, same seed and size, computed from the two artifacts:
    high-frequency energy   mean gradient 1.242 -> 3.678 (2.96x)
                            laplacian mean 3.754 -> 9.254, std 4.629 -> 16.981
    pixels differing by more than 32 of 255   46.6 percent
  An independent visual read (agy/Gemini, because the primary model cannot see
  images) described the 8-step frame as soft and hazy throughout with an
  illegible sign, a placard detached from the cat, and malformed paws, against a
  sharp 28-step frame with legible "qwen2.1.cpp" lettering and coherent paws.
  The 8-step image is therefore what an under-converged sampler looks like, not
  evidence about the deployed default: the bridge defaults to 28 steps, and this
  was the one check that bypassed that default. Nothing in this unit changes the
  step count or the sampling path.

  ENDPOINT POST /v1/images/generations, regression at the documented tier
  command: curl -X POST ... -d '{...,"size":"1024x1024","num_inference_steps":8,
           "seed":7}'
  observed: http_code=200 total_time=45.47s; out/unit7-verify-1024.png PNG
  (1024, 1024) RGBA 2,098,199 bytes; /health still
  {"backend":true,"backend_process_alive":true,"backend_restarts":0,"status":"ok"}.
  The common open-grok path is unaffected by the ceiling change.

  The ceiling is CONFIGURATION-DRIVEN, proved against a throwaway instance
  command: IMAGEGEN_MAX_EDGE=2000 IMAGEGEN_LISTEN_PORT=8899
           IMAGEGEN_BACKEND_PORT=8898 IMAGEGEN_SD_SERVER=/bin/true
           setsid ./bin/imagegen-bridge
  observed: its config line reads "listen=127.0.0.1:8899 max_edge=2000";
    size 1344x1344 -> http_code=502 ("backend unreachable", the /bin/true stub)
      i.e. NOT rejected by the ceiling, so the value really comes from config;
    size 2048x2048 -> http_code=400 naming "the maximum 2000x2000";
    the deployed bridge was untouched and still healthy afterwards
      ({"backend":true,...,"status":"ok"}, exactly one imagegen-bridge process).
  No second real sd-server was started, so the running deployment never
  contended for the card.

The measurement claims in docs/architecture.md (Phase 1 and Phase 2)
  Every figure in the new "Resolution headroom and the VAE decode" section was
  traced back to the raw sd-cli log it came from:
    269.29 s, 275.88 s, 10,060 MiB   -> /data/build-sd/te-split/unit7/pinned-1152-gguf.log
    470.59 s, 502.54 s, 11,276 MiB   -> .../offload-1344-convrot.log
    622.61 MB against 622.44 MB      -> .../res1344-convrot.log AND res1344-gguf.log
                                        (both encoders, identical demand)
    793.24 MB against 654.50 MB      -> .../pinned-1344-maxvram.log
    11,280 / 10,756 / 11,068 MiB     -> the three vram-*.log peak sorts
    decode 2.09 s / 4.04 s           -> tiling-1024-untiled.log / tiling-1024-tiled.log
    11,558 / 8,692 MiB               -> the tiling pair's vram logs
    0.2847 of 255, max 25, 44.81 pct, 0.03 pct, seam bands 0.64-0.75
                                     -> computed from the two PNGs in this pass
  The failure is recorded as a failure: 1344 square does NOT fit under the pinned
  layout, for both text encoders, and the doc no longer says "unverified".
  The isolation claim for Phase 2 was checked in the logs rather than assumed:
  tiling-1024-untiled.log contains no "retrying with spatial tiling" line and no
  tile count (a genuinely untiled decode), while tiling-1024-tiled.log contains
  "num tiles : 3, 3" and "processing 9 tiles". That is what the earlier
  768-square attempt lacked.

Rule 19 mount and the project test suite
  command: bash tests/run.sh   (cargo test --workspace, then tests/qa-gate.sh)
  observed before the report existed: the gate short-circuited with "QA GATE: ALL
  PASS (HEAD is at dev; nothing new to QA)" because the work was not yet
  committed; after the first commit it must extract the new surfaces and demand a
  fresh report naming each of them (BINARY convrot-verify, CONFIG KEY max_edge),
  with the report's last non-empty line being the operative verdict. That
  behaviour is the reason this file exists in this shape.
  tests/run.sh is new in this revision and is the project's single test entry
  point; before it there was no test suite at all, so rule 19's blocking gate had
  nowhere to mount.

Rule 10 dependency scan (before any push)
  command: osv__osv_map_dependencies path=/data/imagegen
  observed: {"manifests_scanned":1,"packages_scanned":155,"vulnerable_packages":0,
  "findings":[]} at 2026-09-22T09:24:43Z against osv.dev. 155 packages is up from
  the previous scan's 115, because the port adds ureq, rustls, ring and their
  transitive dependencies.

Rule 5 canonical build
  command: bash /data/build/linux/build.sh -p /data/imagegen -b convrot-verify
           bash /data/build/linux/build.sh -p /data/imagegen -b imagegen-bridge
  observed: both binaries land in bin/ (convrot-verify 3,318,616 bytes;
  imagegen-bridge 5,311,720 bytes, up from 5,303,304). One operational finding,
  recorded because it cost a build: the builder cannot copy over a RUNNING
  binary - "cp: cannot create regular file '/data/imagegen/bin/imagegen-bridge':
  Text file busy" - and it deletes target/ afterwards, so the freshly built
  binary is discarded. The bridge must be stopped before rebuilding it.

Defects found and fixed during this pass
  1. The bridge advertised a 1536 maximum while the deployed pinned layout cannot
     serve anything above 1152. A 1344-square request was accepted and then failed
     in the backend. Fixed: the ceiling is now max_edge in imagegen.toml (default
     1152, the measured value) and the rejection names the reason. This is the
     same defect class as the silently dead backend fixed in Unit 6: a limit that
     the service cannot honour.
  2. docs/architecture.md quoted three host-RSS figures in decimal GB while
     labelling them GiB (6.7, 10.2, 5.4 where the logs say 6.40, 9.71, 5.11 GiB).
     Found by tracing every figure back to its log, fixed, and worth noting
     because this repository has been bitten by exactly this unit confusion
     before (the Q4_K_M 4.6 GB / GiB note in the same file).
  3. The doc claimed a "12-step schedule fails identically" in a way that
     conflated the step count with the separate --max-vram probe. Reworded to the
     accurate statement: the failure happens during weight preparation before any
     step runs, so step count is irrelevant by construction.
  4. The new tests/qa-gate.sh over-extracted surfaces on its first real run: its
     first version treated every workspace member appearing in a changed
     `members` array as a new binary, so adding the second crate also demanded QA
     evidence for the pre-existing bridge binary. Found by running the gate for
     real against the committed unit rather than trusting it, fixed to surface
     only members whose Cargo.toml did not exist at the base ref. After the fix
     the gate extracts exactly BINARY convrot-verify and CONFIG KEY max_edge, both
     covered above, and exits 0.

NOT verified in this pass, stated plainly
  - 1536 square was never measured. The ceiling now rejects it, and the pinned
    layout cannot serve it, but the offload layout's behaviour at 1536 is unknown.
  - The exact pinned ceiling between 1152 (works) and 1344 (fails) was not
    bisected; 1152 is the largest VERIFIED tier, not necessarily the true maximum.
  - The corrected RSS figures come from /usr/bin/time -v; they were re-read from
    the logs, not re-measured.
  - The verdict threshold cannot catch a systematic weight error below about 1
    percent, which is the tool's stated resolution, not a defect.
  - bin/convrot-verify needs network access to huggingface.co for the BF16 ground
    truth. Its offline behaviour was not exercised; the file-missing and
    short-read paths were, and both exit 1 with a clear message.
  - Image QUALITY at 1344 square was not compared against 1024 square; only fit,
    time and memory were measured.
  - The tiling comparison used the GGUF encoder and the --offload-to-cpu layout.
    The encoder does not affect the decode path, but that is reasoning, not a
    measurement, so it is stated rather than claimed as verified.
  - Measurements ran with the bridge stopped (required: it pins 5.6 GB of VRAM),
    so behaviour under concurrent load was not exercised.

Evidence artifacts committed with this report
  - qa-evidence/qa-gen-1152-8steps-seed5.png: the 1152-square image the deployed
    bridge returned (PNG 1152x1152 RGBA, 2,297,223 bytes), i.e. the new ceiling's
    acceptance proven by its own output. An 8-step FIT probe, not a quality
    sample.
  - qa-evidence/qa-gen-1152-28steps-seed5.png: the same request at the production
    28 steps (PNG 1152x1152 RGBA, 2,826,304 bytes), the honest sample of what the
    deployed default at the new ceiling looks like.
  - qa-evidence/qa-tiling-1024-untiled-vs-tiled.png: the Phase 2 comparison, the
    untiled decode on the left and the forced-tiled decode on the right, same
    seed, from which the pixel differences above were computed.
  The raw sd-cli logs behind every measurement figure stay outside the repository
  at /data/build-sd/te-split/unit7/ (this repository gitignores *.log), and the
  measurement harness is /data/build-sd/te-split/measure-unit7.sh.

verdict: overall PASS
