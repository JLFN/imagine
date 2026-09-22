# Model manifest — Qwen-Image-2.1 local (pinned downloads)

Sizes verified from the Hugging Face repo pages on 2026-09-21. Files are
gitignored; download them into models/ (mkdir models) and verify checksums
before first use. The undirected authority is the SHA256SUMS file in each
source repo; always recompute and compare, never trust the sizes alone.

## 1. Diffusion model (the image generator)

- File:         models/diffusion_models/qwen-image-2.1-Q4_K_M.gguf
- Size:         4.6 GiB
- Source:       https://huggingface.co/abenzerps/Qwen-Image-2.1-GGUF
                (same bytes also at KasugaiSakura/Qwen-Image-2.1-Uncensored-GGUF)
- Checksums:    SHA256SUMS in the source repo
- Note:         GGUF of the 7B DiT; converted for stable-diffusion.cpp.
                Q4_K_M recommended by the converter.

## 2. Text encoder (fixed architectural component)

- File:         models/text_encoders/Qwen3VL-8B-Instruct-Q4_K_M.gguf
- Size:         5.03 GB
- Source:       https://huggingface.co/Qwen/Qwen3-VL-8B-Instruct-GGUF
- Checksums:    repo page
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
- models/text_encoders/qwen3vl_8b_int8_convrot.safetensors   (deployed default TE)
  size    9350798360
  sha256  8bfd0f6e12abf2d2d697ecc888e5e90b0d6741d6708f05799f53afa560452e8f
  (source: Comfy-Org/Qwen-Image-2.1 text_encoders/ HF LFS oid; the same bytes are
  published as KasugaiSakura/Qwen-Image-2.1-Uncensored-GGUF
  text_encoders/qwen3vl_8b_int8_convrot.safetensors, verified against both)
  REQUIRES the patched sd-server built from patches/0001 and patches/0002: stock
  sd.cpp misreads this file's geometry at load and then cannot dequantize its
  int8 token embedding. See docs/convrot-encoder-experiment.md. Reports
  10,104.77 MB of parameters at load against the GGUF encoder's 4,302.33 MB, so it
  costs about 5.8 GB more host RAM for the same VRAM peak; the GGUF encoder above
  remains the lighter option and needs no patches.
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