# Ricochet — bump sound effects specification (003)

This document is the implementation brief for Ricochet's third release. It builds on [SPEC-init-001.md](SPEC-init-001.md) and [SPEC-evolve-002.md](SPEC-evolve-002.md) and the shipped 0.2.0 code, in particular the simulation-event plumbing added in 002 §4.1 (`physics::step` contacts, `Game::tick` transition events, the `Controller` event queue). It adds a new, independent presentation channel — sound — without touching gameplay, rendering or the event types already in place. Implement the changes, tests, and documentation; do not stop at a scaffold or a plan.

## 1. Goal and scope

Ricochet has no audio today. This release adds short, synthesized sound effects for ball contacts: hitting a wall, hitting the paddle, and hitting a brick. All sound is generated in Rust at process start, the same way the sprites and pixel font are generated — there is no audio asset file, and no network or filesystem dependency.

**Hard rule:** sound is presentation only, exactly like 002's visual effects. The simulation must behave identically whether sound is on or off, at 30 or 60 FPS, and whether or not an audio output device exists on the machine. `Game`, `physics` and the event types are not modified by this specification.

**Explicit sound design intent** (carried over from the product discussion that preceded this spec): tones must sound full and rounded, not like an 8-bit/chiptune square wave. There is no music, no continuous or rolling "ball" sound, and no per-state stinger — only one short, distinct tone per kind of bump. Everything else about audio is out of scope.

**Out of scope:**

- Music or any continuous/background/ambient sound.
- Any sound tied to the ball's motion rather than a discrete contact (no rolling, whooshing or travel noise).
- Sound for transition events: `Launched`, `LifeLost`, `LevelCleared`, `Won`, `GameOver`, `Restarted`. Only `Event::Contact` is sounded. A later specification may add stingers for these; this one does not.
- Volume control, mixing levels, or any setting beyond on/off.
- Stereo panning or spatialisation. Output is mono.
- Pitch or timbre variation driven by paddle-hit offset, brick column, or any other continuous input. Each bump kind gets one fixed, precomputed tone; this stays minimal by design, not as a shortcut.
- Microphone input, recording, or any `rodio` feature beyond basic output playback.
- Any platform beyond the project's existing targets (macOS and Linux).

## 2. Carried-over constraints

These rules from 001 and 002 still apply:

- No external assets: all sound, like all art, is generated procedurally in Rust; nothing is loaded from disk or network.
- The terminal must be restored correctly on normal exit, on error and on panic (001/002 acceptance), and nothing audio-related may write to stdout/stderr in a way that corrupts the raw-mode screen.
- `Game`'s `PartialEq` describes simulation state only; sound, like effects, must not read or alter it beyond observing the events already produced.
- Tests run without a terminal. By the same principle, sound logic must be testable without a real audio device, because CI and sandboxed environments typically have none.

## 3. Sound design

Every tone is synthesized once, at 44,100 Hz, mono, 32-bit float samples in [-1, 1]. A tone is built from a fundamental oscillator with a fast downward pitch sweep (this, not the waveform shape, is what makes a bump sound like a rounded "thump" rather than a flat beep), a quiet upper harmonic for warmth, an attack/decay envelope, and a one-pole low-pass smoothing pass. No noise component is used anywhere — every tone stays purely tonal, consistent with "no noise of the ball."

### 3.1 Synthesis pipeline

For a tone with parameters `base_freq`, `end_freq` (sweep target, `end_freq < base_freq`), `sweep_ms`, `duration_ms`, `harmonic_ratio`, `harmonic_gain`, and a fixed `attack_ms = 2.0`:

1. For each sample `n` at time `t = n / 44100`:
   - `progress = min(t / (sweep_ms / 1000), 1.0)`
   - `freq(t) = base_freq + (end_freq - base_freq) * progress`
   - Integrate phase from `freq(t)` (do not compute `sin(2π·freq·t)` directly, since `freq` varies with time); advance a running phase accumulator by `2π·freq(t)/44100` each sample, for both the fundamental and the harmonic (`freq(t) * harmonic_ratio`).
   - `raw = sin(phase) + harmonic_gain * sin(harmonic_phase)`
   - Envelope: linear ramp `t / attack_ms` while `t < attack_ms`, otherwise exponential decay `exp(-(t - attack_ms) / tau)`, where `tau` is chosen so the envelope reaches about 1% of its peak at `duration_ms`.
   - `sample = raw * envelope`
2. Truncate the buffer at `duration_ms`.
3. Apply a single-pass one-pole low-pass filter, `y[n] = y[n-1] + α·(x[n] - y[n-1])`, with `α` chosen for a cutoff around 4–5 kHz at 44,100 Hz. This removes any residual edge harshness and is what makes the tone read as "rounded" instead of digital.
4. Normalise so the buffer's peak absolute value equals a fixed target (0.55) exactly, leaving headroom for simultaneous voices (§4.3). Normalising last, after filtering, is what makes the peak-amplitude test in §7.1 exact rather than approximate.

This pipeline is a pure function of its parameters: `fn synthesize(params: ToneSpec) -> Vec<f32>`. It takes no random input and produces identical output every run, so it can be unit-tested exactly.

### 3.2 Tone table

| Tone | Trigger | Base freq | End freq | Sweep | Duration | Harmonic | Harmonic gain |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Wall | `ContactKind::Wall` | 760 Hz | 600 Hz | 15 ms | 45 ms | ×2 | 0.15 |
| Paddle | `ContactKind::Paddle` | 220 Hz | 160 Hz | 40 ms | 90 ms | ×2 | 0.20 |
| Brick (chip, 2 HP left) | `ContactKind::Brick { hp_after: 2, .. }` | 520 Hz | 420 Hz | 20 ms | 55 ms | ×2 | 0.18 |
| Brick (chip, 1 HP left) | `ContactKind::Brick { hp_after: 1, .. }` | 440 Hz | 350 Hz | 20 ms | 55 ms | ×2 | 0.18 |
| Brick (destroyed) | `ContactKind::Brick { hp_after: 0, .. }` | 680 Hz | 480 Hz | 25 ms | 70 ms | ×2 | 0.28 |

These five values are a first tuning pass, not load-bearing constants; they live together as a small table/constant block in `src/audio.rs` so they can be adjusted by ear after the implementation is running, without touching the synthesis pipeline or the event wiring. Since brick HP only ever decreases by exactly one per hit and starts at 1–3 (001 §3, `levels::generate`), `hp_after` after any single hit is always 0, 1 or 2, so these three tones cover every brick contact; no catch-all fourth brick tone is reachable in normal play, but the mapping should still default unmatched values to the "chip, 2 HP left" tone rather than panicking.

All five tones must be distinct from each other in both base frequency and duration, so the acceptance listening pass in §8.2 can judge "different sounds for different bumping sounds" against a concrete table instead of by ear alone.

## 4. Architecture

### 4.1 Dependency

Add to `Cargo.toml`:

```toml
rodio = { version = "0.23", default-features = false, features = ["playback"] }
```

The `playback` feature is rodio's only requirement for opening an output device and mixing sources; it pulls in `cpal` and nothing else. Every other rodio feature (`wav`, `mp3`, `flac`, `vorbis`, `mp4`, `noise`/`dither`, `recording`) is a file decoder or a dependency this project has no use for, since every tone is synthesized, never decoded from a file. `default-features = false` must be kept so none of those are pulled in.

### 4.2 Module (`src/audio.rs`, new)

```rust
pub enum Level { On, Off }   // CLI-selected; mirrors render::fx::Level's shape
pub struct Audio { /* mixer handle, kept-alive device sink, voice counter, precomputed tones */ }
```

- **Tones.** Generated once, at construction, by `synthesize` (§3.1) from the table in §3.2. Each resulting `Vec<f32>` is leaked once (`Box::leak`) into a `&'static [f32]`, so every later trigger plays it via `rodio::static_buffer::StaticSamplesBuffer::new(channels, sample_rate, tone)` with no per-trigger allocation or copy. This is a deliberate, bounded, one-time leak for the process's lifetime (five short buffers, tens of kilobytes total) — the same trade made for the embedded pixel font and sprite constants: generated once, kept for as long as the process runs.
- **Enable gate.** `Audio` stores an `enabled: bool` set from `level` at construction (`On` → `true`, `Off` → `false`). `trigger` (below) checks this first and returns immediately when `false`, before touching the mixer at all. This is what makes `Off` a provable no-op in a test (§6), rather than relying only on nothing being connected to listen.
- **Device.** `Audio::new(level: Level) -> Audio`:
  - When `level == Level::Off`, never touch an audio device at all — construct a standalone `rodio::mixer::mixer(channels, sample_rate)` pair, drop its consumer half immediately (nothing will ever read it, since `enabled` is already `false`), and keep only the producer `Mixer` to satisfy the field's type.
  - When `level == Level::On`, attempt `rodio::DeviceSinkBuilder::from_default_device()` → `.with_error_callback(|_| {})` → `.open_stream()`. A no-op error callback is required: the default callback prints to stderr on every stream error, which can corrupt the raw-mode terminal screen (carried-over constraint, §2).
    - On success, keep the returned `MixerDeviceSink` alive for the audio module's lifetime (dropping it stops playback) and call `.log_on_drop(false)` on it, for the same terminal-corruption reason. Use `sink.mixer().clone()` as the producer.
    - On failure (no device, sandboxed environment, etc.), fall back to the same standalone-mixer construction as the `Off` case, but leave `enabled` at `true`: sound was wanted, there is simply nowhere to send it, and `Mixer::add` on a producer with no consumer is already a silent no-op (the channel's receiver is gone, so sends are silently ignored). **Opening the device must never be allowed to abort startup or return an error from `Audio::new`** — this mirrors the compression-probe fallback in 002 §3.1 ("Compression is an optimisation... its absence must never abort startup"). Sound is the same kind of optional enhancement.
- **Triggering.** `Audio::ingest(&self, events: &[game::Event])`, mirroring `fx::Effects::ingest`'s shape: for every `game::Event::Contact(contact)`, map `contact.kind` to a tone per §3.2 and call an internal `trigger`. All other event variants are ignored (§1, out of scope).
- **Voice pool.** A fixed cap, `MAX_VOICES: usize = 8`, tracked by an `Arc<AtomicUsize>` counter. `trigger` increments the counter; if it was already at the cap, decrement back and drop the request (the new bump is simply not played — inaudible in practice, since contacts are already deduplicated per tick by `physics::step`, 002 §4.1, and a missed one-shot among several simultaneous ones is not noticeable). Otherwise it wraps the `StaticSamplesBuffer` in a thin `Source`-forwarding adapter that decrements the counter on `Drop`, and adds it to the mixer with `Mixer::add`. This differs from the fx module's "retire the oldest" pool policy (002 §4.2) deliberately: a dropped sound is silent and unnoticed, whereas a retired particle or placement would be visible, so the simpler "refuse the newest" policy is sufficient here and avoids needing to reach into an in-flight mixer source to cancel it.
- **No state borrowed from `Game`.** `ingest` only reads the event slice; it never looks at `&Game`.

### 4.3 Headroom

The 0.55 peak-normalisation target (§3.1) leaves enough headroom that `MAX_VOICES` (8) simultaneous full-amplitude tones sum to at most `8 × 0.55 = 4.4`, which will clip if ever reached simultaneously; in practice no more than 2–3 voices are ever concurrently active (a wall/paddle hit plus at most a few simultaneous brick hits from one tick, 002 §4.1). Clipping at the mixer output is the OS/hardware's problem, not something this specification adds limiting for; if the manual acceptance pass (§8.2) ever hears distortion during a multi-brick clear, lowering the peak target is the fix, not adding a compressor.

### 4.4 Wiring into the app loop

- `app::run_game_with_fx` (src/app.rs) gains a fourth parameter, `sound: audio::Level`. It constructs one `Audio::new(sound)` before the main loop, alongside `Renderer::new(fx)`.
- Inside the existing `if now >= render_at { ... }` branch, right after `let events = control.drain_events();`, add `audio.ingest(&events);`. Audio is triggered from the exact same drained event batch the renderer uses, not from each physics tick as it happens. This is deliberate: it keeps sound and the visual hit-flash/shake in sync with each other (both are driven by the same batch, so they land on the same rendered frame), at the cost of up to one render interval of latency from the physical contact (≤ 33 ms at 30 FPS, ≤ 17 ms at 60 FPS) — well under the ~20–30 ms threshold where audio/visual desync becomes perceptible, and simpler than threading a second drain path through `Controller::elapsed`.
- `app::run(fps, fx, sound)` passes `sound` through to `run_game_with_fx`. `app::run_game(fps, game)` (used by `examples/acceptance.rs`, unchanged call sites) keeps its existing two-argument signature and defaults to `FxLevel::Full` and `audio::Level::On`, exactly as it already defaults `fx`.
- `Controller` (src/app.rs) is **not** touched. It stays the pure, audio-free simulation/input driver already used by `tests/simulation.rs` and `tests/evolution.rs`; those tests are unaffected by this specification.

### 4.5 CLI

- Add `--sound on|off` (default `on`) to `app::parse`, following the exact pattern of the existing `--fx` flag (src/app.rs `parse`).
- Add `sound: audio::Level` to the `Cli::Run` variant.
- Update `app::HELP` and the README's flag list and dependency list (new `rodio` entry, same style as 002's `flate2` addition).
- `src/main.rs`'s `Cli::Run { fps, fx, sound } => app::run(fps, fx, sound)...`; `examples/acceptance.rs`'s existing `Cli::Run { fx, .. }` destructure already uses `..` and needs no change.

### 4.6 Module changes

| Module | Change |
| --- | --- |
| `Cargo.toml` | Add `rodio` with `default-features = false, features = ["playback"]` |
| `src/audio.rs` (new) | Synthesis, tone table, `Level`, `Audio`, voice pool |
| `src/lib.rs` | `pub mod audio;` |
| `src/app.rs` | `--sound` flag, `Cli::Run.sound`, `Audio` construction and `ingest` call in `run_game_with_fx` |
| `src/main.rs` | Pass `sound` through to `app::run` |

## 5. Lifecycle interplay

- **Pause.** No special-case needed. `Game::tick` already returns no events at all while not `ticking()` (paused, title, game-over, etc., 001/002), so `ingest` simply receives nothing and plays nothing while paused. Sound freezes "for free," the same way `fx::Effects` freezes but without needing the explicit freeze logic that module has.
- **Resize or font-size change.** No effect. Tones are generated once at startup, independent of `Geometry`/`Viewport`/scale; nothing about audio needs to regenerate on resize.
- **Restart (`R`) and new level.** No effect beyond the ordinary contact events a fresh level or ball produces; there is no audio state to clear (no trail, no active particles — a triggered tone simply finishes playing on its own).
- **Device loss mid-session.** Not handled beyond the no-op error callback (§4.2) already swallowing stream errors; sound may silently stop if the device disappears, but the game must keep running. No reconnection logic is in scope.
- **Cleanup/exit.** The `Audio` (holding the `MixerDeviceSink` when one was opened) is dropped at the end of `run_game_with_fx`, after the render loop exits, exactly like `Renderer` and `Session` already are. `log_on_drop(false)` (§4.2) ensures this drop is silent.

## 6. Testability without an audio device

`rodio::mixer::mixer(channels, sample_rate)` builds a producer/consumer pair that works standalone, with no device, no `cpal` stream, and no host OS audio API involved — it is pure in-process mixing. This is the key fact that makes sound effects testable in CI and sandboxes with no audio hardware, the same way `Session<W: Write>` being generic over any `Write` (not just a real terminal) is what already makes `render` testable (tests/renderer.rs).

Add, behind the existing `dev-harness` feature (no new feature flag — reuse the one `Cargo.toml` already declares for test-support hooks like `app::fixture`):

```rust
#[cfg(feature = "dev-harness")]
pub fn for_test(level: Level) -> (Audio, rodio::mixer::MixerSource)
```

This builds `Audio` around a standalone mixer pair and, unlike `Audio::new`, always keeps and returns the consumer half, for both `Level::On` and `Level::Off`, so a test can pull raw `f32` samples out of it and assert on them directly. For `Level::Off` this is what makes the muted-path test in §7.1 a genuine end-to-end check of the `enabled` gate (§4.2) — the mixer is fully wired up and listened to, and still produces nothing, because `ingest` never calls `trigger` through to it. It also needs a way to observe the voice pool from outside for the cap test below:

```rust
#[cfg(feature = "dev-harness")]
pub fn active_voices(&self) -> usize
```

## 7. Validation and acceptance

### 7.1 Automated tests (no terminal, no audio device; `tests/audio.rs`, new)

Run with `cargo test --features dev-harness` for the tests that need `for_test`/`active_voices`, alongside the project's existing `cargo test` (with and without `--features dev-harness`) requirement (002 §10.1).

- **Synthesis.**
  - Each of the five tones in the table (§3.2) is non-empty, has length matching its `duration_ms` within one sample, and has a peak absolute value equal to the 0.55 normalisation target within floating-point tolerance.
  - Each tone starts near zero (attack) and ends near zero (decay below some small epsilon in its last 5% of samples) — no end-of-buffer click.
  - All five tone buffers are pairwise distinct (not byte-equal), and, as a coarse spectral sanity check, their zero-crossing rates order the same way their base frequencies do.
  - Calling the synthesis pipeline twice with the same `ToneSpec` produces bit-identical output (pure function, no RNG, no shared mutable state).
- **Event mapping.** Using `audio::for_test(Level::On)`:
  - A `Wall` contact event, ingested and then read back from the `MixerSource` (pulling exactly as many samples as the wall tone's length), matches the wall tone buffer exactly.
  - Likewise for a `Paddle` contact and for `Brick` contacts with `hp_after` 2, 1 and 0, each matching its respective tone.
  - Transition events (`Launched`, `LifeLost`, `LevelCleared`, `Won`, `GameOver`, `Restarted`) ingested alone produce only silence (every pulled sample is exactly `0.0`) — confirms the "bump-only, no stingers" scope (§1) is actually enforced, not just documented.
- **Voice pool.** Ingest `MAX_VOICES + 3` simultaneous contact events in one call; `active_voices()` immediately after is exactly `MAX_VOICES`, never more. Pull enough samples from the `MixerSource` to exhaust the longest tone in play; `active_voices()` afterwards returns to `0`.
- **Muted path.** Using `audio::for_test(Level::Off)`: ingesting any mix of events (including contacts) produces only silence from the `MixerSource`, proving `Level::Off` is a true no-op end to end, not merely "no device opened."
- **CLI.** `--sound on` and `--sound off` parse to `Cli::Run { sound: Level::On | Level::Off, .. }`; `--sound` with no value or an unrecognised value is an error; omitting `--sound` defaults to `Level::On`. (Update the existing `Cli::Run { fps: 30, fx: Level::Full }` equality assertion in `tests/cli.rs` to include the new field.)

### 7.2 Manual acceptance (real hardware; the author, not the assistant, must do this listening pass)

Sound quality is not something these automated tests can judge — they check structure (distinctness, decay, no clicks, correct routing), not how a tone actually sounds. Verify in Ghostty with real speakers or headphones:

1. Run with default flags. Confirm wall, paddle and brick-chip bumps are three clearly different, short, non-harsh tones — rounded, not an 8-bit beep.
2. Confirm a brick-destroying hit sounds distinctly different (brighter/fuller) from a chip that leaves the brick standing.
3. Confirm total silence at the title screen, in `Ready`, while `Paused`, and on `GameOver`/`Won` for as long as no new contact occurs — and that nothing ever loops or plays continuously.
4. Run with `--sound off`; confirm total silence throughout a full game.
5. Clear a tightly packed row of bricks in one pass (several near-simultaneous brick contacts); confirm no crackle, distortion or runaway latency, even though several voices overlap.
6. If possible, deny or disconnect the audio output device and confirm the game still starts, plays and exits normally, with no error, no panic and no stray text corrupting the terminal screen.
7. Quit normally, on an induced error, and on a panic (as in 002 §10.2 item 11); confirm terminal restoration is unaffected by audio.
8. If any tone sounds wrong, retune its row in the §3.2 table (frequencies, sweep, duration, harmonic gain) — these are explicitly the knobs to turn, not the pipeline in §3.1.

## 8. Delivery order

1. **Synthesis.** `src/audio.rs`'s pure `synthesize` function and the §3.2 tone table, with the automated synthesis tests (§7.1) passing before any device or mixer code exists.
2. **Mixing and voice pool.** `Audio::ingest`/`trigger`, the `dev-harness` `for_test`/`active_voices` hooks, and the event-mapping and voice-pool tests (§7.1), all exercised through the standalone `rodio::mixer::mixer` pair — no real device touched yet.
3. **Device integration.** `Audio::new`'s real-device path (`DeviceSinkBuilder`, no-op error callback, `log_on_drop(false)`), the `Off`/fallback paths, and the dependency addition to `Cargo.toml`.
4. **Wiring.** `--sound` CLI flag, `Cli::Run` field, `app::run`/`run_game_with_fx` signature change, the `audio.ingest(&events)` call in the render-cadence branch.
5. **Documentation.** README (new dependency, `--sound` flag, a one-line description of the sound design), `app::HELP`.
6. **Manual acceptance.** The listening pass in §7.2, on real hardware.

Each step must leave the game shippable. Document any material departure from this specification — in particular any retuning of the §3.2 table after the listening pass — and the evidence requiring it.

## 9. References

- rodio 0.23 documentation: https://docs.rs/rodio/0.23.0/rodio/
- rodio's standalone mixer (`rodio::mixer::mixer`, used here both for real playback and for device-free testing): https://docs.rs/rodio/0.23.0/rodio/mixer/index.html
- [SPEC-evolve-002.md](SPEC-evolve-002.md) §4.1: the simulation-event plumbing this specification reuses unchanged.
- [SPEC-init-001.md](SPEC-init-001.md): baseline rules, which remain authoritative where not superseded here.

Specification prepared 31 October 2026.
