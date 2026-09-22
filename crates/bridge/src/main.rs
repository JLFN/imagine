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
//   IMAGEGEN_DIFFUSION       path to diffusion model     (models/diffusion_models/qwen-image-2.1-Q6_K.gguf)
//   IMAGEGEN_VAE             path to vae                 (models/vae/qwen_image_2.1_vae_bf16.safetensors)
//   IMAGEGEN_LLM             path to text encoder        (models/text_encoders/qwen3vl_8b_int8_convrot.safetensors)
//
// The default text encoder is the ComfyUI int8 convrot file, matching the
// configuration the upstream model card recommends. It requires the patched
// sd-server built from patches/0001 and patches/0002 in this repo: stock
// sd.cpp aborts on that file. Measured cost against the GGUF encoder it
// replaces is +5.8 GB of host RAM (10,104.77 MB of parameters against
// 4,302.33 MB), the same VRAM peak, and a somewhat higher conditioning time,
// with image quality a measured tie. Set IMAGEGEN_LLM to
// models/text_encoders/Qwen3VL-8B-Instruct-Q4_K_M.gguf to go back to the GGUF
// encoder, which runs on a stock sd-server.
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

/// What the TOML configuration file may contain.
///
/// Model paths are deliberately NOT defaulted in the binary: a wrong path that
/// silently loads a different model is worse than a refusal to start, so a
/// missing path is a startup error naming the file and the variable. Explicitly
/// environment-overridable so a test can point one value elsewhere without
/// editing the file.
#[derive(Deserialize, Default)]
struct FileConfig {
    sd_server: Option<String>,
    diffusion: Option<String>,
    vae: Option<String>,
    llm: Option<String>,
    backend_args: Option<Vec<String>>,
    listen_ip: Option<String>,
    listen_port: Option<u16>,
    backend_port: Option<u16>,
    steps: Option<u32>,
    cfg_scale: Option<f64>,
}

/// Environment wins, then the file, then the built-in fallback.
fn pick<T: std::str::FromStr>(env_key: &str, file_val: Option<T>, fallback: Option<T>) -> Option<T> {
    std::env::var(env_key)
        .ok()
        .and_then(|v| v.parse::<T>().ok())
        .or(file_val)
        .or(fallback)
}

/// Placement flags that were measured on this workstation: diffusion parameters
/// pinned in VRAM, text encoder graph and parameters on the CPU, VAE parameters
/// in RAM with GPU compute. See docs/architecture.md.
fn default_backend_args() -> Vec<String> {
    [
        "--backend",
        "te=cpu,diffusion=cuda0,vae=cuda0",
        "--params-backend",
        "te=cpu,diffusion=cuda0,vae=cpu",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

impl Config {
    fn load() -> Self {
        let path =
            std::env::var("IMAGEGEN_CONFIG").unwrap_or_else(|_| "imagegen.toml".to_string());

        let file: FileConfig = match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str(&text) {
                Ok(parsed) => {
                    info!("configuration loaded from {}", path);
                    parsed
                }
                Err(e) => {
                    error!("{} is not valid TOML: {}", path, e);
                    std::process::exit(2);
                }
            },
            Err(_) => {
                warn!(
                    "no configuration file at {} (set IMAGEGEN_CONFIG to point elsewhere); \
                     using the environment only",
                    path
                );
                FileConfig::default()
            }
        };

        let required = |file_val: Option<String>, env_key: &str| -> String {
            if let Ok(v) = std::env::var(env_key) {
                return v;
            }
            match file_val {
                Some(v) => v,
                None => {
                    error!(
                        "no model path for {}: set it in {} or in the environment",
                        env_key, path
                    );
                    std::process::exit(2);
                }
            }
        };

        let cfg = Config {
            // Every path, the binary included, comes from the file or the
            // environment. A wrong path is a startup error, not a panic later.
            sd_server: required(file.sd_server, "IMAGEGEN_SD_SERVER"),
            diffusion: required(file.diffusion, "IMAGEGEN_DIFFUSION"),
            vae: required(file.vae, "IMAGEGEN_VAE"),
            llm: required(file.llm, "IMAGEGEN_LLM"),
            listen_ip: pick("IMAGEGEN_LISTEN_IP", file.listen_ip, None)
                .unwrap_or_else(|| "127.0.0.1".to_string()),
            listen_port: pick("IMAGEGEN_LISTEN_PORT", file.listen_port, Some(8787)).unwrap(),
            backend_port: pick("IMAGEGEN_BACKEND_PORT", file.backend_port, Some(8788)).unwrap(),
            steps: pick("IMAGEGEN_STEPS", file.steps, Some(28)).unwrap(),
            cfg_scale: pick("IMAGEGEN_CFG_SCALE", file.cfg_scale, Some(6.0)).unwrap(),
            backend_args: std::env::var("IMAGEGEN_BACKEND_ARGS")
                .ok()
                .map(|s| s.split_whitespace().map(str::to_string).collect())
                .or(file.backend_args)
                .unwrap_or_else(default_backend_args),
        };
        info!(
            "effective configuration: diffusion={} llm={} vae={} listen={}:{} backend_port={} \
             steps={} cfg_scale={} placement={}",
            cfg.diffusion,
            cfg.llm,
            cfg.vae,
            cfg.listen_ip,
            cfg.listen_port,
            cfg.backend_port,
            cfg.steps,
            cfg.cfg_scale,
            cfg.backend_args.join(" ")
        );
        cfg
    }
}

// ---------------------------------------------------------------------------
// Backend process supervision
// ---------------------------------------------------------------------------

fn spawn_backend(cfg: &Config) -> Result<Child, String> {
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
        .map_err(|e| format!("failed to spawn sd-server at {}: {}", cfg.sd_server, e))
}

async fn wait_backend_ready(cfg: &Config, client: &reqwest::Client) -> Result<(), String> {
    let url = format!("http://127.0.0.1:{}/v1/models", cfg.backend_port);
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        // A probe needs its own timeout: without one, a backend that accepts the
        // connection and then never answers blocks this loop forever and the
        // deadline below is never reached.
        match client
            .get(&url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
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
// Backend supervision
// ---------------------------------------------------------------------------

/// Owns the sd-server child so it can be reaped, replaced and killed.
///
/// The original code spawned the backend once and never waited on it, so a
/// backend that died went unnoticed: on 2026-09-22 this bridge served nothing
/// for 8h51m while its sd-server child sat as a zombie and /v1/models kept
/// answering from a static list. The child is owned here instead, and a
/// supervisor task replaces it when it dies.
struct BackendHandle {
    child: std::sync::Mutex<Option<Child>>,
    /// Successful replacements: the backend died and a new one became ready.
    restarts: std::sync::atomic::AtomicU64,
    /// Spawns attempted after a death, including ones that never became ready.
    /// Counted separately so a backend that dies and fails to come back cannot
    /// look like a healthy number of restarts.
    restart_attempts: std::sync::atomic::AtomicU64,
}

impl BackendHandle {
    fn new(child: Child) -> Self {
        Self {
            child: std::sync::Mutex::new(Some(child)),
            restarts: std::sync::atomic::AtomicU64::new(0),
            restart_attempts: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Reap the child if it has exited, returning its status once. Taking the
    /// status out of try_wait is what reaps it, so no zombie is left behind.
    fn reap_if_dead(&self) -> Option<std::process::ExitStatus> {
        let mut guard = self.child.lock().unwrap();
        let status = match guard.as_mut() {
            Some(c) => match c.try_wait() {
                Ok(Some(status)) => Some(status),
                Ok(None) => None,
                Err(e) => {
                    warn!("sd-server status check failed: {}", e);
                    None
                }
            },
            None => None,
        };
        if status.is_some() {
            *guard = None;
        }
        status
    }

    fn is_alive(&self) -> bool {
        match self.child.lock().unwrap().as_mut() {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        }
    }

    fn replace(&self, child: Child) {
        *self.child.lock().unwrap() = Some(child);
        self.restart_attempts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// Called only once a replaced backend has answered the readiness probe.
    fn mark_ready(&self) {
        self.restarts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    fn restarts(&self) -> u64 {
        self.restarts.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn restart_attempts(&self) -> u64 {
        self.restart_attempts
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Kill and reap, so shutdown leaves no zombie either.
    fn kill(&self) {
        if let Some(mut c) = self.child.lock().unwrap().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// Keep the backend up. Polls the child, and on death respawns it with
/// exponential backoff, waiting for it to answer before calling it ready.
async fn supervise_backend(
    cfg: Config,
    client: reqwest::Client,
    handle: std::sync::Arc<BackendHandle>,
    shutting_down: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if shutting_down.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }

        if let Some(status) = handle.reap_if_dead() {
            warn!("sd-server backend exited ({})", status);
        }
        if handle.is_alive() {
            backoff = Duration::from_secs(1);
            continue;
        }

        warn!("sd-server backend is down; restarting in {:?}", backoff);
        tokio::time::sleep(backoff).await;
        if shutting_down.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        match spawn_backend(&cfg) {
            Ok(child) => handle.replace(child),
            Err(e) => {
                // A backend that cannot even be started will not be fixed by
                // waiting, but the bridge stays up so /health can report it.
                error!("{}", e);
                backoff = (backoff * 2).min(Duration::from_secs(30));
                continue;
            }
        }
        match wait_backend_ready(&cfg, &client).await {
            Ok(()) => {
                handle.mark_ready();
                info!(
                    "sd-server backend restarted and ready (restart number {})",
                    handle.restarts()
                );
                backoff = Duration::from_secs(1);
            }
            Err(e) => {
                warn!("restarted backend did not become ready: {}", e);
                handle.kill();
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Shared application state
// ---------------------------------------------------------------------------

struct AppState {
    client: reqwest::Client,
    cfg: Config,
    backend_base: String,
    backend: std::sync::Arc<BackendHandle>,
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
    check_edge_limit(w, h)?;
    Ok((w, h))
}

/// The largest edge this card is documented to cover at this precision. A hard
/// ceiling keeps one caller from asking for a size that cannot fit and failing
/// the generation or aborting the backend; a clear rejection is better than
/// either. Applied to the final size from both request paths.
const MAX_EDGE: u32 = 1536;

fn check_edge_limit(w: u32, h: u32) -> Result<(), String> {
    if w > MAX_EDGE || h > MAX_EDGE {
        return Err(format!(
            "size {}x{} exceeds the supported maximum of {}x{} on this 12 GB GPU",
            w, h, MAX_EDGE, MAX_EDGE
        ));
    }
    Ok(())
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
/// tier to a WIDTHxHEIGHT that is divisible by 32. The result is NOT bounded by
/// the resolution tier: a tier whose long edge exceeds what this card can run
/// produces dimensions that check_edge_limit then rejects with a clear error,
/// rather than a silently clamped image.
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
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false);
    Json(serde_json::json!({
        "status": "ok",
        "backend": backend_ok,
        "backend_process_alive": st.backend.is_alive(),
        "backend_restarts": st.backend.restarts(),
        "backend_restart_attempts": st.backend.restart_attempts()
    }))
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
    // The ceiling has to be applied to the final size, not inside parse_size:
    // the aspect_ratio + resolution path is the one open-grok actually uses, and
    // it would otherwise forward 2048x2048 for "2k" straight past the limit.
    if let Err(msg) = check_edge_limit(width, height) {
        return err(StatusCode::BAD_REQUEST, &msg);
    }

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

    let cfg = Config::load();
    let backend = match spawn_backend(&cfg) {
        Ok(child) => child,
        Err(e) => {
            error!("{}", e);
            std::process::exit(2);
        }
    };

    let client = reqwest::Client::new();

    let backend = std::sync::Arc::new(BackendHandle::new(backend));
    let shutting_down = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Bind and serve BEFORE waiting for the backend. A startup that waits first
    // cannot answer /health while it waits, which is exactly when an operator
    // needs it: a backend that spawns but never answers left the bridge
    // unreachable for 180 seconds and then gone. /health now reports
    // backend false during that window instead.
    let addr = format!("{}:{}", cfg.listen_ip, cfg.listen_port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            error!("failed to bind bridge on {}: {}", addr, e);
            backend.kill();
            std::process::exit(1);
        }
    };

    let state = std::sync::Arc::new(AppState {
        client: client.clone(),
        cfg: cfg.clone(),
        backend_base: format!("http://127.0.0.1:{}", cfg.backend_port),
        backend: backend.clone(),
    });

    // Keep the backend up for the lifetime of the bridge. The supervisor owns
    // every restart, the first start included: it reaps a child that dies and
    // respawns it, and the readiness wait below is a report, not a gate.
    tokio::spawn(supervise_backend(
        cfg.clone(),
        client.clone(),
        backend.clone(),
        shutting_down.clone(),
    ));

    // Report the first readiness without blocking the listener. A backend that
    // never becomes ready is visible in /health as backend false, and a process
    // that is alive but not answering HTTP is NOT auto-restarted yet; that is a
    // known limitation, recorded in docs/architecture.md.
    {
        let cfg = cfg.clone();
        let client = client.clone();
        tokio::spawn(async move {
            match wait_backend_ready(&cfg, &client).await {
                Ok(()) => info!("sd-server backend ready"),
                Err(e) => warn!(
                    "sd-server backend did not answer within the startup window ({}); \
                     the bridge is serving and /health reports backend false",
                    e
                ),
            }
        });
    }

    let app = Router::new()
        .route("/v1/models", get(get_models))
        .route("/v1/images/generations", post(create_generation))
        .route("/health", get(health))
        .with_state(state);

    info!("imagegen bridge listening on http://{}", addr);

    // Shut sd-server down when the bridge is terminated.
    tokio::spawn(async move {
        shutdown_signal().await;
        info!("shutdown signal received, stopping sd-server");
        shutting_down.store(true, std::sync::atomic::Ordering::SeqCst);
        backend.kill();
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