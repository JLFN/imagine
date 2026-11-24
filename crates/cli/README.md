imagegen-cli

A command-line client for any service that implements the OpenAI image API. It
is the shell-side counterpart of the agent tool server: both are thin layers
over the same client library, so they cannot disagree about the contract.

Commands

    imagegen-cli health                     liveness and the live mode
    imagegen-cli models                     the model list
    imagegen-cli generate -p TEXT -o PATH   text to image
    imagegen-cli edit -p TEXT -r PATH ...   image to image
    imagegen-cli turbo [status|on|off]      read or switch the mode

Global options

    --base-url URL     endpoint including the version segment
    --timeout SECONDS  total request timeout
    --json             print the answer as JSON

The endpoint and the timeout can also be set through the environment. The API
key is read from the environment only, never from a flag, so a key does not
enter shell history or a process listing.

Configuration order

A flag wins over the environment, which wins over the built-in default. The
default endpoint is the local bridge.

Examples

    imagegen-cli generate -p "a lighthouse at dusk" -o out/light.png
    imagegen-cli generate -p "a wide harbour" --aspect 16:9 --resolution 1k -o out/harbour.png
    imagegen-cli generate -p "a lighthouse" --size 1024x1024 --seed 42 -o out/light.png
    imagegen-cli --base-url https://provider.example/v1 models

Exit status

Zero on success, one on any failure. A failure prints the service's own reason,
so a script can surface it directly.

Building

    bash <build-root>/linux/build.sh -p <project-root> --bin imagegen-cli

The binary lands in bin/.
