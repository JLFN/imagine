//! `imagegen-cli` — a command-line client for any OpenAI-compatible image API.
//!
//! It exists so the same service an agent reaches through `imagegen-mcp` can be
//! driven from a shell, a script, or a CI job, with no client application in the
//! path. Every subcommand is one request to the endpoint the client crate
//! resolves; nothing is cached between runs.
//!
//! Configuration order, highest priority first: a command-line flag, then the
//! environment, then the built-in default. The endpoint default is the bundled
//! bridge. Secrets are read from the environment only, never from a flag, so a
//! key does not land in shell history or a process listing.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};

use imagegen_client::{
    file_to_data_url, save_images, Client, EditRequest, Endpoint, GenerationRequest, ENV_BASE_URL,
};

/// Client for an OpenAI-compatible image generation API.
#[derive(Parser, Debug)]
#[command(
    name = "imagegen-cli",
    version,
    about = "Generate and edit images through any OpenAI-compatible image API",
    long_about = "Generate and edit images through any OpenAI-compatible image API. \
                  The default endpoint is a local bridge; point --base-url at any provider \
                  that implements the OpenAI image routes. The API key is read from \
                  IMAGEGEN_API_KEY, never from a flag."
)]
struct Cli {
    /// Base URL including the version segment.
    #[arg(long, global = true, env = ENV_BASE_URL, value_name = "URL")]
    base_url: Option<String>,

    /// Request timeout in seconds.
    #[arg(long, global = true, value_name = "SECONDS", default_value_t = 300)]
    timeout: u64,

    /// Print the answer as JSON instead of a summary line.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Generate an image from a text prompt.
    Generate(GenerateArgs),
    /// Edit an image from one or more reference images.
    Edit(EditArgs),
    /// List the models the endpoint reports.
    Models,
    /// Probe liveness and the live sampling state.
    Health,
    /// Report the idle lifecycle: whether a stop would interrupt work, how long
    /// the service has been idle, and whether it will release its backend on its
    /// own. Exits 0 when it is safe to stop, 1 when a request is in flight.
    Lifecycle,
    /// Read or switch the sampling mode.
    Turbo(TurboArgs),
}

#[derive(Args, Debug)]
struct GenerateArgs {
    /// The prompt.
    #[arg(short, long)]
    prompt: String,

    /// Where to write the image, or the first image when n is above one.
    #[arg(short, long, value_name = "PATH", default_value = "out/image.png")]
    output: PathBuf,

    /// Explicit size, width by height, both divisible by 32.
    #[arg(long, value_name = "WxH", conflicts_with_all = ["aspect", "resolution"])]
    size: Option<String>,

    /// Aspect ratio such as 16:9, used with --resolution.
    #[arg(long, value_name = "RATIO")]
    aspect: Option<String>,

    /// Resolution tier such as 1k, used with --aspect.
    #[arg(long, value_name = "TIER")]
    resolution: Option<String>,

    /// Sampling steps.
    #[arg(long, value_name = "N")]
    steps: Option<u32>,

    /// Classifier-free guidance scale.
    #[arg(long, value_name = "SCALE")]
    cfg: Option<f64>,

    /// Seed; omit for a random one.
    #[arg(long, value_name = "N")]
    seed: Option<i64>,

    /// Pin this request to the turbo profile, when the endpoint offers it.
    #[arg(long)]
    turbo: bool,
}

#[derive(Args, Debug)]
struct EditArgs {
    /// The instruction for the change.
    #[arg(short, long)]
    prompt: String,

    /// A reference image; repeat for several.
    #[arg(short, long = "reference", value_name = "PATH", required = true)]
    reference: Vec<PathBuf>,

    /// Where to write the result.
    #[arg(short, long, value_name = "PATH", default_value = "out/edit.png")]
    output: PathBuf,

    /// Explicit size for the result.
    #[arg(long, value_name = "WxH")]
    size: Option<String>,
}

#[derive(Args, Debug)]
struct TurboArgs {
    /// Switch turbo on, off, or print the current mode.
    #[arg(value_enum, default_value_t = TurboAction::Status)]
    action: TurboAction,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum TurboAction {
    /// Print the live mode.
    Status,
    /// Switch the turbo profile on.
    On,
    /// Switch the base profile on.
    Off,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("imagegen-cli: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let endpoint = Endpoint::resolve(cli.base_url.as_deref()).with_timeout_secs(cli.timeout);
    if endpoint.base_url.is_empty() {
        bail!("no endpoint: pass --base-url or set {ENV_BASE_URL}");
    }
    let client = Client::new(endpoint);

    match cli.command {
        Command::Generate(args) => generate(&client, args, cli.json),
        Command::Edit(args) => edit(&client, args, cli.json),
        Command::Models => models(&client, cli.json),
        Command::Health => health(&client, cli.json),
        Command::Lifecycle => lifecycle(&client, cli.json),
        Command::Turbo(args) => turbo(&client, args, cli.json),
    }
}

fn generate(client: &Client, args: GenerateArgs, json: bool) -> Result<()> {
    let mut req = GenerationRequest::new(args.prompt);
    if let Some(size) = args.size {
        req = req.size(size);
    } else if args.aspect.is_some() || args.resolution.is_some() {
        req = req.aspect(
            args.aspect.unwrap_or_else(|| "1:1".to_string()),
            args.resolution.unwrap_or_else(|| "1k".to_string()),
        );
    }
    if let Some(steps) = args.steps {
        req = req.steps(steps);
    }
    if let Some(cfg) = args.cfg {
        req = req.cfg(cfg);
    }
    if let Some(seed) = args.seed {
        req = req.seed(seed);
    }
    if args.turbo {
        req.turbo = Some(true);
    }

    let response = client.generate(&req).context("generation failed")?;
    let written = save_images(&client.endpoint, &response, &args.output)
        .context("writing the generated image failed")?;

    if json {
        let payload = serde_json::json!({
            "created": response.created,
            "images": written.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        for path in &written {
            println!("wrote {}", path.display());
        }
    }
    Ok(())
}

fn edit(client: &Client, args: EditArgs, json: bool) -> Result<()> {
    let mut refs = Vec::with_capacity(args.reference.len());
    for path in &args.reference {
        refs.push(file_to_data_url(path).with_context(|| format!("reading {}", path.display()))?);
    }
    let req = EditRequest {
        prompt: args.prompt,
        image: refs,
        size: args.size,
        n: None,
        response_format: None,
    };
    let response = client.edit(&req).context("edit failed")?;
    let written = save_images(&client.endpoint, &response, &args.output)
        .context("writing the edited image failed")?;
    if json {
        let payload = serde_json::json!({
            "created": response.created,
            "images": written.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        for path in &written {
            println!("wrote {}", path.display());
        }
    }
    Ok(())
}

fn models(client: &Client, json: bool) -> Result<()> {
    let models = client.models().context("listing models failed")?;
    if json {
        let payload: Vec<_> = models
            .iter()
            .map(|m| serde_json::json!({ "id": m.id, "owned_by": m.owned_by }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else if models.is_empty() {
        println!("the endpoint reported no models");
    } else {
        for model in &models {
            match &model.owned_by {
                Some(owner) => println!("{} ({owner})", model.id),
                None => println!("{}", model.id),
            }
        }
    }
    Ok(())
}

fn lifecycle(client: &Client, json: bool) -> Result<()> {
    let health = client.health().context("lifecycle probe failed")?;
    let in_flight = health.in_flight.unwrap_or(0);
    let safe = health.is_idle();
    if json {
        let payload = serde_json::json!({
            "endpoint": client.endpoint.base_url,
            "ready": health.is_ready(),
            "in_flight": in_flight,
            "idle_secs": health.idle_secs,
            "idle_timeout_secs": health.idle_timeout_secs,
            "self_releases": health.self_releases(),
            "safe_to_stop": safe,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("endpoint   {}", client.endpoint.base_url);
        println!("in_flight  {in_flight}");
        match health.idle_secs {
            Some(secs) => println!("idle_secs  {secs}"),
            None => println!("idle_secs  (not reported by this service)"),
        }
        match health.idle_timeout_secs {
            Some(secs) if secs > 0 => {
                println!("idle_shutdown {secs} s (the service releases its backend itself)")
            }
            Some(_) => println!("idle_shutdown disabled (the process runs until it is killed)"),
            None => println!("idle_shutdown (not reported by this service)"),
        }
        println!("safe_to_stop {}", if safe { "yes" } else { "no" });
    }
    // A script must be able to branch on this without parsing prose: a request
    // in flight means a stop would throw the generation away.
    if !safe {
        std::process::exit(1);
    }
    Ok(())
}

fn health(client: &Client, json: bool) -> Result<()> {
    let health = client.health().context("health probe failed")?;
    if json {
        let payload = serde_json::json!({
            "status": health.status,
            "ready": health.is_ready(),
            "backend": health.backend,
            "backend_process_alive": health.backend_process_alive,
            "backend_restarts": health.backend_restarts,
            "mode": health.mode,
            "turbo_available": health.turbo_available,
            "lora": health.lora,
            "sigmas": health.sigmas,
            "steps": health.steps,
            "cfg_scale": health.cfg_scale,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("endpoint  {}", client.endpoint.base_url);
        println!("status    {}", health.status);
        println!("ready     {}", if health.is_ready() { "yes" } else { "no" });
        if let Some(mode) = &health.mode {
            println!("mode      {mode}");
        }
        if let Some(steps) = health.steps {
            println!("steps     {steps}");
        }
        if let Some(cfg) = health.cfg_scale {
            println!("cfg_scale {cfg}");
        }
        if let Some(backend) = health.backend {
            println!(
                "backend   {}",
                if backend {
                    "answering"
                } else {
                    "not answering"
                }
            );
        }
    }
    Ok(())
}

fn turbo(client: &Client, args: TurboArgs, json: bool) -> Result<()> {
    let value = match args.action {
        TurboAction::Status => client.turbo_status(),
        TurboAction::On => client.set_turbo(true),
        TurboAction::Off => client.set_turbo(false),
    }
    .context("sampling mode request failed")?;

    if json {
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    let mode = value
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    println!("mode            {mode}");
    if let Some(available) = value.get("turbo_available").and_then(|v| v.as_bool()) {
        println!("turbo_available {available}");
    }
    if let Some(lora) = value.get("lora").and_then(|v| v.as_str()) {
        println!("lora            {lora}");
    }
    if let Some(sigmas) = value.get("sigmas").and_then(|v| v.as_str()) {
        println!("sigmas          {sigmas}");
    }
    if let Some(steps) = value.get("steps").and_then(|v| v.as_u64()) {
        println!("steps           {steps}");
    }
    if let Some(cfg) = value.get("cfg_scale").and_then(|v| v.as_f64()) {
        println!("cfg_scale       {cfg}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_command_tree_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn the_default_endpoint_is_the_bundled_bridge() {
        let endpoint = imagegen_client::DEFAULT_BASE_URL;
        assert!(endpoint.starts_with("http://"));
        assert!(endpoint.ends_with("/v1"));
    }

    #[test]
    fn generate_parses_a_full_invocation() {
        let cli = Cli::try_parse_from([
            "imagegen-cli",
            "generate",
            "-p",
            "a lighthouse at dusk",
            "--size",
            "1024x1024",
            "--steps",
            "28",
            "--seed",
            "42",
            "--cfg",
            "6.0",
            "-o",
            "out/light.png",
        ])
        .expect("the invocation is valid");
        match cli.command {
            Command::Generate(args) => {
                assert_eq!(args.prompt, "a lighthouse at dusk");
                assert_eq!(args.size.as_deref(), Some("1024x1024"));
                assert_eq!(args.steps, Some(28));
                assert_eq!(args.seed, Some(42));
                assert_eq!(args.output, PathBuf::from("out/light.png"));
            }
            other => panic!("expected generate, got {other:?}"),
        }
    }

    #[test]
    fn lifecycle_is_a_command_of_its_own() {
        let cli = Cli::try_parse_from(["imagegen-cli", "lifecycle"])
            .expect("the lifecycle command parses");
        assert!(matches!(cli.command, Command::Lifecycle));
    }

    #[test]
    fn size_and_aspect_cannot_both_be_set() {
        let parsed = Cli::try_parse_from([
            "imagegen-cli",
            "generate",
            "-p",
            "x",
            "--size",
            "1024x1024",
            "--aspect",
            "16:9",
        ]);
        assert!(parsed.is_err(), "the two size forms must conflict");
    }

    #[test]
    fn edit_requires_at_least_one_reference() {
        let parsed = Cli::try_parse_from(["imagegen-cli", "edit", "-p", "make it night"]);
        assert!(
            parsed.is_err(),
            "an edit without a reference must be refused"
        );
        let parsed =
            Cli::try_parse_from(["imagegen-cli", "edit", "-p", "make it night", "-r", "a.png"]);
        assert!(parsed.is_ok());
    }

    #[test]
    fn turbo_defaults_to_status() {
        let cli = Cli::try_parse_from(["imagegen-cli", "turbo"]).unwrap();
        match cli.command {
            Command::Turbo(args) => assert_eq!(args.action, TurboAction::Status),
            other => panic!("expected turbo, got {other:?}"),
        }
    }
}
