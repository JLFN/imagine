Verification record — turbo-native checkpoint on the reference card

Document control

  Identifier      IMAGEGEN-VERIFY-TURBO
  Revision        1.0
  Status          Approved for public release
  Issue date      2026-11-24
  Owner           repository maintainers
  Classification  Public
  Applies to      recipe.json at the commit carrying this revision
  Related         docs/REFERENCE.md, docs/sampling-profiles.md, recipe.json

1. Purpose

This record states what was run, on what hardware, with which files, and what
came out. It exists so that a claim about the turbo-native profile rests on a
measurement a reader can repeat rather than on prose.

2. Object under test

The turbo-native checkpoint published in the base repository's default branch,
recorded in recipe.json as component q4-k-m-turbo:

  file        models/diffusion_models/qwen-image-2.1-turbo-Q4_K_M.gguf
  size        4,424,021,152 bytes
  checksum    d28bd7607b3096b1348c77df4e82f3e0d39fcfafc68d23cbb8a8ee15bbb20746

The checksum was compared against the repository's own checksum file after the
download, and matched. The companion files were the encoder and decoder already
recorded in the recipe, with checksums that matched their sources.

3. Backend

The host's installed backend was built before this checkpoint's schedule was
supported as a named preset, so the current upstream source was built from
scratch for this test:

  source      stable-diffusion.cpp, commit 3b12781
  version     master-1122-3b12781
  build       CMake, release, CUDA on, CUDA toolkit 13.3
  binaries    sd-cli and sd-server

The build added nothing to the deployment beyond the two binaries. It was
needed because the older binary accepts a comma-separated sigma list but not
the preset name, and the named preset is the form the model's own instructions
use.

4. Procedure

Two runs of the same request, differing only in where the decoder executes:

  model       qwen-image-2.1-turbo-Q4_K_M.gguf
  encoder     Qwen3VL-8B-Instruct-Q4_K_M.gguf, on the CPU in both runs
  decoder     qwen_image_2.1_vae_bf16.safetensors
  sampler     euler, eight steps, guidance 1.0
  schedule    the published nine-value list, passed by its preset name
  size        1024 by 1024, seed 42
  prompt      a natural light portrait photograph of a lighthouse keeper at dawn

5. Results

  Run                                     Sampling   Decode   Wall    Peak video memory
  --------------------------------------  ---------  -------  ------  ------------------
  decoder on the CPU                      18.9 s     70.4 s   103.7 s not measured
  decoder on the GPU                      8.1 s      1.7 s    21.0 s  not measured
  decoder on the GPU, isolated run        10.9 s     1.7 s    14.0 s  11,515 MiB

The isolated run is the one this record relies on for memory: the process ran
alone with an idle card and its video memory was sampled once a second, so the
peak is a measured value rather than a figure read from another run's log.

The two runs produced the same picture, checked by looking at both images. They
are not byte-identical, and the checksums are recorded here so the claim is
falsifiable rather than loose: the decoder-on-CPU file is 2719a539..., the
decoder-on-GPU file is 0213c992.... The difference is expected, because the
decoder runs in a different numeric environment, and it is the reason the claim
is visual equivalence and not byte equality.

Artifacts are the files under out/turbo-check/, which is build output and is not
committed. A second prompt and seed (a stone cottage in a meadow) and a third
(a wide harbour at blue hour) were also generated; the harbour is the isolated
run above. All three images were coherent and followed their prompts.

6. What this establishes

  - the turbo-native checkpoint generates a coherent image at 1024 square on a
    12 GiB card with eight steps and the published schedule;
  - the decoder's placement dominates the wall time of a short run, so a
    deployment of this profile should place the decoder on the GPU;
  - the profile fits the reference card with under 800 MiB of headroom at 1024
    square, so a smaller card needs the decoder on the CPU or a smaller size.

7. What this does not establish

  - a quality rating. The images were checked for coherence and prompt
    adherence by eye, not scored against a reference set. The quality labels
    elsewhere in this repository are from an earlier, separate comparison.
  - that the framebuffer memory reported by the card is the whole cost: host
    memory was not measured in the isolated run, only video memory.
  - sustained behaviour. These were single runs, not a load test.
  - behaviour at other sizes, step counts, or seeds beyond those listed.
  - that the adapter-based turbo profile behaves the same way; it is a
    different mechanism with its own recorded measurement.

8. Reproduction

    tools/recipe-check.sh --profile turbo-native
    bin/sd-cli \
      --diffusion-model models/diffusion_models/qwen-image-2.1-turbo-Q4_K_M.gguf \
      --llm models/text_encoders/Qwen3VL-8B-Instruct-Q4_K_M.gguf \
      --vae models/vae/qwen_image_2.1_vae_bf16.safetensors \
      --sigmas qwen-image-2.1-turbo --steps 8 --cfg-scale 1.0 --sampling-method euler \
      -W 1024 -H 1024 --seed 42 \
      --backend te=cpu,diffusion=cuda0,vae=cuda0 \
      --params-backend te=cpu,diffusion=cuda0,vae=cpu \
      -p "A natural light portrait photograph of a lighthouse keeper at dawn" \
      -o out/turbo-check/turbo-1024-seed42.png

A backend that predates the named preset takes the same nine values as a list,
which is recorded in the profile's sigmas field.
