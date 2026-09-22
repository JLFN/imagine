# Model manifest — Qwen-Image-2.1 local (pinned downloads)

Sizes verified from the Hugging Face repo pages on 2026-09-21. Files are
gitignored; download them into models/ (mkdir models) and verify checksums
before first use. The undirected authority is the SHA256SUMS file in each
source repo; always recompute and compare, never trust the sizes alone.

## 1. Diffusion model (the image generator)

- File:         models/qwen-image-2.1-Q4_K_M.gguf
- Size:         4.6 GiB
- Source:       https://huggingface.co/abenzerps/Qwen-Image-2.1-GGUF
                (same bytes also at KasugaiSakura/Qwen-Image-2.1-Uncensored-GGUF)
- Checksums:    SHA256SUMS in the source repo
- Note:         GGUF of the 7B DiT; converted for stable-diffusion.cpp.
                Q4_K_M recommended by the converter.

## 2. Text encoder (fixed architectural component)

- File:         models/Qwen3VL-8B-Instruct-Q4_K_M.gguf
- Size:         5.03 GB
- Source:       https://huggingface.co/Qwen/Qwen3-VL-8B-Instruct-GGUF
- Checksums:    repo page
- Note:         Qwen-Image-2.1 conditions on Qwen3-VL-8B-Instruct exactly.
                It CANNOT be substituted with the Qwen3.8 Flash Next model
                served by ds4 on the Spark (different embedding space). Try
                any other encoder and generations are incoherent.

## 3. VAE (decoder)

- File:         models/qwen_image_2.1_vae_bf16.safetensors
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
  curl -L -o models/qwen-image-2.1-Q4_K_M.gguf \
    https://huggingface.co/abenzerps/Qwen-Image-2.1-GGUF/resolve/main/qwen-image-2.1-Q4_K_M.gguf
  curl -L -o models/Qwen3VL-8B-Instruct-Q4_K_M.gguf \
    https://huggingface.co/Qwen/Qwen3-VL-8B-Instruct-GGUF/resolve/main/Qwen3VL-8B-Instruct-Q4_K_M.gguf
  curl -L -o models/qwen_image_2.1_vae_bf16.safetensors \
    https://huggingface.co/Comfy-Org/Qwen-Image-2.1/resolve/main/vae/qwen_image_2.1_vae_bf16.safetensors

## Total

About 10.3 GiB on disk. Runtime: diffusion model in VRAM (fits 12 GB),
encoder in system RAM (~5 GB), VAE small.

## Verified downloads (2026-09-21, this workstation)

Downloaded with the curl commands above and verified against the published
SHA256SUMS / HF LFS oids. sha256sum and ls -l output below:

- models/qwen-image-2.1-Q4_K_M.gguf
  size    4604557984
  sha256  833439e91bc1152d28f37aa198c7f6f4218b7de95754c2f7a318a2422ab4b2f8
  (source: abenzerps/Qwen-Image-2.1-GGUF SHA256SUMS; matches HF LFS oid)
- models/Qwen3VL-8B-Instruct-Q4_K_M.gguf
  size    5027784800
  sha256  67d1659bfe71b89d50b45a4ad1a9e5b997e5bb16ce5da66a6a6167abd569e9e2
  (source: Qwen/Qwen3-VL-8B-Instruct-GGUF HF LFS oid)
- models/qwen_image_2.1_vae_bf16.safetensors
  size    675509688
  sha256  bb21f7473051e1ac368515dd3f2e15cd44d7a11748ee8823e1ddca3e4876b7c9
  (source: abenzerps/Qwen-Image-2.1-GGUF SHA256SUMS; matches HF LFS oid)
- models/qwen-image-2.1-Q6_K.gguf   (higher-quality diffusion option)
  size    5876556448
  sha256  a3a0d39bb03cda26302fc048b49d019baaea1c381cbda3994f6f2a7826344fb9
  (source: abenzerps/Qwen-Image-2.1-GGUF SHA256SUMS; matches HF LFS oid)
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
  arrangement; resolution up to ~1344 (or ~1536 on one axis) is UNVERIFIED
  under the pinned default. Q8_0 also exists on disk but is reserved for when
  the VRAM/RAM budget allows; it is not the default.

Note: an initial curl for the diffusion model hit a flaky HTTP/2 stream
(exit 92) and left a truncated 4,240,669,189-byte file, which FAILED the
checksum check. Re-running curl with -C - (resume) + --retry recovered the
file; a mismatch always means re-download, never trust size alone.