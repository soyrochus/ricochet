# Ricochet — visual evolution specification (002)

This document is the implementation brief for Ricochet's second release. It builds on [SPEC-init-001.md](SPEC-init-001.md) and the shipped 0.1.0 code. It replaces the visual-design parts of 001 (§3 and parts of §7) and leaves everything else in force. Implement the changes, tests, and documentation; do not stop at a scaffold or a plan.

## 1. Goal and scope

Release 0.1.0 uses the Kitty graphics protocol as a simple sprite blitter. It has nine flat-colour sprites without anti-aliasing, raw uncompressed uploads, no transparency effects and no animation. The playfield is also capped at 2 physical pixels per logical unit, so on high-DPI displays it fills only part of the window.

This release makes the game look and feel like a modern Breakout-genre game. It stays within the existing cached-sprite architecture and adds two layers of work:

- **A. Static art and protocol efficiency.** Anti-aliased procedural art, depth through alpha shadows and glow, a richer background, a scanline/vignette overlay, compressed uploads and a higher scale cap.
- **B. Motion and feedback.** Sprite atlases selected by source rectangle, a ball trail, hit flashes, brick-shatter animation, debris particles, paddle squash, screen shake, and graphical banners and a title logo.

**Hard rule:** everything in this specification is presentation. The simulation must produce bit-identical state with effects on or off, at 30 or 60 FPS, and at any terminal size. Gameplay rules, constants, physics and timing in 001 §4–§6 do not change.

**Out of scope:**

- Any gameplay change. This includes power-ups, multiball, new brick types (indestructible or metallic), and changes to scoring or speed. These need their own specification, because they change the deterministic rules and their tests.
- Full-frame software rendering (a framebuffer pushed through shared memory).
- Kitty animation-frame playback (`a=f`, `a=a`, `a=c`). 001 already forbids it as the game clock, and Ghostty support is not established.
- Relative placements (`P`/`Q`).
- Unicode placeholders.
- Sound, mouse input, configuration files, and external image or font assets.

Everything else listed as out of scope in 001 §1 remains out of scope.

## 2. Carried-over constraints

These rules from 001 still apply:

- The target is Ghostty 1.4+ on macOS and Linux.
- Only one reader consumes stdin.
- Capability negotiation is bounded.
- Frames are batched, flushed once, and wrapped in synchronized updates.
- Image and placement ownership stays bounded, and cleanup deletes only Ricochet-owned resources.
- HUD rows stay ordinary terminal text, and graphics never overlap them.
- No `TERM` sniffing.
- Tests run without a terminal.

No external assets: all art, including the pixel font, is generated procedurally or embedded as Rust constants.

## 3. Protocol usage

### 3.1 Compressed transmission

- Upload all images with zlib compression (`o=z`) on top of the existing direct, chunked, base64 RGBA transmission (`t=d,f=32`). Add `flate2` with its pure-Rust backend (`default-features = false, features = ["rust_backend"]`).
- Extend negotiation with one bounded probe: a 1×1 `a=q` query using `o=z`.
  - If the terminal accepts it, use compression.
  - If not, fall back to uncompressed uploads and keep the 0.1.0 scale cap of 2.0 (see §3.4).
  - Compression is an optimisation, not a required capability. Its absence must never abort startup.
- Keep chunking at 4096 base64 bytes. The `s`/`v` keys keep the uncompressed pixel dimensions, as the protocol requires.

### 3.2 Sprite atlases and source rectangles

- Group related frames into atlas images. Select a frame per placement with the source-rectangle keys `x`, `y`, `w`, `h` on `a=p`.
- **Changing a frame means re-placing with a different source rectangle.** It never re-uploads anything.
- Extend `Registry::place_clipped` (or replace it) so one call can carry a source rectangle. The existing ball bottom-clip becomes a reduced `h` inside the ball's source rectangle.
- Atlas layout is computed once per scale. Frames are packed on a grid with at least 1 physical pixel of transparent padding, so filtering never bleeds between neighbours.
- Ghostty support for source rectangles must be confirmed in real-terminal acceptance (§10.2). If it is absent, the documented fallback is one image per frame, with the image budget raised to match. Record which path ships.

### 3.3 Layers

Explicit `z` values from bottom to top. Each layer occupies the playfield rectangle only.

| z | Layer |
| --- | --- |
| 0 | Background (gradient, grid, border) |
| 1 | Bricks, with baked shadows |
| 2 | Brick-shatter animations |
| 3 | Debris particles |
| 4 | Paddle, with baked shadow |
| 5 | Ball trail |
| 6 | Ball, with baked glow |
| 7 | Banners and title logo |
| 8 | Scanline/vignette overlay (`--fx full` only) |

All `z` values stay non-negative, so graphics draw above the terminal background colour. HUD text never shares cells with placements.

### 3.4 Scale cap and memory budget

- Raise the uniform scale cap from 2.0 to **3.0** physical pixels per logical unit, only when compression is available. Centring and letterboxing are unchanged.
- **Terminal-side memory:** the decoded RGBA size of all simultaneously uploaded images, at scale 3.0, must not exceed **64 MiB**. A unit test computes this sum from the sprite generators.
- **Transmission:** the compressed transmission of the full image set at scale 3.0 should be under 2 MiB. Measure it and record it in the verification report.

### 3.5 Resource bounds

| Resource | 0.1.0 | 0.2.0 limit |
| --- | --- | --- |
| Images (`MAX_IMAGES`) | 9 | 16 |
| Placements (`MAX_PLACEMENTS`) | 53 | 192 |

Planned placement use:

| Use | Count |
| --- | --- |
| Background | 1 |
| Bricks | 50 |
| Paddle | 1 |
| Ball | 1 |
| Trail | ≤ 8 |
| Shatter animations | ≤ 8 |
| Particles | ≤ 96 |
| Banners | ≤ 2 |
| Overlay | 1 |
| **Total** | **≤ 168** |

Each family uses a fixed range of placement IDs that is reused, never allocated per effect. If a pool is exhausted, the oldest effect in that pool is retired (its placement re-used); never exceed the pool.

## 4. Architecture

### 4.1 Simulation events

The renderer needs to know *what happened*, not only the resulting state.

- `physics::step` reports contacts in its outcome, in the order they were resolved:
  - `Wall { normal }`
  - `Paddle { offset }`, where offset is in [-1, 1]
  - `Brick { index, hp_after }`
  - each with a contact point in logical units
- `Game::tick` returns these contacts together with transition events: `Launched`, `LifeLost`, `LevelCleared { next }`, `Won`, `GameOver`.
- **Events are not stored in `Game`.** `Game`'s `PartialEq` must still describe simulation state only, so the 30/60 FPS equivalence tests keep working unchanged.
- `Controller` collects the returned events into a bounded queue (`VecDeque`, capacity 256, dropping the oldest). The app loop drains it into the renderer once per rendered frame.
- `Renderer::draw` keeps taking `&Game` and additionally receives the drained events.
- Collecting events must not change any physics result. Existing physics and determinism tests must pass unchanged.

### 4.2 Effects state (`render/fx.rs`)

A new module owns all transient visual state: trail history, active flashes, shatter animations, particles, paddle squash, shake, and banner fades.

- **Units.** Effects live in logical units, so they survive resizes without conversion.
- **Clock.**
  - Effects advance on the render clock with `dt = min(frame time, 50 ms)`.
  - They freeze while the game is `Paused`, while the viewport is too small, and while resume is required.
  - They never advance or influence the simulation clock.
  - Particle motion is integrated at a fixed 1/120 s step inside the effects module, so 30 and 60 FPS produce the same trajectories, sampled at different times.
- **Randomness.**
  - Use a small embedded PRNG (xorshift or SplitMix) seeded from the event's contents (brick index, level, a running event counter).
  - The same event history must produce the same effect spawns.
  - Add no `rand` dependency.
- **Interface.** The module exposes something like `fx.ingest(&events)`, `fx.advance(dt, &game)` and `fx.placements() -> impl Iterator<Item = Draw>`. Here `Draw` is a logical position, an atlas frame and a z-layer.
- **Separation from Kitty.** `render/mod.rs` turns `Draw` items into protocol commands and diffs them against the placements from the previous frame. Only changed placements are written, as in 0.1.0. `fx.rs` must contain no Kitty code, so it can be tested in isolation.

### 4.3 Module changes

| Module | Change |
| --- | --- |
| `physics.rs`, `game.rs` | Return contact and transition events; no rule changes |
| `app.rs` | Event queue in `Controller`; `--fx` flag; pass events to renderer |
| `terminal.rs` | Compression probe |
| `render/kitty.rs` | `o=z` uploads, source rectangles, raised limits, placement pools |
| `render/sprites.rs` | Supersampled raster helper, new art, atlas builders |
| `render/font.rs` (new) | Embedded 5×7 pixel font and text rasteriser |
| `render/fx.rs` (new) | Effects state, as in §4.2 |
| `render/mod.rs` | Layering, shake offset, diffing of effect placements |

## 5. Static art (A)

### 5.1 Anti-aliasing

- Replace the single-sample `raster()` with a coverage-based rasteriser that **supersamples adaptively**:
  1. Take 1 sample per pixel.
  2. Where a pixel's 2×2 corner probes disagree on shape membership, re-sample at 4×4 and average the premultiplied colour.
- Shapes are expressed as signed-distance or membership functions in logical units, as now.
- Upload output as straight (non-premultiplied) RGBA, as the protocol requires.
- All edges must be anti-aliased: the ball, the paddle's rounded ends, brick corners (1.5-unit radius), cracks, glyphs and the border.

### 5.2 Palette

Keep the 001 identity colours as base tones:

| Element | Colour |
| --- | --- |
| Navy playfield | `#101827` |
| Pale ball | `#F3F6FF` |
| Cyan paddle | `#65D6E8` |
| Coral brick (3 HP) | `#F17771` |
| Amber brick (2 HP) | `#F5BE5B` |
| Teal brick (1 HP) | `#42B9A7` |

Gradients and bevels are derived from these tones. No new hues are introduced except neutral black and white for shadows, glow and highlights.

### 5.3 Background

- A vertical gradient from `#141E31` at the top to `#0C1220` at the bottom.
- Faint 1-unit grid lines every 40 units, at alpha about 0.06 over the gradient, replacing 0.1.0's dots.
- A 2-unit `#2A3F55` border with a 4-unit inner glow fading to transparent.
- The bottom edge stays open, as now.
- The background is smooth, so it needs supersampling only along the border.

### 5.4 Bricks

- **Body.**
  - A vertical gradient of ±12% lightness around the base tone.
  - A 2-unit bevel: lighter on top and left, darker on bottom and right.
  - Deterministic value noise of ±4% lightness on 2×2-unit cells, seeded per colour.
- **Reinforcement mark.** 3-HP bricks carry a 1-unit inset line 4 units inside the edge.
- **Damage.**
  - Keep 0.1.0's semantics: one crack per damage point, plus missing bevel segments.
  - Cracks become anti-aliased jagged polylines of dark navy with a 1-unit light edge on one side.
  - Damage must stay distinguishable in greyscale (001 §3).
- **Shadow.**
  - Baked into the brick sprite: black at alpha 0.35, offset (+3, +4), blurred with a 2-unit radius.
  - The sprite gets transparent padding to hold it, and the placement position subtracts the padding.
  - The shadow's extent (offset + blur ≤ 6) must stay below `GAP` (8), so neighbouring bricks never overlap at equal `z`. A unit test asserts this.
- **Hit-flash frame.** Each of the six HP variants gets a flash frame: the body blended 60% toward white, with the cracks kept.

### 5.5 Paddle

- A rounded capsule with 3-unit end radii.
- A vertical cyan gradient with a 2-unit top highlight (`#B2F1F9`) and a dark lower lip (`#26849A`).
- A baked shadow at (0, +4), alpha 0.3, 2-unit blur.
- Squash frames are defined in §6.4.

### 5.6 Ball

- An anti-aliased disc with a radial shade: `#F3F6FF` at the centre to `#B8D0E6` at the rim.
- A small specular dot offset up and to the left.
- A baked glow halo out to radius 14 logical units: pale cyan, peak alpha 0.25, falling off quadratically.
- The sprite is 28×28 logical units, placed at `position − 14`.
- Bottom clipping (0.1.0 behaviour) applies to the enlarged sprite. The glow must never paint into the footer rows.

### 5.7 Overlay (`--fx full` only)

- One playfield-sized RGBA image at z 8, uploaded once per scale:
  - **Scanlines:** every third *physical* row is black at alpha 0.10.
  - **Vignette:** black, alpha rising from 0 at 60% radius to 0.35 at the corners.
- It is static and never re-placed except for screen shake (§6.6).

## 6. Motion and feedback (B)

All durations are in effects-clock time (§4.2). Each item lists the event that triggers it.

### 6.1 Ball trail (`Playing` only)

- Keep the ball's last 8 rendered positions.
- Draw them as ghost sprites from a 4-step alpha atlas: alpha 0.40, 0.25, 0.14, 0.06, two ghosts per step.
- The number of ghosts shown scales with `speed / MAX_SPEED`, clamped to [3, 8].
- Clear the history on `Launched`, `LifeLost` and `LevelCleared`, so ghosts never connect discontinuous positions.
- Ghosts use the same bottom clip as the ball.

### 6.2 Brick hit flash (`Brick { hp_after > 0 }`)

Show the variant's flash frame for 70 ms, then the damaged frame. Spawn 4 debris chips (§6.5) at the contact point.

### 6.3 Brick shatter (`Brick { hp_after == 0 }`)

- At the brick's former position, play a 5-frame shatter animation over 200 ms:
  - The brick splits along its crack lines into 4–6 fragments.
  - The fragments drift apart by up to 6 units and fade over the last two frames.
  - The frames are pre-rendered in the brick atlas: one strip per colour.
- Spawn 10 chips of the brick's colour.
- Remove the static brick placement in the same frame the shatter starts.

### 6.4 Paddle squash (`Paddle`)

- A 3-frame atlas animation over 120 ms: 0.1.0 shape → width +4 / height −3 → width +2 / height −1 → back to the 0.1.0 shape.
- Keep the bottom edge and horizontal centre fixed.
- This is visual only; the collision rectangle is unchanged.

### 6.5 Debris particles

- **Shape.** Small square-ish chips, 3–6 units, from a particle atlas: 4 sizes × 3 brick colours × 2 alpha steps.
- **Launch.**
  - Initial speed 120–260 units/s, within ±60° of the contact normal.
  - Gravity 900 units/s².
  - Lifetime 450–700 ms; the chip switches to its low-alpha frame for the last 30%.
- **Removal.** A chip is removed when its lifetime ends or it leaves the playfield rectangle.
- **Pool.** 96 chips; the oldest is retired first.

### 6.6 Screen shake

| Event | Amplitude | Decay |
| --- | --- | --- |
| `LifeLost` | 6 units | Linear over 300 ms |
| `Brick { hp_after == 0 }` | 1.5 units | 80 ms |

- **Behaviour.**
  - Overlapping shakes take the maximum amplitude, not the sum.
  - The offset is a deterministic function of the effects clock (PRNG per shake).
  - It is applied in logical units to every playfield placement, background and overlay included.
- **Clamp.**
  - On each axis, the offset is limited to the letterbox margin available on that side.
  - When there is no margin on an axis, that axis does not shake.
  - Placements must never enter the HUD rows or leave the terminal.
- **Redraws.** Shake re-places every playfield placement while active (about 170 commands of about 60 bytes each). This is within the frame budget (§9). When shake ends, the next frame restores exact positions.

### 6.7 Banners and title (z 7)

- **Pixel font.**
  - An embedded 5×7 font in `render/font.rs` covering `A–Z`, `0–9`, space, `/` and `!`.
  - Glyphs are rasterised as rounded squares with anti-aliasing (§5.1), not hard pixels.
  - Fill: a vertical gradient from pale to cyan.
  - A 1-unit dark outline and a soft drop shadow.
- **Title logo** (`Title` state).
  - "RICOCHET" at 10 logical units per font pixel (480 units wide), centred in the upper third.
  - An 8-frame highlight sweep plays once every 2.5 s.
- **Banners** at 6 units per font pixel, centred:
  - `READY`: in `Ready` before the first launch of a level and after each lost life.
  - `LEVEL 2` or `LEVEL 3`: on `LevelCleared`, while the next level is in `Ready`.
  - `PAUSED`
  - `GAME OVER`
  - `YOU WON`
- **Fade-in.** 4 precomputed alpha steps over 160 ms. A banner is removed immediately when its state ends.
- **HUD.** The HUD rows keep their text, so every message remains readable as plain text. The banners add to it and replace nothing.
- **Pre-rendering.** Banner and logo images are rendered when the scale changes, not when a banner is shown. Logo sweep frames and banner alpha steps share one atlas.

## 7. Effects levels

Add the CLI flag `--fx full|reduced|off`, default `full`, and document it in `--help` and the README.

| Feature | full | reduced | off |
| --- | --- | --- | --- |
| Anti-aliasing, new art, shadows, glow, compression, scale cap | ✓ | ✓ | ✓ |
| Banners and logo (static, no sweep or fade) | ✓ | ✓ | ✓ |
| Logo sweep, banner fade | ✓ | ✓ | — |
| Hit flash, shatter, paddle squash | ✓ | ✓ | — |
| Particles | 96 pool, 10/4 per event | 32 pool, 4/2 per event | — |
| Ball trail | ✓ | 3 ghosts max | — |
| Screen shake | ✓ | — | — |
| Scanline/vignette overlay | ✓ | — | — |

`reduced` serves players sensitive to motion and slow links. `off` gives the static art with no transient effects. The simulation is identical in all three modes (§1 hard rule).

## 8. Lifecycle interplay

- **Resize or font-size change.**
  - Regenerate every scale-dependent image, atlas, overlay and banner.
  - Invalidate all placements, as in 0.1.0.
  - Effects state is in logical units and survives; it re-appears at the new scale on the next frame.
- **Pause and viewport suspension.**
  - Effects freeze in place (§4.2).
  - The `PAUSED` banner shows.
  - Shake offset is frozen; a pause while shaking keeps that offset until resume.
- **Restart (`R`) and new level.** Clear all effects except the banner that belongs to the new state.
- **Cleanup.**
  - Unchanged: delete owned images by number and never touch other images.
  - The raised limits stay bounded, and the existing mock-writer cleanup tests must cover the new image count.

## 9. Performance budgets

| Measure | Budget |
| --- | --- |
| Steady-state output per frame, no shake | ≤ 16 KiB |
| Output per frame during shake | ≤ 24 KiB |
| Sprite and atlas regeneration on a scale change, scale 3.0, release build | ≤ 250 ms |
| Render-side CPU (effects advance + diff + encode), median per frame on the test machine | ≤ 2 ms |

Uploads on a resize do not count toward the per-frame output budgets.

Assert the per-frame output budgets in tests with a mock writer and a scripted event history at maximum effect load. Measure the regeneration time and render-side CPU, and record them in the verification report.

Input latency and simulation timing from 001 §6 must not regress. Rendering must never delay simulation ticks beyond the existing per-iteration bounds.

## 10. Validation and acceptance

### 10.1 Automated tests (no terminal)

- **Determinism.**
  - A full scripted run yields identical `Game` state with `--fx full`, `reduced` and `off`, at 30 and 60 FPS.
  - The 72,000-tick run still passes.
  - All existing physics tests pass unchanged.
- **Events.**
  - Contacts are reported in resolution order.
  - Simultaneous brick contacts yield one event per brick.
  - Transition events fire exactly once.
  - The bounded queue drops the oldest entries when full.
- **Effects.**
  - The same event history produces the same particles and shake offsets.
  - Pools never exceed their limits.
  - Effects freeze while paused or suspended.
  - The trail clears on discontinuities.
  - Particles are removed outside the playfield.
- **Shake clamp.** At geometries with zero, small and large letterbox margins, no placement enters the HUD rows or a negative cell.
- **Art.**
  - Anti-aliased edges contain intermediate alpha values.
  - The shadow extent is below `GAP`.
  - The damaged and undamaged variants of each HP level differ in greyscale luminance at the crack pixels.
  - Atlas frames do not overlap and have padding.
- **Protocol.**
  - `o=z` payloads decompress to exactly `s×v×4` bytes.
  - Chunk boundaries are correct for compressed payloads.
  - Source-rectangle keys are emitted correctly.
  - The fallback path is used when the probe fails.
- **Budgets.**
  - Decoded image memory at scale 3.0 is ≤ 64 MiB.
  - Per-frame output is within §9 under maximum load.
  - Image and placement counts stay bounded across 100 resize/restart cycles.

Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (with and without `--features dev-harness`) and `cargo build --release`.

### 10.2 Manual acceptance in real Ghostty 1.4+

1. **Protocol support.** Confirm that compressed uploads and source-rectangle placements render correctly. Record which path shipped (§3.2).
2. **Scaling.** On a high-DPI display, confirm the playfield now scales beyond 2.0 and fills the window, letterboxed.
3. **Edges.** Confirm smooth edges on the ball, paddle, bricks and glyphs.
4. **Depth.** Confirm shadows and glow look right, and that no neighbouring bricks overlap.
5. **Damage and destruction.** Damage a 3-HP brick down to destruction. Observe the flash, the cracks, the shatter and the chips.
6. **Life loss.** Lose a life. Observe the shake, that the trail resets and the `READY` banner. Confirm the HUD is never overdrawn, including at the minimum 80×30 size.
7. **Pause.** Pause mid-shatter. Effects must freeze and continue on resume.
8. **Resize mid-effect.** Resize and change the font size during effects. There must be no stale placements, and effects must continue at the new scale.
9. **Effects levels.** Run with `--fx reduced` and `--fx off`, and confirm the table in §7.
10. **Endurance.** Play five minutes at 60 FPS in `full`, recording observed frame timing. There must be no flicker, no accumulating placements and no input lag.
11. **Exit.** Quit normally, on an error and on a panic. The terminal must be restored and all graphics gone.

If Ghostty is unavailable, complete the automated checks and mark these steps unverified, as in 001.

## 11. Delivery order

1. **Protocol groundwork.** `o=z` uploads with the probe and fallback, source-rectangle placement, raised limits and placement pools. Verify in Ghostty before going further.
2. **Events.** Plumb contacts and transitions through `physics`, `Game` and `Controller`. Prove determinism is unchanged.
3. **Static art (A).** Supersampled rasteriser, new background, bricks, paddle, ball, overlay and the raised scale cap.
4. **Effects module and atlases (B).** Trail, flash, shatter, particles, squash, then shake with its clamp.
5. **Pixel font, banners and title logo.**
6. **`--fx` levels, budgets and tests.**
7. **Documentation.** Update the README (features, `--fx`, the new dependency, scale cap, limits) and the verification report.

Each step must leave the game shippable. Keep the `terminal_spike` and `acceptance` examples working, and extend `acceptance` with an `effects` scene that triggers every effect on a timer for visual review.

Document any material departure from this specification and the evidence requiring it.

## 12. References

- Kitty graphics protocol (compression `o=z`, source rectangles `x,y,w,h`, `z`-index, deletion): https://sw.kovidgoyal.net/kitty/graphics-protocol/
- Ghostty 1.4 release notes: https://ghostty.org/docs/install/release-notes/1-4-0
- [SPEC-init-001.md](SPEC-init-001.md): baseline rules, which remain authoritative where not superseded here.

Specification prepared 29 October 2026.
