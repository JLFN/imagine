imagegen — workflow guide

Document control

  Identifier      IMAGEGEN-WORKFLOW
  Revision        2.0
  Status          Approved for public release
  Issue date      2026-11-24
  Owner           repository maintainers
  Classification  Public
  Applies to      this repository at the commit carrying this revision
  Related         README.md, docs/REFERENCE.md, docs/architecture.md

This guide is written for the person operating the service. Each task states
what to do, what to expect, and what to do when the expectation is not met.
Commands use placeholders: <project-root> is the checkout directory and
<build-root> is the shared Rust builder directory. Neither is compiled into any
binary.

1. Prepare the model files

1.1  Create the directory layout the configuration expects:

    cd <project-root>
    mkdir -p models/diffusion_models models/text_encoders models/vae bin out

1.2  Let the recipe fetch and verify each file. recipe.json records the source,
size, and checksum of every file, and the checker refuses anything that does not
match:

    tools/recipe-check.sh --profile quality        # what is present, verified
    tools/recipe-check.sh --download q6-k-base     # fetch one file, verified

To download by hand instead, fetch from the source in the recipe and then
confirm:

    sha256sum models/diffusion_models/<diffusion-file>

Expected: the checksum in recipe.json. If it differs, the download is incomplete
or a different revision; fetch it again rather than proceeding. A mismatch is
never something to work around.

1.2a One caution about the turbo-native checkpoint: it lives on the default
branch of its repository, while the non-turbo weights live on the base branch of
the same repository. The recipe records the branch with each file, so use the
recipe rather than guessing a URL. Downloading the wrong branch produces a model
that does not behave as its file name suggests.

1.3  Confirm the encoder matches the backend. One encoder variant requires a
patched backend; the patches directory records which. A stock backend and that
variant refuse each other at startup, which is the intended behaviour.

2. Build the backend

2.1  Build the generation backend from its own project, following that
project's instructions. The result is a server binary that accepts the model
arguments described in docs/REFERENCE.md.

2.1a A backend older than the model needs an upgrade in one respect: the named
schedule preset for the turbo-native checkpoint is recognised only by a recent
backend. Check the help text:

    bin/sd-server --help | grep -- --sigmas

Expected: the line mentions presets. If it lists only a comma-separated form,
either upgrade the backend or pass the nine values directly, which the recipe
records for that profile; the values are identical.

2.2  Place the backend binary at the path the configuration names, by default
bin/sd-server, or set the path explicitly in the configuration.

Expected: the binary runs and prints its version. If it fails to start, the
loader message names the missing library; install that library before
continuing.

3. Build the service

3.1  Build every binary with the shared builder:

    bash <build-root>/linux/build.sh -p <project-root> --all

Expected: the builder finishes and every binary appears in bin/, including
imagegen-bridge, imagegen-cli, and imagegen-mcp.

3.2  Run the tests:

    bash tests/run.sh

Expected: the suite reports pass. A failure names the test and the reason.

4. Configure

4.1  Copy the example configuration and edit the model paths:

    cp imagegen.toml imagegen.local.toml

4.2  Set only what differs from the shipped file. Every key has a documented
default; the shipped file records the measured defaults.

4.3  Put any credential in the environment, never in the file and never in a
flag:

    export IMAGEGEN_API_KEY='...'

The bundled bridge does not check credentials, so a local deployment can leave
the key unset. A hosted endpoint that requires one reads it from this variable.

5. Start the service

5.1  Run the bridge from the project root so relative model paths resolve:

    cd <project-root>
    IMAGEGEN_CONFIG=imagegen.local.toml bin/imagegen-bridge

Expected: the log states the effective configuration and the listen address,
then reports the backend as ready. The first request stages the weights, which
takes longer than later ones.

5.2  In a second terminal, probe the service:

    bin/imagegen-cli health

Expected: status ok, ready yes, and a mode line. If the backend is not
answering, the probe still answers, and the backend line says so; this is how a
starting backend is told apart from a dead service.

6. Make an image

6.1  Generate with defaults:

    bin/imagegen-cli generate -p "a lighthouse at dusk" -o out/light.png

Expected: a path is printed and the file exists. The default size is square at
the base resolution.

6.2  Generate with an explicit size and seed, for a repeatable result:

    bin/imagegen-cli generate -p "a lighthouse at dusk" --size 1024x1024 --seed 42 -o out/light.png

Expected: the same seed and prompt against the same model produce the same
image. A change to the model, the encoder, or the placement invalidates that
expectation.

6.3  Request an aspect ratio instead of an explicit size:

    bin/imagegen-cli generate -p "a wide harbour" --aspect 16:9 --resolution 1k -o out/harbour.png

Expected: an image whose long edge follows the resolution tier and whose short
edge follows the ratio, both rounded to a multiple of 32.

7. Integrate a client application

7.1  Point the application at the service base URL:

    http://127.0.0.1:8787/v1

7.2  Send a generation request shaped like the OpenAI image API:

    curl -s http://127.0.0.1:8787/v1/images/generations \
      -H 'content-type: application/json' \
      -d '{"prompt":"a lighthouse at dusk","size":"1024x1024","seed":42}'

Expected: a JSON object with a creation time and a data array containing one
entry with base64 image data. The exact field set is in docs/REFERENCE.md.

7.3  Read the model identifier from the model list when the client requires
one, rather than hardcoding it.

8. Use the agent tool server

8.1  Register the binary with the agent client's tool configuration, naming
bin/imagegen-mcp as the command.

8.2  Set the endpoint for the server through the environment if it is not the
default:

    IMAGEGEN_BASE_URL=http://127.0.0.1:8787/v1

8.3  Call the health tool first in any automated flow. It separates a dead
service from a rejected request, which is the most common false trail.

Expected: the tools appear and each returns a JSON object. A failure carries an
error marker and the service's reason.

9. Run the fast profile

9.1  The turbo-native checkpoint needs no adapter, so it is not switched on: it
is a different host model with its own settings. Follow the recipe's
turbo-native profile: eight steps, guidance 1.0, euler sampling, the published
schedule, and the decoder on the GPU.

9.2  Verify the placement before a long run. With the decoder on the CPU the
same image takes about five times as long, and the cost is entirely in the
decode:

    grep 'vae decode graph' <backend-log>

Expected: about two seconds with the decoder on the GPU, tens of seconds with it
on the CPU.

9.3  Watch memory on a smaller card. The profile peaks near the ceiling of a
12 GiB card, so a smaller card moves the decoder to the CPU or lowers the size.

10. Switch the sampling mode

9.1  Read the live mode:

    bin/imagegen-cli turbo

9.2  Switch to the accelerator profile, when it is configured:

    bin/imagegen-cli turbo on

Expected: the reported mode changes. If the accelerator is not configured, the
service refuses with a conflict status and keeps the previous mode; the refusal
names what is missing.

9.3  Switch back:

    bin/imagegen-cli turbo off

11. Build a container or a service unit

11.1  Whatever the supervisor, keep two rules: run from a directory where the
relative paths resolve, or set absolute paths in the configuration; and give
the process time to stage weights on its first request.

11.2  Do not place a credential on a command line in a unit file. Use the
environment file mechanism the supervisor provides, with restrictive
permissions.

12. Stop the service

12.1  Stop the bridge process. It stops its backend child on shutdown.

12.2  Confirm nothing remains:

    pgrep -af 'imagegen-bridge|sd-server'

Expected: no output. If a backend remains, the bridge was killed in a way that
prevented cleanup; stop the remaining process by name.

13. Diagnose a failure

  Symptom                        First check
  -----------------------------  ---------------------------------------------
  health answers, backend false  the backend is starting or failed to load
  request refused with 400       the size or a field; the message names it
  request refused with 409       the sampling mode is not available
  request refused with 404       the route is not implemented by this backend
  request takes too long         the size and step count; lower either
  out of memory on the backend   lower the size; the card limit is real

14. Change the deployment safely

14.1  Change one thing at a time: a model file, or a placement flag, or an
accelerator setting, never several at once.

14.2  Record what you changed and what you measured. A quality claim without a
measurement beside it does not survive the next maintainer.

14.3  Re-run the tests after a code change, and re-measure after a model or
placement change.
