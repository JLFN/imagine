# QA report: text encoder reverted to the GGUF file (Unit 16)

qa-model: deepseek-flash (the primary agent, by operator request)
date: 2026-09-27
branch: fix/text-encoder-gguf-revert
unit commit under QA: ff9a724 fix(config): stand on the base Q6_K and the GGUF text encoder
surfaces extracted by tests/qa-gate.sh relative to dev: CONFIG KEY diffusion, CONFIG KEY llm

## Coverage

FUNCTION / ENDPOINT / CLIENT METHOD: none added by this unit; the diff touches
imagegen.toml and four documents, and the gate's extraction agrees (two config
keys, no new routes, no new binaries).

CONFIG KEY diffusion. Verified live: the running sd-server carries
--diffusion-model models/diffusion_models/qwen-image-2.1-Q6_K.gguf, the file is
present at 5,876,556,448 bytes with sha256 a3a0d39bb03cda26302fc048b49d019baaea1c381cbda3994f6f2a7826344fb9,
which is the value models/MANIFEST.md records for it, and a generation completes
through it on the deployment stack (probe 7 below).

CONFIG KEY llm. Verified live: the running sd-server carries
--llm models/text_encoders/Qwen3VL-8B-Instruct-Q4_K_M.gguf, the file is present
at 5,027,784,800 bytes with sha256
67d1659bfe71b89d50b45a4ad1a9e5b997e5bb16ce5da66a6a6167abd569e9e2, which is the
upstream Hugging Face LFS object id for that path; the int8 convrot file the key
previously named is gone from disk; and the conditioning path is exercised end to
end by a probe whose only sensitive surface is prompt adherence and text
rendering (probe 8 below).

## Live evidence

1. Configuration parses and carries the intended effective values
   (python3 tomllib on imagegen.toml): diffusion qwen-image-2.1-Q6_K.gguf,
   llm Qwen3VL-8B-Instruct-Q4_K_M.gguf, steps 28, cfg_scale 6.0, turbo false,
   backend_args vae=cpu in both the --backend and --params-backend lines.

2. Live backend command line, read from /proc/<pid>/cmdline of the running
   sd-server:
   bin/sd-server --diffusion-model models/diffusion_models/qwen-image-2.1-Q6_K.gguf --vae models/vae/qwen_image_2.1_vae_bf16.safetensors --llm models/text_encoders/Qwen3VL-8B-Instruct-Q4_K_M.gguf --listen-ip 127.0.0.1 --listen-port 8788 --backend te=cpu,diffusion=cuda0,vae=cpu --params-backend te=cpu,diffusion=cuda0,vae=cpu

3. Target for the change, captured in the session that produced the retained
   reference images (2026-09-26, session 01a0ddef, chat_history.jsonl): the same
   line, character for character. The revert is therefore a reproduction of a
   recorded configuration, not an invention.

4. Diffusion file on disk: models/diffusion_models/qwen-image-2.1-Q6_K.gguf,
   5,876,556,448 bytes, mtime 2026-09-21 22:15, sha256 as above. It has not been
   touched since before the artifacts were reported, which is what rules it out
   as the cause.

5. Encoder file on disk: models/text_encoders/Qwen3VL-8B-Instruct-Q4_K_M.gguf,
   5,027,784,800 bytes, mtime 2026-09-27 22:25, sha256 as above. Re-fetched from
   the official Qwen repository during the unit and hash-checked against the
   upstream object id, not taken on trust.

6. Convrot removal verified: ls of
   models/text_encoders/qwen3vl_8b_int8_convrot.safetensors fails with "No such
   file or directory"; the directory holds the GGUF encoder only; 9,350,798,360
   bytes were freed. Recovery path (size, sha256, public sources) is recorded in
   models/MANIFEST.md.

7. End-to-end generation through the bridge's OpenAI route with a fixed request
   (512 square, 8 steps, seed 42, cfg 6.0): HTTP 200, artifact
   qa-evidence/qa-encoder-revert-probe-512.png, 477,408 bytes, sha256
   e8140be90d72e035c9fbaf0bfb331d8f59b2289863a8c0c431796930eec650fa, PNG 512x512,
   IEND chunk present, pixel statistics mean 87.05 / stddev 119.73 over 190
   distinct byte values, i.e. a real image rather than a blank, uniform or
   truncated one.

8. Independent legibility check on that artifact (rustcv_describe_image,
   an independent vision provider): the sign in the image transcribes
   exactly as requested, "qwen2.1.cpp", the animal is coherent with recognized
   eyes, nose, ears, mouth and fur, and the critic named only soft detail on the
   paws and whiskers plus a faint smudge on the nose bridge. Text rendering and
   prompt adherence are the conditioning surfaces this unit is about, so this is
   the check that the encoder swap had to pass; the softness is what an 8-step
   sample gives and is not a quality measurement of the 28-step deployment.

9. Documentation consistency: no file in the repository still describes the
   convrot file as the deployed default (grep for "deployed default TE" returns
   no match), and the four documents updated here state the revert, the deletion
   and the limits of the attribution.

10. Bridge-binary neutrality was measured, not assumed: one fixed request through
    today's bridge build and through a build of the 2026-09-26 dev tip (a9e35b6)
    produced byte-identical output, sha256
    e9d27abcfd8e660d725ff95c75835e3c9741959874275edbc3ef55e9f95a614f on both.
    This is why the binary is not part of the unit's scope, and it removes the
    binary as a competing explanation for the artifacts.

## Accepted risks and what this pass does not cover

- The encoder defect is not reproduced. The 2026-09-22 comparison of the two
  encoders recorded a quality tie, and the artifact-bearing prompt was never run
  as a fixed-seed A/B across both encoders. This report verifies that the
  configuration does what it claims; it does not prove the causal claim.
- The probe runs at 8 steps for speed, so its softness says nothing about the
  28-step production path, and no 1024-square QUALITY run was made in this QA
  pass. The production-size end-to-end evidence for the unit is the operator's
  accepted render (session 01a0e467, images/7.jpg, 672x1024, 28 steps).
- Sensitivity to the VAE compute placement (--backend vae=cpu here against the
  committed 2026-09-22 value vae=cuda0) is unmeasured; it moved to match the
  captured process.
- The deleted encoder cannot be compared again on this machine without
  re-downloading 9,350,798,360 bytes from the public sources named in the
  manifest.

verdict: overall PASS
