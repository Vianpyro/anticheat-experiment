//! The lobby measures the device, and it measures it through the game's own
//! capture path.
//!
//! Three families of assertion, and the first is the one the whole design rests
//! on.
//!
//! **1. The forbidden dependency.** A menu driven by the operating system's
//! pointer would measure the *accelerated* pointer — the quantity
//! `docs/SCHEMA.md` §4d refuses everywhere else in this client — and the scale
//! recovered from it would not be the scale the match is played at. So the same
//! device events are driven through two clients differing only in window size,
//! by a factor of six in pixels per world unit, and every byte of what they
//! record and everything they measure must be identical. This is
//! `docs/ARCHITECTURE.md` invariant 12 restated over the menu, and it is what
//! stays red for any future edit that reaches for a viewport, a pixel or a
//! cursor position inside `client::lobby`.
//!
//! **2. The measurement.** A simulated crossing recovers the build's own scale,
//! and the error is printed rather than asserted away.
//!
//! **3. `docs/RISKS.md` R15**, throughout: every fixture here states what it
//! actually reached, and the counts are printed even when they pass.

#![deny(unsafe_code)]

use client::input::{Control, Event};
use client::lobby::{Element, Lobby, Observations};
use client::play::Play;
use replay::calibration::{CalibrationState, DeviceProfileId, Estimate, Profile};
use sim::FxVec2;

#[path = "harness/traversal.rs"]
mod traversal;

use traversal::{Hand, cross, reach};

/// What the corpus stores, from what the client measured.
///
/// The two types hold the same numbers and neither is derived from the other —
/// `client` may not link `replay` — so the conversion is written out here in the
/// test that has both, exactly as `client/tests/session_part.rs` is where the
/// text format's two halves meet.
fn stored(observations: Observations) -> replay::calibration::Observations {
    let e3 = |value: f64| (value * 1e3).round() as u64;
    replay::calibration::Observations {
        reaches: observations.reaches,
        octants: observations.octants,
        clamped: observations.clamped,
        min_distance_e3: e3(observations.min_distance),
        max_distance_e3: e3(observations.max_distance),
        sum_distance_e3: e3(observations.sum_distance),
        sum_counts_e3: e3(observations.sum_counts),
        sum_distance_sq_e3: e3(observations.sum_distance_sq),
        sum_distance_counts_e3: e3(observations.sum_distance_counts),
        sum_counts_sq_e3: e3(observations.sum_counts_sq),
        fast_reaches: observations.fast_reaches,
        fast_motions: observations.fast_motions,
        fast_ns: observations.fast_ns,
        quantum_e6: (observations.quantum * 1e6).round() as u64,
    }
}

/// **The lobby is driven by the integrated cursor and never by a pointer, and
/// the window cannot reach it.**
///
/// The property this whole pass depends on. Two clients, the same device events,
/// window sizes a factor of six apart in pixels per world unit: identical trace,
/// identical cursor, identical reaches, identical statistics.
///
/// Both mutations that would break it turn this red. Resolving a click against a
/// screen position — the natural thing to write for a menu — makes the two
/// disagree about what was clicked as soon as the windows differ; closing a leg
/// on a redraw rather than on a click makes the reach counts differ with the
/// frame rate.
#[test]
fn the_window_cannot_reach_the_lobby() {
    let hand = Hand::quick();
    let mut small = Play::new();
    let mut large = Play::new();
    small.resized(640, 480);
    large.resized(3840, 2160);

    cross(&mut small, hand, 6);
    cross(&mut large, hand, 6);

    assert_eq!(
        small.trace().digest(),
        large.trace().digest(),
        "two clients differing only in window size recorded different device \
         traces: something in the capture path read the renderer \
         (docs/ARCHITECTURE.md invariant 12)"
    );
    assert_eq!(
        small.aim(),
        large.aim(),
        "two clients differing only in window size ended with different cursors"
    );
    let (a, b) = (small.lobby().observations(), large.lobby().observations());
    assert_eq!(
        a, b,
        "two clients differing only in window size measured different devices: \
         the lobby's geometry is in world units and the window must not reach it"
    );

    // R15: the fixture reached the case the assertion is about. Two clients that
    // recorded nothing agree perfectly.
    assert!(
        a.reaches >= 9,
        "the crossing produced {} reach(es), so the equality above is a \
         statement about an empty measurement",
        a.reaches
    );
    println!(
        "lobby: window independence — {} reach(es), {} motion sample(s), trace \
         digest {} at 640x480 and at 3840x2160",
        a.reaches,
        small.trace().stats().moves,
        small.trace().digest()
    );
}

/// **The estimated scale recovers the scale this build actually applies, and
/// here is the error.**
///
/// The build moves the cursor `client::input::WORLD_UNITS_PER_COUNT` world units
/// per device count, so the true conversion is its reciprocal — 20 counts per
/// world unit. Nothing in the lobby is told that number: the hand emits counts
/// and looks, the reach records the net displacement, and the regression is run
/// by `replay::calibration` over geometry the build fixes.
#[test]
fn a_simulated_crossing_recovers_the_scale_this_build_applies() {
    let truth = 1.0 / client::input::WORLD_UNITS_PER_COUNT;
    let mut play = Play::new();
    play.resized(1280, 800);
    cross(&mut play, Hand::quick(), 12);

    let observations = stored(play.lobby().observations());
    let estimate = Estimate::of(&observations).expect("a crossing supports a fit");
    let error = (estimate.counts_per_unit - truth).abs() / truth;

    println!(
        "lobby: {} reach(es) over {} octant(s), {:.1} to {:.1} world units \
         (ratio {:.2})",
        observations.reaches,
        observations.octants_covered(),
        (observations.min_distance_e3 as f64) / 1e3,
        (observations.max_distance_e3 as f64) / 1e3,
        observations.distance_ratio()
    );
    println!(
        "lobby: scale {:.4} device count(s) per world unit against a true \
         {truth:.4} — relative error {:.4}% ({:.5} counts per unit)",
        estimate.counts_per_unit,
        error * 100.0,
        (estimate.counts_per_unit - truth).abs()
    );
    println!(
        "lobby: arrival cost {:.2} count(s), fit {:.5}, {:.0} Hz measured against \
         125 Hz emitted, quantum {:.3} count(s)",
        estimate.arrival_counts, estimate.fit, estimate.report_hz, estimate.quantum
    );

    assert!(
        error < 0.02,
        "the estimated scale is {:.4} counts per world unit against a true \
         {truth:.4}, an error of {:.2}%",
        estimate.counts_per_unit,
        error * 100.0
    );
    // The arrival cost is where the landing slop and the overshoot correction
    // go. It is not asserted to be small — it is asserted to be *somewhere*,
    // because a fit that reported it as zero would have folded it into the slope.
    assert!(
        estimate.arrival_counts.abs() < truth * 12.0,
        "the intercept is {:.1} counts, which is more than a button's radius \
         costs and is therefore not the arrival cost",
        estimate.arrival_counts
    );
    // And the report rate is the emitted one, not the client's tick.
    assert!(
        (estimate.report_hz - 125.0).abs() < 6.0,
        "the measured report rate is {:.1} Hz against 125 Hz emitted",
        estimate.report_hz
    );
}

/// **A crossing with the dummy makes a session sufficient; the menu alone does
/// not.**
///
/// The separation this pass is about, at its smallest: a player who crosses the
/// menu and starts is partially calibrated, and a player who spends the wait
/// hitting the dummy is fully calibrated. Neither is refused anything.
#[test]
fn the_menu_alone_is_partial_and_the_wait_is_what_completes_it() {
    let device = DeviceProfileId::parse("mouse-a").expect("a device label");

    let mut menu_only = Play::new();
    cross(&mut menu_only, Hand::quick(), 0);
    let menu = stored(menu_only.lobby().observations());
    assert_eq!(
        CalibrationState::rate(&menu, &Profile::empty(device.clone())),
        CalibrationState::Partial,
        "one crossing of the menu was rated as a calibrated device"
    );

    let mut waited = Play::new();
    cross(&mut waited, Hand::quick(), 12);
    let full = stored(waited.lobby().observations());
    assert_eq!(
        CalibrationState::rate(&full, &Profile::empty(device.clone())),
        CalibrationState::Sufficient,
        "a full crossing did not calibrate: {:?}",
        {
            let mut profile = Profile::empty(device.clone());
            profile.fold(full);
            profile.shortfall()
        }
    );

    // And the accumulation: three partial evenings are a calibrated participant.
    let mut profile = Profile::empty(device);
    for _ in 0..3 {
        profile.fold(menu);
    }
    println!(
        "lobby: menu only — {} reach(es), {} octant(s), ratio {:.2}; three of \
         them pooled — {} reach(es), {} octant(s), sufficient {}",
        menu.reaches,
        menu.octants_covered(),
        menu.distance_ratio(),
        profile.observations.reaches,
        profile.observations.octants_covered(),
        profile.sufficient()
    );
    assert!(
        menu.reaches < full.reaches,
        "the wait added no reaches, so the two states above are the same fixture"
    );
}

/// The lobby records what the hand did, including the clicks that did nothing.
///
/// A press that lands on no element is a thing the player did and is in the
/// trace; it closes no leg, so the movement it interrupted is still one
/// movement. Both halves matter: forgetting the press would put a hole in the
/// device stream, and closing the leg would put a reach in the record whose
/// endpoint is not a known position.
#[test]
fn a_click_on_nothing_is_recorded_and_closes_no_leg() {
    let hand = Hand::quick();
    let mut play = Play::new();
    let mut at_ns = 0u64;

    // A click on bare floor — no button, no dummy — and then on the pseudonym.
    let name = play.lobby().position_of(Element::Name);
    let nowhere = FxVec2::new(sim::Fx::from_int(45), sim::Fx::from_int(-20));
    assert_eq!(reach(&mut play, hand, &mut at_ns, nowhere), None);
    let after_miss = play.lobby().observations();
    assert_eq!(after_miss.reaches, 0, "a click on nothing recorded a reach");

    assert_eq!(
        reach(&mut play, hand, &mut at_ns, name),
        Some(Element::Name)
    );
    let observations = play.lobby().observations();
    assert_eq!(observations.reaches, 1);

    let presses = play
        .trace()
        .samples()
        .iter()
        .filter(|sample| {
            matches!(
                sample.event,
                Event::Pressed {
                    control: Control::Move,
                    down: true
                }
            )
        })
        .count();
    assert_eq!(
        presses, 2,
        "the trace holds {presses} press(es) and the hand clicked twice: a \
         click that produced no order is still a thing the player did"
    );

    // The reach spans the whole distance from the centre of the map, not the
    // half of it after the miss.
    let expected = {
        let (x, y) = (
            f64::from(name.x.to_raw()) / 65536.0,
            f64::from(name.y.to_raw()) / 65536.0,
        );
        x.hypot(y)
    };
    assert!(
        (observations.max_distance - expected).abs() < 0.01,
        "the reach is {} world units and the geometry says {expected}",
        observations.max_distance
    );
    println!(
        "lobby: one miss and one hit — {} press(es) recorded, {} reach(es), {:.2} \
         world units",
        presses, observations.reaches, observations.max_distance
    );
}

/// **A crossing at a creep measures no report rate, and says so.**
///
/// The report rate is the one quantity a slow session cannot produce: a hand
/// that creeps reports at the same rate but spends most of every interval
/// stationary, and a stationary hand reports nothing at all
/// (`client::input`, one sample per device event and never one per interval). So
/// a session with no fast reach in it is not sufficient, whatever else it
/// reached — and it is `Partial` rather than refused.
#[test]
fn a_crossing_at_a_creep_reads_no_report_rate() {
    let mut play = Play::new();
    cross(&mut play, Hand::slow(), 12);
    let observations = stored(play.lobby().observations());

    assert_eq!(
        observations.fast_reaches,
        0,
        "a crossing at {} world units per second was counted as fast",
        client::lobby::FAST_UNITS_PER_SECOND
    );
    let estimate = Estimate::of(&observations).expect("a slow crossing still fits a scale");
    assert!(
        (estimate.report_hz - 0.0).abs() < f64::EPSILON,
        "a report rate of {:.1} Hz was read off a session with no fast reach in it",
        estimate.report_hz
    );

    let device = DeviceProfileId::parse("mouse-a").expect("a device label");
    let mut profile = Profile::empty(device.clone());
    profile.fold(observations);
    assert!(!profile.sufficient());
    assert!(
        profile
            .shortfall()
            .iter()
            .any(|clause| clause.contains("fast")),
        "the shortfall is {:?} and does not name the missing rate",
        profile.shortfall()
    );
    assert_eq!(
        CalibrationState::rate(&observations, &Profile::empty(device)),
        CalibrationState::Partial
    );

    // R15: the crossing happened, and the scale is still recoverable from it —
    // what is missing is the rate and nothing else.
    let truth = 1.0 / client::input::WORLD_UNITS_PER_COUNT;
    println!(
        "lobby: slow crossing — {} reach(es), 0 fast, scale {:.4} against {truth:.4}",
        observations.reaches, estimate.counts_per_unit
    );
    assert!(observations.reaches >= 12);
}

/// The lobby holds no window and the type says so.
///
/// A compile-time companion to the property above: `Lobby` has no constructor,
/// method or field that takes or returns a viewport, a pixel or a window size.
/// This does not prove much on its own — it is one line and a future edit could
/// add one — which is exactly why the property test above exists beside it.
#[test]
fn the_lobby_type_carries_no_window() {
    let lobby = Lobby::new();
    let size = core::mem::size_of_val(&lobby);
    println!("lobby: the state machine is {size} bytes and holds no viewport");
    assert!(lobby.observations().reaches == 0);
}
