# QA report: image_edit, the idle shutdown, and the lifecycle readout

qa-model: analytical-check (the primary agent, by operator request for this unit)
date: 2026-11-24
branch: fix/lifecycle-idle-vram
unit commit under QA: 575b718 fix(bridge): release the card on idle and report the lifecycle
surfaces extracted by tests/qa-gate.sh relative to dev: ENDPOINT /v1/images/edits,
CONFIG KEY idle_timeout_secs, CONFIG KEY llm_vision

INDEPENDENCE DISCLOSURE
Self-verification by the agent that wrote the code, strictly weaker than an
independent pass. Every claim below is a command and its output from this
session, and the surfaces that failed along the way are reported as failed.

## The failures this unit starts from

1. No edit route. The bridge registered models, generations, turbo and health
   only; POST /v1/images/edits answered 404 with the exact body the agent tool
   sends.
2. No release of the card. docs/architecture.md has described
   idle_timeout_secs as the bridge's own backstop since 2026-09-22 and said
   /health reports in_flight, idle_secs and idle_timeout_secs "so a stop
   decision can be made from data instead of a guess". The source had none of
   those fields, so the documented backstop did not exist, and the stop hook's
   in_flight guard could never fire because it reads a field that was absent.
   Measured cost of the leak: about 5.6 GB of pinned VRAM and 11 GB of host RAM
   for as long as the process lives (docs/architecture.md, lifecycle table).

## Coverage

ENDPOINT /v1/images/edits. Verified live through a running bridge.

- Handler reached with the tool's real wire shape: JSON, the reference as
  {"url":"data:image/png;base64,..."}, no size field, resolution "1k". Unit
  tests pin the single and multiple reference shapes and the bare-string form:
  the_tool_single_reference_body_parses,
  the_tool_multiple_reference_body_parses_in_order,
  bare_data_url_strings_are_accepted_too.
- End to end: HTTP 200 in 26.6 s, a PNG payload. The route answered 404 before
  this unit.
- Refusals, each pinned by a unit test or checked in the handler: no reference,
  a blank reference, a blank prompt, a prompt carrying the extension marker, a
  reference that is not base64, n != 1, and an edge over 1536.

CONFIG KEY llm_vision. Verified live and by behaviour.

- The running sd-server carries "--llm_vision
  models/text_encoders/mmproj-Qwen3VL-8B-Instruct-F16.gguf" in its command line,
  and the startup log names the same path in the effective configuration.
- The load is confirmed by behaviour: the edit that returned no image before the
  key existed now returns one.
- Failure path: a configured path that is not on disk is a startup error
  (std::process::exit(2)), and an unset key makes the edit route answer 501
  with the reason instead of failing inside the backend.

CONFIG KEY idle_timeout_secs. Verified live on a scratch bridge.

- Booted on ports 8797/8798 with IMAGEGEN_IDLE_TIMEOUT_SECS=20 so the test does
  not wait 15 minutes. /health answered "in_flight":0,"idle_secs":7,
  "idle_timeout_secs":20.
- The log recorded "no request for 20s (idle_timeout_secs); stopping the bridge
  and releasing the backend" at 20 s after the last request.
- After it: the bridge process gone, port 8797 quiet, no sd-server left, and
  nvidia-smi back to the 1027 MiB idle baseline. That last line is the point of
  the unit, so it is the check that matters.
- The guard that keeps it from cutting off work is pinned by
  the_idle_timeout_is_opt_out_and_cannot_fire_mid_request: the counter rises to
  1 while a request is held and returns to 0 after, and 0 disables the timer.

## The client, CLI and MCP readout

- imagegen-cli lifecycle against the running service: in_flight 0, idle_secs 10,
  "idle_shutdown 900 s (the service releases its backend itself)",
  safe_to_stop yes, exit 0.
- Against a service that predates the fields, the same command says "(not
  reported by this service)" rather than inventing a value; that was observed
  against the older bridge before it was rebuilt.
- The predicates are pinned by
  health_lifecycle_predicates_treat_absent_fields_as_idle, and the subcommand by
  lifecycle_is_a_command_of_its_own.
- image_health returns the same fields, so an agent can decide without shell
  access.

## What was NOT verified, explicitly

- Any edit larger than 512x192. Larger edits on this deployment are unmeasured.
- Turbo with a reference: the adapter is unavailable on this deployment, so the
  lora entries that ride the request are unit-tested only.
- Multi-reference edits end to end: the parse and order are tested, one
  reference was sent to the backend.
- A generation request after the change. Generation never reads llm_vision or
  the idle fields and its handler gained only the in-flight guard, but no
  generation was re-measured in this unit; that is recorded rather than
  claimed.
- The 900 s production timeout was not waited out; the 20 s scratch run
  exercises the same code path and the same exit, and the difference is only
  the interval.

## Dependency scan (project rule 10)

mcp-call osv-mcp osv_map_dependencies over this workspace reported the same
five pre-existing packages as before this unit (rustls, rustls-webpki, tokio,
url, webpki-roots), none introduced here: the only lockfile additions are
mime_guess and unicase, from reqwest's multipart feature, and neither is
flagged. tokio 1.53.1 has a fixed version (1.53.3) recorded as accepted risk
for a dedicated dependency unit, not this one.

verdict: overall PASS
