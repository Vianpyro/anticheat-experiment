//! The one place the client's session part and the corpus's reader meet.
//!
//! # Why this file exists at all
//!
//! `docs/ARCHITECTURE.md` forbids `client` a normal dependency on `replay`,
//! because `replay` owns the signing key and a client that can link it is a
//! client that can seal a replay. So the two crates cannot share the type that
//! describes a recording session: the client writes `key: value` lines
//! (`client::health::SessionPart::encode`) and the corpus parses them
//! (`replay::session::SeatRecord::decode_part`), hand-written on both sides.
//!
//! That is a format with two implementations and no compiler between them, which
//! is the shape of coupling this workspace normally refuses. What makes it
//! acceptable is that the gap is closed *here*: `client` has `replay` as a
//! **dev**-dependency, so a test binary may link both, and a field added on one
//! side and not the other fails in this file rather than in a corpus.
//!
//! The enforced claim is still about the *normal* graph —
//! `cargo tree -p client --edges normal` shows no path to `replay` — and `ci`
//! checks it. A dev-dependency is not that.

#![deny(unsafe_code)]

use client::health::{Cadence, Declared, SessionPart};
use client::input::{Control, InputTrace};
use replay::session::{SeatRecord, SessionRecord};
use sim::Seat;

/// A trace with something in it, and a cadence that fell behind once.
///
/// `docs/RISKS.md` R15: a part built from an empty trace and a cadence that
/// never ran would round-trip a record of all zeroes, and every field below
/// would be checked against the same number. So the fixture is asymmetric on
/// purpose — the counts differ from each other, the overrun is not the pass, and
/// the assertions at the bottom say what it reached.
fn a_part(seat: Seat) -> SessionPart {
    let mut trace = InputTrace::new();
    for index in 0..64u64 {
        // Deltas that change, so `TraceStats::coincident` measures duplication
        // rather than the fixture, and gaps that are not all equal so the median
        // is a median of something.
        let at_ns = index
            .saturating_mul(8_000_000)
            .saturating_add(index % 3 * 100_000);
        trace.moved(at_ns, 1.0 + (index % 5) as f64, -1.0);
    }
    trace.pressed(600_000_000, Control::Skillshot, true);
    trace.pressed(600_500_000, Control::Skillshot, false);

    let mut cadence = Cadence::with_budget(33_333_333);
    for _ in 0..500 {
        cadence.pass(1_200_000);
    }
    cadence.pass(41_000_000);

    SessionPart {
        seat,
        declared: Declared {
            device_cpi: 1600,
            device_polling_hz: 1000,
            pointer_acceleration: false,
        },
        trace: trace.stats(),
        cadence: cadence.report(),
    }
}

/// **What the client writes is what the corpus reads, field for field.**
#[test]
fn a_part_the_client_writes_is_a_part_the_corpus_reads() {
    let part = a_part(Seat::Red1);
    let text = part.encode();
    println!("session part:\n{text}");

    let (seat, record) =
        SeatRecord::decode_part(&text).expect("the corpus could not read a part this client wrote");
    assert_eq!(seat, Seat::Red1.index(), "the part named the wrong seat");

    let SeatRecord::Human { declared, measured } = record else {
        panic!("a part the client wrote decoded as an empty seat");
    };

    // The declared half, which is the participant's answer travelling unchanged.
    assert_eq!(declared.device_cpi, 1600);
    assert_eq!(declared.device_polling_hz, 1000);
    assert!(!declared.pointer_acceleration);

    // The measured half. Every one of these is checked against the value the
    // client actually holds rather than against a literal, so a change of units
    // on either side fails here.
    assert_eq!(measured.samples, part.trace.samples as u64);
    assert_eq!(measured.motions, part.trace.moves as u64);
    assert_eq!(measured.coincident, part.trace.coincident as u64);
    assert_eq!(measured.median_gap_ns, part.trace.gaps_ns.p50);
    assert_eq!(measured.budget_ns, part.cadence.budget_ns);
    assert_eq!(measured.passes, part.cadence.passes);
    assert_eq!(measured.passes_over_budget, part.cadence.passes_over_budget);
    assert_eq!(measured.worst_overrun_ns, part.cadence.worst_overrun_ns);
    assert_eq!(measured.worst_pass_ns, part.cadence.worst_pass_ns);
    assert_eq!(
        measured.world_units_per_count_e6,
        (client::input::WORLD_UNITS_PER_COUNT * 1e6) as u64
    );
    assert_eq!(measured.platform.tag(), client::health::platform());
    assert_eq!(
        measured.clock.tag(),
        match client::input::CLOCK {
            client::input::Clock::Device => "device",
            client::input::Clock::Dequeue => "dequeue",
        }
    );

    // `docs/RISKS.md` R15: the fixture reached the cases the assertions above
    // are about, or every one of them compared zero against zero.
    assert!(part.trace.samples > 0 && part.trace.moves > 0);
    assert!(
        part.trace.gaps_ns.p50 > 0,
        "every sample shares a timestamp"
    );
    assert!(
        part.cadence.passes_over_budget > 0 && part.cadence.worst_overrun_ns > 0,
        "the fixture never went over budget, so the two numbers R16 is about are \
         both zero and the round trip proves nothing about them"
    );
    assert_ne!(
        part.cadence.worst_overrun_ns, part.cadence.worst_pass_ns,
        "the overrun equals the pass, so an encoder that wrote one of them twice \
         would pass"
    );
}

/// A part naming a seat outside the match is not a part.
#[test]
fn a_part_for_a_seat_that_does_not_exist_is_refused() {
    let text = a_part(Seat::Blue0).encode().replace("seat: 0", "seat: 9");
    assert!(SeatRecord::decode_part(&text).is_none());
}

/// **A part that claims not to be a person is refused at the parser.**
///
/// `docs/SCHEMA.md` excludes synthetic play from this corpus, and the refusal is
/// in the reader rather than in a check somebody has to remember to run: the
/// schema has no variant for a bot, so a file that says it is one does not
/// decode.
#[test]
fn a_part_that_claims_to_be_anything_but_a_person_is_refused() {
    for claim in ["bot", "script", "replay", ""] {
        let text = a_part(Seat::Blue0)
            .encode()
            .replace("provenance: human", &format!("provenance: {claim}"));
        assert!(
            SeatRecord::decode_part(&text).is_none(),
            "a part claiming provenance {claim:?} was accepted"
        );
    }
}

/// Nine parts assemble into one record, and two claiming a seat do not.
#[test]
fn parts_assemble_into_a_session_record_and_a_collision_is_named() {
    let parts: Vec<(String, String)> = [Seat::Blue0, Seat::Blue1, Seat::Red0]
        .into_iter()
        .map(|seat| {
            let part = a_part(seat);
            (part.file_name(), part.encode())
        })
        .collect();

    let record = SessionRecord::assemble(
        replay::MatchId(*b"m6-session-part!"),
        replay::ConsentVersion::current(),
        "2026-09-03",
        replay::session::Supervision::InPerson,
        &parts,
    )
    .expect("three parts did not assemble");
    assert_eq!(record.occupied(), vec![0, 1, 3]);
    assert!(record.degraded(), "the fixture went over budget");

    // …and the mistake an operator collecting nine files will actually make.
    let mut collided = parts.clone();
    collided.push(parts[0].clone());
    let refused = SessionRecord::assemble(
        replay::MatchId(*b"m6-session-part!"),
        replay::ConsentVersion::current(),
        "2026-09-03",
        replay::session::Supervision::InPerson,
        &collided,
    )
    .expect_err("two parts claimed one seat and the record assembled anyway");
    assert!(refused.contains("seat 0"), "{refused}");
}
