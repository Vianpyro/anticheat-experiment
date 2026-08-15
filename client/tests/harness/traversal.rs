//! A synthetic hand that crosses the lobby, shared by the tests that need one.
//!
//! Shared by `#[path]` rather than by a crate, exactly as `harness/mod.rs` and
//! `sim/tests/spec` are: a helper crate for two functions is a crate to
//! maintain, and a copy in each test is a copy to keep in step.
//!
//! # What it is, and the one thing it is careful about
//!
//! It drives [`client::play::Play`] with **device deltas and nothing else** —
//! the same entry point `client::gfx` calls from `winit`'s `device_event`. It
//! never tells the lobby where the cursor is, because it cannot: the cursor is
//! `client::input::Aim`'s and the only way to move it is to emit counts. That is
//! the property `client/tests/lobby.rs` is about, and a harness that could reach
//! around it would be a harness that proved nothing.
//!
//! The hand reads the cursor back between events, which is what a person does
//! with their eyes. It is deliberately *feedback* rather than arithmetic: a hand
//! that computed the exact number of counts from a scale it was told would be a
//! hand that already knew the answer the measurement is looking for.

// Two test binaries include this file and neither uses all of it, which is what
// sharing by `#[path]` costs. The alternative — a copy in each — is the thing
// this file exists to avoid.
#![allow(dead_code)]

use client::input::Control;
use client::lobby::Element;
use client::play::Play;
use sim::FxVec2;

/// How a synthetic hand moves.
#[derive(Clone, Copy, Debug)]
pub struct Hand {
    /// Device counts emitted per motion event: the size of one report.
    pub counts_per_event: f64,
    /// Nanoseconds between two motion events: the device's report period.
    pub gap_ns: u64,
    /// How far past the target the first sweep goes, as a fraction of the
    /// distance. The hand then corrects, exactly as a person does, which is what
    /// puts a fixed cost into every reach for the regression to separate out.
    pub overshoot: f64,
}

impl Hand {
    /// A hand on a 125 Hz mouse, moving fast enough for the report rate to be
    /// readable, and overshooting by a tenth.
    pub const fn quick() -> Self {
        Self {
            counts_per_event: 30.0,
            gap_ns: 8_000_000,
            overshoot: 0.10,
        }
    }

    /// The same hand, creeping. Below `client::lobby::FAST_UNITS_PER_SECOND`.
    pub const fn slow() -> Self {
        Self {
            counts_per_event: 4.0,
            gap_ns: 8_000_000,
            overshoot: 0.02,
        }
    }
}

/// Where the cursor is, in world units.
pub fn cursor(play: &Play) -> (f64, f64) {
    let at: FxVec2 = play.aim();
    (
        f64::from(at.x.to_raw()) / 65536.0,
        f64::from(at.y.to_raw()) / 65536.0,
    )
}

/// A point in world units.
pub fn point(at: FxVec2) -> (f64, f64) {
    (
        f64::from(at.x.to_raw()) / 65536.0,
        f64::from(at.y.to_raw()) / 65536.0,
    )
}

/// Moves the cursor onto `target` and clicks it.
///
/// Answers the clock after the click, so a caller can chain crossings, and the
/// element the click landed on.
pub fn reach(play: &mut Play, hand: Hand, at_ns: &mut u64, target: FxVec2) -> Option<Element> {
    let goal = point(target);
    let start = cursor(play);
    let straight = (goal.0 - start.0, goal.1 - start.1);
    let span = straight.0.hypot(straight.1);

    // The overshoot: the hand aims past the target on the way out. Applied as a
    // point beyond the goal along the same line, so that the correction is a real
    // second movement rather than a number added to a total.
    let beyond = (
        goal.0 + straight.0 * hand.overshoot,
        goal.1 + straight.1 * hand.overshoot,
    );

    // What the hand has learnt about how far the cursor moves per count. It
    // starts as a guess and is corrected by looking, which is the point: a hand
    // handed the sensitivity would be a hand that already knew the answer the
    // measurement is looking for.
    let mut per_count = 0.02f64;

    for (destination, tolerance) in [(beyond, 1.0), (goal, 0.4)] {
        if span < 0.001 {
            break;
        }
        // A bound rather than a `loop`: a hand that cannot converge is a bug in
        // this file, and a test that hangs is worse than one that fails.
        for _ in 0..20_000 {
            let here = cursor(play);
            let delta = (destination.0 - here.0, destination.1 - here.1);
            let remaining = delta.0.hypot(delta.1);
            if remaining <= tolerance {
                break;
            }
            // One report, in the device's own units, along the line. Never more
            // than half of what the hand believes is left, so the approach slows
            // the way a person's does instead of oscillating.
            let step = hand
                .counts_per_event
                .min(remaining / per_count / 2.0)
                .max(1.0);
            let (ux, uy) = (delta.0 / remaining, delta.1 / remaining);
            // The device reports downward-positive; `client::input::Aim` is what
            // negates it, so the harness must not.
            *at_ns = at_ns.saturating_add(hand.gap_ns);
            play.moved(*at_ns, ux * step, -uy * step);

            let moved = {
                let now = cursor(play);
                (now.0 - here.0).hypot(now.1 - here.1)
            };
            if moved > 0.0 {
                per_count = moved / step;
            }
        }
    }

    *at_ns = at_ns.saturating_add(hand.gap_ns);
    play.pressed_in_lobby(*at_ns, Control::Move, true)
}

/// Crosses the whole lobby: the three fixed elements, then `dummies` hits on the
/// training dummy, then ready.
///
/// The traversal a player performs without being asked to, which is the whole
/// design: the interface makes it necessary and nobody is instructed.
pub fn cross(play: &mut Play, hand: Hand, dummies: usize) -> u64 {
    let mut at_ns = 0u64;
    for element in [Element::Name, Element::Consent, Element::Champion] {
        let target = play.lobby().position_of(element);
        let landed = reach(play, hand, &mut at_ns, target);
        assert_eq!(landed, Some(element), "the hand missed {element:?}");
    }
    for hit in 0..dummies {
        let target = play.lobby().dummy_at();
        let landed = reach(play, hand, &mut at_ns, target);
        assert_eq!(landed, Some(Element::Dummy), "the hand missed dummy {hit}");
    }
    let target = play.lobby().position_of(Element::Ready);
    assert_eq!(
        reach(play, hand, &mut at_ns, target),
        Some(Element::Ready),
        "the hand missed ready"
    );
    at_ns
}
