# Ricochet — implementation specification

Build a complete, playable Breakout-style terminal game named **Ricochet**, in Rust, targeting **Ghostty 1.4 or newer on macOS and Linux**. This document is the implementation brief for Codex. Implement the project, tests, and documentation; do not stop at a scaffold or a plan.

## 1. Product and scope

The player moves a paddle horizontally, bouncing a ball into a field of bricks. Bricks have one to three hit points, visibly deteriorate on impact, and disappear when destroyed. Missing the ball costs a life. Clear three levels to win.

Use actual pixel graphics through the Kitty graphics protocol inside the terminal. The ball must move in increments smaller than a terminal character cell. Keep the result compact, responsive, and easy to run with `cargo run --release`.

Required: title screen, keyboard controls, three deterministic levels, score, three lives, pause/resume, restart, win/game-over screens, resize handling, and reliable terminal restoration.

Out of scope for this release: sound, mouse controls, power-ups, multiball, online features, saved scores, configuration files, level editor, external assets, Windows support, and tmux/screen/SSH compatibility guarantees. Do not add a separate GUI window, browser, game engine, or Unicode-only rendering mode.

## 2. Technology decisions

- Rust, stable toolchain, edition 2024. Record the tested compiler version and an accurate `rust-version` in Cargo.toml.
- `crossterm` for terminal lifecycle, keyboard events, focus events, and resize events.
- A small application-owned Kitty renderer for transmitting and placing raster sprites. Crossterm does not provide this renderer.
- `base64` for protocol payloads. Generate small RGBA sprite buffers directly in Rust; an image-processing dependency is unnecessary unless implementation evidence justifies it.
- Standard-library timing and synchronous I/O. No Tokio, ECS, physics engine, or Ratatui in the initial implementation.
- Select compatible stable crate releases at implementation time and commit Cargo.lock. Keep dependencies small. An error-reporting crate is acceptable.

Separate simulation from terminal I/O. Tests must run without Ghostty or an interactive terminal.

## 3. Visual design

Use a dark navy playfield (`#101827`), pale ball (`#F3F6FF`), cyan paddle (`#65D6E8`), and contrasting bricks: coral for 3 HP, amber for 2 HP, teal for 1 HP. Add simple pixel cracks or missing highlight segments so damage remains visible without colour perception. Draw crisp geometric shapes with restrained highlights. No elaborate effects are required.

The logical playfield is **800 × 600 units**, origin at its upper-left, positive y downward. Keep physics in these coordinates regardless of terminal size. Render with a uniform scale and centre the playfield; letterbox rather than distort it. Pixel-round only when rendering.

Reserve two terminal rows above the playfield for `RICOCHET`, score, lives, and level. Reserve two below for controls and state messages. Text remains ordinary terminal text and must not overlap the graphical playfield. Use the text rows for title, pause, and end-state prompts; a pixel font is unnecessary.

Require at least 80 columns × 30 rows and a drawable pixel area large enough for a 400 × 300 playfield. If either requirement fails, suspend play and show a resize prompt. The user can still quit. Do not discard game progress.

## 4. Game geometry and rules

Initial constants (centralise them; modest feel adjustments are permitted and must be documented):

| Property | Value |
| --- | --- |
| Paddle | 112 wide × 14 high; top y = 556 |
| Paddle speed | 600 units/second |
| Ball | Radius 7 |
| Level starting speeds | 300, 340, 380 units/second |
| Speed increase | +4 per destroyed brick; cap at 520 |
| Brick grid | 10 columns × 5 rows |
| Brick size | 68 × 24 |
| Grid gaps | 8 horizontally and vertically |
| Grid origin | x = 24, y = 70 |
| Lives | 3 for the entire run |
| Score | 10 per damage point, plus 25 on destruction |

The grid occupies 752 units horizontally and is centred. Paddle x is clamped so its full rectangle stays in the playfield. Side and top walls reflect the ball. The bottom is open: lose a life when the entire ball passes below y = 600. Award no score for walls or paddle hits.

Every level uses this grid. Rows below are ordered top to bottom:

- Level 1: all cells populated, row HP = `[2, 2, 1, 1, 1]`.
- Level 2: all cells populated, row HP = `[3, 3, 2, 2, 1]`.
- Level 3: row HP = `[3, 3, 3, 2, 2]`; omit cells for which `(row + column) % 4 == 0`, using zero-based indices.

Destroying the final brick completes the level. Between levels, preserve score and remaining lives, reset the paddle to centre, reset ball speed to the next level's starting speed, and wait for launch. Clearing level 3 wins. Level completion takes precedence over a life-loss event at the same simulation time.

After losing a life, preserve brick damage, score, and current ball speed. Centre the paddle and attach the ball just above it. If no lives remain, enter GameOver. A new run resets everything.

In Ready, the attached ball follows the paddle. Space launches upward at 20 degrees from vertical; alternate left/right between launches, with the first launch going right. Use deterministic behaviour, not randomness.

## 5. Controls and state transitions

| Input | Action |
| --- | --- |
| Left / A | Move left while held |
| Right / D | Move right while held |
| Space | Start a run from Title; launch from Ready |
| P / Escape | Pause or resume an active run |
| R | Restart from Paused, Won, or GameOver |
| Q / Ctrl-C | Quit from any state |

Model `Title`, `Ready`, `Playing`, `Paused`, `Won`, and `GameOver` explicitly. Paused retains whether the previous state was Ready or Playing. A too-small viewport is a separate suspension condition: remember the underlying state and require explicit resume when playable dimensions return.

Movement applies in Ready and Playing only. Both directions held means zero movement. One-shot actions fire on press only; ignore autorepeat for launch, pause, restart, and quit. Track held physical actions so releasing A does not cancel a still-held Left key. Clear held keys on focus loss, pause, resize suspension, and restart.

Enable enhanced keyboard reporting, including release events and reporting for ordinary letter keys. Negotiate support rather than assuming legacy input provides releases. If required reporting is unavailable, exit cleanly with an explanation instead of shipping unreliable held-key behaviour. Focus loss automatically pauses an active run; focus gain never resumes automatically.

## 6. Physics and timing

Use a fixed simulation timestep of **1/120 second** with a monotonic clock and accumulator. Target **60 render frames/second** independently. Bound elapsed time added after a stall to 100 ms and process at most 12 simulation ticks per outer iteration; discard excess accumulated backlog. No catch-up movement after pause, focus loss, or resize. Waiting for input or the next deadline must not busy-spin.

Use continuous collision detection, or bounded adaptive substeps proven by tests to prevent tunnelling at maximum speed. Handle the ball as a circle and bricks/paddle as axis-aligned rectangles. If using a point swept against expanded rectangles, resolve corner geometry explicitly or document and test the approximation. Avoid repeated damage when a ball remains in contact after a response.

For swept collisions, find the earliest contact, advance to it, resolve the normal, separate by a small epsilon, then consume the remaining timestep. Bound collision iterations and safely discard remaining time if the cap is reached. At simultaneous contacts, damage each distinct brick at most once for that contact event and reflect each impacted axis once. Use stable brick index order for ties. Preserve speed magnitude except for explicit speed increments.

Paddle collisions should occur when the ball approaches the top surface from above. Account for paddle movement within the timestep; do not let the paddle teleport through the ball. Set the outgoing angle from the hit position:

`offset = clamp((ball.x - paddle.centre_x) / (paddle.width / 2), -1, 1)`

`angle = offset * 65 degrees`

`vx = speed * sin(angle); vy = -speed * cos(angle)`

Prevent indefinite vertical bouncing by enforcing `abs(vx) >= 0.12 * speed` after a paddle hit, using the incoming horizontal sign (or the current launch direction when zero), then recompute negative vy to preserve speed. No random deflection. Side/bottom paddle contacts need not rescue an already-missed ball.

## 7. Terminal and graphics implementation

Before entering fullscreen, require interactive stdin and stdout. `--help` and `--version` must work without a TTY. Provide `--fps 30|60` (default 60); rendering rate must not affect simulation speed.

Capability negotiation must be bounded (maximum two seconds total), identify Kitty graphics support, establish pixel geometry, and enable enhanced keyboard events. Prefer terminal window pixel dimensions from OS/Crossterm where available; use documented terminal queries if needed. Never hard-code a font aspect ratio or trust only TERM/TERM_PROGRAM. A missing required capability produces a clear error and clean exit.

Terminal responses and keyboard events share stdin. Design negotiation with one reader; do not race a raw response reader against Crossterm. Verify what the selected Crossterm version exposes, perform raw protocol probing in a bounded startup phase if necessary, and preserve unrelated input. Once negotiation is complete, hand input ownership to the event loop. Resize geometry updates must follow the same single-reader discipline.

Renderer requirements:

- Upload a small reusable sprite set: ball, paddle, brick variants, playfield/background and borders as needed. Keep IDs and placement IDs in an owned registry.
- Upload sprites during initialisation or when pixel scale changes, not once per animation frame. Reuse placements and image data for motion. Remove obsolete placements explicitly so no trails remain.
- Convert each world position to physical pixels, then to terminal cursor cell and within-cell pixel offsets. Sprite dimensions must retain the same uniform scale as positions. Handle placements that cross cell boundaries. Movement must not snap to character cells.
- Use protocol-correct payload chunking, transmission, placement, and deletion commands. Consult the current protocol rather than inventing escape sequences. Suppress unnecessary successful command replies after probing.
- Batch a frame's commands in one output buffer and flush once. Use synchronized terminal updates where supported, with a guard ensuring each begin is paired with an end even on errors.
- Establish explicit layering for background, bricks, paddle, and ball. Keep graphical placements away from HUD rows. Do not clear the entire terminal every frame.
- Redraw changed HUD values and damaged bricks only when needed. A simple static scene plus moving ball/paddle is sufficient; do not implement protocol animation playback as the game clock.
- On resize or font-size change, recompute dimensions and scale, invalidate old placements, and regenerate scale-dependent sprites. Detect geometry changes even if the character grid remains unchanged (periodic OS geometry checks are acceptable).
- Keep image/placement counts bounded across frames, restarts, and resizes. Delete only Ricochet-owned resources on cleanup.

A cached sprite renderer is the default design. If an observed Ghostty limitation forces framebuffer rendering, document the reason, keep memory bounded, measure throughput, and still meet the visual and responsiveness acceptance checks. Do not silently replace graphics with text.

## 8. Cleanup and error behaviour

Use an idempotent terminal-session guard tracking which modes were actually enabled. Restore the previous screen, cursor visibility, raw mode, keyboard enhancement stack, focus reporting, colours, and synchronized-update state on normal quit, Ctrl-C, startup failure, runtime error, and unwinding panic. Remove owned graphics before leaving the alternate screen. Report errors after restoration.

Handle termination signals through a safe notification mechanism where practical; do not run terminal I/O directly in an async signal handler. Either support suspend/resume safely or explicitly document that it is unsupported. No promise is required for SIGKILL or process abort. Do not log debug output to the live terminal surface.

## 9. Suggested project organisation

Use one Cargo binary project with testable modules. A library target is welcome if it simplifies integration tests.

| Module | Responsibility |
| --- | --- |
| `main.rs` / `app.rs` | CLI, startup, event loop, orchestration |
| `game.rs` | States, lives, scoring, deterministic update |
| `physics.rs` | Collision detection and responses |
| `levels.rs` | Static level generation |
| `input.rs` | Held keys and action mapping |
| `terminal.rs` | Session guard, capabilities, geometry |
| `render/kitty.rs` | Protocol encoding and resource lifecycle |
| `render/sprites.rs` | Procedural raster artwork |
| `render/mod.rs` | World-to-screen mapping and scene updates |

Avoid speculative abstractions. Keep tunable game constants together. Use injected time/input or pure simulation calls in tests. Normal gameplay must not depend on wall-clock time inside physics functions.

## 10. Validation and acceptance

Automated tests must cover behaviour, especially:

- Wall reflections, brick face/corner contacts, simultaneous contacts, maximum-speed tunnelling, no duplicate damage, and no non-finite or stuck states during a long deterministic run.
- Paddle impact angles and minimum horizontal speed; left/right paddle limits; moving-paddle contact; misses cannot be rescued from below.
- Brick HP and score accounting, life loss, level progression, and all end-state/restart transitions.
- Pause and resize suspension do not advance physics; equivalent input histories produce equivalent simulation results at 30 and 60 rendering FPS.
- Held-key combinations and releases, ignored repeat actions, focus-loss clearing.
- Pixel mapping at several font/viewport dimensions; sprite chunk boundaries; bounded resource ownership; cleanup after partial initialisation using a mock writer.

Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, and `cargo build --release`. Fix failures before delivery.

Manually verify in actual Ghostty 1.4+ on at least one supported OS:

1. Start with `cargo run --release`, launch, steer with arrows and A/D, and observe round-ball motion within character cells.
2. Damage a multi-hit brick and confirm its appearance changes before destruction; verify scoring and lives.
3. Pause/resume, lose focus while holding a direction, restart, and quit. No stuck paddle or surprise motion on return.
4. Resize repeatedly, change font size, shrink below the minimum, and restore. Preserve the game; leave no stale images or HUD overlap.
5. Reach GameOver and Won, using test fixtures or a documented development-only harness if necessary; never expose a default gameplay cheat.
6. Exercise ordinary exit and controlled errors; the shell remains usable, cursor visible, echo working, and graphics gone.
7. Play for five minutes at the default frame target. No visible flicker, accumulating trails, unbounded resource growth, or sustained input lag. Record OS, Ghostty version, terminal dimensions, and observed frame timing. Performance is a measured target, not an unsupported guarantee.

If Ghostty is unavailable in the implementation environment, complete automated checks and clearly mark real-terminal checks as unverified. Do not claim visual or input compatibility based only on unit tests or a generic PTY.

## 11. Delivery and execution order

First prove a minimal Kitty rendering/input spike: display and move a ball at sub-cell positions, obtain release events, and restore the terminal. Then build tested simulation, integrate the renderer, add state screens and resize handling, and finish validation. Keep the spike only if it remains a useful documented diagnostic.

Deliver source, Cargo.toml, Cargo.lock, this specification as `SPEC.md`, and a concise README covering prerequisites, build/run commands, controls, supported environments, architecture, test commands, and known limitations. Include a brief verification report separating automated results from actual Ghostty checks. No placeholder implementations, TODO core features, or speculative compatibility claims.

Resolve routine implementation details autonomously within this scope. Document any material departure from this specification and the evidence requiring it.

## 12. Authoritative references

Consult these when implementing protocol details; the design choices and numeric constants above are requirements for Ricochet, not claims from the references.

- Ghostty 1.4 release notes: https://ghostty.org/docs/install/release-notes/1-4-0
- Kitty graphics specification: https://sw.kovidgoyal.net/kitty/graphics-protocol/
- Kitty keyboard specification: https://sw.kovidgoyal.net/kitty/keyboard-protocol/
- Crossterm API: https://docs.rs/crossterm/latest/crossterm/
- Crossterm events: https://docs.rs/crossterm/latest/crossterm/event/index.html

Specification prepared 28 October 2026.
