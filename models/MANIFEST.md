# Model manifest — Qwen-Image-2.1 local (pinned downloads)

Sizes verified from the Hugging Face repo pages on 2026-09-21. Files are
gitignored; download them into models/ (mkdir models) and verify checksums
before first use. The undirected authority is the SHA256SUMS file in each
source repo; always recompute and compare, never trust the sizes alone.

## 1. Diffusion model (the image generator)

The standing QUALITY model and the DRAFT/TURBO host both come from
https://huggingface.co/abenzerps/Qwen-Image-2.1-Uncensored-GGUF (source revision
b3179ad355be050328e483a9dfdd9e60cd62adfa, converted with stable-diffusion.cpp
commit 1330ceb, license Qwen Research License - the same as the base model).
That repository ships the base weights and, beside them, the same weights with a
33.6 MB uncensored LoRA merged ("UC" files); the base files are byte-identical to
the earlier abenzerps/Qwen-Image-2.1-GGUF downloads. All checksums below were
verified locally against the repository's SHA256SUMS on 2026-09-27.

- File:         models/diffusion_models/qwen-image-2.1-UC-Q6_K.gguf
- Size:         5,876,556,576 bytes (5.47 GiB)
- Checksums:    sha256 e14bb312109333b3d73b92ad9b9ac8b29b51f2edca1e7b86d1af1981bf1c4ee3
- Note:         THE DEPLOYED DEFAULT (QUALITY profile, imagegen.toml). The
                uncensored build: base weights with the uncensored LoRA merged,
                chosen for content behaviour, not quality - the measured A/B at
                1024 square on an ordinary scene put it at 9/10 realism, the same
                as the base file. It cannot host the Viggle adapter at 1024
                square on this 12 GB card (measured), so turbo is unavailable in
                this deployment.

- File:         models/diffusion_models/qwen-image-2.1-UC-Q4_K_M.gguf
- Size:         4,604,558,112 bytes (4.29 GiB)
- Checksums:    sha256 e79c8a009f2ecbdb6c70fd663d9aea9ee304a0d91f347e4169a756b8ad141b41
- Note:         The uncensored Q4_K_M, tested as the DRAFT host on 2026-09-27
                (169 s, 7562 MiB peak at 1024 square; critic 5/10 realism against
                6/10 for the base file - no gain). NOT part of the standing
                recipe: the DRAFT/TURBO deployment uses the base Q4_K_M below
                plus the uncensored LoRA as a separate layer, so the base path
                stays the unmodified base model.

- File:         models/diffusion_models/qwen-image-2.1-Q6_K.gguf
- Size:         5,876,556,448 bytes (5.47 GiB)
- Checksums:    sha256 a3a0d39bb03cda26302fc048b49d019baaea1c381cbda3994f6f2a7826344fb9
- Note:         The plain base Q6_K. Byte-identical to the source repository's
                base file (that hash is the repo's own listing). Kept on disk as
                the fallback QUALITY model and as the reference for the identity
                check in docs/sampling-profiles.md.

- File:         models/diffusion_models/qwen-image-2.1-Q4_K_M.gguf
- Size:         4,604,557,984 bytes (4.29 GiB)
- Checksums:    sha256 833439e91bc1152d28f37aa198c7f6f4218b7de95754c2f7a318a2422ab4b2f8
- Note:         The plain base Q4_K_M: the DRAFT/TURBO deployment's host model
                (base mode = DRAFT, with the uncensored LoRA layered on when
                configured; turbo mode = TURBO, with the Viggle adapter as well).

- File:         models/loras/qwen-image-2.1-uncensored-lora.safetensors
- Size:         33,586,704 bytes (32 MiB)
- Source:       same repository (the LoRA the UC files have merged)
- Checksums:    sha256 00ee2cb7f00a37b097640c1880ed0d17e368e30599f34bb348fee0e8c2cc4ca1
                (the repository's LFS object id; verified locally 2026-09-27)
- Note:         Applied as the bridge's EXTRA layer (extra_lora), so it rides on
                both DRAFT and TURBO requests; verified in one run alongside the
                Viggle adapter (454/454 plus 256/256 tensors applied). Measured
                cost at 1024 square: 7571 MiB peak against 7562 MiB without it.
                The benefit is content behaviour (no refusals/sanitising), not
                quality - see docs/sampling-profiles.md.

## 2. Text encoder (fixed architectural component)

- File:         models/text_encoders/Qwen3VL-8B-Instruct-Q4_K_M.gguf
- Size:         5.03 GB
- Source:       https://huggingface.co/Qwen/Qwen3-VL-8B-Instruct-GGUF
- Checksums:    sha256 67d1659bfe71b89d50b45a4ad1a9e5b997e5bb16ce5da66a6a6167abd569e9e2
                (matches the Hugging Face LFS object id; verified locally
                2026-09-27 after the file was re-fetched, 5,027,784,800 bytes)
- Note:         Qwen-Image-2.1 conditions on Qwen3-VL-8B-Instruct exactly.
                It CANNOT be substituted with the Qwen3.8 Flash Next model
                served by ds4 on the Spark (different embedding space). Try
                any other encoder and generations are incoherent.

## 3. VAE (decoder)

- File:         models/vae/qwen_image_2.1_vae_bf16.safetensors
- Size:         0.64 GiB
- Source:       https://huggingface.co/Comfy-Org/Qwen-Image-2.1 (vae/)
                also hosted in the abenzerps/KasugaiSakura repos
- Checksums:    repo page
- Note:         The Qwen-Image-2.1 VAE is NOT interchangeable with the
                earlier Qwen-Image or Wan 2.2 VAE weights (sd.cpp doc).

## 4. Optional: few-step accelerator LoRA (Viggle turbo)

- File:         models/loras/Qwen-Image-2.1-viggle-turbo-v0.2.1-6step-lora-r128.safetensors
- Size:         679,604,800 bytes (648 MiB)
- Source:       https://huggingface.co/Viggle/Qwen-Image-2.1-viggle-turbo
- Checksums:    sha256 bafb91d0047df3f9b8a5a850b0c967f051164314d8aad778dfa34d9c24ec345b
                (matches the Hugging Face LFS object id; verified locally 2026-09-27)
- Note:         A DMD distillation of the base Qwen-Image-2.1 transformer,
                applied on top of the unmodified base weights at runtime. It
                samples in 6 passes with no classifier-free guidance and is
                driven by --sigmas, not by a step count or the shipped
                scheduler config. It is an adapter, NOT a replacement: the
                diffusion GGUF, the text encoder and the VAE above are still
                required. License: Qwen RESEARCH LICENSE AGREEMENT, in the
                repository (non-commercial). Recipe, exact sd-cli command and
                the measured numbers: docs/viggle-turbo-lora.md.
- Variants:     the rank-256 adapter of the same release
                (Qwen-Image-2.1-viggle-turbo-v0.2.1-6step-lora-r256.safetensors,
                1,359,147,904 bytes, sha256
                2a0148f5c73abbed5f97da5ea356e439318aadb281d01fce4af39cdf43728803) is
                upstream's recommended build but is NOT downloaded or verified
                here; the v0.2 release and the PEFT-format copies are optional
                and unused by this stack. Sizes read from the repository's file
                listing 2026-09-27.

## Download commands (for the box where this is set up; here on the workstation)

Preferred tool: the hf CLI (huggingface_hub, resumable). Example per file:

  hf download <repo> <file> --local-dir models

  hf download abenzerps/Qwen-Image-2.1-GGUF qwen-image-2.1-Q4_K_M.gguf --local-dir models
  hf download Qwen/Qwen3-VL-8B-Instruct-GGUF Qwen3VL-8B-Instruct-Q4_K_M.gguf --local-dir models
  hf download abenzerps/Qwen-Image-2.1-GGUF vae/qwen_image_2.1_vae_bf16.safetensors --local-dir models

curl fallback (identical bytes):

  mkdir -p models out
  curl -L -o models/diffusion_models/qwen-image-2.1-Q4_K_M.gguf \
    https://huggingface.co/abenzerps/Qwen-Image-2.1-GGUF/resolve/main/qwen-image-2.1-Q4_K_M.gguf
  curl -L -o models/text_encoders/Qwen3VL-8B-Instruct-Q4_K_M.gguf \
    https://huggingface.co/Qwen/Qwen3-VL-8B-Instruct-GGUF/resolve/main/Qwen3VL-8B-Instruct-Q4_K_M.gguf
  curl -L -o models/vae/qwen_image_2.1_vae_bf16.safetensors \
    https://huggingface.co/Comfy-Org/Qwen-Image-2.1/resolve/main/vae/qwen_image_2.1_vae_bf16.safetensors

## Total

About 10.3 GiB for the Q6_K stack, or 15.9 GiB if the int8 convrot encoder is used
instead of the GGUF one. Runtime: diffusion model pinned in VRAM (fits 12 GB),
encoder in system RAM (4.3 GB as GGUF, 10.1 GB as int8 convrot), VAE small.

## Verified downloads (2026-09-21, this workstation)

Downloaded with the curl commands above and verified against the published
SHA256SUMS / HF LFS oids. sha256sum and ls -l output below:

- models/diffusion_models/qwen-image-2.1-Q4_K_M.gguf
  size    4604557984
  sha256  833439e91bc1152d28f37aa198c7f6f4218b7de95754c2f7a318a2422ab4b2f8
  (source: abenzerps/Qwen-Image-2.1-GGUF SHA256SUMS; matches HF LFS oid)
- models/text_encoders/Qwen3VL-8B-Instruct-Q4_K_M.gguf
  size    5027784800
  sha256  67d1659bfe71b89d50b45a4ad1a9e5b997e5bb16ce5da66a6a6167abd569e9e2
  (source: Qwen/Qwen3-VL-8B-Instruct-GGUF HF LFS oid)
- models/vae/qwen_image_2.1_vae_bf16.safetensors
  size    675509688
  sha256  bb21f7473051e1ac368515dd3f2e15cd44d7a11748ee8823e1ddca3e4876b7c9
  (source: abenzerps/Qwen-Image-2.1-GGUF SHA256SUMS; matches HF LFS oid)
- models/diffusion_models/qwen-image-2.1-Q6_K.gguf   (higher-quality diffusion option)
  size    5876556448
  sha256  a3a0d39bb03cda26302fc048b49d019baaea1c381cbda3994f6f2a7826344fb9
  (source: abenzerps/Qwen-Image-2.1-GGUF SHA256SUMS; matches HF LFS oid)
- models/text_encoders/qwen3vl_8b_int8_convrot.safetensors   (REMOVED FROM DISK 2026-09-27)
  size    9350798360
  sha256  8bfd0f6e12abf2d2d697ecc888e5e90b0d6741d6708f05799f53afa560452e8f
  (source: Comfy-Org/Qwen-Image-2.1 text_encoders/ HF LFS oid; the same bytes are
  published as KasugaiSakura/Qwen-Image-2.1-Uncensored-GGUF
  text_encoders/qwen3vl_8b_int8_convrot.safetensors, verified against both)
  NOT DEPLOYED, and no longer on disk: it held the text-encoder slot for part of
  2026-09-27 as part of the uncensored adoption, the operator reported visible
  artifacts in the generations of that deployment, and the GGUF encoder in
  section 2 above is the standing component. It is a public download, so the
  bytes above (size and sha256) are the whole recovery path; the recipe and the
  measured comparison are docs/convrot-encoder-experiment.md and
  docs/sampling-profiles.md.
  REQUIRES the patched sd-server built from patches/0001 and patches/0002: stock
  sd.cpp misreads this file's geometry at load and then cannot dequantize its
  int8 token embedding. Reports
  10,104.77 MB of parameters at load against the GGUF encoder's 4,302.33 MB, so it
  costs about 5.8 GB more host RAM for the same VRAM peak; the GGUF encoder above
  was always the lighter option and needs no patches.
  The Q6_K diffusion model (~28% larger than Q4_K_M) is the practical
  higher-fidelity default on the 12 GB GPU and is the deployed default in
  the bridge. Its parameters are pinned in VRAM (not RAM-staged): updated
  2026-09-22, see docs/architecture.md, Parameter placement. Earlier text
  here described it as RAM-resident via --offload-to-cpu, which is no longer
  how the bridge runs it. Measured 2026-09-22 with the pinned arrangement at
  1024x1024 / 28 steps: sampling 146.5 s (about 5.2 s/it), total 153.9 s,
  peak VRAM 9,426 MiB, with the VAE decode falling back to spatial tiling at
  that resolution. The earlier --offload-to-cpu figures (about 11.5 s/it at
  1344x1344 with weights RAM-resident, peak 11.1 GB) still apply to that
  arrangement. Resolution under the pinned default is now MEASURED (2026-09-22,
  docs/architecture.md): 1152 square works (sampling 269.29 s for 40 steps,
  peak VRAM 10,060 MiB), and 1344 square does not fit at all - the sampling
  graph fails during weight preparation, short by about 0.2 MB, for both text
  encoders and with an explicit --max-vram budget. 1344 square runs only under
  --offload-to-cpu (502.54 s, 16.1 GiB host RSS). The bridge therefore rejects
  any edge above 1152 with a clear 400. Q8_0 also exists on disk but is reserved
  for when the VRAM/RAM budget allows; it is not the default.

Note: an initial curl for the diffusion model hit a flaky HTTP/2 stream
(exit 92) and left a truncated 4,240,669,189-byte file, which FAILED the
checksum check. Re-running curl with -C - (resume) + --retry recovered the
file; a mismatch always means re-download, never trust size alone.

### Turbo LoRA (downloaded 2026-09-27, this workstation)

- models/loras/Qwen-Image-2.1-viggle-turbo-v0.2.1-6step-lora-r128.safetensors
  size    679604800
  sha256  bafb91d0047df3f9b8a5a850b0c967f051164314d8aad778dfa34d9c24ec345b
  (source: Viggle/Qwen-Image-2.1-viggle-turbo, matches the X-Linked-Etag the
  resolve endpoint serves, which is the LFS object id)
  Downloaded with: hf download Viggle/Qwen-Image-2.1-viggle-turbo \
    Qwen-Image-2.1-viggle-turbo-v0.2.1-6step-lora-r128.safetensors \
    --local-dir models/loras
  Loaded and applied in full by sd-cli: the run's own report is
  "(454 / 454) LoRA tensors have been applied" with no unused-tensor warning,
  so the diffusers key format needs no conversion tool. Full recipe and
  measurements: docs/viggle-turbo-lora.md.