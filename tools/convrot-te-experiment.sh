#!/bin/bash
# Measure the int8 convrot text encoder against the shipped GGUF encoder, both
# with the text encoder's parameters in RAM and its graph on the CPU.
#
# Baseline and candidate differ ONLY in the --llm file, so any quality or
# timing difference is attributable to the encoder.
#
# Needs exclusive GPU use: the production bridge pins 5.6 GB of Q6_K weights in
# VRAM. Stop it first (kill -INT on bin/imagegen-bridge) and restart after.
#
# Usage: bash tools/convrot-te-experiment.sh [outdir] [binary]
set -u

REPO=/data/imagegen
OUT=${1:-/data/build-sd/te-split/convrot-experiment}
BIN=${2:-/data/build-sd/stable-diffusion.cpp/build/bin/sd-cli}

DIFF=${IMAGEGEN_DIFFUSION:-$REPO/models/qwen-image-2.1-Q6_K.gguf}
VAE=$REPO/models/qwen_image_2.1_vae_bf16.safetensors
ENCODER_GGUF=$REPO/models/Qwen3VL-8B-Instruct-Q4_K_M.gguf
ENCODER_CONVROT=$REPO/models/text_encoders/qwen3vl_8b_int8_convrot.safetensors
PROMPT=${PROMPT:-"a lovely cat holding a sign says 'qwen2.1.cpp'"}

# Encoder parameters in RAM, graph on CPU; DiT pinned in VRAM; VAE params in RAM.
PLACEMENT=(--backend te=cpu,diffusion=cuda0,vae=cuda0
           --params-backend te=cpu,diffusion=cuda0,vae=cpu)

mkdir -p "$OUT"
cd "$REPO" || exit 1

if pgrep -x imagegen-bridge >/dev/null 2>&1; then
  echo "REFUSING TO RUN: the production bridge is up and holds VRAM pinned."
  echo "Stop it first with:  kill -INT \$(pgrep -x imagegen-bridge)"
  exit 1
fi

run_one() {
  local tag=$1 encoder=$2
  echo "===== $tag (encoder: $encoder) ====="
  : > "$OUT/vram-$tag.log"
  ( for _ in $(seq 1 900); do
      nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits >> "$OUT/vram-$tag.log" 2>/dev/null
      sleep 1
    done ) &
  local poller=$!
  /usr/bin/time -v "$BIN" \
    --diffusion-model "$DIFF" --vae "$VAE" --llm "$encoder" \
    -p "$PROMPT" --cfg-scale 6.0 --sampling-method euler -v \
    "${PLACEMENT[@]}" \
    -W 1024 -H 1024 --steps 28 --seed 42 -o "$OUT/$tag.png" > "$OUT/$tag.log" 2>&1
  local rc=$?
  kill "$poller" 2>/dev/null; wait "$poller" 2>/dev/null
  echo "exit=$rc"
  grep -E "total params memory size" "$OUT/$tag.log"
  grep -iE "get_learned_condition completed|sampling completed|decode_first_stage completed|generate_image completed|Maximum resident set size|Elapsed \(wall" "$OUT/$tag.log"
  grep -iE "assert|aborted|error|failed" "$OUT/$tag.log" | head -3
  echo "peak VRAM MiB: $(sort -n "$OUT/vram-$tag.log" 2>/dev/null | tail -1)"
  ls -l "$OUT/$tag.png" 2>/dev/null || echo "NO IMAGE"
  echo
}

run_one baseline "$ENCODER_GGUF"
run_one candidate "$ENCODER_CONVROT"

echo "===== comparison ====="
for tag in baseline candidate; do
  printf '%-10s total=%s sampling=%s peakVRAM=%s rss=%s\n' "$tag" \
    "$(grep -oE 'generate_image completed in [0-9.]+s' "$OUT/$tag.log" | grep -oE '[0-9.]+')" \
    "$(grep -oE 'sampling completed, taking [0-9.]+s' "$OUT/$tag.log" | grep -oE '[0-9.]+')" \
    "$(sort -n "$OUT/vram-$tag.log" 2>/dev/null | tail -1)" \
    "$(grep -oE 'Maximum resident set size \(kbytes\): [0-9]+' "$OUT/$tag.log" | grep -oE '[0-9]+')"
done
