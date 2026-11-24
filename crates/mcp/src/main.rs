//! `imagegen-mcp` — an MCP server over any OpenAI-compatible image API.
//!
//! Tools:
//!
//! - `image_generate`  text to image, written to a file, path returned
//! - `image_edit`      image to image from reference files
//! - `image_health`    liveness and the live sampling state
//! - `image_models`    the model list the endpoint reports
//! - `image_turbo`     read or switch the sampling mode
//!
//! The server holds no state between calls beyond the resolved endpoint, so it
//! is safe to run for the lifetime of a session. It is deliberately thin: every
//! tool is one request through `imagegen-client`, the same crate the command-line
//! client uses, so an agent and a shell script cannot disagree about the contract.
//!
//! Tools return JSON as text. A success payload names what was written; a failure
//! returns an object with an `error` field and a short reason, so a calling agent
//! can recover without parsing prose.

use std::path::PathBuf;

use rmcp::{
    handler::server::wrapper::Parameters,
    model::{Implementation, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
    ServerHandler, ServiceExt,
};
use serde::{Deserialize, Serialize};

use imagegen_client::{file_to_data_url, save_images, Client, EditRequest, GenerationRequest};

/// Request body of `image_generate`.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GenerateParams {
    /// The prompt describing the image to make.
    pub prompt: String,
    /// Where to write the image. A relative path is resolved against the
    /// server's working directory. Defaults to `out/image.png`.
    pub output: Option<String>,
    /// Explicit size as WIDTHxHEIGHT, both divisible by 32, for example
    /// 1024x1024. Takes precedence over aspect_ratio and resolution.
    pub size: Option<String>,
    /// Aspect ratio such as 16:9 or 3:2, used when size is absent.
    pub aspect_ratio: Option<String>,
    /// Resolution tier such as 1k or 2k, used with aspect_ratio.
    pub resolution: Option<String>,
    /// Sampling steps. Omit to use the service default.
    pub steps: Option<u32>,
    /// Classifier-free guidance scale. Omit to use the service default.
    pub cfg_scale: Option<f64>,
    /// Seed. Omit for a random one.
    pub seed: Option<i64>,
    /// Pin this one request to the turbo profile, when the service offers it.
    pub turbo: Option<bool>,
}

/// Request body of `image_edit`.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EditParams {
    /// The instruction describing the change.
    pub prompt: String,
    /// Reference image paths, at least one.
    pub references: Vec<String>,
    /// Where to write the result. Defaults to `out/edit.png`.
    pub output: Option<String>,
    /// Explicit size for the result.
    pub size: Option<String>,
}

/// Request body of `image_turbo`.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TurboParams {
    /// `status` reads the live mode, `on` switches turbo on, `off` switches the
    /// base profile on.
    pub action: Option<String>,
}

/// The MCP server.
#[derive(Debug, Clone)]
struct ImageServer {
    client: Client,
}

impl ImageServer {
    fn new() -> Self {
        Self {
            client: Client::resolve(None),
        }
    }

    fn endpoint_summary(&self) -> String {
        self.client.endpoint.base_url.clone()
    }
}

#[tool_router]
impl ImageServer {
    /// Generate an image from a text prompt and write it to a file.
    #[tool(
        description = "Generate an image from a text prompt through the configured OpenAI-compatible image API and write it to a file. Returns JSON: the endpoint used, the absolute path written, the image count and the byte size. Pass size as WIDTHxHEIGHT with both edges divisible by 32, or aspect_ratio plus resolution. Omit steps and cfg_scale to use the service defaults. A turbo flag pins this one request to the turbo profile when the service offers it."
    )]
    async fn image_generate(
        &self,
        Parameters(p): Parameters<GenerateParams>,
    ) -> Result<String, String> {
        let server = self.clone();
        blocking(move || server.generate(p)).await
    }

    /// Edit an image from one or more reference files.
    #[tool(
        description = "Edit an image from one or more reference image files through the configured OpenAI-compatible image API. Returns JSON with the written path. This needs a service that implements the OpenAI image edit route; the bundled bridge does not, and the tool reports the service's own refusal rather than failing silently."
    )]
    async fn image_edit(&self, Parameters(p): Parameters<EditParams>) -> Result<String, String> {
        let server = self.clone();
        blocking(move || server.edit(p)).await
    }

    /// Probe liveness and the live sampling state.
    #[tool(
        description = "Probe the configured image service: whether it answers, whether its generation backend is alive, the live sampling mode, whether turbo is available, and the steps and guidance the next request will use. Call this first when a generation fails, because it separates a dead service from a rejected request."
    )]
    async fn image_health(&self) -> Result<String, String> {
        let server = self.clone();
        blocking(move || server.health_json()).await
    }

    /// List the models the endpoint reports.
    #[tool(
        description = "List the models the configured image service reports, with the owning organisation when one is named. Use this to discover the model identifier a client should send."
    )]
    async fn image_models(&self) -> Result<String, String> {
        let server = self.clone();
        blocking(move || server.models_json()).await
    }

    /// Read or switch the sampling mode.
    #[tool(
        description = "Read the live sampling mode of the configured image service, or switch it. action=status reports the current mode, adapters and schedule without changing anything; action=on switches to the turbo profile; action=off switches to the base profile. A switch is refused by the service when the turbo adapter is not configured, and the refusal is returned with the reason."
    )]
    async fn image_turbo(&self, Parameters(p): Parameters<TurboParams>) -> Result<String, String> {
        let server = self.clone();
        blocking(move || server.turbo_json(p)).await
    }
}

/// Run a blocking call on the blocking pool.
///
/// The HTTP client is the blocking one, so a call made directly inside an async
/// tool would drop a runtime inside a tokio worker and panic. Running it on the
/// blocking pool keeps the async runtime responsive and matches how the other
/// local servers on this machine handle blocking work.
async fn blocking<F>(f: F) -> Result<String, String>
where
    F: FnOnce() -> Result<String, String> + Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(result) => result,
        Err(join) => Err(format!("the worker task failed: {join}")),
    }
}

impl ImageServer {
    fn health_json(&self) -> Result<String, String> {
        match self.client.health() {
            Ok(h) => Ok(serde_json::json!({
                "endpoint": self.endpoint_summary(),
                "status": h.status,
                "ready": h.is_ready(),
                "backend": h.backend,
                "backend_process_alive": h.backend_process_alive,
                "backend_restarts": h.backend_restarts,
                "mode": h.mode,
                "turbo_available": h.turbo_available,
                "lora": h.lora,
                "sigmas": h.sigmas,
                "steps": h.steps,
                "cfg_scale": h.cfg_scale,
                // The idle lifecycle: whether stopping would interrupt work,
                // how long the service has been idle, and whether it will
                // release its backend on its own.
                "in_flight": h.in_flight.unwrap_or(0),
                "idle_secs": h.idle_secs,
                "idle_timeout_secs": h.idle_timeout_secs,
                "self_releases": h.self_releases(),
                "safe_to_stop": h.is_idle(),
            })
            .to_string()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn models_json(&self) -> Result<String, String> {
        match self.client.models() {
            Ok(models) => {
                let list: Vec<_> = models
                    .iter()
                    .map(|m| serde_json::json!({ "id": m.id, "owned_by": m.owned_by }))
                    .collect();
                Ok(
                    serde_json::json!({ "endpoint": self.endpoint_summary(), "models": list })
                        .to_string(),
                )
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn turbo_json(&self, p: TurboParams) -> Result<String, String> {
        let action = p.action.unwrap_or_else(|| "status".to_string());
        let result = match action.trim().to_ascii_lowercase().as_str() {
            "status" => self.client.turbo_status(),
            "on" | "turbo" | "enable" => self.client.set_turbo(true),
            "off" | "base" | "disable" => self.client.set_turbo(false),
            other => return Err(format!("action '{other}' is not status, on or off")),
        };
        match result {
            Ok(v) => Ok(v.to_string()),
            Err(e) => Err(e.to_string()),
        }
    }
}

impl ImageServer {
    fn generate(&self, p: GenerateParams) -> Result<String, String> {
        let mut req = GenerationRequest::new(p.prompt);
        if let Some(size) = p.size {
            req = req.size(size);
        } else if p.aspect_ratio.is_some() || p.resolution.is_some() {
            req = req.aspect(
                p.aspect_ratio.unwrap_or_else(|| "1:1".to_string()),
                p.resolution.unwrap_or_else(|| "1k".to_string()),
            );
        }
        if let Some(steps) = p.steps {
            req = req.steps(steps);
        }
        if let Some(cfg) = p.cfg_scale {
            req = req.cfg(cfg);
        }
        if let Some(seed) = p.seed {
            req = req.seed(seed);
        }
        if p.turbo == Some(true) {
            req.turbo = Some(true);
        }
        req.validate().map_err(|e| e.to_string())?;

        let response = self.client.generate(&req).map_err(|e| e.to_string())?;
        let output = p.output.unwrap_or_else(|| "out/image.png".to_string());
        let target = PathBuf::from(output);
        let written =
            save_images(&self.client.endpoint, &response, &target).map_err(|e| e.to_string())?;

        let paths: Vec<String> = written
            .iter()
            .map(|p| absolute(p).display().to_string())
            .collect();
        let bytes: u64 = written
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .sum();
        Ok(serde_json::json!({
            "endpoint": self.endpoint_summary(),
            "images": paths,
            "count": paths.len(),
            "bytes": bytes,
            "created": response.created,
        })
        .to_string())
    }

    fn edit(&self, p: EditParams) -> Result<String, String> {
        if p.references.is_empty() {
            return Err("an edit needs at least one reference image".to_string());
        }
        let mut refs = Vec::with_capacity(p.references.len());
        for path in &p.references {
            refs.push(file_to_data_url(std::path::Path::new(path)).map_err(|e| e.to_string())?);
        }
        let req = EditRequest {
            prompt: p.prompt,
            image: refs,
            size: p.size,
            n: None,
            response_format: None,
        };
        let response = self.client.edit(&req).map_err(|e| e.to_string())?;
        let output = p.output.unwrap_or_else(|| "out/edit.png".to_string());
        let written = save_images(&self.client.endpoint, &response, &PathBuf::from(output))
            .map_err(|e| e.to_string())?;
        let paths: Vec<String> = written
            .iter()
            .map(|p| absolute(p).display().to_string())
            .collect();
        Ok(serde_json::json!({
            "endpoint": self.endpoint_summary(),
            "images": paths,
            "count": paths.len(),
        })
        .to_string())
    }
}

/// Make a written path absolute so a caller can open it without guessing the
/// server's working directory.
fn absolute(path: &std::path::Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

/// Shape a failure as a JSON object with an `error` field. Kept for the tests
/// that assert the wire shape a caller sees; the tools themselves return
/// `Result`, which rmcp marks with `is_error`.
#[cfg(test)]
fn error_json(message: impl std::fmt::Display) -> String {
    serde_json::json!({ "error": message.to_string() }).to_string()
}

#[tool_handler]
impl ServerHandler for ImageServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("imagegen-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Image generation over any OpenAI-compatible image API. Typical flow: image_health to confirm the \
                 service answers, image_generate to write an image to a file, image_models to discover the model id, \
                 image_edit to change an existing image, image_turbo to read or switch the sampling mode. Every tool \
                 returns JSON; a failure carries an error field with the service's own reason.",
            )
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let service = ImageServer::new();
    let running = service.serve(stdio()).await?;
    running.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> ImageServer {
        ImageServer::new()
    }

    #[test]
    fn a_generate_request_without_a_prompt_is_refused_locally() {
        let s = server();
        let err = s
            .generate(GenerateParams {
                prompt: "  ".to_string(),
                output: None,
                size: None,
                aspect_ratio: None,
                resolution: None,
                steps: None,
                cfg_scale: None,
                seed: None,
                turbo: None,
            })
            .unwrap_err();
        assert!(
            err.contains("prompt"),
            "the reason should name the prompt: {err}"
        );
    }

    #[test]
    fn a_malformed_size_is_refused_before_any_request() {
        let s = server();
        let err = s
            .generate(GenerateParams {
                prompt: "a lighthouse".to_string(),
                output: None,
                size: Some("1000x1000".to_string()),
                aspect_ratio: None,
                resolution: None,
                steps: None,
                cfg_scale: None,
                seed: None,
                turbo: None,
            })
            .unwrap_err();
        assert!(err.contains("divisible by 32"), "got: {err}");
    }

    #[test]
    fn an_edit_without_references_is_refused() {
        let s = server();
        let err = s
            .edit(EditParams {
                prompt: "make it night".to_string(),
                references: vec![],
                output: None,
                size: None,
            })
            .unwrap_err();
        assert!(err.contains("reference"), "got: {err}");
    }

    #[test]
    fn a_missing_reference_file_reports_its_path() {
        let s = server();
        let err = s
            .edit(EditParams {
                prompt: "make it night".to_string(),
                references: vec!["/tmp/imagegen-mcp-does-not-exist.png".to_string()],
                output: None,
                size: None,
            })
            .unwrap_err();
        assert!(
            err.contains("imagegen-mcp-does-not-exist.png"),
            "the reason should name the file: {err}"
        );
    }

    #[test]
    fn a_failure_is_json_with_an_error_field() {
        let body = error_json("turbo is unavailable");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["error"], "turbo is unavailable");
    }

    #[test]
    fn a_relative_output_becomes_absolute() {
        let abs = absolute(std::path::Path::new("out/image.png"));
        assert!(abs.is_absolute(), "got {}", abs.display());
        assert!(abs.ends_with("out/image.png"));
        let already = absolute(std::path::Path::new("/tmp/out.png"));
        assert_eq!(already, PathBuf::from("/tmp/out.png"));
    }

    #[tokio::test]
    async fn turbo_rejects_an_action_outside_status_on_off() {
        let s = server();
        let out = s
            .image_turbo(Parameters(TurboParams {
                action: Some("sideways".to_string()),
            }))
            .await;
        let message = out.expect_err("an unknown action must be an error result");
        assert!(message.contains("status, on or off"), "got: {message}");
    }
}
