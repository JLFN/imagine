// imagegen-bridge: a thin OpenAI image API server that supervises
// stable-diffusion.cpp's sd-server backend.
//
// open-grok points [toolset.image_gen] at this bridge
// (http://127.0.0.1:8787/v1). The bridge spawns sd-server once (warm, so
// the ~9.3 GB model stack stays resident and later requests avoid the
// ~35 s cold start), translates the OpenAI `images/generations` request
// into sd-server's explicit `/sdapi/v1/txt2img` body (so fields such as
// num_inference_steps / seed / cfg_scale are honored, which the OpenAI
// endpoint would ignore), and wraps the PNGs back into the OpenAI
// {data:[{b64_json}]} shape.
//
// Configuration is read from the environment (defaults match this repo's
// layout and the measured Unit 1 invocation):
//   IMAGEGEN_SD_SERVER       path to sd-server           (bin/sd-server)
//   IMAGEGEN_DIFFUSION       path to diffusion model     (models/qwen-image-2.1-Q6_K.gguf)
//   IMAGEGEN_VAE             path to vae                 (models/qwen_image_2.1_vae_bf16.safetensors)
//   IMAGEGEN_LLM             path to text encoder        (models/Qwen3VL-8B-Instruct-Q4_K_M.gguf)
//   IMAGEGEN_BACKEND_ARGS    sd-server placement flags   (--backend te=cpu,diffusion=cuda0,vae=cuda0
//                                                         --params-backend te=cpu,diffusion=cuda0,vae=cpu)
//   IMAGEGEN_LISTEN_IP       bridge listen IP            (127.0.0.1)
//   IMAGEGEN_LISTEN_PORT     bridge port                 (8787)
//   IMAGEGEN_BACKEND_PORT    internal sd-server port     (8788)
//   IMAGEGEN_STEPS           default sampling steps      (28)
//   IMAGEGEN_CFG_SCALE       default CFG scale           (6.0)
//
// Placement rationale (measured 2026-09-22 on this box, 1024x1024 / 28 steps /
// seed 42, same prompt throughout, RTX 4070 SUPER 12 GB; peak VRAM from a 1 s
// nvidia-smi poll, host RSS from /usr/bin/time -v):
//   --offload-to-cpu, Q4_K_M: sampling 131.5 s, VAE decode 2.1 s untiled,
//     peak VRAM 11,572 MiB, host RSS 10.6 GiB
//   DiT pinned + encoder on CPU, Q4_K_M: sampling 132.9 s, decode 4.4 s tiled,
//     peak VRAM 8,172 MiB, host RSS 6.2 GiB
//   everything pinned including the encoder: cannot complete a generation at
//     all (the diffusion model runs out of memory during sampling)
//   this default (Q6_K pinned, encoder on CPU, VAE parameters in RAM):
//     sampling 146.5 s, decode 4.7 s tiled, total 153.9 s, peak VRAM 9,426 MiB
// So the split holds ~4.4 GiB less host RAM and ~3.4 GiB less of the VRAM peak
// at the same sampling speed, at the cost of the VAE decode falling back to
// spatial tiling above a small resolution (4.4-4.7 s at 1024 square; at 512
// square the same layout decoded untiled in 1.43 s): the pinned DiT weights
// are what starve it, not the VAE's own (keeping them in RAM does not avoid
// the fallback). See docs/architecture.md.

use std::process::{Child, Command};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tracing::{error, info, warn};

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Config {
    sd_server: String,
    diffusion: String,
    vae: String,
    llm: String,
    listen_ip: String,
    listen_port: u16,
    backend_port: u16,
    steps: u32,
    cfg_scale: f64,
    backend_args: Vec<String>,
}

fn env_str(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Whitespace-separated argument list, so one env var can carry a flag pair
/// such as `--backend te=cpu,diffusion=cuda0` (each token stays one argv entry).
fn env_args(key: &str, default: &str) -> Vec<String> {
    env_str(key, default)
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn env_u16(key: &str, default: u16) -> u16 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_f64(key: &str, default: f64) -> f64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

impl Config {
    fn from_env() -> Self {
        Config {
            sd_server: env_str("IMAGEGEN_SD_SERVER", "bin/sd-server"),
            diffusion: env_str(
                "IMAGEGEN_DIFFUSION",
                "models/qwen-image-2.1-Q6_K.gguf",
            ),
            vae: env_str("IMAGEGEN_VAE", "models/qwen_image_2.1_vae_bf16.safetensors"),
            llm: env_str(
                "IMAGEGEN_LLM",
                "models/Qwen3VL-8B-Instruct-Q4_K_M.gguf",
            ),
            listen_ip: env_str("IMAGEGEN_LISTEN_IP", "127.0.0.1"),
            listen_port: env_u16("IMAGEGEN_LISTEN_PORT", 8787),
            backend_port: env_u16("IMAGEGEN_BACKEND_PORT", 8788),
            steps: env_u32("IMAGEGEN_STEPS", 28),
            cfg_scale: env_f64("IMAGEGEN_CFG_SCALE", 6.0),
            backend_args: env_args(
                "IMAGEGEN_BACKEND_ARGS",
                "--backend te=cpu,diffusion=cuda0,vae=cuda0 \
                 --params-backend te=cpu,diffusion=cuda0,vae=cpu",
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Backend process supervision
// ---------------------------------------------------------------------------

fn spawn_backend(cfg: &Config) -> Child {
    info!(
        "starting sd-server backend on 127.0.0.1:{} ({} / {} / {})",
        cfg.backend_port, cfg.diffusion, cfg.llm, cfg.vae
    );
    info!(
        "backend placement args: {}",
        cfg.backend_args.join(" ")
    );
    Command::new(&cfg.sd_server)
        .arg("--diffusion-model")
        .arg(&cfg.diffusion)
        .arg("--vae")
        .arg(&cfg.vae)
        .arg("--llm")
        .arg(&cfg.llm)
        .arg("--listen-ip")
        .arg("127.0.0.1")
        .arg("--listen-port")
        .arg(cfg.backend_port.to_string())
        .args(&cfg.backend_args)
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn sd-server at {}: {}", cfg.sd_server, e))
}

async fn wait_backend_ready(cfg: &Config, client: &reqwest::Client) -> Result<(), String> {
    let url = format!("http://127.0.0.1:{}/v1/models", cfg.backend_port);
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        match client.get(&url).send().await {
            Ok(r) if r.status().is_success() => return Ok(()),
            Ok(r) => warn!("backend not ready ({}), retrying", r.status()),
            Err(e) => warn!("backend not reachable ({}), retrying", e),
        }
        if Instant::now() >= deadline {
            return Err("timed out waiting for sd-server backend".into());
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

// ---------------------------------------------------------------------------
// Shared application state
// ---------------------------------------------------------------------------

struct AppState {
    client: reqwest::Client,
    cfg: Config,
    backend_base: String,
}

// ---------------------------------------------------------------------------
// Endpoints
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct GenRequest {
    prompt: String,
    n: Option<u32>,
    size: Option<String>,
    #[serde(default)]
    aspect_ratio: Option<String>,
    #[serde(default)]
    resolution: Option<String>,
    num_inference_steps: Option<u32>,
    seed: Option<i64>,
    cfg_scale: Option<f64>,
}

fn parse_size(size: &str) -> Result<(u32, u32), String> {
    let (w, h) = size
        .split_once('x')
        .ok_or_else(|| format!("invalid size '{}', expected WIDTHxHEIGHT", size))?;
    let w: u32 = w.trim().parse().map_err(|_| format!("invalid width in '{}'", size))?;
    let h: u32 = h.trim().parse().map_err(|_| format!("invalid height in '{}'", size))?;
    if w == 0 || h == 0 {
        return Err(format!("size '{}' must be positive", size));
    }
    if w % 32 != 0 || h % 32 != 0 {
        return Err(format!(
            "size '{}' is not supported: width and height must be divisible by 32",
            size
        ));
    }
    Ok((w, h))
}

/// Resolution tier -> long-edge pixels. open-grok sends "1k" by default.
fn resolution_base(resolution: &str) -> u32 {
    let r = resolution.trim().to_ascii_lowercase();
    if r.starts_with("2k") || r == "1080p" {
        2048
    } else if r.starts_with("4k") || r == "2160p" {
        3840
    } else {
        1024
    }
}

/// Map an aspect ratio string ("16:9", "1:1", "3:2", ...) and a resolution
/// tier to a WIDTHxHEIGHT that is divisible by 32 and stays within the long
/// edge given by the resolution. Falls back to a square at the resolution
/// base when the ratio cannot be parsed. This honors open-grok's
/// aspect_ratio + resolution fields (it does not send a `size`).
fn dims_from_aspect(aspect_ratio: &str, resolution: &str) -> (u32, u32) {
    let long = resolution_base(resolution);
    let snap32 = |v: u32| -> u32 { ((v + 15) / 32).max(1) * 32 };
    let base = long; // use the resolution tier as the long edge
    let mut parts = aspect_ratio.splitn(2, ':');
    let (Some(arw), Some(arh)) = (parts.next(), parts.next()) else {
        return (base, base);
    };
    let (Ok(arw), Ok(arh)) = (arw.trim().parse::<f64>(), arh.trim().parse::<f64>()) else {
        return (base, base);
    };
    if arw <= 0.0 || arh <= 0.0 {
        return (base, base);
    }
    // landscape (ratio >= 1): width is the long edge
    if arw >= arh {
        let w = snap32(base);
        let h = snap32(((base as f64) * (arh / arw)).round() as u32).max(32);
        (w, h)
    } else {
        let h = snap32(base);
        let w = snap32(((base as f64) * (arw / arh)).round() as u32).max(32);
        (w, h)
    }
}

async fn get_models(State(st): State<std::sync::Arc<AppState>>) -> Json<serde_json::Value> {
    let _ = &st; // model id is fixed for this single-model bridge
    Json(serde_json::json!({
        "data": [{ "id": "Qwen-Image-2.1", "object": "model", "owned_by": "local" }]
    }))
}

async fn health(State(st): State<std::sync::Arc<AppState>>) -> Json<serde_json::Value> {
    let url = format!("{}/v1/models", st.backend_base);
    let backend_ok = st
        .client
        .get(&url)
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false);
    Json(serde_json::json!({ "status": "ok", "backend": backend_ok }))
}

async fn create_generation(
    State(st): State<std::sync::Arc<AppState>>,
    body: String,
) -> (StatusCode, Json<serde_json::Value>) {
    let req: GenRequest = match serde_json::from_str(&body) {
        Ok(req) => req,
        Err(e) => return err(StatusCode::BAD_REQUEST, &format!("invalid request body: {}", e)),
    };

    if req.prompt.trim().is_empty() {
        return err(StatusCode::BAD_REQUEST, "prompt is required");
    }

    let (width, height) = match &req.size {
        Some(s) => match parse_size(s) {
            Ok(wh) => wh,
            Err(msg) => return err(StatusCode::BAD_REQUEST, &msg),
        },
        None => {
            // open-grok sends aspect_ratio + resolution instead of size.
            let ar = req.aspect_ratio.as_deref().unwrap_or("1:1");
            let res = req.resolution.as_deref().unwrap_or("1k");
            dims_from_aspect(ar, res)
        }
    };

    let n = req.n.unwrap_or(1);
    if n != 1 {
        return err(
            StatusCode::BAD_REQUEST,
            "n must be 1 (multi-image batches are not supported)",
        );
    }
    let steps = req.num_inference_steps.unwrap_or(st.cfg.steps);
    let cfg_scale = req.cfg_scale.unwrap_or(st.cfg.cfg_scale);
    // seed < 0 (or omitted) means random, matching sd-server's convention.
    let seed = req.seed.unwrap_or(-1);

    let body = serde_json::json!({
        "prompt": req.prompt,
        "negative_prompt": "",
        "width": width,
        "height": height,
        "steps": steps,
        "cfg_scale": cfg_scale,
        "seed": seed,
        "batch_size": n,
    });

    info!("generating {} image(s) at {}x{} steps={} cfg={} seed={}", n, width, height, steps, cfg_scale, seed);

    let started = Instant::now();
    let resp = st
        .client
        .post(format!("{}/sdapi/v1/txt2img", st.backend_base))
        .json(&body)
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            error!("backend request failed: {}", e);
            return err(
                StatusCode::BAD_GATEWAY,
                &format!("backend unreachable: {}", e),
            );
        }
    };

    let status = resp.status();
    let resp_text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        warn!("backend returned {}: {}", status, resp_text);
        return err(
            StatusCode::BAD_GATEWAY,
            &format!("backend generation failed ({}): {}", status, resp_text),
        );
    }

    let value: serde_json::Value =
        match serde_json::from_str(&resp_text) {
            Ok(v) => v,
            Err(e) => {
                error!("backend returned unparseable body: {}", e);
                return err(StatusCode::BAD_GATEWAY, "backend returned an invalid response");
            }
        };

    let images = match value.get("images").and_then(|a| a.as_array()) {
        Some(arr) => arr,
        None => {
            warn!("backend response missing images: {}", resp_text);
            return err(StatusCode::BAD_GATEWAY, "backend returned no images");
        }
    };

    let data: Vec<serde_json::Value> = images
        .iter()
        .map(|img| serde_json::json!({ "b64_json": img }))
        .collect();

    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    info!("generated {} image(s) in {:?}", data.len(), started.elapsed());

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "created": created,
            "data": data,
        })),
    )
}

fn err(status: StatusCode, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "error": {
                "message": message,
                "type": "invalid_request_error",
                "param": null,
                "code": null,
            }
        })),
    )
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cfg = Config::from_env();
    let mut backend = spawn_backend(&cfg);

    let client = reqwest::Client::new();
    if let Err(e) = wait_backend_ready(&cfg, &client).await {
        error!("{}", e);
        let _ = backend.kill();
        std::process::exit(1);
    }
    info!("sd-server backend ready");

    let state = std::sync::Arc::new(AppState {
        client,
        cfg: cfg.clone(),
        backend_base: format!("http://127.0.0.1:{}", cfg.backend_port),
    });

    let app = Router::new()
        .route("/v1/models", get(get_models))
        .route("/v1/images/generations", post(create_generation))
        .route("/health", get(health))
        .with_state(state);

    let addr = format!("{}:{}", cfg.listen_ip, cfg.listen_port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            error!("failed to bind bridge on {}: {}", addr, e);
            let _ = backend.kill();
            std::process::exit(1);
        }
    };
    info!("imagegen bridge listening on http://{}", addr);

    // Shut sd-server down when the bridge is terminated.
    tokio::spawn(async move {
        shutdown_signal().await;
        info!("shutdown signal received, stopping sd-server");
        let _ = backend.kill();
        std::process::exit(0);
    });

    axum::serve(listener, app).await.unwrap_or_else(|e| {
        error!("axum serve error: {}", e);
        std::process::exit(1);
    });
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}