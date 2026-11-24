imagegen-mcp

An agent tool server, speaking the Model Context Protocol over standard input
and output, over any service that implements the OpenAI image API.

Tools

  Tool            Arguments                                   Purpose
  --------------  ------------------------------------------  ----------------
  image_generate  prompt, size or aspect, steps, seed, output  text to image
  image_edit      prompt, references, size, output            image to image
  image_health    none                                        probe
  image_models    none                                        model list
  image_turbo     action: status, on, off                     sampling mode

Each tool answers a JSON text block. A failure is marked as an error by the
protocol, which a calling agent can branch on without reading prose.

Design notes

  - The server holds no state between calls beyond the resolved endpoint, so it
    is safe to run for the lifetime of a session.
  - Every tool runs its blocking HTTP work on the blocking pool. The client
    library is the blocking one; calling it directly inside an async handler
    would drop a runtime inside a worker and panic. This is not theoretical: it
    was caught by driving the server over the protocol before release.
  - Local validation runs before a round trip, so a malformed prompt, size, or
    missing reference file is refused with a reason that names the field.
  - Written image paths are returned absolute, so a caller can open the file
    without guessing the server's working directory.

Configuration

The endpoint comes from the environment, falling back to the local bridge. The
API key is read from the environment only.

    IMAGEGEN_BASE_URL=http://127.0.0.1:8787/v1
    IMAGEGEN_API_KEY=

Use

Register the binary as a command-based tool server in the client's
configuration. Typical flow inside a session: call the health tool first, then
generate, then read the returned path.

Building

    bash <build-root>/linux/build.sh -p <project-root> --bin imagegen-mcp

The binary lands in bin/.
