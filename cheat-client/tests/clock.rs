//! Exploit class 4: time manipulation — a client that lies about its own clock.
//!
//! `docs/SCOPE.md` adversary model: *the attacker can control the client clock
//! and input timing; only server-observed time is evidence.* This file is the
//! attacker exercising that, and the defence it runs into.
//!
//! # The exploit works: the client can claim anything
//!
//! `ClientMessage::Input` carries a `claimed_at_ms`, and the attacker writes
//! whatever it likes there — a frozen clock, one running backwards, one jumped to
//! the far future. The server records every one. There is no validation to fail,
//! which is what makes the lie *sayable*.
//!
//! # The defence is structural: no rule reads it
//!
//! `docs/SCOPE.md` says only server-observed time is evidence, and the way this
//! project honours that is not a detector — it is that `Match::deliver` stamps
//! each input with the server's own tick and its own `received_at_ms`, and no rule
//! in `sim` ever reads `claimed_at_ms`. So the world a lying clock produces is
//! **byte-identical** to the world an honest one produces, which is the assertion
//! that matters: the attacker can move its claimed clock arbitrarily and the match
//! does not move at all.
//!
//! What the field is *for* is the divergence between it and the server's arrival
//! time, which is exploit class 4's signal — recorded in the `TimedInput` log, and
//! read by a detector at M8. M7's job is to show it is recorded and inert, and
//! that a client cannot make the server act on its own clock. The detector, and
//! any error bound, is M8's.

#![deny(unsafe_code)]

use cheat_client::bot::Bot;
use protocol::Action;
use sim::{FxVec2, RULES, Seat, base_position};

#[path = "harness/authority.rs"]
mod authority;

use authority::started_match;

/// A claimed-clock function: given the tick, what the client *claims* the time is.
type ClaimedClock = fn(u32) -> u64;

/// Plays an identical match under a given claimed-clock, and returns the world's
/// digest and the recorded (claimed, received) pairs.
///
/// Every seat drives the *same* actions on the same ticks; the only thing that
/// varies between two runs is what the client writes into `claimed_at_ms`. So any
/// difference in the resulting digest would be a rule reading the claimed clock —
/// which is exactly what must not happen.
fn play_under(clock: ClaimedClock) -> (sim::Digest, Vec<(u64, u64)>) {
    let mut game = started_match(0x0F1E_2D3C_4B5A_6978, 9);
    let mut bots: Vec<Bot> = (0..9).map(|_| Bot::new()).collect();

    for seat in Seat::ALL {
        let frame = bots[seat.index()].intend_at(Action::Move(FxVec2::ZERO), clock(0));
        game.deliver(seat, frame.as_bytes().as_slice(), clock(0).wrapping_add(1))
            .expect("the opening move was accepted");
    }

    for tick in 0..300u32 {
        if tick % 240 == 60 {
            for seat in Seat::ALL {
                let direction = base_position(seat.team(), &RULES).neg();
                // The lie is here: `claimed_at_ms` is whatever the clock says,
                // which bears no relation to the server's own `received_at_ms`
                // passed to `deliver` below.
                let frame = bots[seat.index()].intend_at(Action::Skillshot(direction), clock(tick));
                let _ = game.deliver(
                    seat,
                    frame.as_bytes().as_slice(),
                    // The server's clock: honest, monotone, the attacker cannot
                    // touch it. Distinct from the claim on purpose.
                    u64::from(tick).saturating_mul(33),
                );
            }
        }
        let _ = game.tick();
    }

    let recording = game.recording();
    let pairs = recording
        .inputs
        .iter()
        .map(|timed| (timed.claimed_at_ms, timed.received_at_ms))
        .collect();
    (game.digest(), pairs)
}

/// An honest clock, a frozen one, one that runs backwards, and one in the far
/// future all produce the same match.
#[test]
fn a_lying_clock_changes_the_telemetry_and_never_the_world() {
    // Honest: the claim tracks the tick.
    let honest: ClaimedClock = |tick| u64::from(tick).saturating_mul(33);
    // Frozen: the client insists no time has passed.
    let frozen: ClaimedClock = |_| 0;
    // Backwards: the client claims to act earlier each time.
    let backwards: ClaimedClock = |tick| 10_000_000u64.saturating_sub(u64::from(tick));
    // Far future: the client claims to be acting years from now.
    let future: ClaimedClock = |tick| 9_000_000_000_000u64.saturating_add(u64::from(tick));

    let (honest_digest, honest_pairs) = play_under(honest);
    let (frozen_digest, frozen_pairs) = play_under(frozen);
    let (backwards_digest, _) = play_under(backwards);
    let (future_digest, future_pairs) = play_under(future);

    // R15: the match has to contain inputs, or "the clock did not matter" is a
    // statement about an empty log.
    assert!(!honest_pairs.is_empty(), "no inputs were recorded (R15)");

    // The defence: the world is identical under every clock. No rule read the
    // claim.
    assert_eq!(
        honest_digest, frozen_digest,
        "a frozen claimed clock changed the world: a rule reads claimed_at_ms"
    );
    assert_eq!(
        honest_digest, backwards_digest,
        "a backwards claimed clock changed the world: a rule reads claimed_at_ms"
    );
    assert_eq!(
        honest_digest, future_digest,
        "a far-future claimed clock changed the world: a rule reads claimed_at_ms"
    );
    println!("clock: four different claimed clocks, one identical world digest");

    // The exploit *worked*, in the only sense it can: the lie is in the record.
    // The server kept the client's claim and its own observation side by side, so
    // the divergence M8 will read is there — and it is enormous under the frozen
    // and future clocks, which is the class-4 signal existing rather than being
    // acted upon.
    assert!(
        frozen_pairs
            .iter()
            .any(|(claimed, received)| claimed != received),
        "the frozen clock left no divergence in the record, so the class-4 signal \
         was not captured"
    );
    let worst_future_divergence = future_pairs
        .iter()
        .map(|(claimed, received)| claimed.abs_diff(*received))
        .max()
        .unwrap_or(0);
    assert!(
        worst_future_divergence > 1_000_000_000,
        "the far-future clock's divergence was not recorded"
    );
    println!(
        "clock: the far-future lie is recorded as a divergence of up to {worst_future_divergence} \
         ms between claimed and observed — inert now, the class-4 signal at M8"
    );

    // And the honest run's claim and observation stay close, so the divergence is
    // a signal and not noise every run carries.
    let honest_divergence = honest_pairs
        .iter()
        .map(|(claimed, received)| claimed.abs_diff(*received))
        .max()
        .unwrap_or(0);
    assert!(
        honest_divergence < worst_future_divergence,
        "the honest run diverged as much as the lying one, so the signal is not one"
    );
}
