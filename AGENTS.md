Agent instructions for work inside this repository

This file is the project-local instruction set. The global rule set always wins
where the two meet; this file adds what is specific to this project.

Read first

1. README.md, for what the project is and how its parts fit.
2. docs/REFERENCE.md and docs/WORKFLOW.md, for the interface and the operating
   procedure.
3. models/MANIFEST.md and recipe.json, for the files a deployment needs.

Layout

  crates/bridge           the HTTP service over the OpenAI image API
  crates/client           the shared HTTP client
  crates/cli              the command-line client
  crates/mcp              the agent tool server
  crates/convrot-verify   an encoder verification tool
  docs/                   the document set
  models/MANIFEST.md      the model registry; model files are never committed
  packaging/hooks         optional lifecycle scripts
  patches/                backend patches, only for one encoder variant
  qa-evidence/            quality evidence and reports
  tests/                  the test entry point and the review gate
  recipe.json             the machine-readable model recipe
  recipe.schema.json      the rules recipe.json must satisfy

Rules that always apply here

- No emoji, and no decorative glyphs, in any file or message.
- Prose is plain text. Markdown is used only where a file's format requires it,
  such as the documents in docs/ and this file's source tree.
- Conventional commit messages: type, scope, subject, then a body that reads
  like a verification report and states what was checked and how.
- Every factual claim traces to a checked source, cited by file and line or by
  URL. A claim without a source is marked unverified.
- No personal data and no host-specific absolute paths in code, configuration,
  tests, or examples. Use placeholders such as <project-root> and <build-root>.
- Configuration precedence is explicit and the same everywhere: a flag, then
  the environment, then a configuration file, then a default.
- Rust builds use the shared builder, and a dependency scan runs before a push.
- The review gate runs when a pull request is prepared, not on every commit.

Never

- Never commit a model file, a built binary, or a generated image. The ignore
  rules enforce this; do not weaken them.
- Never write a credential into a file, a flag, or a log.
- Never point a client at an endpoint with a session credential; use the
  configured key for that endpoint only.
- Never claim a capability the code does not implement. If a route is absent,
  the document says so and the client reports the refusal.

Document conventions

Documents in docs/ follow one form: a control block that names the identifier,
revision, status, issue date, and owner; numbered clauses; a scope statement; a
list of intended readers; and references. Dates use the calendar form
year-month-day. A document that states a measurement names the machine and the
conditions. When a document and the code disagree, the code wins and the
document is corrected in the same change.
