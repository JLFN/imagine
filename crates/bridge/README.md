imagegen-bridge

A Rust HTTP service that exposes the OpenAI image API and supervises a
generation backend. Any client that speaks the OpenAI image routes can use it:
an agent session, an application, or a shell script. Nothing in the bridge is
specific to one of them.

Purpose

The backend already implements an OpenAI-shaped generation route, but that
route ignores fields a richer client sends, and it leaves the model resident or
absent depending on how it was started. The bridge adds three things:

  - process supervision: it starts the backend once with the model arguments,
    waits until it answers, replaces it when it dies, and stops it on shutdown;
  - request translation: fields the OpenAI route does not carry, such as a seed,
    a step count, and a guidance scale, are translated into the backend's own
    request body so they are honoured rather than silently dropped;
  - clear refusals: a malformed request returns an OpenAI-shaped error object
    with a status that says what is wrong, instead of a backend failure leaking
    through.

Routes

  Method  Path                     Behaviour
  ------  -----------------------  --------------------------------------------
  GET     /v1/models               the model list
  POST    /v1/images/generations   text to image, base64 response
  GET     /v1/turbo                the live sampling mode
  POST    /v1/turbo                switch the sampling mode
  GET     /health                  liveness and backend state

A generation body may carry a turbo flag, which pins that one request to the
sampling mode it names. The full field list and every error are in
docs/REFERENCE.md.

Size rules

A size is either an explicit width by height, both divisible by 32, or an aspect
ratio plus a resolution tier from which the bridge derives the dimensions. A
maximum edge applies to the result of both paths, so a tier the card cannot run
is refused with a message that names the maximum rather than being reduced
silently.

Configuration

Model and binary paths have no value built into the binary. They come from
imagegen.toml in the working directory, or from the environment, and a missing
one is a startup error that names what is missing. Environment variables use an
IMAGEGEN_ prefix over the configuration key, for example IMAGEGEN_DIFFUSION.

Run the bridge from the directory where the relative model paths resolve, or
set absolute paths in the configuration.

Sampling modes

One process serves two profiles:

  - base is the default: the base model, the backend's own scheduler, and the
    configured step count and guidance;
  - turbo is a few-step accelerated profile, available only when its adapter,
    its multiplier, and its schedule are all configured and the adapter is on
    disk and listed by the backend.

The live mode is process state, not a file. It is reported by the health route
and by the mode route, switched by a request to the mode route, or pinned for
one request by a flag in the body. A switch is verified against the backend
before it takes effect; when the adapter cannot be confirmed, the answer is a
conflict and the mode is unchanged.

Model layout

    models/diffusion_models/   the diffusion models
    models/text_encoders/      the text encoders
    models/vae/                the decoder
    models/loras/              optional adapters

recipe.json records every file, its size, its checksum, and a public source.
tools/recipe-check.sh verifies what is on disk against it.

Measured behaviour

A warm request at the base resolution inside a typical client timeout completes
in about three minutes on a 12 GiB card at 28 steps. Raising the step count or
the size moves a request past a three-minute budget; that is a property of the
hardware, not of the bridge.

Building

    bash <build-root>/linux/build.sh -p <project-root> --bin imagegen-bridge

The binary lands in bin/.
