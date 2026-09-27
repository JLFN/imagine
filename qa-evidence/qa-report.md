QA report for the Viggle turbo LoRA unit (the adapter recipe in
docs/viggle-turbo-lora.md, its registration in models/MANIFEST.md section 4, and
the measurements those two documents assert).

qa-model: self-verification by the primary agent, chosen by the operator for this
unit (2026-09-27). The project's earlier independent passes used
qwen38-flash-next-ds4; this pass carries no independent-model weight.
Date: 2026-09-27. Revision tested: d3a1686 on feature/viggle-turbo-lora.
Unit type: documentation and measurement record. No code, configuration or
dependency was changed, so there is no function, endpoint or client method to
exercise.

INDEPENDENCE DISCLOSURE, read this first
This pass was performed by the same agent that wrote the recipe and ran the
generations, at the operator's request. It is self-verification and strictly
weaker than an independent-model pass. To compensate, every check below re-reads
the artefact the claim came from (the run logs, the safetensors header, the
sd.cpp sources, the Hugging Face metadata) and quotes it verbatim, rather than
restating the document. The things NOT verified are listed explicitly at the end.

Surfaces covered: ALL NEW SURFACES. The unit adds two documents and no
executable surface. Each factual claim in them was re-derived below.

CLAIM: the adapter applies in full and needs no key conversion
  command: grep -E "LoRA tensors have been applied|apply lora" /tmp/turbo-run1.log /tmp/volvo-a.log /tmp/volvo-b.log
  observed: in all three LoRA runs,
    diffusion_engine.cpp:1673 - apply lora at runtime
    lora.hpp:982 - (454 / 454) LoRA tensors have been applied, lora_file_path =
      models/loras/Qwen-Image-2.1-viggle-turbo-v0.2.1-6step-lora-r128.safetensors
  No "unused lora tensor" line and no "incompatible LoRA tensors have been
  skipped" line appears in any run, so the count is the whole adapter.
  source check: the suffix map in /data/build-sd/stable-diffusion.cpp
  src/name_conversion.cpp maps ".lora_A.weight" to ".weight.lora_down" and
  ".lora_B.weight" to ".weight.lora_up", which is exactly the key shape the file
  carries (verified by reading the safetensors header over HTTP: 454 tensors,
  all named ...lora_A.weight / ...lora_B.weight under transformer.*).

CLAIM: the file really contains 454 tensors and the 10 target modules per block
  command: HTTP range read of the safetensors header plus a tensor-name dump of
  models/diffusion_models/qwen-image-2.1-Q4_K_M.gguf
  observed: header parses to "total tensors: 454", metadata
  transformer.lora_alpha = 128 and transformer.r = 128 (alpha equals rank, hence
  the documented scale of 1.0), and the module list in the metadata matches the
  document's list (to_q, to_k, to_v, to_out.0, img_mlp.proj, img_mlp.gate_layer,
  img_mlp.out, modulation.1, timestep_embedder.linear_1, linear_2).
  The GGUF carries the matching base tensors, e.g.
  transformer_blocks.0.attn.to_q.weight, modulation.1.weight,
  time_text_embed.timestep_embedder.linear_1.weight.

CLAIM: the published 6-node schedule needs a 7th value to become 6 steps
  command: read src/pipeline/request.cpp and src/pipeline/diffusion_engine.cpp,
  then grep the run log
  observed: request.cpp sets total_steps = sigmas.size() - 1 for a custom sigma
  list, and diffusion_engine.cpp derives its loop as sigmas.size() - 1 as well.
  The run log agrees:
    request.cpp:381 - total_steps != custom_sigmas_count - 1, set total_steps to 6
    request.cpp:420 - sampling using Euler method
    the per-step line reaches "6/6 - 2.58s/it"
  The document's note that the two WARN lines are unconditional rather than a
  misconfiguration matches the source: the first LOG_WARN is emitted outside any
  comparison, and the second fires because sample_steps (6) is not less than
  total_steps (6).

CLAIM: 60.56 s for the turbo run against 168.83 s for the base model
  command: grep -oE "generate_image completed in [0-9.]+s|[0-9]+/[0-9]+ - [0-9.]+s/it" /tmp/turbo-run1.log /tmp/base-run1.log /tmp/volvo-a.log /tmp/volvo-b.log
  observed:
    turbo  : 1/6 - 17.00s/it, then 2.58-2.59 s/it for steps 2-6, generate 60.56 s
    base   : 0/28 through 28/28 all at 4.40-4.41 s/it, generate 168.83 s
    volvo A: 1/6 - 17.18s/it, 2.73 s/it, generate 63.74 s
    volvo B: 1/6 - 30.50s/it, 2.58 s/it, generate 93.65 s
  The wall figure of 61.63 s comes from /usr/bin/time on the turbo process, which
  is the only run wrapped that way. The fixed-cost spread in the third table
  (text-encoder load 21.77 / 24.81 s, conditioning 24.98 / 27.28 / 44.61 s) is
  read from the same four logs. The VAE decode row is a derivation, not a logged
  total: the logs show a 9-tile decode at about 2.2 s per tile, and the document
  now says so.

CLAIM: the checksum in the table
  command: sha256sum models/loras/...r128.safetensors, compared with the
  X-Linked-Etag served for the same path on huggingface.co
  observed: both are
  bafb91d0047df3f9b8a5a850b0c967f051164314d8aad778dfa34d9c24ec345b, and the size
  is 679,604,800 bytes, matching the repository's blob listing. The r256 row's
  hash was read from the same header and is labelled in the document and manifest
  as not downloaded or verified locally.

CLAIM: licence and provenance
  command: fetch the repository README, LICENSE and file listing
  observed: the README states the adapter is a LoRA over Qwen/Qwen-Image-2.1
  with text encoder, VAE and processor not redistributed; the LICENSE file is the
  Qwen RESEARCH LICENSE AGREEMENT (research or evaluation only). The document
  says the base stack is still required, which matches both.

CLAIM: the server rejects the prompt tag but accepts a structured LoRA list, so
the bridge needs wiring
  command: read examples/server/api.md in the local sd.cpp checkout
  observed: "Prompt-embedded <lora:...> tags are intentionally unsupported"
  (listed for the OpenAI, sdapi and sdcpp routes), "The server only accepts
  explicit LoRA entries from the lora field", and the field tables carry
  lora[].path, lora[].multiplier, lora[].is_high_noise and
  sample_params.custom_sigmas. This is a documentation-level check only; no
  server was started for this unit, which is why section 6 of the recipe is
  marked UNVERIFIED.

CLAIM: the document changed no code
  command: git show --stat d3a1686
  observed: docs/viggle-turbo-lora.md (new) and models/MANIFEST.md only.

CLAIM: the artefact paths printed in section 5 exist
  command: ls -l out/*.png, ls -l models/loras/
  observed: all four PNGs and the LoRA file are present with the sizes quoted.

Gates
- Rule 10, osv-mcp: osv_map_dependencies on /data/imagegen scanned 1 manifest
  and 124 packages, 0 vulnerable packages. Clean at 2026-09-27T16:36:43Z.
- Rule 12, graphify-rs: AST-only rebuild at commit time, 51 nodes, 82 edges,
  9 communities; output stays gitignored.
- Rule 6, visibility: gh repo view JLFN/imagegen --json isPrivate,visibility
  reports isPrivate true, PRIVATE, checked immediately before the push.

NOT verified, and stated as such in the recipe
1. Editing with the turbo LoRA (image-to-image, ref_images): not run.
2. The rank-256 adapter: not downloaded, not compared against rank 128.
3. Resolutions above 1024 and the 2K training area: not run.
4. --flow-shift tuning (sd.cpp's 3.0 default against 1.0): not compared.
5. Peak VRAM for the 6-step run: not measured.
6. The cfg-isolation test that would split the per-step gain between step count
   and guidance: not run, so the document labels that attribution an inference.
7. The bridge wiring for a structured lora entry plus custom_sigmas: not
   implemented, and no server run was made.
8. A warm-service run (weights already loaded): the document says it should be
   cheaper than 61 s; that is a hypothesis, not a measurement.

Open items recorded, not defects of this revision
- The adapter is licensed for non-commercial use only, and the base stack's own
  licences still apply; the documents say so rather than assuming reuse rights.
- Host CPU load moves the fixed costs of a run (conditioning and step 1) by
  roughly a factor of two between otherwise identical runs, so single-run timings
  in this project should keep their process context attached.
- docs/viggle-turbo-lora.md section 6 describes work the bridge does not do yet;
  it is a specification for the follow-up unit, not a claim about current
  behaviour.

verdict: overall PASS
