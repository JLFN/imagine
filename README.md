imagegen — local image-generation partner for open-grok

WHAT THIS PROJECT IS

A recipe plus implementation to give open-grok a fully local image-generation
endpoint on this workstation (Intel i7-14700F, 31 GB RAM, RTX 4070 SUPER
12 GB VRAM, Linux, 411 GB free disk), so the open-grok chat can create images
without the xAI API and without the NVIDIA Spark playing any part.

Components:
- Runner: stable-diffusion.cpp (pure C/C++ on ggml; day-0 Qwen-Image-2.1
  support, added 2026-09-20). Not llama.cpp and not ds4.
- Model: Qwen-Image-2.1 Q4_K_M GGUF (4.6 GiB) plus Qwen3-VL-8B-Instruct
  Q4_K_M encoder (5.03 GB) plus the Qwen-Image-2.1 VAE (0.64 GiB).
  Total about 10.3 GiB on disk.
- Bridge: a small Rust HTTP server exposing the OpenAI-compatible
  /v1/images/generations route. It spawns the sd-cli runner on demand and
  reaps it after an idle window, so the ~6-7 GB model memory is zero when
  nothing is being generated.
- Wiring: a [toolset.image_gen] block in ~/.opengrok/config.toml that points
  open-grok's built-in image tools at the local bridge.

STATUS

Scaffold only (git repo initialized on branch dev, no remote). The
executable plan lives in imagegen-handoff.md — read that first. Work here
proceeds in phases (runner, model download, CLI smoke, bridge, wiring,
QA/push). Every claim in this project must be verified from source; several
are already pinned below from the 2026-09-21 setup session.

VERIFIED CONSTRAINTS (from the setup session; re-verify before relying)

1. ds4 cannot generate images. Its only image paths are vision encoders
   (input); the image_generation_call strings in ds4_server.c are historical
   tool-call replay for Codex clients ("DS4 doesn't register these tools").
   Image generation needs a diffusion pipeline.
2. llama.cpp cannot run this model. Its "diffusion" support (LLaDA, Dream)
   is text diffusion. The GGUF at abenzerps/Qwen-Image-2.1-GGUF is converted
   with, and meant for, stable-diffusion.cpp / ComfyUI-GGUF.
3. The text encoder is a fixed architectural component: Qwen3-VL-8B-Instruct.
   It cannot be swapped for the Qwen3.8 Flash Next model served by ds4 on
   the Spark. Use the Q4_K_M GGUF (5.03 GB) from Qwen/Qwen3-VL-8B-Instruct-GGUF.
4. open-grok [toolset.image_gen] activates only with a non-empty base_url
   plus a resolvable api_key or env_key. response_encoding "auto" accepts
   b64_json and url responses. The configured key always authenticates a
   third-party endpoint; the session bearer is never sent to it.
5. On-demand serving is possible two ways here: a tiny bridge daemon that
   spawns/reaps sd-cli (recommended), or systemd user socket activation
   (the user systemd session is running — verified).

RESOURCES

- Intel i7-14700F, 28 threads; 31 GB RAM (~15 GB free typical); RTX 4070
  SUPER 12 GB VRAM; NVMe SSD. 12 GB VRAM fits the Q4_K_M diffusion model
  with the encoder offloaded to system RAM. Native 2K is tight; ~1024-1328
  output is the comfortable default.
- The Spark (gx10-e1d2, 192.168.1.91) keeps running the Qwen 3.8 text brain;
  image generation is intentionally local to this workstation.

REPO LAYOUT

- imagegen-handoff.md   hook-managed handoff + executable plan (read first)
- README.md             this file
- AGENTS.md             agent instructions for this project
- docs/architecture.md  component design and wiring details
- models/MANIFEST.md    pinned model files, URLs, sizes, checksums (files not committed)
- crates/bridge/        the Rust bridge server (not yet written)
- bin/                  sd-cli runner binary (gitignored)
- models/               the three model files (gitignored)
- out/                  generated test images (gitignored)