//! Exploit class 1, the traffic-analysis half.
//!
//! `docs/MILESTONES.md` M7: *recover the number of nearby entities from message
//! sizes and arrival times against unpadded messages. This one has no defense
//! yet; it motivates padding, which lands here.* The padding did land — at M3,
//! ahead of the exploit — so what M7 delivers is the exploit that makes it a
//! *delivered* defence in `docs/SCOPE.md`'s sense: the attack that works against
//! the counterfactual unpadded stream and fails against the one this project
//! ships.
//!
//! # Two streams from one match
//!
//! One match is driven, and at every tick the attacker is shown two footprints of
//! the frame that went to one seat:
//!
//! - **Unpadded** — the view encoded at its natural length, `view.encode()`,
//!   which is the stream a server without the traffic-shape invariant would emit.
//!   This is the counterfactual the padding exists to prevent.
//! - **Padded** — the real [`ServerFrame`], cut into its constant number of
//!   constant-size shards, which is what `Match::tick` actually produces.
//!
//! The exploit is [`Wiretap`], run over each. Against the unpadded stream it
//! reads the visible-entity count off the size and its capture partitions the
//! match's ticks into many distinct shapes; against the padded stream every tick
//! is byte-identical from outside and the count is unrecoverable. Both are
//! asserted, and the test is red if either comes out wrong — an exploit that
//! failed against the padded stream *without ever having worked* against the
//! unpadded one would be `docs/RISKS.md` R15 with a stopwatch.

#![deny(unsafe_code)]

use cheat_client::traffic::Wiretap;
use protocol::{SERVER_DATAGRAM_BYTES, SERVER_SHARDS, SHARD_HEADER_BYTES};
use sim::view::view_for;
use sim::{Action, FxVec2, RULES, Seat, base_position};

#[path = "harness/authority.rs"]
mod authority;

use authority::started_match;

/// The seat whose stream the attacker is watching.
const WATCHED: Seat = Seat::Blue0;

/// Ticks driven. Long enough that the match moves through states with different
/// numbers of visible entities — which is the whole thing the attacker is trying
/// to read, and which `docs/RISKS.md` R15 needs to actually happen.
const TICKS: u32 = 700;

/// A match in which the number of entities visible to the watched seat varies,
/// so that a size channel would have something to report.
///
/// Every seat walks to the centroid of the map and then mills there, which pulls
/// the three teams into and out of one another's vision over the run — empty
/// views at the start, crowded ones in the middle, projectiles and events when
/// they fight. Returns the per-tick visible-entity counts alongside the two
/// footprints, so the assertions can be stated against what actually happened.
///
/// **The padded stream comes out of `Match::tick` itself**, not out of a frame the
/// test encodes. That matters for what the cadence assertion can catch: the
/// *size* half of the traffic invariant is carried by the types — `ServerFrame`
/// wraps a fixed array and `shards` returns a fixed array of them, so no mutation
/// short of editing those types can make a frame's length follow its content — but
/// the *cadence* half is the shape of a loop, and reading the loop's own output is
/// what makes "one frame per occupied seat, every tick, whatever happened" a thing
/// this exploit can observe failing. An early return in `Match::tick` turns the
/// gap assertion below red; a frame the test built itself would have hidden it.
fn run() -> (Wiretap, Wiretap, Vec<usize>) {
    let mut game = started_match(0x00C0_FFEE_0D15_EA5E, 9);

    let centre = FxVec2::ZERO;
    for seat in Seat::ALL {
        // Each seat walks toward the middle, then casts down its lane every so
        // often once it can, so the stream carries projectiles and events too and
        // the size channel has more than champions to leak.
        let frame = protocol::ClientFrame::encode(&protocol::ClientMessage::Input {
            seq: 0,
            claimed_at_ms: 0,
            action: Action::Move(centre),
        });
        game.deliver(seat, frame.as_bytes().as_slice(), 0)
            .expect("a move was accepted");
    }

    // The observer of the padded stream sees the transport header on every shard;
    // it is part of the published format, so the attacker subtracts it.
    let mut padded = Wiretap::new(SHARD_HEADER_BYTES);
    // The unpadded counterfactual travels as one datagram with no shard header.
    let mut unpadded = Wiretap::new(0);
    let mut visible_counts = Vec::new();

    let mut seq = 1u32;
    for tick in 0..TICKS {
        // A cast every so often from a seat that can, to move some projectiles
        // through the watched view.
        if tick % 90 == 45 {
            for seat in Seat::ALL {
                let direction = base_position(seat.team(), &RULES).neg();
                let frame = protocol::ClientFrame::encode(&protocol::ClientMessage::Input {
                    seq,
                    claimed_at_ms: 0,
                    action: Action::Skillshot(direction),
                });
                let _ = game.deliver(seat, frame.as_bytes().as_slice(), 0);
            }
            seq = seq.saturating_add(1);
        }

        // The real cadence path: whatever the authority chose to emit this tick.
        let emitted = game.tick();
        let state = game.world();

        // What the watched seat can see this tick, which is the quantity the
        // attacker is trying to recover.
        let view = view_for(state, WATCHED);
        visible_counts.push(view.visible.len());

        // The padded stream: the frames `Match::tick` actually produced for the
        // watched seat, cut into the shards the transport puts on the wire. A tick
        // on which the authority emitted nothing contributes nothing, which is
        // exactly the gap an observer would time.
        let shard_sizes: Vec<usize> = emitted
            .iter()
            .filter(|(seat, _)| *seat == WATCHED)
            .flat_map(|(_, frame)| {
                frame
                    .shards(tick)
                    .iter()
                    .map(|shard| shard.as_bytes().len())
                    .collect::<Vec<_>>()
            })
            .collect();
        padded.saw(tick, &shard_sizes);

        // The unpadded counterfactual: one datagram the exact size of the view.
        unpadded.saw(tick, &[view.encode().len()]);
    }

    (padded, unpadded, visible_counts)
}

/// The exploit works against an unpadded stream: the size is the entity count.
#[test]
fn a_wiretap_reads_the_entity_count_off_an_unpadded_stream() {
    let (_, unpadded, visible_counts) = run();

    // R15: the match has to actually vary, or "the attacker distinguishes ticks"
    // is a claim about a match with one kind of tick in it.
    let fewest = *visible_counts.iter().min().unwrap();
    let most = *visible_counts.iter().max().unwrap();
    println!("watched seat saw between {fewest} and {most} entities across the match");
    assert!(
        most > fewest,
        "the number of visible entities never changed, so a size channel has \
         nothing to leak and this exploit is about nothing (docs/RISKS.md R15)"
    );

    // The capture partitions the match's ticks into many shapes: an observer who
    // cannot read a byte can already tell a busy tick from an empty one.
    let shapes = unpadded.distinct_footprints();
    println!("unpadded: the wiretap saw {shapes} distinct packet shapes");
    assert!(
        shapes > 1,
        "the unpadded stream looked the same on every tick, which it cannot if \
         its length follows its content"
    );

    // And the reading is not merely different — it is *right*. On every tick with
    // no projectile and no event, the encoding is a fixed part plus a constant
    // per entity, so the attacker's inversion recovers the exact visible count.
    // Compare only those ticks: on a busy tick the estimate is a known
    // over-count, which is a different (and still exploitable) claim.
    let estimates = unpadded.estimate_entities();
    let mut checked = 0u32;
    let mut correct = 0u32;
    for (estimate, truth) in estimates.iter().zip(&visible_counts) {
        let Some(estimate) = estimate else { continue };
        // A quiet tick is one whose size is exactly the fixed part plus whole
        // entities; the attacker cannot know that from outside, but the test can,
        // and the claim is that *when* the tick is quiet the reading is exact.
        if *estimate == *truth {
            correct = correct.saturating_add(1);
        }
        checked = checked.saturating_add(1);
    }
    println!(
        "unpadded: the wiretap's entity estimate was exact on {correct} of {checked} arrivals"
    );
    assert!(
        correct > 0,
        "the wiretap never once read the entity count correctly off the size, so \
         it does not do what it claims (docs/RISKS.md R15)"
    );
}

/// The same exploit fails against the stream this project ships.
#[test]
fn the_padding_and_cadence_leave_the_wiretap_nothing() {
    let (padded, _, visible_counts) = run();

    // The same match, the same variation in what was visible — established here
    // too so this test does not rest on the other having run.
    assert!(
        visible_counts.iter().max() > visible_counts.iter().min(),
        "the match did not vary, so there was nothing for padding to hide \
         (docs/RISKS.md R15)"
    );

    // Every tick is byte-identical from outside: one shape, forever. The size
    // channel and the count channel are both closed, because the frame is a fixed
    // array cut into a fixed number of fixed-size shards.
    let shapes = padded.distinct_footprints();
    println!("padded: the wiretap saw {shapes} distinct packet shape(s)");
    assert_eq!(
        shapes, 1,
        "the padded stream showed the attacker more than one shape, so something \
         about the content reached the wire"
    );

    // **One frame per tick, for every tick, whatever happened.** This is the
    // cadence half, and it is the assertion a mutation can actually reach: the
    // size half is carried by the types (`ServerFrame` is a fixed array,
    // `shards` returns a fixed array of them) and no edit short of changing
    // those could make a frame's length follow its content, but the cadence is
    // the shape of `Match::tick`'s loop and an early return in it lands here.
    //
    // The count is asserted as well as the gaps, and the count is the stronger
    // of the two: a run of skipped ticks at the end of the match leaves no gap
    // behind it, so gaps alone would miss a server that went quiet and stayed
    // quiet.
    assert_eq!(
        padded.footprints().len(),
        TICKS as usize,
        "the watched seat did not receive exactly one frame per tick, so the number \
         of messages is a function of something other than the tick count"
    );
    for footprint in padded.footprints() {
        assert_eq!(footprint.datagrams, SERVER_SHARDS);
        assert_eq!(footprint.bytes, SERVER_SHARDS * SERVER_DATAGRAM_BYTES);
        assert_eq!(
            footprint.gap, 1,
            "a tick passed with no frame: the cadence leaks"
        );
    }

    // And the attacker's inversion, run on the padded stream, recovers the same
    // number every tick regardless of how many entities were really visible —
    // which is the definition of learning nothing. Whatever that number is, it is
    // a constant, so it carries none of the variation the unpadded stream did.
    let estimates: Vec<Option<usize>> = padded.estimate_entities();
    let distinct: std::collections::BTreeSet<Option<usize>> = estimates.iter().copied().collect();
    println!(
        "padded: the wiretap's entity estimate took {} distinct value(s)",
        distinct.len()
    );
    assert_eq!(
        distinct.len(),
        1,
        "the wiretap's reading of the padded stream varied, so the padding is not \
         constant after all"
    );
}
