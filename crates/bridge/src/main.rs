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
//   IMAGEGEN_STEPS           base-mode sampling steps    (28)
//   IMAGEGEN_CFG_SCALE       base-mode CFG scale         (6.0)
//   IMAGEGEN_TURBO           start in turbo mode         (false: base model)
//   IMAGEGEN_TURBO_CFG_SCALE turbo-mode CFG scale        (1.0)
//   IMAGEGEN_LORA_DIR        LoRA directory, handed to sd-server as
//                            --lora-model-dir            (models/loras)
//   IMAGEGEN_LORA            LoRA file, relative to IMAGEGEN_LORA_DIR; unset
//                            means no turbo adapter is available at all
//   IMAGEGEN_LORA_MULTIPLIER adapter multiplier           (1.0)
//   IMAGEGEN_EXTRA_LORA      a second adapter, applied in EVERY mode (base and
//                            turbo); unset means none. Used here for the
//                            uncensored LoRA, which layers on the base weights
//   IMAGEGEN_EXTRA_LORA_MULTIPLIER  its multiplier        (1.0)
//   IMAGEGEN_SIGMAS          the turbo sigma schedule, comma separated, e.g.
//                            1.0,0.9375,0.875,0.75,0.5,0.25,0.0; unset or empty
//                            means turbo mode has no schedule and is
//                            therefore unavailable
//
// Two sampling profiles in one process. BASE is the default: the base model with
// sd.cpp's own scheduler, IMAGEGEN_STEPS and IMAGEGEN_CFG_SCALE. TURBO is the
// Viggle Qwen-Image-2.1 few-step distillation (docs/viggle-turbo-lora.md): the
// adapter and its published schedule, and nothing else changes. The mode is
// process state, not a startup-only choice: it starts at IMAGEGEN_TURBO and is
// switched at runtime with
//   curl -s -X POST http://127.0.0.1:8787/v1/turbo -d '{"enabled":true}'
// or for one request only by sending "turbo": true in the generation body.
// Switching turbo ON is verified against the backend (adapter on disk and listed
// by it) before it takes effect, and the active mode is reported by /health and
// /v1/turbo, so a request never runs silently with or without the adapter.
//
// The EXTRA adapter is a layer, not a mode: when configured it rides on every
// request, base and turbo alike, and a turbo request carries both adapters in
// the server's structured lora list (the API takes a list, and the engine loops
// over it). With no extra adapter configured the base path sends no lora entry
// at all, which is the byte-identical-to-pre-turbo behaviour the identity check
// in docs/sampling-profiles.md relies on.
//
// Few-step turbo adapters (the Viggle Qwen-Image-2.1 distillation; see
// docs/viggle-turbo-lora.md) are published as a sigma schedule rather than a
// step count, and sd-server is deliberately not a place where a prompt tag can
// carry a LoRA. Both facts shape the wiring here: the adapter goes out as a
// structured `lora` entry, and the schedule goes out inside the native JSON the
// server reads from the prompt, because the sdapi route has no sigma field.
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
    /// Base-profile step count. Turbo mode derives its steps from the schedule
    /// instead, because that is what the distillation was trained on.
    steps: u32,
    /// Base-profile CFG scale.
    cfg_scale: f64,
    /// Mode the bridge starts in. false (the default) is the base model.
    turbo: bool,
    /// Turbo-profile CFG scale. The distillations sample without guidance.
    turbo_cfg_scale: f64,
    /// Whether turbo mode can be enabled at all: an adapter and a sigma schedule
    /// are configured AND the adapter file is on disk. Decided once at startup;
    /// the file is not re-checked per request, so removing it under a running
    /// bridge fails in the backend instead.
    turbo_available: bool,
    backend_args: Vec<String>,
    /// Directory handed to sd-server as --lora-model-dir. The `lora` path below
    /// is resolved by the server as a path RELATIVE to this directory: the
    /// server builds its LoRA cache from a recursive scan of the directory and
    /// only accepts a match against a cache entry (routes_sdapi.cpp lora[] ->
    /// runtime.cpp get_lora_full_path). An absolute path would not match.
    lora_dir: Option<String>,
    /// Adapter file name, relative to `lora_dir`. None means no turbo adapter is
    /// configured, which makes turbo mode unavailable.
    lora: Option<String>,
    lora_multiplier: f64,
    /// A second adapter, applied to every request in every mode. Used here for
    /// the uncensored LoRA so it can layer on the base weights without a merged
    /// model file. None means no extra layer.
    extra_lora: Option<String>,
    extra_lora_multiplier: f64,
    /// The turbo sigma schedule, which also carries the turbo step count
    /// (sd.cpp derives steps = sigmas.len() - 1). Empty means turbo mode cannot
    /// be entered: the distillation is not trained for the base scheduler.
    sigmas: Vec<f64>,
}

/// Which sampling profile a request runs under.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    /// The base model, sd.cpp's own scheduler, `steps` and `cfg_scale`.
    Base,
    /// The few-step turbo adapter and its published sigma schedule.
    Turbo,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Mode::Base => "base",
            Mode::Turbo => "turbo",
        }
    }
}

/// What one request actually asks the backend for, once the mode is resolved.
struct Profile<'a> {
    mode: Mode,
    /// The adapters to send, in application order: the turbo adapter first (in
    /// turbo mode) and the extra layer after it. Empty means the request carries
    /// no lora entry at all.
    loras: Vec<(&'a str, f64)>,
    /// The sigma schedule to embed in the prompt, if this profile has one.
    sigmas: &'a [f64],
    /// Step count before the schedule has its say.
    steps: u32,
    cfg_scale: f64,
}

impl Profile<'_> {
    /// The adapters as one comma-separated string, for logs and /health.
    fn loras_arg(&self) -> String {
        if self.loras.is_empty() {
            "(none)".to_string()
        } else {
            self.loras
                .iter()
                .map(|(path, _)| *path)
                .collect::<Vec<_>>()
                .join(",")
        }
    }
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
    turbo: Option<bool>,
    turbo_cfg_scale: Option<f64>,
    lora_dir: Option<String>,
    lora: Option<String>,
    lora_multiplier: Option<f64>,
    extra_lora: Option<String>,
    extra_lora_multiplier: Option<f64>,
    sigmas: Option<Vec<f64>>,
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

        // LoRA and sigma schedule. Setting an environment variable to the empty
        // string is the documented way to turn an optional setting off without
        // editing the file, so "" means None here, not an empty path.
        let env_or_file_opt = |env_key: &str, file_val: Option<String>| -> Option<String> {
            match std::env::var(env_key) {
                Ok(v) => {
                    let trimmed = v.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                }
                Err(_) => file_val,
            }
        };
        let lora_dir = env_or_file_opt("IMAGEGEN_LORA_DIR", file.lora_dir);
        let lora = env_or_file_opt("IMAGEGEN_LORA", file.lora);
        let lora_multiplier =
            pick("IMAGEGEN_LORA_MULTIPLIER", file.lora_multiplier, Some(1.0)).unwrap();
        let extra_lora = env_or_file_opt("IMAGEGEN_EXTRA_LORA", file.extra_lora);
        let extra_lora_multiplier = pick(
            "IMAGEGEN_EXTRA_LORA_MULTIPLIER",
            file.extra_lora_multiplier,
            Some(1.0),
        )
        .unwrap();
        let sigmas = match std::env::var("IMAGEGEN_SIGMAS") {
            Ok(v) => match parse_sigmas(&v) {
                Ok(s) => s,
                Err(e) => {
                    error!("{}", e);
                    std::process::exit(2);
                }
            },
            Err(_) => file.sigmas.unwrap_or_default(),
        };
        // parse_sigmas rejects a one-value schedule from the environment; the
        // file path needs the same check, or steps = len - 1 would be zero.
        if !sigmas.is_empty() && sigmas.len() < 2 {
            error!(
                "the sigma schedule needs at least two values, got {}",
                sigmas.len()
            );
            std::process::exit(2);
        }
        let turbo = pick("IMAGEGEN_TURBO", file.turbo, Some(false)).unwrap();
        let turbo_cfg_scale =
            pick("IMAGEGEN_TURBO_CFG_SCALE", file.turbo_cfg_scale, Some(1.0)).unwrap();
        if lora.is_some() && lora_dir.is_none() {
            error!(
                "IMAGEGEN_LORA is set without IMAGEGEN_LORA_DIR: sd-server resolves the \
                 adapter path relative to that directory, so the adapter would be ignored"
            );
            std::process::exit(2);
        }
        if extra_lora.is_some() && lora_dir.is_none() {
            error!(
                "IMAGEGEN_EXTRA_LORA is set without IMAGEGEN_LORA_DIR: sd-server resolves the \
                 adapter path relative to that directory, so the adapter would be ignored"
            );
            std::process::exit(2);
        }
        // The extra layer rides on every request, so a missing file would change
        // every image silently: that is a startup error, not a warning.
        if let (Some(dir), Some(name)) = (&lora_dir, &extra_lora) {
            let full = std::path::Path::new(dir).join(name);
            if !full.is_file() {
                error!(
                    "no extra adapter at {} (lora_dir = {}): refusing to start rather than \
                     generate every image without the configured layer",
                    full.display(),
                    dir
                );
                std::process::exit(2);
            }
        }
        // Turbo mode needs three things to be usable: an adapter file name, a
        // sigma schedule (the distillation is published as a schedule, not as a
        // step count) and the adapter file itself on disk. A startup IN turbo
        // mode must have all three. A startup in base mode only has to know
        // whether the switch would work: a request cannot enter turbo without the
        // toggle handler or its own explicit flag, and both refuse loudly rather
        // than generate a silently different image, so a missing file is a
        // warning here, not a refusal to serve the base model.
        let adapter_on_disk = match (&lora_dir, &lora) {
            (Some(dir), Some(name)) => std::path::Path::new(dir).join(name).is_file(),
            _ => false,
        };
        if turbo && lora.is_none() {
            error!(
                "IMAGEGEN_TURBO is set but no adapter is configured: set IMAGEGEN_LORA to the \
                 Viggle turbo adapter (docs/viggle-turbo-lora.md), or leave the mode at base"
            );
            std::process::exit(2);
        }
        if turbo && sigmas.is_empty() {
            error!(
                "IMAGEGEN_TURBO is set but no sigma schedule is configured: the turbo adapter \
                 ships with its own schedule, and the base scheduler is not what it was \
                 distilled for"
            );
            std::process::exit(2);
        }
        if turbo && !adapter_on_disk {
            error!(
                "IMAGEGEN_TURBO is set but there is no adapter at {}/{}: refusing to start \
                 rather than generate without the configured LoRA",
                lora_dir.as_deref().unwrap_or("(unset)"),
                lora.as_deref().unwrap_or("(unset)")
            );
            std::process::exit(2);
        }
        if !turbo && lora.is_some() && !adapter_on_disk {
            warn!(
                "the configured turbo adapter {} is not on disk under {}: the bridge starts in \
                 base mode as configured, but enabling turbo will be refused",
                lora.as_deref().unwrap_or("(none)"),
                lora_dir.as_deref().unwrap_or("(unset)")
            );
        }
        let turbo_available = lora.is_some() && sigmas.len() >= 2 && adapter_on_disk;

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
            turbo,
            turbo_cfg_scale,
            turbo_available,
            backend_args: std::env::var("IMAGEGEN_BACKEND_ARGS")
                .ok()
                .map(|s| s.split_whitespace().map(str::to_string).collect())
                .or(file.backend_args)
                .unwrap_or_else(default_backend_args),
            lora_dir,
            lora,
            lora_multiplier,
            extra_lora,
            extra_lora_multiplier,
            sigmas,
        };
        info!(
            "effective configuration: diffusion={} llm={} vae={} listen={}:{} backend_port={} \
             mode={} turbo_available={} steps={} cfg_scale={} lora={} lora_multiplier={} \
             extra_lora={} extra_lora_multiplier={} sigmas={} turbo_cfg_scale={} placement={}",
            cfg.diffusion,
            cfg.llm,
            cfg.vae,
            cfg.listen_ip,
            cfg.listen_port,
            cfg.backend_port,
            if cfg.turbo { "turbo" } else { "base" },
            cfg.turbo_available,
            cfg.steps,
            cfg.cfg_scale,
            cfg.lora.as_deref().unwrap_or("(none)"),
            cfg.lora_multiplier,
            cfg.extra_lora.as_deref().unwrap_or("(none)"),
            cfg.extra_lora_multiplier,
            if cfg.sigmas.is_empty() {
                "(none)".to_string()
            } else {
                sigmas_arg(&cfg.sigmas)
            },
            cfg.turbo_cfg_scale,
            cfg.backend_args.join(" ")
        );
        cfg
    }

    /// The sampling profile a mode means, resolved against the configuration.
    ///
    /// Base never touches the turbo adapter and never carries a schedule, even
    /// when both are configured: the two are the turbo recipe and the base model
    /// was not distilled for them. Turbo takes the adapter, the published
    /// schedule (which is also where its step count comes from) and the turbo
    /// CFG scale. The EXTRA adapter, when configured, is not part of either
    /// profile: it is a layer applied to both, so it appears in both lists.
    fn profile(&self, mode: Mode) -> Profile<'_> {
        let mut loras: Vec<(&str, f64)> = Vec::new();
        if mode == Mode::Turbo {
            if let Some(viggle) = self.lora.as_deref() {
                loras.push((viggle, self.lora_multiplier));
            }
        }
        if let Some(extra) = self.extra_lora.as_deref() {
            loras.push((extra, self.extra_lora_multiplier));
        }
        match mode {
            Mode::Turbo => Profile {
                mode,
                loras,
                sigmas: &self.sigmas,
                steps: steps_for(self.steps, &self.sigmas),
                cfg_scale: self.turbo_cfg_scale,
            },
            Mode::Base => Profile {
                mode,
                loras,
                sigmas: &[],
                steps: self.steps,
                cfg_scale: self.cfg_scale,
            },
        }
    }

    /// Every configured adapter, turbo first. Used by the LoRA cache check and
    /// the startup log: a configured adapter that the backend cannot see is a
    /// refused request, so all of them are verified.
    fn configured_loras(&self) -> Vec<&str> {
        let mut out = Vec::new();
        if let Some(viggle) = self.lora.as_deref() {
            out.push(viggle);
        }
        if let Some(extra) = self.extra_lora.as_deref() {
            out.push(extra);
        }
        out
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
    if let Some(dir) = cfg.lora_dir.as_deref() {
        info!("backend lora directory: {}", dir);
    }
    let mut cmd = Command::new(&cfg.sd_server);
    cmd.arg("--diffusion-model")
        .arg(&cfg.diffusion)
        .arg("--vae")
        .arg(&cfg.vae)
        .arg("--llm")
        .arg(&cfg.llm)
        .arg("--listen-ip")
        .arg("127.0.0.1")
        .arg("--listen-port")
        .arg(cfg.backend_port.to_string());
    if let Some(dir) = cfg.lora_dir.as_deref() {
        // Without this the server's LoRA cache is empty and every configured
        // adapter path is rejected as "invalid lora path".
        cmd.arg("--lora-model-dir").arg(dir);
    }
    cmd.args(&cfg.backend_args)
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
    /// The live sampling mode. Starts at cfg.turbo and is what the toggle
    /// endpoint writes; a per-request flag overrides it for that request only.
    turbo: std::sync::atomic::AtomicBool,
    /// Whether the backend's LoRA cache has been primed in this process, and
    /// whether the configured adapter was in the list when it was.
    lora_primed: std::sync::atomic::AtomicBool,
    lora_visible: std::sync::Mutex<Option<bool>>,
}

impl AppState {
    fn mode(&self) -> Mode {
        if self.turbo.load(std::sync::atomic::Ordering::SeqCst) {
            Mode::Turbo
        } else {
            Mode::Base
        }
    }
}

/// Ask the backend for its LoRA list.
///
/// sd-server builds that cache lazily, and its sdapi generation route does not
/// refresh it, so on a cold backend every configured adapter path is rejected
/// with "invalid lora path" until something asks for the list. The bridge
/// therefore primes it once, and the answer doubles as a check that the
/// configured adapter is actually visible to the server.
async fn prime_lora_cache(st: &AppState) -> bool {
    let url = format!("{}/sdapi/v1/loras", st.backend_base);
    let resp = st
        .client
        .get(&url)
        .timeout(Duration::from_secs(10))
        .send()
        .await;
    let list_ok = match resp {
        Ok(r) if r.status().is_success() => match r.json::<serde_json::Value>().await {
            Ok(v) => {
                let wanted = st.cfg.configured_loras();
                let entries = v.as_array();
                let missing: Vec<&str> = wanted
                    .iter()
                    .copied()
                    .filter(|want| {
                        !entries
                            .map(|entries| {
                                entries
                                    .iter()
                                    .any(|e| e.get("path").and_then(|p| p.as_str()) == Some(*want))
                            })
                            .unwrap_or(false)
                    })
                    .collect();
                let visible = missing.is_empty();
                *st.lora_visible.lock().unwrap() = Some(visible);
                if !visible {
                    error!(
                        "the backend does not list the configured adapter(s) {}; requests \
                         carrying them will be refused by it",
                        missing.join(", ")
                    );
                } else {
                    info!(
                        "backend lora cache primed; configured adapter(s) {} visible",
                        if wanted.is_empty() {
                            "(none)".to_string()
                        } else {
                            wanted.join(", ")
                        }
                    );
                }
                true
            }
            Err(e) => {
                warn!("could not parse the backend lora list: {}", e);
                false
            }
        },
        Ok(r) => {
            warn!(
                "backend lora list returned {}; leaving the LoRA cache alone",
                r.status()
            );
            false
        }
        Err(e) => {
            warn!("backend lora list request failed: {}", e);
            false
        }
    };
    list_ok
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
    /// Per-request mode override: true forces the turbo profile, false forces
    /// the base one, absent uses the live mode (see POST /v1/turbo).
    #[serde(default)]
    turbo: Option<bool>,
}

/// Body of POST /v1/turbo. Exactly one of the two forms is expected; an empty
/// body is rejected rather than guessed at.
#[derive(Deserialize, Default)]
struct TurboRequest {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    mode: Option<String>,
}

/// Parse the toggle endpoint's body. Either form is accepted:
/// {"enabled": <bool>} or {"mode": "turbo"|"base"|"on"|"off"}.
fn parse_turbo_request(body: &str) -> Result<bool, String> {
    let text = body.trim();
    if text.is_empty() {
        return Err(
            "expected a JSON body: {\"enabled\": true} or {\"mode\": \"turbo\"|\"base\"}".to_string(),
        );
    }
    let req: TurboRequest =
        serde_json::from_str(text).map_err(|e| format!("invalid request body: {}", e))?;
    if let Some(enabled) = req.enabled {
        return Ok(enabled);
    }
    match req.mode.as_deref().map(str::trim) {
        Some(m) if m.eq_ignore_ascii_case("turbo") || m.eq_ignore_ascii_case("on") => Ok(true),
        Some(m) if m.eq_ignore_ascii_case("base") || m.eq_ignore_ascii_case("off") => Ok(false),
        Some(other) => Err(format!(
            "mode '{}' is neither turbo nor base",
            other
        )),
        None => Err("expected \"enabled\" or \"mode\" in the request body".to_string()),
    }
}

/// Parse a comma-separated sigma list. An empty string or the word "none" means
/// "let sd.cpp build its own schedule".
fn parse_sigmas(value: &str) -> Result<Vec<f64>, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for part in trimmed.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match part.parse::<f64>() {
            Ok(v) => out.push(v),
            Err(_) => return Err(format!("sigma list: '{}' is not a number", part)),
        }
    }
    if out.len() == 1 {
        return Err("sigma list: a schedule needs at least two values".to_string());
    }
    Ok(out)
}

/// The comma-separated form, for logs and for the environment override.
fn sigmas_arg(sigmas: &[f64]) -> String {
    sigmas
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Attach a custom sigma schedule to the prompt.
///
/// The sdapi route has no sigma field of its own, so the schedule travels as the
/// native JSON the server extracts from the prompt: it strips
/// <sd_cpp_extra_args>...</sd_cpp_extra_args>, then parses the payload with
/// gen_params.from_json_str, which reads sample_params.custom_sigmas. The tag is
/// removed before generation, so it never reaches the text encoder.
fn prompt_with_sigmas(prompt: &str, sigmas: &[f64]) -> String {
    if sigmas.is_empty() {
        return prompt.to_string();
    }
    let extra = serde_json::json!({ "sample_params": { "custom_sigmas": sigmas } });
    format!("{}<sd_cpp_extra_args>{}</sd_cpp_extra_args>", prompt, extra)
}

/// sd.cpp derives its step count from the sigma list length (steps = len - 1).
/// Deriving it here keeps the two from disagreeing, and a disagreement is not
/// cosmetic: too few steps drops the last denoise.
fn steps_for(steps: u32, sigmas: &[f64]) -> u32 {
    if sigmas.is_empty() {
        steps
    } else {
        (sigmas.len() - 1) as u32
    }
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

/// The live mode and the sampling settings it means, as reported by /v1/turbo.
fn turbo_status(st: &AppState) -> serde_json::Value {
    let mode = st.mode();
    let profile = st.cfg.profile(mode);
    serde_json::json!({
        "mode": mode.as_str(),
        "turbo": mode == Mode::Turbo,
        "turbo_available": st.cfg.turbo_available,
        // What the next generation will be asked for, so one call answers
        // "which profile is live and what does it mean".
        "lora": profile.loras_arg(),
        "sigmas": if profile.sigmas.is_empty() {
            "(scheduler)".to_string()
        } else {
            sigmas_arg(profile.sigmas)
        },
        "steps": steps_for(profile.steps, profile.sigmas),
        "cfg_scale": profile.cfg_scale,
        "lora_visible": *st.lora_visible.lock().unwrap(),
    })
}

/// GET /v1/turbo: the live sampling mode.
async fn get_turbo(State(st): State<std::sync::Arc<AppState>>) -> Json<serde_json::Value> {
    Json(turbo_status(&st))
}

/// POST /v1/turbo: switch the sampling mode for the requests that follow.
///
/// Body: {"enabled": true|false} or {"mode": "turbo"|"base"}. Switching turbo ON
/// is verified before it takes effect: with no adapter on disk, or with a
/// backend that does not list the adapter, the switch is refused and the mode is
/// left where it was. The mode is process state and is not persisted, so a
/// restart returns to the configured default.
async fn set_turbo(
    State(st): State<std::sync::Arc<AppState>>,
    body: String,
) -> (StatusCode, Json<serde_json::Value>) {
    let enable = match parse_turbo_request(&body) {
        Ok(v) => v,
        Err(msg) => return err(StatusCode::BAD_REQUEST, &msg),
    };
    if enable == (st.mode() == Mode::Turbo) {
        return (StatusCode::OK, Json(turbo_status(&st)));
    }
    if enable {
        if !st.cfg.turbo_available {
            return err(
                StatusCode::CONFLICT,
                "turbo cannot be enabled: the adapter and its sigma schedule are not both \
                 configured (see /health turbo_available, and docs/viggle-turbo-lora.md)",
            );
        }
        // Enabling is the only point where the adapter is verified against the
        // backend, and it is verified here rather than trusted: sd-server
        // rejects a valid adapter path until something asks it for the list, and
        // a mode that says "turbo" while the backend drops the adapter is the
        // silent quality change this whole path exists to prevent.
        let primed = prime_lora_cache(&st).await;
        let visible = *st.lora_visible.lock().unwrap();
        if !primed || visible != Some(true) {
            return err(
                StatusCode::CONFLICT,
                &format!(
                    "turbo was not enabled: the backend did not confirm the adapter {} \
                     (lora_visible={:?}); the mode is unchanged",
                    st.cfg.lora.as_deref().unwrap_or("(none)"),
                    visible
                ),
            );
        }
        st.lora_primed
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    st.turbo.store(enable, std::sync::atomic::Ordering::SeqCst);
    info!(
        "sampling mode switched to {} by POST /v1/turbo",
        st.mode().as_str()
    );
    (StatusCode::OK, Json(turbo_status(&st)))
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
    let mode = st.mode();
    let profile = st.cfg.profile(mode);
    Json(serde_json::json!({
        "status": "ok",
        "backend": backend_ok,
        "backend_process_alive": st.backend.is_alive(),
        "backend_restarts": st.backend.restarts(),
        "backend_restart_attempts": st.backend.restart_attempts(),
        // The live mode, and what the next generation will be asked for: the
        // adapter in use, the schedule, and the step count derived from it.
        // lora_visible is null until the backend has been asked for its list.
        "mode": mode.as_str(),
        "turbo_available": st.cfg.turbo_available,
        "lora": profile.loras_arg(),
        "lora_visible": *st.lora_visible.lock().unwrap(),
        "sigmas": if profile.sigmas.is_empty() {
            "(scheduler)".to_string()
        } else {
            sigmas_arg(profile.sigmas)
        },
        "steps": steps_for(profile.steps, profile.sigmas),
        "cfg_scale": profile.cfg_scale
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
    // The mode this request runs in: its own flag wins, otherwise the live mode
    // set by POST /v1/turbo, otherwise the configured default.
    let mode = match req.turbo {
        Some(true) => Mode::Turbo,
        Some(false) => Mode::Base,
        None => st.mode(),
    };
    if mode == Mode::Turbo && !st.cfg.turbo_available {
        return err(
            StatusCode::CONFLICT,
            "turbo was requested but is not available: the adapter and its sigma schedule \
             are not both configured (see /health turbo_available)",
        );
    }
    let profile = st.cfg.profile(mode);
    // Any request carrying an adapter needs the backend's LoRA cache primed
    // first: sd-server's sdapi route does not refresh it, so an unprimed cache
    // turns a valid adapter path into "invalid lora path". With no adapters in
    // the profile (the plain base path) this is skipped entirely.
    if !profile.loras.is_empty()
        && !st
            .lora_primed
            .load(std::sync::atomic::Ordering::SeqCst)
        && prime_lora_cache(&st).await
    {
        st.lora_primed
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    let requested_steps = req.num_inference_steps.unwrap_or(profile.steps);
    let steps = steps_for(requested_steps, profile.sigmas);
    if steps != requested_steps {
        warn!(
            "the sigma schedule carries {} steps, so it overrides steps={}",
            steps, requested_steps
        );
    }
    let cfg_scale = req.cfg_scale.unwrap_or(profile.cfg_scale);
    // seed < 0 (or omitted) means random, matching sd-server's convention.
    let seed = req.seed.unwrap_or(-1);

    let prompt = prompt_with_sigmas(&req.prompt, profile.sigmas);
    let mut body = serde_json::json!({
        "prompt": prompt,
        "negative_prompt": "",
        "width": width,
        "height": height,
        "steps": steps,
        "cfg_scale": cfg_scale,
        "seed": seed,
        "batch_size": n,
    });
    // The server takes LoRAs only from this structured field; a <lora:...> tag
    // in the prompt is deliberately ignored there, and the field is a list the
    // engine applies in order. The turbo profile puts the distillation first and
    // the extra layer (when configured) after it; the plain base path sends no
    // lora entry at all.
    if !profile.loras.is_empty() {
        body["lora"] = serde_json::Value::Array(
            profile
                .loras
                .iter()
                .map(|(path, multiplier)| {
                    serde_json::json!({ "path": path, "multiplier": multiplier })
                })
                .collect(),
        );
    }

    info!(
        "generating {} image(s) at {}x{} mode={} steps={} cfg={} seed={} lora={} sigmas={}",
        n,
        width,
        height,
        profile.mode.as_str(),
        steps,
        cfg_scale,
        seed,
        profile.loras_arg(),
        if profile.sigmas.is_empty() {
            "(scheduler)".to_string()
        } else {
            sigmas_arg(profile.sigmas)
        }
    );

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
        turbo: std::sync::atomic::AtomicBool::new(cfg.turbo),
        lora_primed: std::sync::atomic::AtomicBool::new(false),
        lora_visible: std::sync::Mutex::new(None),
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
        let st = state.clone();
        tokio::spawn(async move {
            match wait_backend_ready(&st.cfg, &st.client).await {
                Ok(()) => {
                    info!("sd-server backend ready");
                    // Prime the adapter list as soon as the backend answers, so
                    // a cold start cannot refuse the first LoRA request and so
                    // /health can report whether the adapter is visible. The
                    // first generation primes too, in case this has not landed.
                    if !st.cfg.configured_loras().is_empty() {
                        let primed = prime_lora_cache(&st).await;
                        st.lora_primed
                            .store(primed, std::sync::atomic::Ordering::SeqCst);
                    }
                }
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
        // The turbo toggle: GET reports the live mode, POST switches it. Same
        // state as the per-request "turbo" flag on a generation body.
        .route("/v1/turbo", get(get_turbo).post(set_turbo))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigma_list_parses_and_rejects_bad_input() {
        // Absent schedule: the server's own scheduler decides.
        assert_eq!(parse_sigmas("").unwrap(), Vec::<f64>::new());
        assert_eq!(parse_sigmas("none").unwrap(), Vec::<f64>::new());
        assert_eq!(parse_sigmas("  NONE  ").unwrap(), Vec::<f64>::new());
        // The Viggle v0.2.1 schedule, with slack around the separators.
        assert_eq!(
            parse_sigmas("1.0, 0.9375 ,0.875,0.75,0.5,0.25,0.0").unwrap(),
            vec![1.0, 0.9375, 0.875, 0.75, 0.5, 0.25, 0.0]
        );
        // A typo must not silently become a different schedule.
        assert!(parse_sigmas("1.0,abc").is_err());
        // One value is not a schedule.
        assert!(parse_sigmas("1.0").is_err());
    }

    #[test]
    fn steps_follow_the_schedule_when_one_is_configured() {
        let sigmas = vec![1.0, 0.9375, 0.875, 0.75, 0.5, 0.25, 0.0];
        assert_eq!(steps_for(28, &sigmas), 6);
        assert_eq!(steps_for(28, &[]), 28);
    }

    #[test]
    fn the_prompt_carries_the_schedule_without_losing_its_text() {
        let prompt = prompt_with_sigmas("a capybara", &[1.0, 0.5, 0.0]);
        assert!(prompt.starts_with("a capybara<sd_cpp_extra_args>"));
        assert!(prompt.ends_with("</sd_cpp_extra_args>"));
        let inner = prompt
            .trim_start_matches("a capybara<sd_cpp_extra_args>")
            .trim_end_matches("</sd_cpp_extra_args>");
        let parsed: serde_json::Value = serde_json::from_str(inner).unwrap();
        assert_eq!(parsed["sample_params"]["custom_sigmas"][0], 1.0);
        assert_eq!(
            parsed["sample_params"]["custom_sigmas"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn no_schedule_leaves_the_prompt_untouched() {
        assert_eq!(prompt_with_sigmas("a capybara", &[]), "a capybara");
    }

    #[test]
    fn sigma_list_renders_for_logs_and_the_health_endpoint() {
        assert_eq!(
            sigmas_arg(&[1.0, 0.9375, 0.0]),
            "1,0.9375,0".to_string()
        );
    }

    /// A config with both profiles available, built directly instead of read
    /// from the environment so the tests do not depend on imagegen.toml.
    fn turbo_capable_config() -> Config {
        Config {
            sd_server: "bin/sd-server".to_string(),
            diffusion: "qwen-image.gguf".to_string(),
            vae: "vae.safetensors".to_string(),
            llm: "te.safetensors".to_string(),
            listen_ip: "127.0.0.1".to_string(),
            listen_port: 8787,
            backend_port: 8788,
            steps: 28,
            cfg_scale: 6.0,
            turbo: false,
            turbo_cfg_scale: 1.0,
            turbo_available: true,
            backend_args: Vec::new(),
            lora_dir: Some("models/loras".to_string()),
            lora: Some("Qwen-Image-2.1-viggle-turbo-v0.2.1-6step-lora-r128.safetensors".to_string()),
            lora_multiplier: 1.0,
            extra_lora: None,
            extra_lora_multiplier: 1.0,
            sigmas: vec![1.0, 0.9375, 0.875, 0.75, 0.5, 0.25, 0.0],
        }
    }

    #[test]
    fn the_base_profile_ignores_the_adapter_and_its_schedule() {
        let cfg = turbo_capable_config();
        let base = cfg.profile(Mode::Base);
        assert_eq!(base.mode, Mode::Base);
        // Configured, and still not used: the two are the turbo recipe.
        assert!(cfg.lora.is_some());
        assert!(base.loras.is_empty());
        assert!(base.sigmas.is_empty());
        assert_eq!(base.steps, 28);
        assert_eq!(base.cfg_scale, 6.0);
        // What goes into the prompt for this profile: nothing added.
        assert_eq!(prompt_with_sigmas("a capybara", base.sigmas), "a capybara");
    }

    #[test]
    fn the_turbo_profile_carries_the_adapter_and_takes_steps_from_the_schedule() {
        let cfg = turbo_capable_config();
        let turbo = cfg.profile(Mode::Turbo);
        assert_eq!(turbo.mode, Mode::Turbo);
        assert_eq!(turbo.loras.len(), 1);
        assert_eq!(turbo.loras[0].0, cfg.lora.as_deref().unwrap());
        assert_eq!(turbo.loras[0].1, 1.0);
        assert_eq!(turbo.sigmas.len(), 7);
        assert_eq!(turbo.steps, 6);
        assert_eq!(turbo.cfg_scale, 1.0);
        assert!(prompt_with_sigmas("a capybara", turbo.sigmas).contains("custom_sigmas"));
    }

    #[test]
    fn the_extra_layer_rides_on_both_modes_and_after_the_turbo_adapter() {
        let mut cfg = turbo_capable_config();
        cfg.extra_lora = Some("qwen-image-2.1-uncensored-lora.safetensors".to_string());
        cfg.extra_lora_multiplier = 0.8;

        let base = cfg.profile(Mode::Base);
        assert_eq!(base.loras.len(), 1);
        assert_eq!(base.loras[0].0, "qwen-image-2.1-uncensored-lora.safetensors");
        assert_eq!(base.loras[0].1, 0.8);

        let turbo = cfg.profile(Mode::Turbo);
        assert_eq!(turbo.loras.len(), 2);
        // Turbo first, the extra layer after it, in application order.
        assert_eq!(turbo.loras[0].0, cfg.lora.as_deref().unwrap());
        assert_eq!(turbo.loras[1].0, "qwen-image-2.1-uncensored-lora.safetensors");
        assert_eq!(turbo.loras_arg().matches(',').count(), 1);
        // And the configured list the cache check verifies has both.
        assert_eq!(cfg.configured_loras().len(), 2);
    }

    #[test]
    fn the_toggle_body_accepts_both_forms_and_rejects_anything_else() {
        assert!(parse_turbo_request(r#"{"enabled": true}"#).unwrap());
        assert!(!parse_turbo_request(r#"{"enabled": false}"#).unwrap());
        assert!(parse_turbo_request(r#"{"mode": "turbo"}"#).unwrap());
        assert!(parse_turbo_request(r#"{"mode": " ON "}"#).unwrap());
        assert!(!parse_turbo_request(r#"{"mode": "Base"}"#).unwrap());
        assert!(!parse_turbo_request(r#"{"mode": "off"}"#).unwrap());
        // An empty or undecided body is an error, never a guess.
        assert!(parse_turbo_request("").is_err());
        assert!(parse_turbo_request("{}").is_err());
        assert!(parse_turbo_request(r#"{"mode": "fast"}"#).is_err());
        assert!(parse_turbo_request("not json").is_err());
    }

    #[test]
    fn a_generation_body_may_pin_its_own_mode() {
        let req: GenRequest =
            serde_json::from_str(r#"{"prompt": "x", "turbo": true}"#).unwrap();
        assert_eq!(req.turbo, Some(true));
        let req: GenRequest =
            serde_json::from_str(r#"{"prompt": "x", "turbo": false}"#).unwrap();
        assert_eq!(req.turbo, Some(false));
        // Absent means "whatever the live mode is".
        let req: GenRequest = serde_json::from_str(r#"{"prompt": "x"}"#).unwrap();
        assert_eq!(req.turbo, None);
    }
}