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
use client::input::{Aim, Control, Event, InputTrace, WORLD_UNITS_PER_COUNT};
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
    for step in 0..SLOW {
        // 0.03 device counts a step, which is 0.0015 world units: a fortieth of
        // the narrow side of a terminal character cell, so a terminal would
        // have reported roughly one of these 770 events.
        events.push((step * period_ns, SLOW_COUNTS, 0.0));
    }
    for step in SLOW..SLOW + FAST {
        // Twelve counts a step, which is 0.6 world units: half a cell, so a
        // terminal would have reported every second one.
        events.push((step * period_ns, FAST_COUNTS, 0.0));
    }
    events
}

/// The fixture's two stretches, and the sensitivity arithmetic that keeps the
/// aim **inside** the map.
///
/// That last part is load bearing and it was not, once: an earlier version of
/// this fixture walked far enough to hit the clamp, so every client it drove
/// ended at exactly `map_half_extent` and "two windows produce the same aim" was
/// true of any two windows whatever. A mutation that scaled the aim by the
/// window's aspect ratio passed. The travel is now well inside the map and
/// [`capture_is_a_function_of_the_device_and_not_of_the_window`] asserts that it
/// is, so the hole cannot be reopened by a later edit to these numbers.
const SLOW: u64 = 250;
const FAST: u64 = 100;
const SLOW_COUNTS: f64 = 0.03;
const FAST_COUNTS: f64 = 12.0;
/// Where the fixture leaves the aim, in world units: comfortably inside the
/// map's `map_half_extent`.
const TRAVEL_COUNTS: f64 = SLOW as f64 * SLOW_COUNTS + FAST as f64 * FAST_COUNTS;

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
    assert_eq!(
        small.trace().len(),
        usize::try_from(SLOW + FAST).unwrap(),
        "the fixture did not record every event it fed in"
    );

    // …and the aim comparison above must be a comparison of two live values
    // rather than of two saturated ones. Without this, a mutation that scales
    // the aim by anything at all passes, because both clients pile up against
    // the same clamp. That happened.
    assert_ne!(
        small.aim().x,
        RULES.map_half_extent,
        "the fixture saturated the clamp, so the aim assertion proves nothing"
    );
    assert_eq!(
        small.aim().x,
        Fx::from_raw(((TRAVEL_COUNTS * WORLD_UNITS_PER_COUNT) * 65536.0) as i32),
        "the aim is not the device's travel in world units"
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
            .all(|sample| matches!(sample.event, Event::Moved { .. })),
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
        .filter_map(|sample| match sample.event {
            Event::Moved { dx, .. } => Some(dx),
            Event::Pressed { .. } | Event::Viewed { .. } => None,
        })
        .sum();
    assert!(
        (counts - TRAVEL_COUNTS).abs() < 1e-9,
        "the trace does not hold the device's own units: {counts} against \
         {TRAVEL_COUNTS}"
    );

    // …and the aim is that same walk multiplied by a sensitivity nobody else
    // has to know about, which is the whole of the difference between the two
    // paths.
    let aimed = f64::from(play.aim().x.to_raw()) / 65536.0;
    assert!(
        (aimed - counts * WORLD_UNITS_PER_COUNT).abs() < 1e-4,
        "the aim is {aimed} world units for {counts} counts"
    );
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

    let events = usize::try_from(SLOW + FAST).unwrap();
    assert_eq!(stats.samples, events);
    assert_eq!(stats.moves, events);
    assert_eq!(stats.gaps_ns.min, 8_000_000);
    assert_eq!(stats.gaps_ns.p50, 8_000_000);
    assert_eq!(stats.gaps_ns.p95, 8_000_000);
    assert_eq!(stats.gaps_ns.p99, 8_000_000);
    assert_eq!(stats.gaps_ns.max, 8_000_000);
    assert_eq!(stats.span_ns, (SLOW + FAST - 1) * 8_000_000);
    // The pair R14's timestamp half is closed against, on a stream whose answer
    // is exactly zero spread. An instrument that reported a spread here would
    // make every number in that entry a number about itself.
    assert!(
        (stats.gap_mean_ns - 8_000_000.0).abs() < 1e-6,
        "the mean gap is {} ns",
        stats.gap_mean_ns
    );
    assert_eq!(
        stats.gap_sd_ns, 0.0,
        "a stream with a constant period reported a spread of {} ns",
        stats.gap_sd_ns
    );

    // The finest motion in the stream is the slow stretch's 0.03 counts, which
    // is 0.0015 world units — against a terminal cell of 1.16 world units
    // across and 4.11 down. That ratio is what R14 becomes.
    let finest = stats.finest_count.expect("the stream has motion in it");
    assert!(
        (finest - SLOW_COUNTS).abs() < 1e-12,
        "finest motion was {finest}"
    );
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
        samples[0].event,
        Event::Pressed {
            control: Control::Move,
            down: true
        }
    );
    assert_eq!(
        samples[1].event,
        Event::Pressed {
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

/// A device event delivered twice is reported, not filtered.
///
/// This statistic exists because the platform really does this: an X11 pointer
/// grab makes the server deliver every raw motion event a second time, five
/// microseconds after the first, and the playable client does not take a grab
/// for exactly that reason (`client::gfx`). The two halves below are the design:
/// the duplicate stays in the record, and the number says it is there.
///
/// Filtering would have been the tempting fix and it is the same mistake in a
/// better disguise — a predicate on the contents of the record, which is what
/// the terminal's cell-crossing sampler was.
#[test]
fn a_duplicated_device_event_is_counted_and_kept() {
    let mut clean = InputTrace::new();
    let mut doubled = InputTrace::new();
    for (at_ns, dx, dy) in slow_then_fast() {
        clean.moved(at_ns, dx, dy);
        doubled.moved(at_ns, dx, dy);
        // What an X11 grab does: the same delta again, five microseconds later.
        doubled.moved(at_ns + 5_000, dx, dy);
    }

    assert_eq!(clean.stats().coincident, 0, "a clean stream was accused");
    assert_eq!(
        doubled.stats().coincident,
        clean.len(),
        "a duplicated stream was not detected"
    );
    assert_eq!(
        doubled.len(),
        clean.len() * 2,
        "the duplicates were filtered instead of kept"
    );

    // …and the damage the statistic exists to make visible: the median
    // inter-arrival collapses from the device's report period to the duplicate
    // gap, which is what a detector at M8 would otherwise calibrate on.
    assert_eq!(clean.stats().gaps_ns.p50, 8_000_000);
    assert_eq!(doubled.stats().gaps_ns.p50, 5_000);
}

/// A device that legitimately reports at 8 kHz is not accused of duplicating.
#[test]
fn the_fastest_real_device_is_not_mistaken_for_a_duplicate() {
    let mut trace = InputTrace::new();
    // 8 kHz is 125 microseconds, the fastest report rate on sale, and the same
    // delta twice in a row is perfectly ordinary at constant speed.
    for index in 0..500u64 {
        trace.moved(index * 125_000, 1.0, 0.0);
    }
    assert_eq!(
        trace.stats().coincident,
        0,
        "an 8 kHz mouse was reported as a platform duplicating events"
    );
}
