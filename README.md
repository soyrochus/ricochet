# Ricochet

[![Rust](https://img.shields.io/badge/rust-1.94%2B-000000?logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-000000?logo=apple&logoColor=white)](https://ghostty.org/)
[![Kitty Graphics](https://img.shields.io/badge/graphics-Kitty%20protocol-00AEEF?logo=terminal)](https://sw.kovidgoyal.net/kitty/graphics-protocol/)
[![Terminal](https://img.shields.io/badge/terminal-Ghostty%201.4%2B-8B5CF6?logo=ghostty)](https://ghostty.org/docs/install/release-notes/1-4-0)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

<p align="center">
  <img src="images/ricochet.png" alt="Ricochet gameplay preview" width="900" />
</p>

Breakout, reimagined in the terminal with Kitty graphics.

Ricochet is a terminal-first Breakout game that showcases what is possible when a Rust app uses the Kitty graphics protocol instead of a traditional GUI. The playfield is rendered as a compact, pixel-accurate scene with sub-cell motion, layered sprite placement, and a HUD that stays crisp in ordinary terminal text. The result is a game that feels like a real graphical application while staying fast, portable, and easy to run directly from the shell.

This project demonstrates the practical side of Kitty graphics: dynamic sprite generation, frame batching, geometry-aware scaling, letterboxed presentation, and responsive input handling in a real-time game loop. The gameplay remains intentionally compact and deterministic, with three handcrafted levels, a fixed physics timestep, and a clean terminal lifecycle that restores the screen safely when the app exits or pauses.

## Run

Requires **Ghostty 1.4+ on macOS or Linux**, interactive stdin/stdout, and Rust
**1.94 or newer** (edition 2024). Implementation toolchain: Rust 1.99.0; the
declared 1.94 minimum was also compiled and tested. No external assets or runtime
services are needed.

```sh
cargo run --release
cargo run --release -- --fps 30
cargo run -- --help
cargo run -- --version
```

Use at least **80 columns × 30 rows**, with enough physical pixels for a
**400 × 300** playfield after reserving four text rows. A larger window improves
detail. Resize suspension preserves progress; press **P** or **Esc** after
restoring a playable size. A focus change pauses an active run and never resumes
it automatically.

The startup probe checks actual Kitty graphics support and enhanced keyboard
flags **1 + 2 + 8**, including ordinary-letter release events. Unsupported
capabilities cause a clean exit with an explanation. `TERM`/`TERM_PROGRAM` are
not used as capability evidence.

## Controls

| Key | Action |
| --- | --- |
| Left / A | Move left while held |
| Right / D | Move right while held |
| Space | Start from Title; launch from Ready |
| P / Esc | Pause or resume |
| R | New run from Paused, Won or GameOver |
| Q / Ctrl-C | Quit from any state |

Both directions cancel. A and Left (and D and Right) are tracked independently.
Repeated one-shot keys are ignored. A lost life keeps score, brick damage and
speed; a new level keeps score/lives and resets speed. Launches alternate right
and left. Clear all three levels to win.

## Implementation

- `game.rs`, `levels.rs`, `constants.rs`: explicit states and deterministic rules
  in an 800 × 600 logical world. All gameplay constants match [SPEC.md](SPEC.md).
- `physics.rs`: continuous circle/rectangle face and rounded-corner collision
  detection. The moving paddle is swept up to its wall clamp, then treated as
  stationary. Top contacts use the specified ±65° angle and horizontal minimum.
  Simultaneous collinear contacts share one normal; independent simultaneous
  normals reverse each axis once. Distinct bricks take damage once per event.
  Separation is 0.0001 logical units; collision iterations are capped at 16.
- `timing.rs`, `input.rs`, `app.rs`: 120 Hz fixed simulation, independent 30/60 Hz
  rendering, 100 ms stall clamp and 12 ticks per iteration. Input waits use
  Crossterm polling. Discontinuities reset elapsed time and held input.
- `terminal.rs`: one bounded raw startup reader extracts protocol replies and
  preserves intervening game inputs. Crossterm 0.29 does not expose Kitty APC
  replies publicly. After negotiation only Crossterm reads input; pixel geometry
  is read through its OS `TIOCGWINSZ` wrapper every 250 ms and on resize events.
- `render/`: nine procedural RGBA images and at most 53 placements, with
  terminal-allocated image IDs addressed through owned image-number aliases.
  Pixel positions map to cursor cells plus within-cell offsets. Moving
  placements retain their IDs; static bricks and HUD lines update only when
  changed. A frame is batched and flushed once. Synchronized updates are queried
  and used when supported; guards pair begin/end commands on errors.

Scale is uniform, capped at 2 physical pixels per logical unit to bound sprite
memory, and centred with letterboxing. Only rendering rounds to pixels. Images
are regenerated on geometry changes. The open-bottom ball is cropped before the
footer. Cleanup cancels partial protocol strings, removes owned images, restores
keyboard/focus/cursor/screen/colours and raw mode, then reports errors. Panics
unwind through the same guard. SIGINT, SIGTERM, SIGHUP, SIGQUIT and SIGTSTP notify
the loop through atomic flags, without terminal I/O inside signal handlers.

## Validation and diagnostics

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

Tests run without a terminal. They cover collisions (including true corner
arcs, simultaneous contacts and maximum speed), paddle motion, scoring, state
transitions, focus/resize suspension, equivalent 30/60 FPS histories, startup
input replay, pixel mapping, chunk boundaries, bounded ownership and cleanup.
A repeated 72,000-tick deterministic simulation checks finite, moving states.

The optional development harness is absent from the normal game CLI:

```sh
# Move the diagnostic ball with A/D or arrows; watch the release counter.
# Resize quits this diagnostic; rerun it at the new size.
cargo run --release --features dev-harness --example terminal_spike

# End-state, damaged-brick and controlled error/panic acceptance screens.
cargo run --release --features dev-harness --example acceptance -- won
cargo run --release --features dev-harness --example acceptance -- game-over
cargo run --release --features dev-harness --example acceptance -- damaged
cargo run --release --features dev-harness --example acceptance -- error
cargo run --release --features dev-harness --example acceptance -- panic

# Synthetic PTY protocol/lifecycle checks (Python 3, standard library only).
cargo build --release --examples --features dev-harness
cargo build --release
python3 tools/verify_pty.py --output verification/pty-results.json
cargo test --all-features
```

The PTY harness supplies synthetic capability/key/focus/resize responses and
models resource ownership. **It does not establish Ghostty compatibility.**
See [verification/REPORT.md](verification/REPORT.md) for observed results and the
remaining real-terminal acceptance steps.

## Limits

Actual Ghostty gameplay and visual/input acceptance remain **unverified** in the
implementation environment. Linux runtime acceptance is also unverified.

Nonzero OS pixel geometry is required; terminals that do not provide it are
rejected. There is no terminal-query fallback after input ownership passes to
Crossterm, and no font aspect ratio is assumed. Suspend/resume is unsupported:
SIGTSTP exits cleanly and Ctrl-Z is ignored in raw mode. SIGKILL and process abort
cannot restore the terminal. Windows, tmux/screen/SSH guarantees, sound, mouse,
saved scores and configuration files are outside this release's scope.

## Protocol references

- [Kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/)
- [Kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/)
- [Crossterm 0.29 events](https://docs.rs/crossterm/0.29.0/crossterm/event/index.html)
- [Ghostty 1.4 release notes](https://ghostty.org/docs/install/release-notes/1-4-0)

## Principles of Participation

Everyone is invited and welcome to contribute: open issues, propose pull requests, share ideas, or help improve documentation. Participation is open to all, regardless of background or viewpoint. 

This project follows the [FOSS Pluralism Manifesto](./FOSS_PLURALISM_MANIFESTO.md), which affirms respect for people, freedom to critique ideas, and space for diverse perspectives. 

## License and Copyright

Copyright (c) 2026, Iwan van der Kleijn

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.
