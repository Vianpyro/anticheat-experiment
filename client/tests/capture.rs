//! What the capture path is allowed to depend on, and what it must record.
//!
//! `docs/RISKS.md` R14 said aim was quantised to a character cell and that
//! everything timing-shaped was untouched. The first half was true and the
//! second was not: a terminal reports the pointer only when it crosses into a
//! new cell, so the *rate* of events measured the pointer's speed. Replacing the
//! renderer fixes neither by itself — a windowed client that read the aim off
//! the drawn cursor would have rebuilt R14 in pixels, and one that sampled on a
//! redraw would have rebuilt the speed dependence at the frame rate.
//!
//! So the three properties below are the reason the renderer changed, and the
//! renderer is only the means. Each is exercised by mutation in the pull request
//! that introduced it; the mutation and the message are recorded there rather
//! than here, because a comment claiming a test has teeth is the thing the
//! project does not accept from itself.

#![deny(unsafe_code)]

use client::draw::Viewport;
use client::input::{Aim, Control, InputTrace, Motion, WORLD_UNITS_PER_COUNT};
use client::play::{Aiming, Play};
use sim::view::{OwnView, PlayerView};
use sim::{Cooldowns, Fx, Liveness, Outcome, RULES, Seat, Tick, base_position, champion_entity_id};

fn a_view(seat: Seat) -> PlayerView {
    PlayerView {
        tick: Tick(1),
        outcome: Outcome::InProgress,
        own: OwnView {
            id: champion_entity_id(seat),
            position: base_position(seat.team(), &RULES),
            liveness: Liveness::Alive {
                hp: RULES.champion_max_hp,
            },
            cooldowns: Cooldowns::default(),
        },
        visible: Vec::new(),
        events: Vec::new(),
    }
}

/// A device event stream with a slow stretch and a fast one, at a constant
/// device report rate.
///
/// This is the shape the whole change is about: a hand that moves slowly and
/// then quickly produces the *same number of events per second* from a mouse and
/// wildly different numbers from a terminal, because a terminal reports cell
/// crossings and a mouse reports its own clock.
fn slow_then_fast() -> Vec<(u64, f64, f64)> {
    let period_ns = 8_000_000u64; // 125 Hz, an ordinary USB mouse
    let mut events = Vec::new();
    for step in 0..250u64 {
        // 0.03 device counts a step: far below one world unit, and far below
        // anything a character cell would have noticed.
        events.push((step * period_ns, 0.03, 0.0));
    }
    for step in 250..500u64 {
        // Forty counts a step: two world units, which would have crossed one or
        // two terminal cells every single time.
        events.push((step * period_ns, 40.0, 0.0));
    }
    events
}

/// **The property this whole change exists for.** The capture path is a
/// function of the device event stream and of nothing the renderer knows.
///
/// Two clients differing only in window size — a 640×480 and a 3840×2160, which
/// is a factor of six in pixels per world unit — are given byte-identical device
/// events. If any part of capture consulted a viewport, a scale factor or a
/// drawn position, the two would diverge, and the aim would once again have the
/// resolution of somebody's monitor rather than of their hand. That is R14
/// rebuilt in pixels.
///
/// The digest is over the raw `f64` bits of every delta, so this is equality of
/// what the device said and not of some rounded summary of it.
#[test]
fn capture_is_a_function_of_the_device_and_not_of_the_window() {
    let mut small = Play::new();
    small.resized(640, 480);
    let mut large = Play::new();
    large.resized(3840, 2160);
    assert_ne!(
        small.viewport(),
        large.viewport(),
        "the two clients have to differ in the thing that must not matter"
    );
    assert_ne!(
        Viewport::new(640, 480).scale(),
        Viewport::new(3840, 2160).scale(),
        "…and differ in pixels per world unit, which is the quantity a naive \
         capture would reach for"
    );

    for (at_ns, dx, dy) in slow_then_fast() {
        small.moved(at_ns, dx, dy);
        large.moved(at_ns, dx, dy);
    }

    assert_eq!(
        small.trace().digest(),
        large.trace().digest(),
        "the recorded telemetry depends on the size of the window"
    );
    assert_eq!(
        small.aim(),
        large.aim(),
        "the aim the simulation is given depends on the size of the window"
    );
    assert!(
        small.trace().len() == 500,
        "the fixture recorded {} events instead of 500",
        small.trace().len()
    );
}

/// The same, for control presses: a click means the aim, and the aim is not a
/// pixel.
#[test]
fn a_press_means_the_same_order_under_any_window() {
    let seat = Seat::Blue0;
    let view = a_view(seat);
    let aiming = Aiming {
        view: &view,
        seat,
        own: view.own.position,
    };

    let mut small = Play::new();
    small.resized(320, 240);
    let mut large = Play::new();
    large.resized(2560, 1440);
    for client in [&mut small, &mut large] {
        for (at_ns, dx, dy) in slow_then_fast() {
            client.moved(at_ns, dx, dy);
        }
        client.pressed(9_000_000_000, Control::Move, true, &aiming);
        client.pressed(9_010_000_000, Control::Move, false, &aiming);
    }

    assert_eq!(
        small.trace().digest(),
        large.trace().digest(),
        "a recorded press depends on the size of the window"
    );
    assert_eq!(
        small.intention(),
        large.intention(),
        "the order a click produces depends on the size of the window"
    );
}

/// One sample per device event, and never one sample per change.
///
/// The counting half is trivial and the second assertion is the one with teeth:
/// motion so small that the aim's fixed-point value does not move at all is
/// still recorded. Any sampler conditioned on "the position changed" — which is
/// exactly what a terminal is — records nothing here and fails.
#[test]
fn every_device_event_is_recorded_even_when_nothing_visibly_moves() {
    let mut play = Play::new();
    let before = play.aim();

    // A hundredth of the fixed-point resolution, sixty times over: six tenths
    // of one representable unit, so the aim does not move at all and the record
    // grows by sixty.
    let step = (1.0 / 65536.0 / 100.0) / WORLD_UNITS_PER_COUNT;
    for index in 0..60u64 {
        play.moved(index * 1_000_000, step, 0.0);
    }

    assert_eq!(
        play.aim(),
        before,
        "the fixture is wrong: this motion was supposed to be below the \
         fixed-point resolution"
    );
    assert_eq!(
        play.trace().len(),
        60,
        "motion that did not move the aim was not recorded, which is the \
         terminal's speed-dependent sampling with a window in front of it"
    );
    assert!(
        play.trace()
            .samples()
            .iter()
            .all(|sample| matches!(sample.motion, Motion::Moved { .. })),
        "something other than motion reached the trace"
    );
}

/// The same property at the other end of the range: motion the clamp absorbs is
/// still a thing the hand did.
///
/// A player who pushes the aim into the corner of the map and keeps pushing
/// produces device events whose effect on the aim is nil. A record that dropped
/// them would under-report exactly the moments a player is doing the most.
#[test]
fn motion_the_clamp_absorbs_is_still_recorded() {
    let mut play = Play::new();
    // Far past the map's own extent, twice.
    for index in 0..2000u64 {
        play.moved(index * 1_000_000, 100.0, 0.0);
    }
    let cornered = play.aim();
    let recorded = play.trace().len();
    for index in 2000..2100u64 {
        play.moved(index * 1_000_000, 100.0, 0.0);
    }

    assert_eq!(play.aim(), cornered, "the fixture did not reach the clamp");
    assert_eq!(
        play.aim().x,
        RULES.map_half_extent,
        "the aim is clamped to something other than the map"
    );
    assert_eq!(
        play.trace().len(),
        recorded + 100,
        "motion absorbed by the clamp was not recorded"
    );
}

/// The clamp is a rule constant, not a window.
///
/// Stated separately from the byte-equality property because it is the specific
/// mistake that is easiest to make: confining the aim to the visible window is
/// the natural thing to write, and it would make the recorded aim a function of
/// a monitor's aspect ratio.
#[test]
fn the_aim_is_confined_by_the_map_and_not_by_the_window() {
    let mut aim = Aim::centred();
    for _ in 0..10_000 {
        aim.apply(100.0, -100.0);
    }
    assert_eq!(aim.world().x, RULES.map_half_extent);
    assert_eq!(aim.world().y, RULES.map_half_extent);

    let mut back = Aim::centred();
    for _ in 0..10_000 {
        back.apply(-100.0, 100.0);
    }
    assert_eq!(back.world().x, RULES.map_half_extent.neg());
    assert_eq!(back.world().y, RULES.map_half_extent.neg());
}

/// The trace is a function of the device events alone, and not of how many
/// frames happened between them.
///
/// The frame-rate version of the speed dependence: a client that recorded on
/// redraw, or that folded several device events into one sample per frame, would
/// have a trace that changed when the machine got busier. The two runs here
/// interleave the same events with no frames and with a frame between every
/// event.
#[test]
fn the_trace_does_not_depend_on_how_often_the_client_drew() {
    let seat = Seat::Blue0;
    let view = a_view(seat);

    let mut never_drew = Play::new();
    for (at_ns, dx, dy) in slow_then_fast() {
        never_drew.moved(at_ns, dx, dy);
    }

    let mut drew_constantly = Play::new();
    for (at_ns, dx, dy) in slow_then_fast() {
        drew_constantly.moved(at_ns, dx, dy);
        // Everything a frame does: ask for the intention, read the aim, and
        // compose a screen.
        let _ = drew_constantly.intention();
        let scene = client::draw::Scene {
            view: &view,
            seat,
            own: view.own.position,
            aim: drew_constantly.aim(),
        };
        let _ = client::draw::compose(&scene);
    }

    assert_eq!(
        never_drew.trace().digest(),
        drew_constantly.trace().digest(),
        "the recorded telemetry depends on how often the client drew a frame"
    );
}

/// Changing the sensitivity changes what the simulation is told and not one byte
/// of what is recorded.
///
/// The two paths, stated as a difference rather than as an equality: the trace
/// holds the device's own units, so a player who turns the sensitivity up
/// contributes telemetry that is comparable with everyone else's. If the record
/// held world units, every participant's corpus entry would be scaled by a
/// setting, and the M8 null model would be a distribution over configuration
/// files.
#[test]
fn the_record_is_in_the_devices_units_and_the_aim_is_not() {
    let mut play = Play::new();
    for (at_ns, dx, dy) in slow_then_fast() {
        play.moved(at_ns, dx, dy);
    }

    let counts: f64 = play
        .trace()
        .samples()
        .iter()
        .filter_map(|sample| match sample.motion {
            Motion::Moved { dx, .. } => Some(dx),
            Motion::Pressed { .. } => None,
        })
        .sum();
    // The fixture walks 0.03 × 250 + 40 × 250 counts to the right.
    assert!(
        (counts - (0.03 * 250.0 + 40.0 * 250.0)).abs() < 1e-9,
        "the trace does not hold the device's own units: {counts}"
    );

    // …and the aim, which is that walk in world units, is clamped by the map
    // rather than being the same number.
    assert_eq!(play.aim().x, RULES.map_half_extent);
}

/// The measurement the probe reports, checked on a stream whose answer is known.
///
/// Not a property of the client so much as of the instrument: a statistic that
/// is wrong makes the closing argument for R14 wrong too, and this is a stream
/// whose inter-arrival distribution is 8 ms by construction.
#[test]
fn the_reported_distribution_is_the_one_the_stream_had() {
    let mut trace = InputTrace::new();
    for (at_ns, dx, dy) in slow_then_fast() {
        trace.moved(at_ns, dx, dy);
    }
    let stats = trace.stats();

    assert_eq!(stats.samples, 500);
    assert_eq!(stats.moves, 500);
    assert_eq!(stats.gaps_ns.min, 8_000_000);
    assert_eq!(stats.gaps_ns.p50, 8_000_000);
    assert_eq!(stats.gaps_ns.max, 8_000_000);
    assert_eq!(stats.span_ns, 499 * 8_000_000);

    // The finest motion in the stream is the slow stretch's 0.03 counts, which
    // is 0.0015 world units — against a terminal cell of 1.16 world units
    // across and 4.11 down. That ratio is what R14 becomes.
    let finest = stats.finest_count.expect("the stream has motion in it");
    assert!((finest - 0.03).abs() < 1e-12, "finest motion was {finest}");
    let world = stats
        .finest_world_units
        .expect("the stream has motion in it");
    assert!(world < 0.002, "finest world-unit motion was {world}");
}

/// A press that produces no order is still a thing the player did.
///
/// `Targeted` with nobody in range changes nothing about the game, and a record
/// that kept only the presses that worked would be a record of the rules rather
/// than of the hand — and would delete exactly the mistimed inputs a reaction
/// latency detector at M8 is looking for.
#[test]
fn a_press_that_did_nothing_is_recorded_anyway() {
    let seat = Seat::Blue0;
    let view = a_view(seat);
    let aiming = Aiming {
        view: &view,
        seat,
        own: view.own.position,
    };

    let mut play = Play::new();
    play.pressed(1_000_000, Control::Targeted, true, &aiming);
    play.pressed(1_500_000, Control::Targeted, false, &aiming);

    assert_eq!(
        play.trace().len(),
        2,
        "a press with nobody in range was not recorded"
    );
    assert_eq!(
        play.intention(),
        sim::Action::Idle,
        "a targeted spell with nobody in range produced an order anyway"
    );
}

/// Releases are recorded as well as presses, so that a hold has a duration.
#[test]
fn a_release_is_a_device_event_too() {
    let seat = Seat::Blue0;
    let view = a_view(seat);
    let aiming = Aiming {
        view: &view,
        seat,
        own: view.own.position,
    };
    let mut play = Play::new();
    play.pressed(1_000_000, Control::Move, true, &aiming);
    play.pressed(41_000_000, Control::Move, false, &aiming);

    let samples = play.trace().samples();
    assert_eq!(samples.len(), 2);
    assert_eq!(
        samples[0].motion,
        Motion::Pressed {
            control: Control::Move,
            down: true
        }
    );
    assert_eq!(
        samples[1].motion,
        Motion::Pressed {
            control: Control::Move,
            down: false
        }
    );
    assert_eq!(samples[1].at_ns - samples[0].at_ns, 40_000_000);
}

/// The aim rounds once, at the boundary where the simulation's integers begin,
/// and the sub-resolution remainder is not lost on the way.
///
/// A hundred deltas each a hundredth of the fixed-point resolution add up to one
/// unit of it. An accumulator that rounded every event would have lost all
/// hundred, which is a bias toward zero on every slow movement a player makes.
#[test]
fn motion_finer_than_the_fixed_point_resolution_accumulates() {
    let mut aim = Aim::centred();
    let hundredth = (1.0 / 65536.0 / 100.0) / WORLD_UNITS_PER_COUNT;
    for _ in 0..100 {
        aim.apply(hundredth, 0.0);
    }
    assert_eq!(
        aim.world().x,
        Fx::from_raw(1),
        "a hundred hundredths of a fixed-point unit did not add up to one"
    );
}
