// imagegen-bridge: a thin OpenAI image API server that supervises
// stable-diffusion.cpp's sd-server backend.
//
// A client points its image-provider setting at this bridge
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

use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
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
    /// The Qwen3-VL vision tower (`--llm_vision`, the mmproj file). Editing
    /// needs it: Qwen Image 2.1 conditions on a reference image through the
    /// vision tower, and the encoder answers "no vision weights detected,
    /// vision disabled" without it (src/model/te/llm.hpp:466). Generation does
    /// not use it, so it stays optional and generation never depends on it.
    llm_vision: Option<String>,
    /// Seconds of no request after which the whole process exits, releasing the
    /// ~11 GB of host RAM and the pinned VRAM the backend holds. 0 disables it.
    /// The timer never fires while a request is in flight, so a long first
    /// request cannot be cut off by its own timeout. Documented in
    /// docs/architecture.md, "On-demand lifecycle".
    idle_timeout_secs: u64,
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
    llm_vision: Option<String>,
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
    idle_timeout_secs: Option<u64>,
}

/// Environment wins, then the file, then the built-in fallback.
fn pick<T: std::str::FromStr>(
    env_key: &str,
    file_val: Option<T>,
    fallback: Option<T>,
) -> Option<T> {
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
        let path = std::env::var("IMAGEGEN_CONFIG").unwrap_or_else(|_| "imagegen.toml".to_string());

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
        // The vision tower is optional and generation never reads it, so an
        // unset value is not an error: it only makes editing unavailable, which
        // the edit route reports per request. "" turns it off explicitly.
        let llm_vision = env_or_file_opt("IMAGEGEN_LLM_VISION", file.llm_vision);
        // 0 disables the idle shutdown. 900 s is the value docs/architecture.md
        // records for this box; a shorter one releases VRAM sooner but restages
        // ~10 GB of weights (35-40 s) on the next request.
        let idle_timeout_secs =
            pick("IMAGEGEN_IDLE_TIMEOUT_SECS", file.idle_timeout_secs, Some(900)).unwrap();
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
        // A configured vision tower that is not on disk would make every edit
        // fail in the backend with an empty image; refuse at startup instead,
        // because the path is a configuration error, not a runtime condition.
        // An unset value is different: it means this deployment edits nothing,
        // and the edit route says so per request.
        if let Some(vision) = llm_vision.as_deref() {
            if !std::path::Path::new(vision).is_file() {
                error!(
                    "no vision tower at {} (IMAGEGEN_LLM_VISION): editing needs it, so refusing \
                     to start with a path that cannot load. Unset it to run generation only",
                    vision
                );
                std::process::exit(2);
            }
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
            llm_vision,
            idle_timeout_secs,
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
    info!("backend placement args: {}", cfg.backend_args.join(" "));
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
    if let Some(vision) = cfg.llm_vision.as_deref() {
        // The reference-image path of Qwen Image 2.1 runs through this tower;
        // without it the backend logs "vision disabled" and returns empty
        // conditioning for an edit (src/conditioning/conditioner.hpp:2840).
        cmd.arg("--llm_vision").arg(vision);
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
    /// Requests currently being served. The idle timer reads it and never exits
    /// while it is non-zero, so a long first request cannot be cut off by its own
    /// timeout. Reported by /health so a stop decision can be made from data.
    in_flight: std::sync::atomic::AtomicUsize,
    /// When the last request finished, as an Instant. The idle timer measures
    /// from here; /health reports the derived seconds.
    last_request_done: std::sync::Mutex<Instant>,
}

/// Count a request as in flight for as long as the guard lives, then record the
/// moment it finished. Held across the whole handler body.
struct InFlight(std::sync::Arc<AppState>);

impl InFlight {
    fn begin(st: &std::sync::Arc<AppState>) -> Self {
        st.in_flight
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self(st.clone())
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        if let Ok(mut at) = self.0.last_request_done.lock() {
            *at = Instant::now();
        }
        self.0
            .in_flight
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
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

/// The image-edit request the agent tool sends: JSON, not multipart, with the
/// references as base64 data URLs.
///
/// The wire shape is fixed by the calling tool, not chosen here. It posts
/// `model`, `prompt`, `n`, `resolution` or `aspect_ratio`, `response_format`,
/// and the references either as a single `{"url": ...}` object (one reference)
/// or an array of them (several): every reference is a data URL, and the
/// prompt is the instruction. A caller may also send the references as bare
/// data-URL strings in `image` or `images`, which is accepted for the same
/// reason the backend accepts both forms: it is the documented shape of the
/// route's own image fields.
#[derive(Deserialize)]
struct EditRequest {
    prompt: String,
    /// One reference: `{"url": "data:..."}` or a bare data URL.
    #[serde(default)]
    image: Option<ReferenceField>,
    /// Several references, each in the same two forms.
    #[serde(default)]
    images: Option<Vec<ReferenceField>>,
    n: Option<u32>,
    size: Option<String>,
    /// Accepted for wire compatibility with the calling tool, which sends these
    /// for a multi-reference edit. They do not size the result: with no explicit
    /// `size` the canvas follows the reference, which is what the backend's own
    /// multipart edit route does (routes_openai.cpp:123-134) and what a caller
    /// sending only `resolution` expects.
    #[serde(default)]
    #[allow(dead_code)]
    aspect_ratio: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    resolution: Option<String>,
    num_inference_steps: Option<u32>,
    seed: Option<i64>,
    cfg_scale: Option<f64>,
}

/// One reference image in either form the callers use: the object with a `url`
/// field (the agent tool's shape) or the bare string (sd.cpp's own shape).
#[derive(Deserialize, Clone)]
#[serde(untagged)]
enum ReferenceField {
    Url { url: String },
    Bare(String),
}

impl ReferenceField {
    fn into_data_url(self) -> String {
        match self {
            ReferenceField::Url { url } => url,
            ReferenceField::Bare(data) => data,
        }
    }
}

/// sd.cpp extracts its native request schema from the prompt with a
/// non-greedy `<sd_cpp_extra_args>(.*?)</sd_cpp_extra_args>` search and removes
/// the block before generation (examples/server/routes_sdapi.cpp:144). A prompt
/// that carries the marker itself would therefore be misparsed, so it is
/// refused rather than silently truncated.
const EXTRA_ARGS_OPEN: &str = "<sd_cpp_extra_args>";
const EXTRA_ARGS_CLOSE: &str = "</sd_cpp_extra_args>";

/// Reference images are megabytes of base64. axum refuses a body over 2 MB for
/// a `String` extractor by default (axum-core extract/default_body_limit.rs),
/// which is smaller than one compressed reference plus its data-URL overhead;
/// 16 MB bounds several references without capping real traffic.
const EDIT_BODY_LIMIT: usize = 16 * 1024 * 1024;

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
            "expected a JSON body: {\"enabled\": true} or {\"mode\": \"turbo\"|\"base\"}"
                .to_string(),
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
        Some(other) => Err(format!("mode '{}' is neither turbo nor base", other)),
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

/// Collect the references from either field, array first, in the order given:
/// sd.cpp conditions on them in that order (its own -r is repeated in order).
fn collect_ref_images(req: &EditRequest) -> Vec<String> {
    if let Some(images) = &req.images {
        if !images.is_empty() {
            return images.iter().cloned().map(ReferenceField::into_data_url).collect();
        }
    }
    req.image
        .clone()
        .map(ReferenceField::into_data_url)
        .into_iter()
        .collect()
}

/// Check an edit before anything is sent: it needs a reference, none of them may
/// be blank, and the prompt may not carry the extension-block marker.
fn validate_edit_inputs(prompt: &str, refs: &[String]) -> Result<(), String> {
    if refs.is_empty() {
        return Err(
            "image_edit requires at least one reference image in 'image' or 'images'".to_string(),
        );
    }
    if refs.iter().any(|r| r.trim().is_empty()) {
        return Err("image_edit received an empty reference image".to_string());
    }
    if prompt.trim().is_empty() {
        return Err("prompt is required".to_string());
    }
    if prompt.contains(EXTRA_ARGS_OPEN) || prompt.contains(EXTRA_ARGS_CLOSE) {
        return Err(format!("prompt must not contain the {} marker", EXTRA_ARGS_OPEN));
    }
    Ok(())
}

/// Decode a reference from a data URL or a bare base64 string, the two forms
/// callers send, and return the bytes with the mime type to upload them as.
fn decode_image_data(s: &str) -> Result<(Vec<u8>, String), String> {
    let s = s.trim();
    let (mime, payload) = if let Some(rest) = s.strip_prefix("data:") {
        let (meta, payload) = rest
            .split_once(',')
            .ok_or_else(|| "malformed data URL: no comma separator".to_string())?;
        let head = meta.trim();
        let mime = match head.strip_suffix(";base64") {
            Some(mime) => mime.trim(),
            None => {
                return Err(format!(
                    "unsupported data URL encoding in '{}', only base64 is accepted",
                    head
                ))
            }
        };
        let mime = if mime.is_empty() { "image/png" } else { mime };
        (mime.to_string(), payload.to_string())
    } else {
        ("image/png".to_string(), s.to_string())
    };

    // Some clients wrap long base64 payloads; whitespace is not significant.
    let compact: String = payload.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if compact.is_empty() {
        return Err("empty reference image".to_string());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(compact.as_bytes())
        .map_err(|e| format!("invalid base64 reference image: {}", e))?;
    Ok((bytes, mime))
}

fn parse_size(size: &str) -> Result<(u32, u32), String> {
    let (w, h) = size
        .split_once('x')
        .ok_or_else(|| format!("invalid size '{}', expected WIDTHxHEIGHT", size))?;
    let w: u32 = w
        .trim()
        .parse()
        .map_err(|_| format!("invalid width in '{}'", size))?;
    let h: u32 = h
        .trim()
        .parse()
        .map_err(|_| format!("invalid height in '{}'", size))?;
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

/// Resolution tier -> long-edge pixels. A typical client sends "1k" by default.
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
    // The idle state, for the stop hook and for an operator: a stop decision
    // must be made from data, not from a guess that nothing is running.
    let in_flight = st.in_flight.load(std::sync::atomic::Ordering::SeqCst);
    let idle_secs = st
        .last_request_done
        .lock()
        .map(|at| at.elapsed().as_secs())
        .unwrap_or(0);
    Json(serde_json::json!({
        "status": "ok",
        "backend": backend_ok,
        "backend_process_alive": st.backend.is_alive(),
        "backend_restarts": st.backend.restarts(),
        "backend_restart_attempts": st.backend.restart_attempts(),
        "in_flight": in_flight,
        "idle_secs": idle_secs,
        "idle_timeout_secs": st.cfg.idle_timeout_secs,
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
    // Held for the whole body: the idle timer must never fire mid-generation.
    let _in_flight = InFlight::begin(&st);
    let req: GenRequest = match serde_json::from_str(&body) {
        Ok(req) => req,
        Err(e) => {
            return err(
                StatusCode::BAD_REQUEST,
                &format!("invalid request body: {}", e),
            )
        }
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
            // A typical client sends aspect_ratio + resolution instead of size.
            let ar = req.aspect_ratio.as_deref().unwrap_or("1:1");
            let res = req.resolution.as_deref().unwrap_or("1k");
            dims_from_aspect(ar, res)
        }
    };
    // The ceiling has to be applied to the final size, not inside parse_size:
    // the aspect_ratio + resolution path is the one a typical client uses, and
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
        && !st.lora_primed.load(std::sync::atomic::Ordering::SeqCst)
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

    let value: serde_json::Value = match serde_json::from_str(&resp_text) {
        Ok(v) => v,
        Err(e) => {
            error!("backend returned unparseable body: {}", e);
            return err(
                StatusCode::BAD_GATEWAY,
                "backend returned an invalid response",
            );
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

    info!(
        "generated {} image(s) in {:?}",
        data.len(),
        started.elapsed()
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "created": created,
            "data": data,
        })),
    )
}

/// POST /v1/images/edits — edit an image from one or more references.
///
/// Two facts fix the shape of this handler, and both are read from the code
/// rather than assumed:
///
/// - the caller posts JSON, not multipart, with the references as base64 data
///   URLs (the agent tool's image_edit, and the shipped client and CLI);
/// - the backend registers the edit route as multipart and takes prompt, the
///   reference files, mask, n, size and output_format from the form
///   (examples/server/routes_openai.cpp:80-144). It extracts the same
///   `<sd_cpp_extra_args>` block from the prompt that the sdapi route does
///   (:199), so steps, guidance, seed and a structured lora entry ride there.
///
/// The alternative was tried and rejected on evidence: sending the references
/// inside the extension block over /sdapi/v1/txt2img reaches the vision tower
/// but the graph then dies with "Qwen Image 2.1: missing reference image slots"
/// and SIGSEGVs (src/model/diffusion/qwen_image_2_1.hpp:120), because that
/// path never fills the `image_slots` the layout needs; the same reference
/// through the multipart route returned a 512x192 PNG in 30 s. So the bridge
/// decodes the caller's data URLs and uploads them as multipart parts, which is
/// the one path measured to work with the vision tower loaded.
async fn create_edit(
    State(st): State<std::sync::Arc<AppState>>,
    body: String,
) -> (StatusCode, Json<serde_json::Value>) {
    // Held for the whole body: the idle timer must never fire mid-edit.
    let _in_flight = InFlight::begin(&st);
    if st.cfg.llm_vision.is_none() {
        return err(
            StatusCode::NOT_IMPLEMENTED,
            "this bridge cannot edit: it was started without a vision tower \
             (IMAGEGEN_LLM_VISION / llm_vision in the configuration). Qwen Image 2.1 conditions \
             on a reference only through the vision tower \
             (src/conditioning/conditioner.hpp:2840), and generation does not need it. \
             Set the mmproj file and restart the bridge, or use image_generate",
        );
    }

    let req: EditRequest = match serde_json::from_str(&body) {
        Ok(req) => req,
        Err(e) => {
            return err(
                StatusCode::BAD_REQUEST,
                &format!("invalid request body: {}", e),
            )
        }
    };

    let refs = collect_ref_images(&req);
    if let Err(message) = validate_edit_inputs(&req.prompt, &refs) {
        return err(StatusCode::BAD_REQUEST, &message);
    }
    // Decode every reference here rather than leaving it to the backend: a
    // malformed data URL would otherwise surface as an empty conditioning and a
    // shape error deep in the engine, which reads like a model failure instead
    // of the bad request it is.
    for (index, reference) in refs.iter().enumerate() {
        if let Err(message) = decode_image_data(reference) {
            return err(
                StatusCode::BAD_REQUEST,
                &format!("reference {} is not a usable image: {}", index + 1, message),
            );
        }
    }

    // The caller's own size wins. With no size, the requested canvas is left
    // unset on purpose: the backend then sizes the edit from the reference
    // (SDGenerationParams::set_width_and_height_if_unset, common.cpp:2219),
    // which is what a caller that sends only `resolution` expects, and the
    // backend's multipart route ignores resolution as well
    // (routes_openai.cpp:123-134).
    let (width, height) = match &req.size {
        Some(s) => match parse_size(s) {
            Ok(wh) => (Some(wh.0), Some(wh.1)),
            Err(msg) => return err(StatusCode::BAD_REQUEST, &msg),
        },
        None => (None, None),
    };
    if let (Some(w), Some(h)) = (width, height) {
        if let Err(msg) = check_edge_limit(w, h) {
            return err(StatusCode::BAD_REQUEST, &msg);
        }
    }

    let n = req.n.unwrap_or(1);
    if n != 1 {
        return err(
            StatusCode::BAD_REQUEST,
            "n must be 1 (multi-image batches are not supported)",
        );
    }
    let mode = st.mode();
    if mode == Mode::Turbo && !st.cfg.turbo_available {
        return err(
            StatusCode::CONFLICT,
            "turbo is the live mode but is not available (see /health turbo_available)",
        );
    }
    let profile = st.cfg.profile(mode);
    let requested_steps = req.num_inference_steps.unwrap_or(profile.steps);
    let steps = steps_for(requested_steps, profile.sigmas);
    let cfg_scale = req.cfg_scale.unwrap_or(profile.cfg_scale);
    let seed = req.seed.unwrap_or(-1);

    // Everything the multipart form has no field of its own goes in the
    // extension block the route extracts from the prompt (routes_openai.cpp:156
    // and :199): the step count, the guidance as `guidance.txt_cfg` (what
    // SDGenerationParams::from_json_str reads, common.cpp:2066), the seed, and
    // the profile's adapters as the structured list the engine applies in order
    // (common.cpp:1841). The references themselves ride the form, not this
    // block: see the handler comment for the measurement behind that.
    let mut extra = serde_json::json!({
        "sample_params": {
            "sample_steps": steps,
            "guidance": { "txt_cfg": cfg_scale },
            "seed": seed,
        },
    });
    if !profile.loras.is_empty() {
        extra["lora"] = serde_json::Value::Array(
            profile
                .loras
                .iter()
                .map(|(path, multiplier)| {
                    serde_json::json!({ "path": path, "multiplier": multiplier })
                })
                .collect(),
        );
    }
    let prompt = format!(
        "{} {}{}{}",
        req.prompt, EXTRA_ARGS_OPEN, extra, EXTRA_ARGS_CLOSE
    );

    let mut form = reqwest::multipart::Form::new()
        .text("prompt", prompt)
        .text("n", "1");
    if let (Some(w), Some(h)) = (width, height) {
        form = form.text("size", format!("{}x{}", w, h));
    }
    for (index, reference) in refs.iter().enumerate() {
        // Decoded above, so this cannot fail here.
        let (bytes, mime) = match decode_image_data(reference) {
            Ok(decoded) => decoded,
            Err(message) => return err(StatusCode::BAD_REQUEST, &message),
        };
        let name = format!("reference-{}.img", index);
        let part = reqwest::multipart::Part::bytes(bytes.clone())
            .file_name(name.clone())
            .mime_str(&mime)
            .unwrap_or_else(|_| reqwest::multipart::Part::bytes(bytes).file_name(name));
        form = form.part("image[]", part);
    }

    info!(
        "editing with {} reference image(s) at {} mode={} steps={} cfg={} seed={}",
        refs.len(),
        match (width, height) {
            (Some(w), Some(h)) => format!("{}x{}", w, h),
            _ => "the reference's size".to_string(),
        },
        profile.mode.as_str(),
        steps,
        cfg_scale,
        seed
    );

    let started = Instant::now();
    let resp = st
        .client
        .post(format!("{}/v1/images/edits", st.backend_base))
        .multipart(form)
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            error!("backend edit request failed: {}", e);
            return err(
                StatusCode::BAD_GATEWAY,
                &format!("backend unreachable: {}", e),
            );
        }
    };

    let status = resp.status();
    let resp_text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        warn!("backend returned {} to the edit: {}", status, resp_text);
        return err(
            StatusCode::BAD_GATEWAY,
            &format!("backend edit failed ({}): {}", status, resp_text),
        );
    }

    let value: serde_json::Value = match serde_json::from_str(&resp_text) {
        Ok(v) => v,
        Err(e) => {
            error!("backend returned unparseable body to the edit: {}", e);
            return err(
                StatusCode::BAD_GATEWAY,
                "backend returned an invalid response",
            );
        }
    };

    // The multipart edit route already answers in the OpenAI shape this bridge
    // serves (routes_openai.cpp:335-360): {"created":..,"data":[{"b64_json":..}]}.
    // Validate that shape rather than trusting it, and pass the entries through
    // so the caller gets the same envelope as generation.
    let data = match value.get("data").and_then(|d| d.as_array()) {
        Some(items) if !items.is_empty() && items.iter().all(|i| i.get("b64_json").is_some()) => {
            items.to_vec()
        }
        _ => {
            warn!("backend edit response carried no image data: {}", resp_text);
            return err(
                StatusCode::BAD_GATEWAY,
                "backend returned no images for the edit",
            );
        }
    };

    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    info!("edited {} image(s) in {:?}", data.len(), started.elapsed());

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
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
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
        in_flight: std::sync::atomic::AtomicUsize::new(0),
        last_request_done: std::sync::Mutex::new(Instant::now()),
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
        // Edit bodies carry reference images as base64, which is megabytes:
        // axum's 2 MB default would reject them before the handler runs.
        .route("/v1/images/edits", post(create_edit).layer(DefaultBodyLimit::max(EDIT_BODY_LIMIT)))
        // The turbo toggle: GET reports the live mode, POST switches it. Same
        // state as the per-request "turbo" flag on a generation body.
        .route("/v1/turbo", get(get_turbo).post(set_turbo))
        .route("/health", get(health))
        .with_state(state.clone());

    // The idle shutdown: when no request has been served for
    // cfg.idle_timeout_secs, release the backend. This is what makes the
    // on-demand model pay off (docs/architecture.md, "On-demand lifecycle"):
    // it frees roughly 11 GB of host RAM and 5.6 GB of pinned VRAM, and the
    // PreToolUse hook restarts the bridge in about a second on the next
    // request. It never fires with a request in flight, and it exits 0 so a
    // supervisor's restart-on-failure does not bring it straight back.
    if cfg.idle_timeout_secs > 0 {
        let st = state.clone();
        let timeout = Duration::from_secs(cfg.idle_timeout_secs);
        tokio::spawn(async move {
            // Poll at a fraction of the timeout so the exit is prompt, and
            // refuse to fire while anything is being served.
            let tick = (timeout / 10).max(Duration::from_secs(1));
            loop {
                tokio::time::sleep(tick).await;
                if st.in_flight.load(std::sync::atomic::Ordering::SeqCst) > 0 {
                    continue;
                }
                let idle = st
                    .last_request_done
                    .lock()
                    .map(|at| at.elapsed())
                    .unwrap_or_default();
                if idle >= timeout {
                    info!(
                        "no request for {}s (idle_timeout_secs); stopping the bridge and \
                         releasing the backend",
                        idle.as_secs()
                    );
                    st.backend.kill();
                    std::process::exit(0);
                }
            }
        });
    }

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
        assert_eq!(sigmas_arg(&[1.0, 0.9375, 0.0]), "1,0.9375,0".to_string());
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
            lora: Some(
                "Qwen-Image-2.1-viggle-turbo-v0.2.1-6step-lora-r128.safetensors".to_string(),
            ),
            lora_multiplier: 1.0,
            extra_lora: None,
            extra_lora_multiplier: 1.0,
            sigmas: vec![1.0, 0.9375, 0.875, 0.75, 0.5, 0.25, 0.0],
            llm_vision: Some("mmproj-Qwen3VL-8B-Instruct-F16.gguf".to_string()),
            idle_timeout_secs: 900,
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
        assert_eq!(
            base.loras[0].0,
            "qwen-image-2.1-uncensored-lora.safetensors"
        );
        assert_eq!(base.loras[0].1, 0.8);

        let turbo = cfg.profile(Mode::Turbo);
        assert_eq!(turbo.loras.len(), 2);
        // Turbo first, the extra layer after it, in application order.
        assert_eq!(turbo.loras[0].0, cfg.lora.as_deref().unwrap());
        assert_eq!(
            turbo.loras[1].0,
            "qwen-image-2.1-uncensored-lora.safetensors"
        );
        assert_eq!(turbo.loras_arg().matches(',').count(), 1);
        // And the configured list the cache check verifies has both.
        assert_eq!(cfg.configured_loras().len(), 2);
    }

    const DATA_URL: &str =
        "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8AAAwAB/AF+7lPWAAAAAElFTkSuQmCC";

    /// The exact body the agent tool sends for a single reference: JSON with the
    /// reference as an object carrying a `url` data URL, and no `size` field
    /// (only `resolution`), which used to make the whole request unparseable.
    #[test]
    fn the_tool_single_reference_body_parses() {
        let body = format!(
            r#"{{"model":"grok-imagine-image-quality","prompt":"watercolor","n":1,
                "resolution":"1k","response_format":"b64_json",
                "image":{{"url":"{}"}}}}"#,
            DATA_URL
        );
        let req: EditRequest = serde_json::from_str(&body).unwrap();
        assert_eq!(req.prompt, "watercolor");
        assert_eq!(req.size, None);
        assert_eq!(collect_ref_images(&req), vec![DATA_URL.to_string()]);
    }

    #[test]
    fn the_tool_multiple_reference_body_parses_in_order() {
        let body = format!(
            r#"{{"prompt":"mix them","n":1,"resolution":"1k","aspect_ratio":"16:9",
                "images":[{{"url":"{}"}},{{"url":"{}"}}]}}"#,
            DATA_URL, DATA_URL
        );
        let req: EditRequest = serde_json::from_str(&body).unwrap();
        assert_eq!(
            collect_ref_images(&req),
            vec![DATA_URL.to_string(), DATA_URL.to_string()]
        );
    }

    /// The shipped client and CLI use the plain string form, so it must keep
    /// working: `image` as one string, `images` as an array of strings.
    #[test]
    fn bare_data_url_strings_are_accepted_too() {
        let single: EditRequest =
            serde_json::from_str(&format!(r#"{{"prompt":"p","image":"{}"}}"#, DATA_URL)).unwrap();
        assert_eq!(collect_ref_images(&single), vec![DATA_URL.to_string()]);

        let many: EditRequest = serde_json::from_str(&format!(
            r#"{{"prompt":"p","images":["{}","{}"]}}"#,
            DATA_URL, DATA_URL
        ))
        .unwrap();
        assert_eq!(collect_ref_images(&many).len(), 2);
    }

    #[test]
    fn edit_inputs_require_a_reference_and_refuse_blank_ones() {
        let err = validate_edit_inputs("no reference", &[]).unwrap_err();
        assert!(err.contains("at least one reference"), "got: {err}");

        let err = validate_edit_inputs("blank", &["   ".to_string()]).unwrap_err();
        assert!(err.contains("empty reference"), "got: {err}");

        let err = validate_edit_inputs("  ", &[DATA_URL.to_string()]).unwrap_err();
        assert!(err.contains("prompt is required"), "got: {err}");
    }

    /// A prompt holding the marker would make sd.cpp's non-greedy extractor
    /// parse the wrong block, so it is refused rather than passed on.
    #[test]
    fn edit_inputs_reject_a_prompt_containing_the_marker() {
        let err = validate_edit_inputs(
            &format!("try {EXTRA_ARGS_OPEN}{{\"steps\":1}}{EXTRA_ARGS_CLOSE}"),
            &[DATA_URL.to_string()],
        )
        .unwrap_err();
        assert!(err.contains("must not contain"), "got: {err}");
    }

    /// A data URL decodes to its bytes and carries its own mime type; junk and
    /// empty payloads are refused rather than forwarded.
    #[test]
    fn decode_image_data_reads_a_data_url_and_rejects_junk() {
        let (bytes, mime) = decode_image_data(DATA_URL).unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(&bytes[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);

        assert!(decode_image_data("not base64 !!!").is_err());
        assert!(decode_image_data("data:image/png,raw-not-base64").is_err());
        assert!(decode_image_data("data:image/png;base64,").is_err());
        assert!(decode_image_data("   ").is_err());
    }

    /// The idle timeout is the lever that releases the pinned VRAM, so its
    /// guards are pinned: 0 disables it, and the timer must not be able to fire
    /// while a request is in flight.
    #[test]
    fn the_idle_timeout_is_opt_out_and_cannot_fire_mid_request() {
        let mut cfg = turbo_capable_config();
        assert_eq!(cfg.idle_timeout_secs, 900);
        cfg.idle_timeout_secs = 0;
        assert_eq!(cfg.idle_timeout_secs, 0, "0 means never exit on idle");

        // The handle owns a real short-lived child so nothing is faked.
        let child = Command::new("true")
            .spawn()
            .expect("spawn a harmless child for the handle");
        let state = std::sync::Arc::new(AppState {
            client: reqwest::Client::new(),
            cfg: turbo_capable_config(),
            backend_base: "http://127.0.0.1:8788".to_string(),
            backend: std::sync::Arc::new(BackendHandle::new(child)),
            turbo: std::sync::atomic::AtomicBool::new(false),
            lora_primed: std::sync::atomic::AtomicBool::new(false),
            lora_visible: std::sync::Mutex::new(None),
            in_flight: std::sync::atomic::AtomicUsize::new(0),
            last_request_done: std::sync::Mutex::new(Instant::now()),
        });
        assert_eq!(state.in_flight.load(std::sync::atomic::Ordering::SeqCst), 0);
        {
            let _busy = InFlight::begin(&state);
            assert_eq!(state.in_flight.load(std::sync::atomic::Ordering::SeqCst), 1);
        }
        assert_eq!(
            state.in_flight.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the guard must release the count so the idle timer can fire again"
        );
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
        let req: GenRequest = serde_json::from_str(r#"{"prompt": "x", "turbo": true}"#).unwrap();
        assert_eq!(req.turbo, Some(true));
        let req: GenRequest = serde_json::from_str(r#"{"prompt": "x", "turbo": false}"#).unwrap();
        assert_eq!(req.turbo, Some(false));
        // Absent means "whatever the live mode is".
        let req: GenRequest = serde_json::from_str(r#"{"prompt": "x"}"#).unwrap();
        assert_eq!(req.turbo, None);
    }
}
