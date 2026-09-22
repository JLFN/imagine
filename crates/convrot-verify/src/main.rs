//! Decisive correctness check for the int8 convrot text encoder path.
//!
//! Ground truth is the SAME model's weights in BF16 (Comfy-Org/Qwen-Image-2.1,
//! text_encoders/qwen3vl_8b_bf16.safetensors), fetched by HTTP byte range: only
//! the header plus the few tensors under test are downloaded.
//!
//! For each module we reconstruct W from the int8 file the way the patched
//! sd.cpp does (per-row scale, then the normalized regular Hadamard along the
//! row to undo the offline convrot rotation) and compare against the BF16
//! weights. Agreement at int8-quantization level (about 1 percent) proves the
//! dequantization and the rotation direction are right; a wrong rotation would
//! decorrelate completely.
//!
//! This is the Rust port of the experiment's Python script (global guardrail
//! rule 14). The measurement is 1:1 with it: the same ranged fetch, the same
//! safetensors header parse, the same dequantize plus rotation-undo, and the
//! same three reported numbers plus the wrong-rotation control. Two deliberate
//! differences, neither of which affects any measured number:
//!
//!   1. the int8 file is an optional command-line argument, defaulting to the
//!      repository-relative path, instead of a hard-coded absolute path;
//!   2. a verdict line and an exit code were added, so the check can gate a
//!      build instead of only printing numbers for a human to read.

use std::env;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::process::ExitCode;

const BASE: &str = "https://huggingface.co/Comfy-Org/Qwen-Image-2.1/resolve/main/text_encoders/";
const BF16_NAME: &str = "qwen3vl_8b_bf16.safetensors";
const DEFAULT_INT8: &str = "models/text_encoders/qwen3vl_8b_int8_convrot.safetensors";

/// Agreement thresholds for the verdict. The measured values are pearson
/// 0.99996 and rel_l2 0.0086-0.0089; a wrong rotation lands at rel_l2 1.37.
const PASS_PEARSON: f64 = 0.99;
const PASS_REL_L2: f64 = 0.02;

struct Tensor {
    bytes: Vec<u8>,
    dtype: String,
    shape: Vec<u64>,
}

type Hdr = serde_json::Map<String, serde_json::Value>;

/// Ranged HTTP GET, inclusive end offset as in the HTTP Range specification.
fn fetch(url: &str, start: u64, end: u64) -> Result<Vec<u8>, String> {
    let range = format!("bytes={}-{}", start, end - 1);
    let resp = ureq::get(url)
        .set("Range", &range)
        .call()
        .map_err(|e| format!("GET {url} [{range}]: {e}"))?;
    let mut buf = Vec::new();
    resp.into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| format!("read {url} [{range}]: {e}"))?;
    Ok(buf)
}

fn parse_header(raw: &[u8]) -> Result<Hdr, String> {
    let v: serde_json::Value =
        serde_json::from_slice(raw).map_err(|e| format!("safetensors header: {e}"))?;
    let mut map: Hdr = v
        .as_object()
        .ok_or("safetensors header is not a JSON object")?
        .clone();
    map.remove("__metadata__");
    Ok(map)
}

/// Local safetensors header: a u64 little-endian length, then that many bytes of
/// JSON. Returns the tensor map and the absolute offset of the data section.
fn header_local(path: &str) -> Result<(Hdr, u64), String> {
    let mut f = File::open(path).map_err(|e| format!("open {path}: {e}"))?;
    let mut len = [0u8; 8];
    f.read_exact(&mut len)
        .map_err(|e| format!("read header length of {path}: {e}"))?;
    let n = u64::from_le_bytes(len);
    let mut raw = vec![0u8; n as usize];
    f.read_exact(&mut raw)
        .map_err(|e| format!("read header of {path}: {e}"))?;
    Ok((parse_header(&raw)?, 8 + n))
}

/// Remote safetensors header, fetched as two ranges so only the header is
/// downloaded from a multi-gigabyte file.
fn header_remote(url: &str) -> Result<(Hdr, u64), String> {
    let first = fetch(url, 0, 8)?;
    if first.len() != 8 {
        return Err(format!("expected 8 header-length bytes, got {}", first.len()));
    }
    let mut len = [0u8; 8];
    len.copy_from_slice(&first);
    let n = u64::from_le_bytes(len);
    let raw = fetch(url, 8, 8 + n)?;
    Ok((parse_header(&raw)?, 8 + n))
}

fn offsets(hdr: &Hdr, name: &str) -> Result<(u64, u64, String, Vec<u64>), String> {
    let e = hdr.get(name).ok_or_else(|| format!("tensor {name} not in header"))?;
    let offs = e
        .get("data_offsets")
        .and_then(|v| v.as_array())
        .ok_or_else(|| format!("tensor {name}: no data_offsets"))?;
    let s = offs.first().and_then(|v| v.as_u64()).ok_or("bad data_offsets[0]")?;
    let t = offs.get(1).and_then(|v| v.as_u64()).ok_or("bad data_offsets[1]")?;
    let dtype = e
        .get("dtype")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("tensor {name}: no dtype"))?
        .to_string();
    let shape = e
        .get("shape")
        .and_then(|v| v.as_array())
        .ok_or_else(|| format!("tensor {name}: no shape"))?
        .iter()
        .map(|v| v.as_u64().unwrap_or(0))
        .collect();
    Ok((s, t, dtype, shape))
}

fn local_tensor(hdr: &Hdr, base: u64, path: &str, name: &str) -> Result<Tensor, String> {
    let (s, t, dtype, shape) = offsets(hdr, name)?;
    let mut f = File::open(path).map_err(|e| format!("open {path}: {e}"))?;
    f.seek(SeekFrom::Start(base + s))
        .map_err(|e| format!("seek {path}: {e}"))?;
    let mut bytes = vec![0u8; (t - s) as usize];
    f.read_exact(&mut bytes)
        .map_err(|e| format!("read {name} from {path}: {e}"))?;
    Ok(Tensor { bytes, dtype, shape })
}

fn remote_tensor(url: &str, hdr: &Hdr, base: u64, name: &str) -> Result<Tensor, String> {
    let (s, t, dtype, shape) = offsets(hdr, name)?;
    let bytes = fetch(url, base + s, base + t)?;
    Ok(Tensor { bytes, dtype, shape })
}

/// Same transform as ggml_regular_hadamard_group_f32: normalized H4 tensor-power,
/// which is its own inverse, so it undoes the offline convrot rotation.
fn hadamard_group(v: &mut [f32], group_size: usize) {
    let s = 1.0f32 / (group_size as f32).sqrt();
    for x in v.iter_mut() {
        *x *= s;
    }
    let mut stride = 1usize;
    while stride < group_size {
        let mut base = 0usize;
        while base < group_size {
            for j in 0..stride {
                let i0 = base + j;
                let i1 = base + j + stride;
                let i2 = base + j + 2 * stride;
                let i3 = base + j + 3 * stride;
                let (a, b, c, d) = (v[i0], v[i1], v[i2], v[i3]);
                v[i0] = a + b + c - d;
                v[i1] = a + b - c + d;
                v[i2] = a - b + c + d;
                v[i3] = -a + b + c + d;
            }
            base += 4 * stride;
        }
        stride *= 4;
    }
}

fn pearson(a: &[f32], b: &[f32]) -> f64 {
    let n = a.len() as f64;
    let ma = a.iter().map(|&x| x as f64).sum::<f64>() / n;
    let mb = b.iter().map(|&x| x as f64).sum::<f64>() / n;
    let (mut num, mut da, mut db) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..a.len() {
        let x = a[i] as f64 - ma;
        let y = b[i] as f64 - mb;
        num += x * y;
        da += x * x;
        db += y * y;
    }
    num / (da.sqrt() * db.sqrt())
}

fn rel_l2_and_max_abs(mine: &[f32], bf: &[f32]) -> (f64, f32) {
    let (mut dnorm, mut bnorm, mut maxabs) = (0.0f64, 0.0f64, 0.0f32);
    for i in 0..mine.len() {
        let d = mine[i] - bf[i];
        dnorm += (d as f64) * (d as f64);
        bnorm += (bf[i] as f64) * (bf[i] as f64);
        maxabs = maxabs.max(d.abs());
    }
    ((dnorm / bnorm).sqrt(), maxabs)
}

/// One module: reconstruct the int8 weights, compare against BF16, and print the
/// same numbers the Python baseline printed. Returns (pearson, rel_l2, control).
fn check(
    name: &str,
    int8_hdr: &Hdr,
    int8_base: u64,
    int8_path: &str,
    bf16_url: &str,
    bf16_hdr: &Hdr,
    bf16_base: u64,
    rows: usize,
    group_size: usize,
) -> Result<(f64, f64, f64), String> {
    let q = local_tensor(int8_hdr, int8_base, int8_path, name)?;
    if q.dtype != "I8" {
        return Err(format!("{name}: expected I8, header says {}", q.dtype));
    }
    let sc_name = format!("{}.weight_scale", &name[..name.len() - ".weight".len()]);
    let sc = local_tensor(int8_hdr, int8_base, int8_path, &sc_name)?;
    let scales: Vec<f32> = sc
        .bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();

    if q.shape.len() != 2 {
        return Err(format!("{name}: expected a 2-D weight, got shape {:?}", q.shape));
    }
    let height = q.shape[0] as usize;
    let width = q.shape[1] as usize;
    if width % group_size != 0 {
        return Err(format!(
            "{name}: width {width} is not a multiple of the hadamard group {group_size}"
        ));
    }

    // BF16 ground truth: only the first rows are needed, which are a contiguous
    // prefix in the row-major layout. BF16 is the top half of the fp32 bit
    // pattern, so shifting the 16 bits up converts exactly.
    let b = remote_tensor(bf16_url, bf16_hdr, bf16_base, name)?;
    let bshape_w = b.shape.get(1).copied().unwrap_or(0) as usize;
    let take = rows.min(b.shape.first().copied().unwrap_or(0) as usize);
    if bshape_w != width {
        return Err(format!(
            "{name}: int8 width {width} does not match the BF16 width {bshape_w}"
        ));
    }
    let bf: Vec<f32> = b.bytes[..take * bshape_w * 2]
        .chunks_exact(2)
        .map(|c| {
            let u = u16::from_le_bytes([c[0], c[1]]);
            f32::from_bits((u as u32) << 16)
        })
        .collect();

    let mut mine = vec![0.0f32; take * bshape_w];
    let mut norot = vec![0.0f32; take * bshape_w];
    for r in 0..take {
        if r >= height {
            return Err(format!("{name}: row {r} beyond the int8 table height {height}"));
        }
        let scale = scales.get(r).copied().unwrap_or(1.0);
        let qrow = &q.bytes[r * width..r * width + bshape_w];
        let row = &mut mine[r * bshape_w..(r + 1) * bshape_w];
        let ctl = &mut norot[r * bshape_w..(r + 1) * bshape_w];
        for c in 0..bshape_w {
            let v = (qrow[c] as i8) as f32 * scale;
            row[c] = v;
            ctl[c] = v;
        }
        let mut g = 0;
        while g < bshape_w {
            hadamard_group(&mut row[g..g + group_size], group_size);
            g += group_size;
        }
    }

    let (rel_l2, max_abs) = rel_l2_and_max_abs(&mine, &bf);
    let p = pearson(&mine, &bf);
    let control = rel_l2_and_max_abs(&norot, &bf).0;

    println!("{name}");
    println!(
        "  shape [{height}, {width}] scales {} rows checked {take}",
        scales.len()
    );
    println!("  pearson={p:.6} rel_l2={rel_l2:.4} max_abs={max_abs:.4}");
    println!("  (control, rotation NOT undone: rel_l2={control:.4})");
    Ok((p, rel_l2, control))
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let int8_path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| DEFAULT_INT8.to_string());
    let bf16_url = format!("{BASE}{BF16_NAME}");

    let run = || -> Result<bool, String> {
        let (i8_hdr, i8_base) = header_local(&int8_path)?;
        let (bf_hdr, bf_base) = header_remote(&bf16_url)?;
        println!(
            "int8 tensors: {}  bf16 tensors: {}",
            i8_hdr.len(),
            bf_hdr.len()
        );
        println!();

        let mut pass = true;
        for (name, rows) in [
            ("model.layers.0.self_attn.q_proj.weight", 8usize),
            ("model.embed_tokens.weight", 4usize),
        ] {
            let (p, rel, _) = check(
                name,
                &i8_hdr,
                i8_base,
                &int8_path,
                &bf16_url,
                &bf_hdr,
                bf_base,
                rows,
                256,
            )?;
            if !(p >= PASS_PEARSON && rel <= PASS_REL_L2) {
                pass = false;
            }
            println!();
        }
        Ok(pass)
    };

    match run() {
        Ok(true) => {
            println!(
                "verdict: PASS (pearson >= {PASS_PEARSON}, rel_l2 <= {PASS_REL_L2} on every tensor checked)"
            );
            ExitCode::SUCCESS
        }
        Ok(false) => {
            println!("verdict: FAIL (a tensor disagreed with its BF16 ground truth)");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
