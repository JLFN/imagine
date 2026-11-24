imagegen — interface reference

Document control

  Identifier      IMAGEGEN-REFERENCE
  Revision        2.0
  Status          Approved for public release
  Issue date      2026-11-24
  Owner           repository maintainers
  Classification  Public
  Applies to      this repository at the commit carrying this revision
  Related         README.md, docs/WORKFLOW.md, docs/architecture.md

This reference states the interface exactly as the code implements it. Each
route cites the file and line that defines it. When prose and code disagree,
the code wins and this document is corrected.

1. Conventions

  - JSON in, JSON out, UTF-8, content type application/json.
  - A success answers a 2xx status. A refusal answers a non-2xx status with an
    error object, described in section 8.
  - Sizes are integers in pixels, both divisible by 32.
  - Seeds are signed integers. A negative seed, or no seed, means random.
  - Times are seconds since the Unix epoch, as unsigned integers.

2. Endpoints at a glance

  Method  Path                     Handler
  ------  -----------------------  -------------------------------------------
  GET     /v1/models               crates/bridge/src/main.rs:1028
  POST    /v1/images/generations   crates/bridge/src/main.rs:1152
  GET     /v1/turbo                crates/bridge/src/main.rs:1058
  POST    /v1/turbo                crates/bridge/src/main.rs:1069
  GET     /health                  crates/bridge/src/main.rs:1117

The route table is registered at crates/bridge/src/main.rs:1449.

3. Model list

GET /v1/models

Answers one object. The identifier is the one a client should send as its model
name.

    {
      "data": [
        { "id": "Qwen-Image-2.1", "object": "model", "owned_by": "local" }
      ]
    }

4. Generate an image

POST /v1/images/generations

4.1  Request fields. The first column is the wire name; the second records
what the bridge does with it (crates/bridge/src/main.rs:839).

  Field                Type      Behaviour
  -------------------  --------  --------------------------------------------
  prompt               string    required; an empty or blank prompt is refused
  n                    integer   must be 1; any other value is refused
  size                 string    WIDTHxHEIGHT, both divisible by 32
  aspect_ratio         string    used when size is absent, such as 16:9
  resolution           string    used with aspect_ratio, such as 1k or 2k
  num_inference_steps  integer   overrides the profile step count
  seed                 integer   negative or absent means random
  cfg_scale            number    overrides the profile guidance scale
  turbo                boolean   pins this request to the accelerator profile

4.2  Size resolution. When size is present it is parsed and checked directly
(crates/bridge/src/main.rs:950). When size is absent, the aspect ratio and the
resolution tier produce the dimensions, rounded to a multiple of 32
(crates/bridge/src/main.rs:1002): 1k resolves to a 1024-pixel long edge, 2k to
2048, 4k to 3840. The maximum edge is then applied to the result
(crates/bridge/src/main.rs:973), so a tier this card cannot run is refused
rather than silently reduced.

4.3  Sampling profile. A request runs in the live mode unless its own turbo
flag says otherwise. In the accelerator profile the step count follows the
schedule, because the schedule defines the number of denoise steps
(crates/bridge/src/main.rs:942); a step count that disagrees is ignored and
logged.

4.4  Response.

    {
      "created": 1790000000,
      "data": [
        { "b64_json": "<base64 PNG>" }
      ]
    }

One entry per image. The bundled bridge always answers base64 data, whatever
response encoding a caller requests.

5. Sampling mode

GET /v1/turbo reports the live mode and what the next request will use.

    {
      "mode": "base",
      "turbo": false,
      "turbo_available": false,
      "lora": "(none)",
      "sigmas": "(scheduler)",
      "steps": 28,
      "cfg_scale": 6.0
    }

POST /v1/turbo switches the mode. Either form of body is accepted:

    { "enabled": true }
    { "mode": "turbo" }

The words on, off, turbo, and base are accepted wherever a mode name is
accepted. A switch is verified against the backend before it takes effect. When
the accelerator is not configured, not on disk, or not listed by the backend,
the answer is a 409 conflict, the mode is unchanged, and the message names the
reason.

6. Health

GET /health answers even while the backend is starting, which is the point of
the route.

    {
      "status": "ok",
      "backend": true,
      "backend_process_alive": true,
      "backend_restarts": 0,
      "in_flight": 0,
      "idle_secs": 12,
      "idle_timeout_secs": 900,
      "mode": "base",
      "turbo_available": false,
      "lora": "(none)",
      "sigmas": "(scheduler)",
      "steps": 28,
      "cfg_scale": 6.0
    }

backend answers whether the backend responded to a probe. When it is false and
backend_process_alive is true, the backend is still loading. When both are
false, it has died or was refused at startup.

in_flight is the number of requests being served. A stop must not be taken while
it is above zero: the request would be thrown away. idle_secs is the time since
the last request finished, and idle_timeout_secs is the threshold at which the
service releases its backend by itself (idle_timeout_secs in the configuration;
0 disables that and means nothing frees the card but a stop). Those three fields
are what the lifecycle command reads, and it exits 1 when a request is in flight
so a script can branch on them rather than parsing prose:

    imagegen-cli lifecycle

7. Image to image

The client sends an edit as a POST to /images/edits with a prompt and one or
more reference images as data URLs. The bridge registers that route and forwards
the edit to the backend's own edit route.

Editing needs the backend's vision tower. Qwen Image 2.1 conditions on a
reference image by running it through that tower, and without it the backend
logs "no vision weights detected, vision disabled" and returns no image. The
bridge passes the tower through `llm_vision`, and an edit on a bridge started
without it is refused with 501 and that reason rather than attempted. Verified
live on 2026-11-24: a 512x192 edit returned HTTP 200 with a PNG in 27 s.

8. Errors

A refusal has this shape, and the status carries the meaning:

    {
      "error": {
        "message": "size '1000x1000' is not supported: width and height must be divisible by 32",
        "type": "invalid_request_error",
        "param": null,
        "code": null
      }
    }

  Status  Meaning                                    Typical cause
  ------  -----------------------------------------  ---------------------
  400     the request is not acceptable              a bad size or field
  409     the request conflicts with current state   the mode is unavailable
  502     the backend failed or did not answer       an out of memory abort
  501     the route needs something not configured   an edit without a vision tower

A caller should read the message, not only the status; the message names the
field and the rule that refused it.

9. Command-line client

  Command                                        Purpose
  ---------------------------------------------  ------------------------
  imagegen-cli health                            liveness and mode
  imagegen-cli lifecycle                         idle state and whether a stop is safe
  imagegen-cli models                            model list
  imagegen-cli generate -p TEXT -o PATH          text to image
  imagegen-cli edit -p TEXT -r PATH -o PATH      image to image
  imagegen-cli turbo [status|on|off]             sampling mode

Global options are the endpoint base URL, the request timeout, and a JSON
output switch. The endpoint and the timeout can also be set through the
environment. The API key is read from the environment only.

10. Agent tool server

  Tool             Arguments                                    Purpose
  ---------------  -------------------------------------------  -------------
  image_generate   prompt, size or aspect, steps, seed, output  text to image
  image_edit       prompt, references, size, output             image to image
  image_health     none                                         probe
  image_models     none                                         model list
  image_turbo      action: status, on, off                      sampling mode

Each tool answers a JSON text block. A failure is marked as an error by the
protocol and carries the reason as text.

11. Configuration keys

  Key                       Meaning
  ------------------------  --------------------------------------------
  sd_server                 backend binary path
  diffusion                 diffusion model path
  vae                       decoder model path
  llm                       text encoder path
  backend_args              backend placement flags
  listen_ip, listen_port    bridge bind address
  backend_port              internal backend port
  steps, cfg_scale          base profile sampling defaults
  turbo                     mode the bridge starts in
  turbo_cfg_scale           guidance for the accelerator profile
  lora_dir, lora            accelerator adapter and its directory
  lora_multiplier           accelerator adapter weight
  extra_lora                a second adapter applied in every mode
  extra_lora_multiplier     its weight
  sigmas                    accelerator schedule

Every key can be overridden by an environment variable of the same name with an
IMAGEGEN_ prefix. The configuration file is read from the working directory
unless IMAGEGEN_CONFIG names another path.

12. Limits

  - one image per request;
  - a maximum edge of 1536 pixels on the reference card;
  - the backend determines whether editing is available;
  - the request must complete inside the caller's timeout; a large size at a
    high step count can exceed it.
