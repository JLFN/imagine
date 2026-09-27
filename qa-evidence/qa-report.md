QA report for imagegen Unit 8: the hook-driven on-demand lifecycle (start on an
image request, stop on session end, and the bridge's own idle shutdown), plus
re-verification of the Unit 7 surfaces this branch also carries.

qa-model: deepseek-flash (the primary agent, by operator request)
Date: 2026-09-22. Revision tested: the working tree on
feature/on-demand-lifecycle, which is STACKED on feature/unit7-resolution-and-port
(d36eb68), so the diff against dev contains both units. Rebuilt binaries:
bin/imagegen-bridge 5,348,760 bytes (this unit) and bin/convrot-verify (Unit 7).

INDEPENDENCE DISCLOSURE, read this first
This pass was performed by the same agent that wrote the code, at the operator's
request, so it is self-verification and strictly weaker than an independent QA
model's pass. Every claim below was exercised live - a real headless open-grok
session, the real systemd unit, the real hook scripts through their installed
symlinks - and the output is quoted verbatim. Nothing was verified by reading the
code alone. The explicit "NOT verified" list at the end is part of the evidence.

Surfaces covered, each with the command and what was observed.

CONFIG KEY idle_timeout_secs (the bridge's own stop, imagegen.toml)
  Unit tests: cargo test -p imagegen-bridge
  observed: 8 passed; 0 failed, including the four new ones -
  idle_timer_fires_only_after_the_timeout,
  idle_timer_never_fires_mid_request_or_when_disabled,
  activity_guard_counts_and_refreshes, plus the five pre-existing size/ceiling
  tests.
  Live, the running bridge reports and logs the value:
  - effective configuration: ... max_edge=1152 idle_timeout_secs=900 placement=...
  - GET /health -> {"status":"ok","backend":true,"backend_process_alive":true,
    "backend_restarts":0,"backend_restart_attempts":0,"in_flight":0,
    "idle_secs":126,"idle_timeout_secs":900}
  The shutdown itself, with the timeout shortened to 20 s so the test fits in a
  session:
  command: IMAGEGEN_IDLE_TIMEOUT_SECS=20 setsid nohup env
           IMAGEGEN_IDLE_TIMEOUT_SECS=20 LD_LIBRARY_PATH=... ./bin/imagegen-bridge
  observed: up after 1s, health idle_timeout_secs 20; the bridge exited BY ITSELF
  after 48 s (the timer polls every 30 s, so the exit lands within one poll of the
  timeout), with the log line "idle for 30s (timeout 20s), releasing the backend;
  the next request restages the model stack", and pgrep -x sd-server afterwards
  returned 0, so the backend was reaped rather than orphaned.

CONFIG KEY max_edge (inherited from the Unit 7 branch, re-verified here)
  command: curl -X POST http://127.0.0.1:8787/v1/images/generations -d
           '{"prompt":"x","size":"1344x1344"}'
  observed: http_code=400 with "size 1344x1344 exceeds the maximum 1152x1152
  supported by the configured layout (diffusion parameters pinned in VRAM) ..." and
  the same run's config line showing max_edge=1152. Full Unit 7 coverage, with the
  measurements behind the value, is in qa-evidence/unit7-qa-report.md.

BINARY convrot-verify (inherited from the Unit 7 branch, re-verified here)
  commands and observed exit codes, captured without a pipeline to avoid masking:
    bin/convrot-verify                                  exit=0
      verdict: PASS (pearson >= 0.99, rel_l2 <= 0.02 on every tensor checked)
      embed_tokens pearson=0.999961 rel_l2=0.0089 max_abs=0.0010 control 1.3746
    bin/convrot-verify /tmp/fake-weights.safetensors     exit=1
      verdict: FAIL (a tensor disagreed with its BF16 ground truth)
    bin/convrot-verify /tmp/nope.safetensors             exit=1
      error: open /tmp/nope.safetensors: No such file or directory (os error 2)

The hook-driven lifecycle (the unit's actual subject)

Two hook scripts, installed as symlinks from this repository so the repo is the
source of truth (verified: ls -la ~/.opengrok/hooks/bin shows
ensure-imagegen-bridge and stop-imagegen-bridge pointing at
/data/imagegen/packaging/hooks/*, and both run through the symlink).

  END-TO-END START, through a real session
  command: bash /data/build-sd/e2e-hook-test.sh
    (stop --force, confirm the bridge is down, then run
     open-grok --cwd /data/imagegen -p "Use the image_gen tool to generate one
     image of a red cube on a white table...")
  observed: the headless session exit=0 and reported the tool result: a PNG at
  .../images/1.jpg, 1,154,861 bytes, 1024x1024. The lifecycle log proves the hook
  did the start, nobody else:
    12:02:29 stop: stopping the bridge (sessions=2 in_flight=0 force=1)
    12:02:29 stop: systemctl --user stop imagegen-bridge.service issued
    12:02:38 started imagegen-bridge.service
    12:02:39 healthy (foreground wait 10s)
  and the bridge log shows the request it then served:
    "generating 1 image(s) at 1024x1024 steps=28 cfg=6 seed=-1" at 12:02:39
    "generated 1 image(s) in 175.280632557s"
  175.28 s is inside open-grok's 240 s read timeout with the cold staging cost
  included, which is the number that matters for on-demand use.

  START FALLBACK, with the systemd unit not installed
  command: mv the unit away; systemctl --user daemon-reload; ensure-imagegen-bridge
  observed: "unit load state: not-found"; the script launched the binary detached,
  logged "detached launch attempted (pid 2335456)", the pid file matched the real
  process (pgrep -x imagegen-bridge -> 2335456), and /health answered with
  idle_timeout_secs 900. The unit was then restored and the deployment returned to
  unit management (active, MainPID 2335572, health ok).

  STOP, guarded (three paths exercised)
  command: stop-imagegen-bridge            -> observed "stop: skipped, 2 open-grok
    sessions are alive (use --force to override)" and the bridge stayed up.
  command: stop-imagegen-bridge --force    -> observed "stop: systemctl --user stop
    imagegen-bridge.service issued" then "bridge is down, ports released".
  command: stop-imagegen-bridge (bridge already down) -> observed "nothing to do,
    /health does not answer".
  The in-flight guard was NOT exercised against a live generation (see NOT
  verified); it reads the in_flight field, which /health reports as 0 when idle and
  which the unit test covers as a counter.

  CLEAN STOP THROUGH SYSTEMD (the new SIGTERM handler)
  command: systemctl --user stop imagegen-bridge.service
  observed: bridge process 0, sd-server 0, ports 8787/8788 free, and the bridge's
  own log carries "SIGTERM received" followed by "shutdown signal received,
  stopping sd-server" - i.e. the bridge reaped its own backend rather than relying
  on systemd killing the whole cgroup. (The same test on the previous binary passed
  only because of the cgroup kill, which is why it was re-run after the rebuild.)

  WHICH EVENTS FIRE, measured with a temporary probe hook
  A throwaway hook logger recorded, for a headless run:
    session_start, pre_tool_use, stop (reason end_turn)
  and no session_end. The probe files were removed afterwards (verified: the only
  hook json files left are callgraph, handoff, imagegen-bridge, skill-allowlist).
  So the start half fires in headless and interactive runs, while the SessionEnd
  stop applies to interactive sessions only and the idle timer is the backstop for
  headless ones. This is recorded in docs/architecture.md rather than papered over.

Rule 19 gate and the test entry point
  command: bash tests/run.sh   (cargo test --workspace, then tests/qa-gate.sh)
  observed: the gate extracts BINARY convrot-verify, CONFIG KEY max_edge and
  CONFIG KEY idle_timeout_secs for this branch and requires a fresh report naming
  each of them with the operative verdict as the last non-empty line.

Rule 10 dependency scan (before any push)
  command: osv__osv_map_dependencies path=/data/imagegen
  observed: no new dependencies were added by this unit (no Cargo.toml dependency
  change), so the lockfile is the one scanned for Unit 7: 1 manifest, 155 packages,
  0 vulnerable. Re-scanned at push time.

Rule 5 canonical build
  command: bash /data/build/linux/build.sh -p /data/imagegen -b imagegen-bridge
  observed: built and copied to bin/imagegen-bridge (5,348,760 bytes). The bridge
  was stopped first: the builder cannot replace a running binary ("Text file busy")
  and deletes target/ afterwards, so a build against a live bridge silently
  discards its output.

NOT verified in this pass, stated plainly
  - The SessionEnd stop hook has never been observed firing. Headless runs do not
    emit session_end on this build (measured), so it could only be exercised by an
    interactive session ending, which this pass did not do. The script itself, its
    guards and its systemd/SIGINT stop paths are verified by direct invocation.
  - The in-flight guard was not tested against a real generation in progress; only
    the counter and the /health field were verified.
  - The unit's Restart=on-failure behavior was not exercised by killing the bridge
    with a nonzero exit or an unhandled signal.
  - The hooks were verified in a headless session and by direct invocation, not in
    an interactive TUI session: the PreToolUse start is proven end to end, the
    SessionEnd registration is proven only by inspection of the loaded hook files.
  - The bridge's own idle shutdown was tested with a 20 s timeout; the shipped
    900 s default was not waited out.
  - No concurrency test was run: two sessions requesting images at once, and the
    stop hook racing a starting request, are both untested.
  - The hook scripts are bash; they were run with bash on this machine only (no
    other shell, no other host).

verdict: overall PASS
