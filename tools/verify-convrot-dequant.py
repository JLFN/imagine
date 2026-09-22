#!/usr/bin/env python3
"""Decisive correctness check for the int8 convrot text encoder path.

Ground truth is the SAME model's weights in BF16 (Comfy-Org/Qwen-Image-2.1,
text_encoders/qwen3vl_8b_bf16.safetensors), fetched by HTTP byte range: only the
header plus the few tensors under test are downloaded.

For each module we reconstruct W from the int8 file the way the patched sd.cpp
does (per-row scale, then the normalized regular Hadamard along the row to undo
the offline convrot rotation) and compare against the BF16 weights. Agreement at
int8-quantization level (about 1 percent) proves the dequantization and the
rotation direction are right; a wrong rotation would decorrelate completely.
"""
import json, struct, math, sys, urllib.request
import numpy as np

BASE = "https://huggingface.co/Comfy-Org/Qwen-Image-2.1/resolve/main/text_encoders/"
INT8 = "/data/imagegen/models/text_encoders/qwen3vl_8b_int8_convrot.safetensors"
BF16_URL = BASE + "qwen3vl_8b_bf16.safetensors"

def fetch(url, start, end):
    req = urllib.request.Request(url, headers={"Range": f"bytes={start}-{end-1}"})
    with urllib.request.urlopen(req, timeout=120) as r:
        return r.read()

def header(path_or_url, is_url=False):
    if is_url:
        first = fetch(path_or_url, 0, 8)
        n = struct.unpack("<Q", first)[0]
        raw = fetch(path_or_url, 8, 8 + n)
    else:
        with open(path_or_url, "rb") as f:
            n = struct.unpack("<Q", f.read(8))[0]
            raw = f.read(n)
    hdr = json.loads(raw)
    meta = hdr.pop("__metadata__", None)
    return hdr, 8 + n

def local_tensor(hdr, base, path, name):
    e = hdr[name]; s, t = e["data_offsets"]
    with open(path, "rb") as f:
        f.seek(base + s); return f.read(t - s), e["dtype"], e["shape"]

def remote_tensor(url, hdr, base, name):
    e = hdr[name]; s, t = e["data_offsets"]
    return fetch(url, base + s, base + t), e["dtype"], e["shape"]

def hadamard_group(values, group_size):
    """Same transform as ggml_regular_hadamard_group_f32: normalized H4^⊗n, and
    its own inverse, so it undoes the offline rotation."""
    v = values.copy()
    v *= 1.0 / math.sqrt(group_size)
    stride = 1
    while stride < group_size:
        for base in range(0, group_size, 4 * stride):
            for j in range(stride):
                i0, i1, i2, i3 = base + j, base + j + stride, base + j + 2*stride, base + j + 3*stride
                a, b, c, d = v[i0], v[i1], v[i2], v[i3]
                v[i0] = a + b + c - d
                v[i1] = a + b - c + d
                v[i2] = a - b + c + d
                v[i3] = -a + b + c + d
        stride *= 4
    return v

def check(name, int8_hdr, int8_base, bf16_hdr, bf16_base, rows=4, group_size=256):
    q, _, qshape = local_tensor(int8_hdr, int8_base, INT8, name)
    sc_name = name[:-len(".weight")] + ".weight_scale"
    sc_raw, _, scshape = local_tensor(int8_hdr, int8_base, INT8, sc_name)
    scales = np.frombuffer(sc_raw, dtype="<f4")
    qi = np.frombuffer(q, dtype=np.int8).reshape(qshape[0], qshape[1])

    # BF16 ground truth: for the big embedding table fetch only the first rows,
    # which is a contiguous prefix in row-major layout. BF16 is the top half of
    # the fp32 bit pattern, so shifting the 16 bits up converts exactly.
    b, _, bshape = remote_tensor(BF16_URL, bf16_hdr, bf16_base, name)
    take = min(rows, bshape[0])
    raw = np.frombuffer(b[:take * bshape[1] * 2], dtype="<u2")
    bf = (raw.astype(np.uint32) << 16).view(np.float32).reshape(take, bshape[1])

    mine = np.empty_like(bf)
    for r in range(take):
        row = qi[r].astype(np.float32) * (scales[r] if r < scales.size else 1.0)
        for g in range(0, bshape[1], group_size):
            row[g:g + group_size] = hadamard_group(row[g:g + group_size], group_size)
        mine[r] = row
    d = mine - bf
    print(f"{name}")
    print(f"  shape {qshape} scales {scales.size} rows checked {take}")
    print(f"  pearson={np.corrcoef(mine.ravel(), bf.ravel())[0,1]:.6f} "
          f"rel_l2={np.linalg.norm(d)/np.linalg.norm(bf):.4f} "
          f"max_abs={np.abs(d).max():.4f}")
    # What a WRONG rotation looks like, for scale: skip the undo.
    norot = np.empty_like(bf)
    for r in range(take):
        norot[r] = qi[r].astype(np.float32) * (scales[r] if r < scales.size else 1.0)
    print(f"  (control, rotation NOT undone: rel_l2={np.linalg.norm(norot-bf)/np.linalg.norm(bf):.4f})")

i8_hdr, i8_base = header(INT8)
bf_hdr, bf_base = header(BF16_URL, is_url=True)
print("int8 tensors:", len(i8_hdr), " bf16 tensors:", len(bf_hdr))
print()
check("model.layers.0.self_attn.q_proj.weight", i8_hdr, i8_base, bf_hdr, bf_base, rows=8)
print()
check("model.embed_tokens.weight", i8_hdr, i8_base, bf_hdr, bf_base, rows=4)
