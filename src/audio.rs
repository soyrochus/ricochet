//! Synthesized bump sound effects. There is no audio asset file: every tone
//! is generated once at startup, the same way sprites and the pixel font are
//! generated. No music, no continuous sound; only short one-shot tones for
//! ball contacts (wall, paddle, brick).
use crate::{game::Event, physics::ContactKind};
use rodio::{
    ChannelCount, Sample, SampleRate, Source,
    mixer::{self, Mixer},
    source::SeekError,
    static_buffer::StaticSamplesBuffer,
};
use std::{
    f64::consts::TAU,
    num::NonZero,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

const SAMPLE_RATE_HZ: u32 = 44_100;
const MAX_VOICES: usize = 8;
const PEAK: f32 = 0.55;
const ATTACK_MS: f64 = 2.0;

fn channels() -> ChannelCount {
    NonZero::new(1).unwrap()
}
fn sample_rate() -> SampleRate {
    NonZero::new(SAMPLE_RATE_HZ).unwrap()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Level {
    #[default]
    On,
    Off,
}

struct ToneSpec {
    base_freq: f64,
    end_freq: f64,
    sweep_ms: f64,
    duration_ms: f64,
    harmonic_ratio: f64,
    harmonic_gain: f64,
}

const WALL: ToneSpec = ToneSpec {
    base_freq: 760.0,
    end_freq: 600.0,
    sweep_ms: 15.0,
    duration_ms: 45.0,
    harmonic_ratio: 2.0,
    harmonic_gain: 0.15,
};
const PADDLE: ToneSpec = ToneSpec {
    base_freq: 220.0,
    end_freq: 160.0,
    sweep_ms: 40.0,
    duration_ms: 90.0,
    harmonic_ratio: 2.0,
    harmonic_gain: 0.20,
};
const BRICK_HIGH: ToneSpec = ToneSpec {
    base_freq: 520.0,
    end_freq: 420.0,
    sweep_ms: 20.0,
    duration_ms: 55.0,
    harmonic_ratio: 2.0,
    harmonic_gain: 0.18,
};
const BRICK_LOW: ToneSpec = ToneSpec {
    base_freq: 440.0,
    end_freq: 350.0,
    sweep_ms: 20.0,
    duration_ms: 55.0,
    harmonic_ratio: 2.0,
    harmonic_gain: 0.18,
};
const BRICK_DESTROY: ToneSpec = ToneSpec {
    base_freq: 680.0,
    end_freq: 480.0,
    sweep_ms: 25.0,
    duration_ms: 70.0,
    harmonic_ratio: 2.0,
    harmonic_gain: 0.28,
};

/// Pure, deterministic: a fast downward pitch sweep plus a quiet upper
/// harmonic, attack/decay envelope, a one-pole low-pass for roundness, then
/// peak normalisation. No noise component anywhere.
fn synthesize(spec: &ToneSpec) -> Vec<f32> {
    let count = (spec.duration_ms / 1000.0 * f64::from(SAMPLE_RATE_HZ)).round() as usize;
    let mut buffer = Vec::with_capacity(count);
    let mut phase = 0.0_f64;
    let mut harmonic_phase = 0.0_f64;
    // Envelope reaches ~1% of its peak by duration_ms.
    let decay_span = ((spec.duration_ms - ATTACK_MS) / 1000.0).max(1.0 / f64::from(SAMPLE_RATE_HZ));
    let tau = decay_span / 100.0_f64.ln();
    for i in 0..count {
        let t_ms = i as f64 / f64::from(SAMPLE_RATE_HZ) * 1000.0;
        let progress = (t_ms / spec.sweep_ms).min(1.0);
        let freq = spec.base_freq + (spec.end_freq - spec.base_freq) * progress;
        phase += TAU * freq / f64::from(SAMPLE_RATE_HZ);
        harmonic_phase += TAU * freq * spec.harmonic_ratio / f64::from(SAMPLE_RATE_HZ);
        let oscillator = phase.sin() + spec.harmonic_gain * harmonic_phase.sin();
        let envelope = if t_ms < ATTACK_MS {
            t_ms / ATTACK_MS
        } else {
            (-(t_ms - ATTACK_MS) / 1000.0 / tau).exp()
        };
        buffer.push((oscillator * envelope) as f32);
    }
    lowpass(&mut buffer);
    normalize(&mut buffer);
    buffer
}

fn lowpass(buffer: &mut [f32]) {
    // alpha chosen for a cutoff around 4-5 kHz at 44.1 kHz.
    const ALPHA: f32 = 0.5;
    let mut previous = 0.0_f32;
    for sample in buffer.iter_mut() {
        previous += ALPHA * (*sample - previous);
        *sample = previous;
    }
}

fn normalize(buffer: &mut [f32]) {
    let peak = buffer.iter().fold(0.0_f32, |m, &s| m.max(s.abs()));
    if peak > 0.0 {
        let scale = PEAK / peak;
        for sample in buffer.iter_mut() {
            *sample *= scale;
        }
    }
}

fn leak(buffer: Vec<f32>) -> &'static [f32] {
    Box::leak(buffer.into_boxed_slice())
}

struct Tones {
    wall: &'static [f32],
    paddle: &'static [f32],
    brick_high: &'static [f32],
    brick_low: &'static [f32],
    brick_destroy: &'static [f32],
}
impl Tones {
    fn generate() -> Self {
        Self {
            wall: leak(synthesize(&WALL)),
            paddle: leak(synthesize(&PADDLE)),
            brick_high: leak(synthesize(&BRICK_HIGH)),
            brick_low: leak(synthesize(&BRICK_LOW)),
            brick_destroy: leak(synthesize(&BRICK_DESTROY)),
        }
    }
}

/// Decrements the shared voice counter when the mixer finishes or drops this
/// one-shot, so the pool bound in `Audio::trigger` reflects reality.
struct Voice {
    inner: StaticSamplesBuffer,
    voices: Arc<AtomicUsize>,
}
impl Drop for Voice {
    fn drop(&mut self) {
        self.voices.fetch_sub(1, Ordering::Relaxed);
    }
}
impl Iterator for Voice {
    type Item = Sample;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}
impl Source for Voice {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }
    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }
    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.inner.try_seek(pos)
    }
}

/// Bump sound effects. Presentation only, like `render::fx::Effects`: it
/// reads events, never `Game`, and never changes simulation behaviour.
pub struct Audio {
    mixer: Mixer,
    // Never read; held only so the cpal stream stays open until Audio drops.
    #[allow(dead_code)]
    sink: Option<rodio::MixerDeviceSink>,
    voices: Arc<AtomicUsize>,
    tones: Tones,
    enabled: bool,
}
impl Audio {
    pub fn new(level: Level) -> Self {
        let tones = Tones::generate();
        let voices = Arc::new(AtomicUsize::new(0));
        if level == Level::Off {
            let (mixer, _consumer) = mixer::mixer(channels(), sample_rate());
            return Self {
                mixer,
                sink: None,
                voices,
                tones,
                enabled: false,
            };
        }
        match rodio::DeviceSinkBuilder::from_default_device()
            .and_then(|builder| builder.with_error_callback(|_| {}).open_stream())
        {
            Ok(mut sink) => {
                sink.log_on_drop(false);
                let mixer = sink.mixer().clone();
                Self {
                    mixer,
                    sink: Some(sink),
                    voices,
                    tones,
                    enabled: true,
                }
            }
            Err(_) => {
                let (mixer, _consumer) = mixer::mixer(channels(), sample_rate());
                Self {
                    mixer,
                    sink: None,
                    voices,
                    tones,
                    enabled: true,
                }
            }
        }
    }
    /// Reads `Event::Contact` only; all transition events are ignored by
    /// design (no stingers, no music, no ball-motion sound).
    pub fn ingest(&self, events: &[Event]) {
        if !self.enabled {
            return;
        }
        for event in events {
            if let Event::Contact(contact) = event {
                let tone = match contact.kind {
                    ContactKind::Wall { .. } => self.tones.wall,
                    ContactKind::Paddle { .. } => self.tones.paddle,
                    ContactKind::Brick { hp_after: 0, .. } => self.tones.brick_destroy,
                    ContactKind::Brick { hp_after: 1, .. } => self.tones.brick_low,
                    ContactKind::Brick { .. } => self.tones.brick_high,
                };
                self.trigger(tone);
            }
        }
    }
    fn trigger(&self, tone: &'static [f32]) {
        if tone.is_empty() {
            return;
        }
        if self.voices.fetch_add(1, Ordering::Relaxed) >= MAX_VOICES {
            self.voices.fetch_sub(1, Ordering::Relaxed);
            return;
        }
        let voice = Voice {
            inner: StaticSamplesBuffer::new(channels(), sample_rate(), tone),
            voices: Arc::clone(&self.voices),
        };
        self.mixer.add(voice);
    }
}

/// Test-only construction around a standalone, device-free mixer pair
/// (`rodio::mixer::mixer`), usable with no audio hardware. The consumer half
/// is always returned, for both levels, so the `Off` muted path can be
/// proven silent end to end rather than merely unconnected.
#[cfg(feature = "dev-harness")]
pub fn for_test(level: Level) -> (Audio, mixer::MixerSource) {
    let (mixer, consumer) = mixer::mixer(channels(), sample_rate());
    let audio = Audio {
        mixer,
        sink: None,
        voices: Arc::new(AtomicUsize::new(0)),
        tones: Tones::generate(),
        enabled: level == Level::On,
    };
    (audio, consumer)
}
#[cfg(feature = "dev-harness")]
impl Audio {
    pub fn active_voices(&self) -> usize {
        self.voices.load(Ordering::Relaxed)
    }
}
