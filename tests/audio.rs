#![cfg(feature = "dev-harness")]
use ricochet::{
    audio::{Level, for_test},
    game::Event,
    physics::{ContactEvent, ContactKind, Vec2},
};

fn contact(kind: ContactKind) -> Event {
    Event::Contact(ContactEvent {
        kind,
        point: Vec2::new(0.0, 0.0),
        normal: Vec2::new(0.0, 1.0),
    })
}
fn pull(source: &mut impl Iterator<Item = f32>, count: usize) -> Vec<f32> {
    source.by_ref().take(count).collect()
}

#[test]
fn wall_paddle_and_brick_contacts_each_play_their_own_tone() {
    let (sfx, mut out) = for_test(Level::On);
    for (kind, len_ms) in [
        (
            ContactKind::Wall {
                normal: Vec2::new(1.0, 0.0),
            },
            45,
        ),
        (ContactKind::Paddle { offset: 0.0 }, 90),
        (
            ContactKind::Brick {
                index: 0,
                hp_after: 2,
            },
            55,
        ),
        (
            ContactKind::Brick {
                index: 1,
                hp_after: 1,
            },
            55,
        ),
        (
            ContactKind::Brick {
                index: 2,
                hp_after: 0,
            },
            70,
        ),
    ] {
        sfx.ingest(&[contact(kind)]);
        let samples = pull(&mut out, len_ms * 44_100 / 1000 + 50);
        assert!(
            samples.iter().any(|&s| s.abs() > 0.05),
            "{kind:?} was silent"
        );
    }
}

#[test]
fn transition_events_play_nothing() {
    let (sfx, mut out) = for_test(Level::On);
    sfx.ingest(&[
        Event::Launched,
        Event::LifeLost,
        Event::LevelCleared { next: 2 },
        Event::Won,
        Event::GameOver,
        Event::Restarted,
    ]);
    // Nothing was ever added to the mixer, so it yields nothing at all: a
    // mixer with no active or pending source ends its iterator immediately
    // rather than emitting zero-valued samples.
    assert!(pull(&mut out, 4_410).is_empty());
}

#[test]
fn sound_off_is_silent_end_to_end() {
    let (sfx, mut out) = for_test(Level::Off);
    sfx.ingest(&[
        contact(ContactKind::Wall {
            normal: Vec2::new(1.0, 0.0),
        }),
        contact(ContactKind::Paddle { offset: 0.0 }),
        contact(ContactKind::Brick {
            index: 0,
            hp_after: 0,
        }),
    ]);
    assert!(pull(&mut out, 4_410).is_empty());
}

#[test]
fn tones_have_a_soft_attack_and_decay_with_no_end_of_buffer_click() {
    let (sfx, mut out) = for_test(Level::On);
    sfx.ingest(&[contact(ContactKind::Paddle { offset: 0.0 })]);
    // Pulling far more than the 90ms tone can contain: the mixer's own
    // source exhaustion (not this count) ends the collected buffer, so its
    // length is exactly the tone's real sample count.
    let samples = pull(&mut out, 100_000);
    assert!(samples.len() < 10_000, "tone ran far longer than 90ms");
    assert!(samples[0].abs() < 0.05, "no soft attack: {}", samples[0]);
    let peak = samples.iter().fold(0.0_f32, |m, &s| m.max(s.abs()));
    assert!(peak > 0.1, "tone never reached audible level");
    let tail = &samples[samples.len() - 50..];
    assert!(
        tail.iter().all(|&s| s.abs() < 0.01),
        "tone did not decay to silence by the end of its own buffer"
    );
}

#[test]
fn tones_are_pairwise_distinct_and_deterministic() {
    let kinds = [
        ContactKind::Wall {
            normal: Vec2::new(1.0, 0.0),
        },
        ContactKind::Paddle { offset: 0.0 },
        ContactKind::Brick {
            index: 0,
            hp_after: 2,
        },
        ContactKind::Brick {
            index: 0,
            hp_after: 1,
        },
        ContactKind::Brick {
            index: 0,
            hp_after: 0,
        },
    ];
    let capture = |kind: ContactKind| {
        let (sfx, mut out) = for_test(Level::On);
        sfx.ingest(&[contact(kind)]);
        pull(&mut out, 4_410)
    };
    let buffers: Vec<Vec<f32>> = kinds.into_iter().map(capture).collect();
    for (i, a) in buffers.iter().enumerate() {
        // Same kind synthesized again, from a fresh Audio, is bit-identical:
        // synthesis is a pure function of the tone table, with no RNG.
        assert_eq!(*a, capture(kinds[i]), "tone {i} is not deterministic");
        for (j, b) in buffers.iter().enumerate() {
            if i != j {
                assert_ne!(a, b, "tones {i} and {j} are identical");
            }
        }
    }
}

#[test]
fn voice_pool_is_capped_and_recovers() {
    let (sfx, mut out) = for_test(Level::On);
    let flood: Vec<Event> = (0..16)
        .map(|i| {
            contact(ContactKind::Brick {
                index: i,
                hp_after: 2,
            })
        })
        .collect();
    sfx.ingest(&flood);
    assert_eq!(sfx.active_voices(), 8);
    // The longest tone in play (brick hp_after=2, 55ms) finishes well within
    // one second; draining that much silences every voice.
    let _ = pull(&mut out, 44_100);
    assert_eq!(sfx.active_voices(), 0);
}
