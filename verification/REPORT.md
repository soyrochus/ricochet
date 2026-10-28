# Ricochet verification

Date: 2026-10-28. Environment: macOS 26.6.2 (25G83), Apple ARM64,
Darwin 25.6.0. Primary compiler: `rustc 1.99.0 (b940084d7 2026-09-28)`;
Cargo 1.99.0. Supported compiler floor: Rust 1.94, separately verified.

## Automated checks

| Check | Result |
| --- | --- |
| `cargo fmt --check` | Pass |
| `cargo clippy --all-targets -- -D warnings` | Pass |
| `cargo test` | Pass: 33 tests |
| `cargo build --release` | Pass |
| `cargo clippy --all-targets --all-features -- -D warnings` | Pass |
| `cargo test --all-features` | Pass: 34 tests, including development fixtures |
| `cargo +1.94.0 test --all-features --locked` | Pass |
| `cargo build --release --examples --features dev-harness` | Pass |
| `python3 tools/verify_pty.py --output verification/pty-results.json` | Pass: 19 synthetic PTY scenarios |

Simulation coverage includes exact swept face/corner contacts, simultaneous wall
and brick contacts, no duplicate damage, speed caps, score and HP accounting,
moving/clamped paddle contacts, misses from below, alternating launches, level
completion before simultaneous life loss, all end/restart transitions and
focus/resize suspension. Two equivalent input histories produce exactly equal
states at 30 and 60 render FPS. The deterministic soak repeats 72,000 ticks
(ten simulated minutes) twice and checks finite values, speed magnitude, paddle
bounds and continued motion. This is simulated time, not a real-terminal soak.

Renderer tests reconstruct base64 payloads across 4096-byte boundaries, check
cell/offset mapping at multiple font sizes and minimum dimensions, verify round
ball transparency and geometric damage markings, crop the bottom of the ball,
and test cached scene updates across movement, damage, restarts and resizes.
Mock writers verify partial-initialization cleanup, raw-mode restoration despite
output failure, session cleanup during unwinding and synchronized-frame closure
after an output error.

## Synthetic PTY observations

Full measurements: [pty-results.json](pty-results.json). The harness supplies
capability replies; it does not render graphics or emulate Ghostty.

- Synthetic viewport: 100 × 40 cells, 1000 × 800 pixels. Resize scenario visits
  60 × 20 / 600 × 400, returns to 100 × 40 / 1000 × 800, then changes to
  1200 × 960 pixels without changing the character grid.
- The diagnostic receives ordinary-letter press/release events through
  Crossterm, including input preserved during startup probing.
- Game flow exercises launch, held movement, focus loss/gain, ignored repeats,
  pause/resume and restart. Won and GameOver fixtures reach their screens through
  actual simulation, then restart through the production event loop.
- Capability rejection (graphics missing, ordinary-key reporting missing and
  no response) restores the terminal; the silent probe expires at its two-second
  deadline. Process startup/cleanup makes total measured wall time slightly
  longer than the probe deadline.
- Normal quit, enhanced Ctrl-C, SIGINT, SIGTERM, SIGHUP, SIGQUIT, SIGTSTP, injected
  runtime error and injected unwind restore canonical mode, echo and ISIG on the
  PTY. Cursor-show, focus-disable, keyboard-pop and alternate-screen exit commands
  are present. Error messages follow restoration output.
- The protocol model peaks at nine images / 53 placements and has no remaining
  resources after exit. Repeated resizes replace images instead of accumulating
  them; ordinary animation frames reuse data. This checks emitted commands, not
  Ghostty's GPU allocations.
- Short synthetic 60/30 FPS runs produce output-frame intervals near the
  respective 16.7/33.3 ms targets. See the JSON for the measured medians. These
  are PTY output timings, **not actual terminal presentation or input latency**.

## Actual Ghostty acceptance: unverified

No Ghostty binary was found on PATH, in `/Applications`, in the user's
Applications directory or in the checked Homebrew locations. No actual Ghostty
1.4+ session was used. Therefore OS/Ghostty/window/font/frame-timing measurements
for a real-terminal session are unavailable, and no visual, native-input or
five-minute responsiveness claim is made.

To finish acceptance in Ghostty 1.4+ on macOS or Linux:

1. Record OS, `ghostty --version`, cell dimensions, physical pixel dimensions
   and font size; run `cargo run --release` directly in Ghostty.
2. Start, launch and steer using both arrows and A/D. Confirm round-ball
   within-cell motion and a multi-hit brick's geometric damage before destruction;
   check score and lives against SPEC.md.
3. Pause/resume; lose focus with a direction held, return, release it and resume.
   Restart and quit. Check for stuck keys or unexpected movement.
4. Resize repeatedly and change font size. Shrink below either minimum, then
   restore and explicitly resume. Check saved progress, no stale placements,
   uniform scaling and no HUD overlap.
5. Use the opt-in `acceptance` examples for Won, GameOver, damage, controlled
   error and panic; check shell echo/cursor and removal of graphics after exit.
6. Play for five real minutes at the default target. Record observed frame
   timing, flicker/trails, responsiveness and resource behavior. Repeat at 30 FPS
   if comparing render targets.

## Implementation choices and limits

Gameplay constants are unchanged. Scale is capped at 2× to bound raster memory;
larger windows letterbox. Circle corners are geometric arcs rather than expanded
rectangle approximations. Independent simultaneous contact normals reverse each
axis once, deterministically.

Pixel geometry comes from the OS/Crossterm wrapper. Missing OS pixel dimensions
produce a clean capability error; no raw geometry-query fallback is mixed into
the Crossterm event loop. Suspend/resume is explicitly unsupported; SIGTSTP exits
cleanly. See README.md for the remaining release boundaries.
