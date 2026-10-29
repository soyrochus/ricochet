# Ricochet 0.2 verification

Date: 2026-10-29. Environment: macOS 26.6.2 (25G83), Apple ARM64.
Primary compiler: `rustc 1.99.0 (b940084d7 2026-09-28)` / Cargo 1.99.0.
Supported compiler floor: Rust 1.94, separately compiled and tested.
Requirements: [SPEC-evolve-002.md](../specs/SPEC-evolve-002.md), with the
[baseline simulation rules](../specs/SPEC-init-001.md) unchanged.

## Automated checks

| Check | Result |
| --- | --- |
| `cargo fmt --check` | Pass |
| `cargo clippy --all-targets -- -D warnings` | Pass |
| `cargo clippy --all-targets --features dev-harness -- -D warnings` | Pass |
| `cargo test` | Pass: 51 tests |
| `cargo test --features dev-harness` | Pass: 52 tests |
| `cargo +1.94.0 test --features dev-harness --locked` | Pass: 52 tests |
| `cargo build --release` | Pass |
| `cargo build --release --examples --features dev-harness` | Pass |
| `python3 tools/verify_pty.py --output verification/pty-results.json` | Pass: 24 synthetic PTY cases |

The existing 18 physics/simulation tests pass unchanged, including the repeated
72,000-tick soak. The additional full-run test executes 72,000 ticks in each of
six combinations (full/reduced/off × 30/60 FPS) and compares exact `Game` state.
Render events are drained through the bounded controller queue. Effects borrow
`&Game`; contacts, launches, life loss and transitions never become game state.
No gameplay constants, collision calculations or timing rules were changed.

New coverage checks ordered simultaneous contacts and contact points, exactly-once
transitions, oldest-event dropping, deterministic particle spawns and shake,
equal particle integration at 30/60 FPS, pool bounds and oldest-slot reuse,
50 ms effects clamping, pause/viewport/resume freezing, trail discontinuities,
particle launch/lifetime bounds and removal, animation and banner timing,
new-level/restart clearing, static-brick removal in the shatter frame, and effects
surviving resize. Overlapping shakes use the maximum decaying envelope; a burst
of brick hits cannot retire a stronger life-loss shake.

Art/protocol tests verify coverage alpha, greyscale crack contrast, the shadow
extent below the brick gap, every required font glyph, nonoverlapping atlas
rectangles with transparent gutters, decoded memory, compressed and uncompressed
payload round trips, 4096-byte boundaries, optional-probe rejection/absence,
source rectangle encoding, clipping on every edge, the ball halo at the open
bottom, and fixed resource ownership over 100 resize/restart cycles. Shake is
checked with zero, small and large margins; both corners of every emitted
placement stay inside the terminal and between the HUD rows. Mock writers cover
partial initialization, unwinding, output failure and synchronized-frame closure.

## Measured budgets

Reproduce with:

```sh
cargo run --release --features dev-harness --example visual_metrics
```

The saved machine-readable result is [visual-metrics.json](visual-metrics.json).
Generation covers the complete full-mode scale-3 image set. Transmission counts
base64, command headers and chunk terminators, after zlib level 6. Frame timings
cover effects advance, placement diffing, encoding and writes to an in-memory
mock writer over 600 frames. Initial uploads are excluded from frame timing and
output limits. The scripted bursts saturate the 96-chip and eight-shatter pools,
retain all 50 bricks and fill the eight-ghost trail, deliberately exceeding the
load of most ordinary gameplay frames.

| Measure | Observed | Budget |
| --- | --- | --- |
| Decoded RGBA memory | 64,064,672 bytes (61.10 MiB) | ≤ 64 MiB |
| Complete compressed transmission | 1,305,753 bytes (1.25 MiB) | < 2 MiB target |
| Sprite/atlas regeneration | 193.815 ms | ≤ 250 ms |
| Median render-side CPU | 0.0262 ms | ≤ 2 ms |
| Peak steady frame output | 12,205 bytes | ≤ 16,384 bytes |
| Peak shake frame output | 12,222 bytes | ≤ 24,576 bytes |
| Uploaded images / peak placements | 7 / 166 | ≤ 16 / 192 |

These are local release-build CPU and emitted-byte observations. They do not
measure GPU memory, native terminal presentation, network throughput or human
input latency. Tests enforce 16 KiB steady / 24 KiB shake output and the 64 MiB
decoded-image bound; timing measurements remain hardware-dependent.

## Synthetic PTY observations

[pty-results.json](pty-results.json) contains the final 24 cases. The harness
supplies capability responses and validates emitted commands; it is not Ghostty.
It also base64-decodes/decompresses every image, checks the exact RGBA dimensions,
validates source rectangles against their uploaded images and models ownership.

- Successful probes use compressed uploads throughout. Explicit compression
  rejection and an absent reply both start successfully with raw uploads.
  Missing graphics, missing enhanced keys and a silent terminal reject cleanly;
  the silent negotiation stops at the two-second deadline.
- The diagnostic receives ordinary-letter releases and startup-preserved input.
  Production flow exercises movement, focus loss/gain, pause, restart, resize
  suspension and a font-size change with an unchanged character grid.
- Full, reduced and off modes run. The timed effects scene reaches all banners,
  flashes, destruction, debris, squash, shake and an automatic mid-shatter pause.
- Normal quit, Ctrl-C, SIGINT, SIGTERM, SIGHUP, SIGQUIT, SIGTSTP, a controlled
  runtime error and an unwinding panic restore echo/canonical/ISIG modes. Cursor
  show, keyboard pop, focus disable and alternate-screen exit are present;
  errors appear after restoration. No modeled resources remain after exit.
- Short 30/60 FPS runs record PTY output intervals in the JSON. These are output
  timestamps, not display frame timings or an endurance/latency acceptance test.

## Shipped protocol path and native acceptance

The implementation ships **cached atlases with `x,y,w,h` source rectangles**,
direct RGBA uploads and an optional `o=z` capability. Seven images are used in
full mode: background, ball/trail, paddle, bricks/flash/shatter, chips, typography
and overlay. Reduced/off omit the overlay and use six. The typography image
contains both the logo sweep and banner fades, with separate padded row grids
to keep total decoded storage below 64 MiB. Fixed placement IDs stay below 168;
the registry hard limits are 16 images / 192 placements.

A fresh CLI check found `/Applications/Ghostty.app` and reported **Ghostty 1.4.0**.
The native computer-use tool rejected access to `com.mitchellh.ghostty` with
“Computer Use is not allowed to use the app … for safety reasons.” Consequently,
no interactive Ghostty session was controlled or visually accepted. In particular,
source-rectangle rendering has not been confirmed in the real terminal. Unit
and PTY protocol checks and offline inspection of generated brick/font atlases
do not establish that support.

All eleven native acceptance steps in spec §10.2 remain **unverified**:

1. Compressed upload and source-rectangle rendering. If rectangles fail in actual
   Ghostty, implement/test the specified one-image-per-frame fallback with its
   revised image budget before accepting that terminal path.
2. High-DPI scaling beyond 2.0 and correct letterboxing.
3. Smooth ball, paddle, brick and glyph edges.
4. Shadows/glow and absence of neighbouring-brick overlap.
5. A 3-HP brick's flash, damage cracks, shatter and chips.
6. Life-loss shake, cleared trail, READY and no HUD overdraw, including 80×30.
7. Mid-shatter pause and continuation after resume.
8. Resize and font-size changes during effects without stale placements.
9. Visual behavior of full/reduced/off against spec §7.
10. Five minutes of actual 60 FPS gameplay, observing frame timing, flicker,
    accumulating placements and input lag.
11. Native terminal restoration and graphics removal after normal/error/panic exit.

Use `cargo run --release --features dev-harness --example acceptance -- effects`
for the repeating presentation scene, adding `--fx reduced` or `--fx off` for
comparison. It automatically pauses mid-shatter and resumes; P also permits
manual pause. Q quits. The `terminal_spike`, `won`, `game-over`, `damaged`, `error`
and `panic` fixtures remain available. Record OS, Ghostty version, window pixels,
cell geometry, font size and observed frame timing when completing acceptance.

## Implementation decisions and acceptance departure

The only delivery-order departure is continuing after automated protocol checks
without native confirmation in step 1: application access was denied as described
above. This preserves the spec's explicit distinction between automated evidence
and real-terminal acceptance; that acceptance is still pending.

Launch/restart notifications return from `Game::action`, where those transitions
actually occur; `Game::tick` returns contacts and life/level/end transitions.
An additional `Restarted` notification clears presentation state without storing
an event flag in `Game`. This keeps `PartialEq` restricted to simulation state.
Compression rejection retains the specified 2.0 cap. No unsupported-terminal
fallback has been assumed from a synthetic test.

Pixel geometry still comes from OS/Crossterm; only one reader owns stdin at a
time. Gameplay remains 120 Hz with the 100 ms / 12-tick iteration bounds. No
external assets, Kitty animation playback, framebuffer transport, configuration,
mouse, sound or new gameplay rules were added. Suspend/resume remains unsupported;
SIGTSTP exits cleanly. See README.md for the remaining platform boundaries.
