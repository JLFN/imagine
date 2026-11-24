//! Shared client for an OpenAI-compatible image API.
//!
//! One implementation backs both `imagegen-cli` and `imagegen-mcp`, so the shell
//! and an agent session cannot drift apart in how they address the service.
//!
//! The object of this client is any endpoint that speaks the OpenAI image API:
//! the bundled bridge (`crates/bridge`), `stable-diffusion.cpp`'s own server, or
//! a hosted provider reached through the same routes. Nothing here is specific
//! to one vendor or one client application.
//!
//! Routes used:
//!
//! - `GET  {base}/models`                the model list
//! - `POST {base}/images/generations`    text to image
//! - `POST {base}/images/edits`          image to image, when the server offers it
//! - `GET  {base}/turbo`                 the sampling mode this bridge reports
//! - `POST {base}/turbo`                 switch the sampling mode
//! - `GET  {base}/health`                liveness and backend state
//!
//! `base` includes the version segment (`http://127.0.0.1:8787/v1`) because the
//! OpenAI routes live under it. `/health` sits outside the version segment on the
//! bundled bridge, so it is addressed by its own join.

use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use serde::{Deserialize, Serialize};

/// Default endpoint of the bundled bridge.
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:8787/v1";

/// Environment variable that overrides the endpoint.
pub const ENV_BASE_URL: &str = "IMAGEGEN_BASE_URL";

/// Environment variable that supplies the API key, when the endpoint needs one.
pub const ENV_API_KEY: &str = "IMAGEGEN_API_KEY";

/// Environment variable that sets the request timeout in seconds.
pub const ENV_TIMEOUT_SECS: &str = "IMAGEGEN_TIMEOUT_SECS";

/// A failure from the endpoint, from the transport, or from local file work.
#[derive(Debug)]
pub enum ClientError {
    /// The request could not be built or the response could not be read.
    Transport(String),
    /// The server answered with a non-success status.
    Status {
        /// HTTP status code.
        status: u16,
        /// The message the server reported, when it reported one.
        message: String,
    },
    /// The server answered successfully but the body did not have the shape the
    /// OpenAI image API defines.
    Shape(String),
    /// The request was refused locally, before any round trip.
    Invalid(String),
    /// A local file operation failed while writing or reading an image.
    Io(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Transport(m) => write!(f, "request failed: {m}"),
            ClientError::Status { status, message } => {
                write!(f, "endpoint answered HTTP {status}: {message}")
            }
            ClientError::Shape(m) => write!(f, "unexpected response shape: {m}"),
            ClientError::Invalid(m) => write!(f, "invalid request: {m}"),
            ClientError::Io(m) => write!(f, "file error: {m}"),
        }
    }
}

impl std::error::Error for ClientError {}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, ClientError>;

/// Connection settings for one endpoint.
#[derive(Debug, Clone)]
pub struct Endpoint {
    /// Base URL including the version segment, without a trailing slash.
    pub base_url: String,
    /// Bearer token, when the endpoint requires one. Never logged.
    pub api_key: Option<String>,
    /// Total request timeout.
    pub timeout: Duration,
}

impl Default for Endpoint {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: None,
            timeout: Duration::from_secs(300),
        }
    }
}

impl Endpoint {
    /// Build an endpoint, normalising a trailing slash off the base URL.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: normalise_base(&base_url.into()),
            ..Self::default()
        }
    }

    /// Attach a bearer token.
    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        let key = key.into();
        self.api_key = if key.trim().is_empty() {
            None
        } else {
            Some(key)
        };
        self
    }

    /// Set the request timeout in seconds.
    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.timeout = Duration::from_secs(secs.max(1));
        self
    }

    /// Resolve the endpoint from the first source that supplies a value:
    /// an explicit argument, then the environment, then the default.
    pub fn resolve(explicit: Option<&str>) -> Self {
        let base = explicit
            .map(str::to_string)
            .or_else(|| std::env::var(ENV_BASE_URL).ok())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let mut endpoint = Self::new(base);
        if let Ok(key) = std::env::var(ENV_API_KEY) {
            endpoint = endpoint.with_api_key(key);
        }
        if let Ok(secs) = std::env::var(ENV_TIMEOUT_SECS) {
            if let Ok(n) = secs.parse::<u64>() {
                endpoint = endpoint.with_timeout_secs(n);
            }
        }
        endpoint
    }

    /// The origin without the version segment, used for routes that sit beside it.
    fn origin(&self) -> String {
        match self.base_url.rsplit_once('/') {
            Some((head, tail))
                if tail.starts_with('v') && tail[1..].chars().all(|c| c.is_ascii_digit()) =>
            {
                head.to_string()
            }
            _ => self.base_url.clone(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base_url, path.trim_start_matches('/'))
    }

    fn side_url(&self, path: &str) -> String {
        format!("{}/{}", self.origin(), path.trim_start_matches('/'))
    }

    fn client(&self) -> Result<reqwest::blocking::Client> {
        reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .build()
            .map_err(|e| ClientError::Transport(e.to_string()))
    }

    fn auth(&self, rb: reqwest::blocking::RequestBuilder) -> reqwest::blocking::RequestBuilder {
        match &self.api_key {
            Some(key) => rb.bearer_auth(key),
            None => rb,
        }
    }
}

/// Strip trailing slashes from a base URL.
pub fn normalise_base(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

/// One generated or edited image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Image {
    /// Base64-encoded image bytes, when the server answered inline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b64_json: Option<String>,
    /// A URL the image can be fetched from, when the server answered by reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// What the server said it revised, when it says anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revised_prompt: Option<String>,
}

impl Image {
    /// The decoded bytes, fetching the URL when the server answered by reference.
    pub fn bytes(&self, endpoint: &Endpoint) -> Result<Vec<u8>> {
        if let Some(b64) = &self.b64_json {
            return base64::engine::general_purpose::STANDARD
                .decode(b64.trim())
                .map_err(|e| ClientError::Shape(format!("b64_json is not valid base64: {e}")));
        }
        if let Some(url) = &self.url {
            let client = endpoint.client()?;
            let resp = endpoint
                .auth(client.get(url))
                .send()
                .map_err(|e| ClientError::Transport(e.to_string()))?;
            let status = resp.status();
            if !status.is_success() {
                return Err(ClientError::Status {
                    status: status.as_u16(),
                    message: format!("image download from {url} failed"),
                });
            }
            return resp
                .bytes()
                .map(|b| b.to_vec())
                .map_err(|e| ClientError::Transport(e.to_string()));
        }
        Err(ClientError::Shape(
            "the image entry carries neither b64_json nor url".to_string(),
        ))
    }
}

/// The `data` array of an image response.
#[derive(Debug, Clone, Deserialize)]
pub struct ImageResponse {
    /// Seconds since the Unix epoch, as the OpenAI schema defines it.
    #[serde(default)]
    pub created: u64,
    /// The images the server produced.
    #[serde(default)]
    pub data: Vec<Image>,
}

/// A text-to-image request.
#[derive(Debug, Clone, Default, Serialize)]
pub struct GenerationRequest {
    /// The prompt. Required by every server.
    pub prompt: String,
    /// Number of images. The bundled bridge supports one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    /// Explicit `WIDTHxHEIGHT`, both divisible by 32 on the bundled bridge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    /// Aspect ratio such as `16:9`; used when `size` is absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,
    /// Resolution tier such as `1k`; used when `size` is absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    /// Sampling steps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_inference_steps: Option<u32>,
    /// Seed; a negative value or absence means random.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    /// Classifier-free guidance scale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cfg_scale: Option<f64>,
    /// Pin this one request to the turbo profile, when the server offers it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turbo: Option<bool>,
    /// Response encoding, `b64_json` or `url`. The bundled bridge answers inline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<String>,
}

impl GenerationRequest {
    /// A request with a prompt and nothing else set.
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            ..Self::default()
        }
    }

    /// Set the explicit size.
    pub fn size(mut self, size: impl Into<String>) -> Self {
        self.size = Some(size.into());
        self
    }

    /// Set the aspect ratio and resolution tier.
    pub fn aspect(mut self, aspect: impl Into<String>, resolution: impl Into<String>) -> Self {
        self.aspect_ratio = Some(aspect.into());
        self.resolution = Some(resolution.into());
        self
    }

    /// Set the seed.
    pub fn seed(mut self, seed: i64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Set the step count.
    pub fn steps(mut self, steps: u32) -> Self {
        self.num_inference_steps = Some(steps);
        self
    }

    /// Set the guidance scale.
    pub fn cfg(mut self, cfg: f64) -> Self {
        self.cfg_scale = Some(cfg);
        self
    }

    /// Validate the request before spending a round trip.
    pub fn validate(&self) -> Result<()> {
        if self.prompt.trim().is_empty() {
            return Err(ClientError::Invalid("prompt must not be empty".to_string()));
        }
        if let Some(n) = self.n {
            if n != 1 {
                return Err(ClientError::Invalid(
                    "n must be 1: the bundled bridge serves one image per request".to_string(),
                ));
            }
        }
        if let Some(size) = &self.size {
            parse_size(size)?;
        }
        Ok(())
    }
}

/// Parse `WIDTHxHEIGHT` and enforce the divisibility rule the bridge applies.
pub fn parse_size(size: &str) -> Result<(u32, u32)> {
    let invalid = |m: String| ClientError::Invalid(m);
    let (w, h) = size
        .split_once(['x', 'X'])
        .ok_or_else(|| invalid(format!("size '{size}' is not WIDTHxHEIGHT")))?;
    let w: u32 = w
        .trim()
        .parse()
        .map_err(|_| invalid(format!("size '{size}' has a non-numeric width")))?;
    let h: u32 = h
        .trim()
        .parse()
        .map_err(|_| invalid(format!("size '{size}' has a non-numeric height")))?;
    if w == 0 || h == 0 {
        return Err(invalid(format!("size '{size}' must be positive")));
    }
    if !w.is_multiple_of(32) || !h.is_multiple_of(32) {
        return Err(invalid(format!(
            "size '{size}' is not supported: width and height must be divisible by 32"
        )));
    }
    Ok((w, h))
}

/// An image-to-image request. The reference images travel inline as data URLs.
#[derive(Debug, Clone, Default, Serialize)]
pub struct EditRequest {
    /// The instruction for the change.
    pub prompt: String,
    /// Reference images as data URLs, in the shape the OpenAI edit schema uses.
    pub image: Vec<String>,
    /// Explicit `WIDTHxHEIGHT` for the result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    /// Number of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    /// Response encoding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<String>,
}

/// The verdict a health probe returns.
#[derive(Debug, Clone, Deserialize)]
pub struct Health {
    /// `ok` when the service is up.
    #[serde(default)]
    pub status: String,
    /// Whether the generation backend answered.
    #[serde(default)]
    pub backend: Option<bool>,
    /// Whether the backend process is alive.
    #[serde(default)]
    pub backend_process_alive: Option<bool>,
    /// Restarts observed since startup.
    #[serde(default)]
    pub backend_restarts: Option<u64>,
    /// The live sampling mode.
    #[serde(default)]
    pub mode: Option<String>,
    /// Whether turbo can be entered.
    #[serde(default)]
    pub turbo_available: Option<bool>,
    /// The adapters in use.
    #[serde(default)]
    pub lora: Option<String>,
    /// The schedule in use.
    #[serde(default)]
    pub sigmas: Option<String>,
    /// Steps the next request will use.
    #[serde(default)]
    pub steps: Option<u32>,
    /// Guidance the next request will use.
    #[serde(default)]
    pub cfg_scale: Option<f64>,
}

impl Health {
    /// True when the service reports itself up and its backend reachable.
    pub fn is_ready(&self) -> bool {
        self.status == "ok" && self.backend.unwrap_or(true)
    }
}

/// One entry of the model list.
#[derive(Debug, Clone, Deserialize)]
pub struct Model {
    /// The model identifier.
    pub id: String,
    /// The object type, normally `model`.
    #[serde(default)]
    pub object: Option<String>,
    /// Who serves it.
    #[serde(default)]
    pub owned_by: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ModelList {
    #[serde(default)]
    data: Vec<Model>,
}

/// The OpenAI error object, when a server reports one.
#[derive(Debug, Clone, Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Debug, Clone, Deserialize)]
struct ErrorBody {
    #[serde(default)]
    message: String,
}

/// Read the server's message out of an error body, falling back to the raw text.
fn error_message(status: u16, body: &str) -> ClientError {
    let message = serde_json::from_str::<ErrorEnvelope>(body)
        .map(|e| {
            if e.error.message.is_empty() {
                body.trim().to_string()
            } else {
                e.error.message
            }
        })
        .unwrap_or_else(|_| body.trim().to_string());
    let message = if message.is_empty() {
        "no message in the response".to_string()
    } else {
        message
    };
    ClientError::Status { status, message }
}

/// A client bound to one endpoint.
#[derive(Debug, Clone)]
pub struct Client {
    /// The endpoint this client talks to.
    pub endpoint: Endpoint,
}

impl Client {
    /// Build a client for an endpoint.
    pub fn new(endpoint: Endpoint) -> Self {
        Self { endpoint }
    }

    /// Build a client from an explicit base URL, or the default when absent.
    pub fn resolve(explicit: Option<&str>) -> Self {
        Self::new(Endpoint::resolve(explicit))
    }

    /// Generate an image from text.
    pub fn generate(&self, req: &GenerationRequest) -> Result<ImageResponse> {
        req.validate()?;
        let client = self.endpoint.client()?;
        let resp = self
            .endpoint
            .auth(client.post(self.endpoint.url("images/generations")))
            .json(req)
            .send()
            .map_err(|e| ClientError::Transport(e.to_string()))?;
        self.decode_response(resp)
    }

    /// Edit an image. Requires a server that implements `images/edits`; the
    /// bundled bridge does not, so this reports the route's own answer.
    pub fn edit(&self, req: &EditRequest) -> Result<ImageResponse> {
        if req.prompt.trim().is_empty() {
            return Err(ClientError::Invalid("prompt must not be empty".to_string()));
        }
        if req.image.is_empty() {
            return Err(ClientError::Invalid(
                "an edit needs at least one reference image".to_string(),
            ));
        }
        let client = self.endpoint.client()?;
        let resp = self
            .endpoint
            .auth(client.post(self.endpoint.url("images/edits")))
            .json(req)
            .send()
            .map_err(|e| ClientError::Transport(e.to_string()))?;
        self.decode_response(resp)
    }

    /// Read the model list.
    pub fn models(&self) -> Result<Vec<Model>> {
        let client = self.endpoint.client()?;
        let resp = self
            .endpoint
            .auth(client.get(self.endpoint.url("models")))
            .send()
            .map_err(|e| ClientError::Transport(e.to_string()))?;
        let status = resp.status();
        let body = resp
            .text()
            .map_err(|e| ClientError::Transport(e.to_string()))?;
        if !status.is_success() {
            return Err(error_message(status.as_u16(), &body));
        }
        let list: ModelList = serde_json::from_str(&body)
            .map_err(|e| ClientError::Shape(format!("model list is not JSON: {e}")))?;
        Ok(list.data)
    }

    /// Read the sampling mode the service reports.
    pub fn turbo_status(&self) -> Result<serde_json::Value> {
        let client = self.endpoint.client()?;
        let resp = self
            .endpoint
            .auth(client.get(self.endpoint.url("turbo")))
            .send()
            .map_err(|e| ClientError::Transport(e.to_string()))?;
        let status = resp.status();
        let body = resp
            .text()
            .map_err(|e| ClientError::Transport(e.to_string()))?;
        if !status.is_success() {
            return Err(error_message(status.as_u16(), &body));
        }
        serde_json::from_str(&body)
            .map_err(|e| ClientError::Shape(format!("turbo status is not JSON: {e}")))
    }

    /// Switch the sampling mode.
    pub fn set_turbo(&self, enabled: bool) -> Result<serde_json::Value> {
        let client = self.endpoint.client()?;
        let resp = self
            .endpoint
            .auth(client.post(self.endpoint.url("turbo")))
            .json(&serde_json::json!({ "enabled": enabled }))
            .send()
            .map_err(|e| ClientError::Transport(e.to_string()))?;
        let status = resp.status();
        let body = resp
            .text()
            .map_err(|e| ClientError::Transport(e.to_string()))?;
        if !status.is_success() {
            return Err(error_message(status.as_u16(), &body));
        }
        serde_json::from_str(&body)
            .map_err(|e| ClientError::Shape(format!("turbo answer is not JSON: {e}")))
    }

    /// Probe liveness. `/health` sits beside the version segment on the bundled
    /// bridge, so the side URL is tried first and the versioned one second.
    pub fn health(&self) -> Result<Health> {
        let client = self.endpoint.client()?;
        let candidates = [
            self.endpoint.side_url("health"),
            self.endpoint.url("health"),
        ];
        let mut last: Option<ClientError> = None;
        for url in candidates {
            match self.endpoint.auth(client.get(&url)).send() {
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().unwrap_or_default();
                    if !status.is_success() {
                        last = Some(error_message(status.as_u16(), &body));
                        continue;
                    }
                    return serde_json::from_str::<Health>(&body)
                        .map_err(|e| ClientError::Shape(format!("health is not JSON: {e}")));
                }
                Err(e) => last = Some(ClientError::Transport(e.to_string())),
            }
        }
        Err(last.unwrap_or_else(|| ClientError::Transport("no health route answered".into())))
    }

    fn decode_response(&self, resp: reqwest::blocking::Response) -> Result<ImageResponse> {
        let status = resp.status();
        let body = resp
            .text()
            .map_err(|e| ClientError::Transport(e.to_string()))?;
        if !status.is_success() {
            return Err(error_message(status.as_u16(), &body));
        }
        let parsed: ImageResponse = serde_json::from_str(&body)
            .map_err(|e| ClientError::Shape(format!("image response is not JSON: {e}")))?;
        if parsed.data.is_empty() {
            return Err(ClientError::Shape(
                "the response carried no data array entries".to_string(),
            ));
        }
        Ok(parsed)
    }
}

/// Write the images of a response to disk, returning the paths written.
///
/// A single image is written to `path` exactly; several are numbered from it
/// (`out.png`, `out-2.png`, `out-3.png`, ...). The extension is kept when the
/// path already has one.
pub fn save_images(
    endpoint: &Endpoint,
    response: &ImageResponse,
    path: &Path,
) -> Result<Vec<PathBuf>> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ClientError::Io(format!("cannot create {}: {e}", parent.display())))?;
        }
    }
    let mut written = Vec::with_capacity(response.data.len());
    for (index, image) in response.data.iter().enumerate() {
        let target = if index == 0 {
            path.to_path_buf()
        } else {
            numbered(path, index + 1)
        };
        let bytes = image.bytes(endpoint)?;
        std::fs::write(&target, &bytes)
            .map_err(|e| ClientError::Io(format!("cannot write {}: {e}", target.display())))?;
        written.push(target);
    }
    Ok(written)
}

/// `out.png` plus 2 becomes `out-2.png`, keeping the extension.
fn numbered(path: &Path, n: usize) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "image".to_string());
    let name = match path.extension() {
        Some(ext) => format!("{stem}-{n}.{}", ext.to_string_lossy()),
        None => format!("{stem}-{n}"),
    };
    path.with_file_name(name)
}

/// Read a local image and encode it as a data URL for an edit request.
pub fn file_to_data_url(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)
        .map_err(|e| ClientError::Io(format!("cannot read {}: {e}", path.display())))?;
    let mime = match path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some(other) => {
            return Err(ClientError::Io(format!(
                "unsupported reference image type '{other}': use png, jpg, or webp"
            )))
        }
        None => {
            return Err(ClientError::Io(format!(
                "{} has no extension, so its image type is unknown",
                path.display()
            )))
        }
    };
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:{mime};base64,{encoded}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_normalises_and_keeps_the_version_segment() {
        let e = Endpoint::new("http://127.0.0.1:8787/v1/");
        assert_eq!(e.base_url, "http://127.0.0.1:8787/v1");
        assert_eq!(
            e.url("images/generations"),
            "http://127.0.0.1:8787/v1/images/generations"
        );
        assert_eq!(e.side_url("health"), "http://127.0.0.1:8787/health");
    }

    #[test]
    fn origin_without_a_version_segment_is_unchanged() {
        let e = Endpoint::new("https://api.example.com/v1");
        assert_eq!(e.origin(), "https://api.example.com");
        let e = Endpoint::new("https://api.example.com/openai");
        assert_eq!(e.origin(), "https://api.example.com/openai");
    }

    #[test]
    fn size_parsing_enforces_the_divisibility_rule() {
        assert_eq!(parse_size("1024x1024").unwrap(), (1024, 1024));
        assert_eq!(parse_size("1152x672").unwrap(), (1152, 672));
        assert!(parse_size("1000x1000").is_err());
        assert!(parse_size("1024").is_err());
        assert!(parse_size("0x32").is_err());
    }

    #[test]
    fn empty_prompt_is_rejected_before_a_round_trip() {
        let req = GenerationRequest::new("   ");
        assert!(req.validate().is_err());
        let req = GenerationRequest::new("a lighthouse").steps(28).seed(42);
        assert!(req.validate().is_ok());
    }

    #[test]
    fn optional_fields_are_omitted_from_the_body() {
        let req = GenerationRequest::new("a lighthouse");
        let body = serde_json::to_value(&req).unwrap();
        assert_eq!(body["prompt"], "a lighthouse");
        assert!(body.get("size").is_none());
        assert!(body.get("seed").is_none());
    }

    #[test]
    fn several_images_are_numbered_from_the_first_path() {
        let base = Path::new("/tmp/imagegen-out/out.png");
        assert_eq!(
            numbered(base, 2),
            PathBuf::from("/tmp/imagegen-out/out-2.png")
        );
        assert_eq!(
            numbered(base, 3),
            PathBuf::from("/tmp/imagegen-out/out-3.png")
        );
        let no_ext = Path::new("/tmp/imagegen-out/out");
        assert_eq!(
            numbered(no_ext, 2),
            PathBuf::from("/tmp/imagegen-out/out-2")
        );
    }

    #[test]
    fn b64_images_decode_without_a_network_round_trip() {
        let e = Endpoint::new(DEFAULT_BASE_URL);
        let image = Image {
            b64_json: Some(base64::engine::general_purpose::STANDARD.encode(b"png-bytes")),
            url: None,
            revised_prompt: None,
        };
        assert_eq!(image.bytes(&e).unwrap(), b"png-bytes");
    }

    #[test]
    fn the_data_url_decoder_rejects_a_truncated_payload() {
        let e = Endpoint::new(DEFAULT_BASE_URL);
        let image = Image {
            b64_json: Some("not base64 !!".to_string()),
            url: None,
            revised_prompt: None,
        };
        assert!(matches!(image.bytes(&e), Err(ClientError::Shape(_))));
    }

    #[test]
    fn a_local_validation_failure_is_reported_as_invalid_not_as_a_response_shape() {
        let err = GenerationRequest::new("x")
            .size("1000x1000")
            .validate()
            .unwrap_err();
        assert!(matches!(err, ClientError::Invalid(_)), "got {err:?}");
        assert!(err.to_string().starts_with("invalid request"));
    }

    #[test]
    fn an_image_without_payload_is_a_shape_error() {
        let e = Endpoint::new(DEFAULT_BASE_URL);
        let image = Image {
            b64_json: None,
            url: None,
            revised_prompt: None,
        };
        assert!(matches!(image.bytes(&e), Err(ClientError::Shape(_))));
    }

    #[test]
    fn a_server_error_body_is_read_as_a_message() {
        let err = error_message(409, r#"{"error":{"message":"turbo is unavailable"}}"#);
        match err {
            ClientError::Status { status, message } => {
                assert_eq!(status, 409);
                assert_eq!(message, "turbo is unavailable");
            }
            other => panic!("expected a status error, got {other:?}"),
        }
    }

    #[test]
    fn a_non_json_error_body_falls_back_to_its_text() {
        let err = error_message(502, "bad gateway");
        match err {
            ClientError::Status { status, message } => {
                assert_eq!(status, 502);
                assert_eq!(message, "bad gateway");
            }
            other => panic!("expected a status error, got {other:?}"),
        }
    }

    #[test]
    fn a_data_url_carries_its_media_type() {
        let dir = std::env::temp_dir().join(format!("imagegen-client-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("ref.png");
        std::fs::write(&png, b"x").unwrap();
        let url = file_to_data_url(&png).unwrap();
        assert!(url.starts_with("data:image/png;base64,"));
        let unknown = dir.join("ref.bmp");
        std::fs::write(&unknown, b"x").unwrap();
        assert!(file_to_data_url(&unknown).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
