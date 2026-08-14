//! The client writes a telemetry part; `replay` reads one. This is where the two
//! are required to be the same file.
//!
//! # The coupling this closes
//!
//! `docs/ARCHITECTURE.md` forbids `client` a normal dependency on `replay`,
//! because `replay` owns the signing key and a client that can link it is a
//! client that can seal a replay. So the two crates cannot share a type, and the
//! device stream crosses the boundary as **bytes** with a writer on one side and
//! a reader on the other — which is the same arrangement `client/tests/session_part.rs`
//! closes for the session record, and it has the same failure mode: a field added
//! on one side and not the other is silent until a corpus is on a disk.
//!
//! This test binary links both, through the dev-dependency the enforced
//! `--edges normal` claim in `ci` excludes, and requires the writer's bytes to
//! decode into exactly the samples that went in.
//!
//! # What it asserts beyond a round trip
//!
//! Three things the round trip alone would not catch, and each of them is a way
//! the corpus could be quietly wrong:
//!
//! - **The `f64` deltas survive by their bits**, including the ends of the type.
//!   The whole reason a record is twenty-five bytes rather than eleven is that
//!   rounding a device count to an integer is `docs/RISKS.md` R14's grid arriving
//!   through a saving, and a test of whole numbers would not notice a writer that
//!   rounded.
//! - **A view anchor is not a device event.** `docs/SCHEMA.md` §6 refuses a seat
//!   that recorded zero device events, which is the corpus's one mechanical
//!   defence against a headless client — and a headless client receives views.
//! - **The counts in the part are the counts the session record carries**, since
//!   `Corpus::store` refuses the two disagreeing and the two are written by
//!   different functions from the same trace.

#![deny(unsafe_code)]

use client::health::{telemetry_part, telemetry_part_name};
use client::input::{Control, Event, InputTrace, WORLD_UNITS_PER_COUNT};
use replay::telemetry::{Event as ReadEvent, SAMPLE_BYTES, TelemetryPart};
use sim::Seat;

/// A trace holding every kind of record, with deltas chosen for the encoding.
///
/// `docs/RISKS.md` R15: [`the_fixture_reaches_every_record_kind`] is the floor
/// that keeps this honest, because a trace of `(1.0, -1.0)` motions would pin the
/// easy half of an `f64` encoding and the hard half of none.
fn a_trace() -> InputTrace {
    let mut trace = InputTrace::new();
    let deltas = [
        (1.0f64, -1.0f64),
        (0.5, -0.25),
        (-3.0, 0.0),
        (f64::MIN_POSITIVE, -f64::MIN_POSITIVE),
        (1.0e300, -1.0e-300),
        (0.0, 0.0),
    ];
    let mut at_ns = 1_000_000u64;
    for (index, (dx, dy)) in deltas.into_iter().enumerate() {
        at_ns += 8_000_000;
        trace.moved(at_ns, dx, dy);
        if index % 2 == 0 {
            at_ns += 1_234_567;
            trace.pressed(
                at_ns,
                [
                    Control::Move,
                    Control::Attack,
                    Control::Skillshot,
                    Control::Targeted,
                    Control::Stop,
                ][index % 5],
                index % 4 == 0,
            );
        }
        at_ns += 2_000_000;
        trace.viewed(at_ns, index as u32 * 16, index as u32);
    }
    trace
}

/// **The criterion.** What the client writes is what `replay` reads, sample for
/// sample.
#[test]
fn what_the_client_writes_is_what_the_corpus_reads() {
    let trace = a_trace();
    let bytes = telemetry_part(Seat::Red1, &trace);

    let part =
        TelemetryPart::decode(&bytes).expect("the corpus cannot read the part the client wrote");

    assert_eq!(part.seat, Seat::Red1);
    assert_eq!(part.stream.dropped, trace.dropped());
    assert_eq!(
        part.stream.world_units_per_count_e6,
        (WORLD_UNITS_PER_COUNT * 1e6) as u64,
        "the sensitivity the client applied is not the one the corpus records, so \
         a recorded aim cannot be reconstructed from the stream"
    );
    assert_eq!(
        part.stream.samples.len(),
        trace.len(),
        "the part holds a different number of records than the trace"
    );

    for (written, read) in trace.samples().iter().zip(part.stream.samples.iter()) {
        assert_eq!(written.at_ns, read.at_ns);
        match (written.event, read.event) {
            (Event::Moved { dx, dy }, ReadEvent::Moved { dx: rx, dy: ry }) => {
                // By the **bits**, not by equality: `0.0 == -0.0` and a writer
                // that lost a sign would pass a comparison and put a mirrored
                // trajectory into the corpus.
                assert_eq!(dx.to_bits(), rx.to_bits(), "a delta changed on the way out");
                assert_eq!(dy.to_bits(), ry.to_bits(), "a delta changed on the way out");
            }
            (
                Event::Pressed { control, down },
                ReadEvent::Pressed {
                    control: read_control,
                    down: read_down,
                },
            ) => {
                assert_eq!(control.tag(), read_control.tag());
                assert_eq!(down, read_down);
            }
            (Event::Viewed { tick, seq }, ReadEvent::Viewed { tick: rt, seq: rs }) => {
                assert_eq!(tick, rt.0);
                assert_eq!(seq, rs);
            }
            (written, read) => panic!("{written:?} was read back as {read:?}"),
        }
    }

    // …and the reader's own writer agrees with the client's, which is what makes
    // this two implementations rather than one.
    assert_eq!(
        part.encode(),
        bytes,
        "replay::telemetry::TelemetryPart::encode and client::health::telemetry_part \
         write different bytes for the same stream"
    );

    println!(
        "telemetry part: {} bytes for {} record(s), {} bytes each",
        bytes.len(),
        part.stream.samples.len(),
        SAMPLE_BYTES
    );
}

/// The counts the two files carry about one seat are the same counts.
///
/// `Corpus::store` refuses a session record and a companion that disagree, and
/// the two numbers are produced by different functions — `InputTrace::stats` for
/// the session part, `SeatStream::facts` for the companion — from the same trace.
/// A drift between them would be a whole recording session refused at filing
/// time, after nine people had gone home.
#[test]
fn the_two_files_count_the_same_seat_the_same_way() {
    let trace = a_trace();
    let stats = trace.stats();
    let part = TelemetryPart::decode(&telemetry_part(Seat::Blue0, &trace)).expect("decode");
    let facts = part.stream.facts();

    assert_eq!(
        u64::try_from(stats.samples).expect("a count"),
        facts.samples,
        "the session record and the companion count different device events"
    );
    assert_eq!(
        u64::try_from(stats.moves).expect("a count"),
        facts.motions,
        "the session record and the companion count different motions"
    );
    assert_eq!(
        u64::try_from(stats.views).expect("a count"),
        facts.views,
        "the two files count different view anchors"
    );
    println!(
        "telemetry part: {} device event(s), {} motion(s), {} anchor(s), counted \
         alike on both sides",
        facts.samples, facts.motions, facts.views
    );
}

/// **A seat that only received views recorded no device event.**
///
/// The property `docs/SCHEMA.md` §6's silent-seat refusal rests on, asserted from
/// the client's side as well as the corpus's: a headless client drives the
/// protocol and receives thirty views a second, and if the anchors counted as
/// device events it would walk through the one mechanical defence this corpus
/// has against synthetic play.
#[test]
fn a_seat_that_only_received_views_is_still_a_silent_seat() {
    let mut trace = InputTrace::new();
    for index in 0..100u64 {
        trace.viewed(index * 33_333_333, index as u32, index as u32);
    }
    let stats = trace.stats();
    assert_eq!(
        stats.samples, 0,
        "a hundred view anchors counted as device events, so a headless client \
         reads as a person"
    );
    assert_eq!(stats.views, 100);

    let part = TelemetryPart::decode(&telemetry_part(Seat::Green2, &trace)).expect("decode");
    assert_eq!(part.stream.facts().samples, 0);
    assert_eq!(part.stream.facts().views, 100);
    println!("telemetry part: 100 anchors, 0 device events — a silent seat stays silent");
}

/// The inter-arrival distribution is over device events and nothing else.
///
/// `docs/SCHEMA.md` §4b requires `median_gap_ns` to be read against the declared
/// `device_polling_hz`, and a view anchor thirty times a second in the middle of
/// that distribution would be the client's tick rate showing up as the mouse's
/// report rate. The mutation this refuses is a one-line one — counting every
/// sample rather than every device sample — so the property is stated rather
/// than left to the comment.
#[test]
fn the_inter_arrival_distribution_does_not_see_the_view_anchors() {
    let mut with_anchors = InputTrace::new();
    let mut without = InputTrace::new();
    for index in 0..50u64 {
        let at = index * 8_000_000;
        with_anchors.moved(at, 1.0, 0.0);
        without.moved(at, 1.0, 0.0);
        // Anchors landing between two device events, at a different rate.
        if index % 4 == 0 {
            with_anchors.viewed(at + 4_000_000, index as u32, index as u32);
        }
    }
    assert_eq!(
        with_anchors.stats().gaps_ns,
        without.stats().gaps_ns,
        "the anchors moved the inter-arrival distribution, so `median_gap_ns` no \
         longer means what docs/SCHEMA.md §4b says it means"
    );
    assert_eq!(with_anchors.stats().samples, without.stats().samples);
    assert!(with_anchors.stats().views > 0, "no anchor was recorded");
    println!(
        "telemetry: {} anchor(s) added, median gap unchanged at {} ns",
        with_anchors.stats().views,
        without.stats().gaps_ns.p50
    );
}

/// The file is named by its seat and by nothing else.
#[test]
fn a_part_is_named_by_its_seat_and_names_nobody() {
    for seat in [Seat::Blue0, Seat::Red1, Seat::Green2] {
        let name = telemetry_part_name(seat);
        assert_eq!(name, format!("seat-{}.telemetry-part", seat.index()));
        assert!(
            !name.contains('.') || name.ends_with(".telemetry-part"),
            "the extension is what `.gitignore` and `ci` refuse"
        );
    }
}

/// `docs/RISKS.md` R15: the fixture reaches every case the assertions are about.
#[test]
fn the_fixture_reaches_every_record_kind() {
    let trace = a_trace();
    let mut moved = 0u32;
    let mut pressed = 0u32;
    let mut viewed = 0u32;
    let mut extreme = 0u32;
    let mut fractional = 0u32;
    let mut controls: Vec<u8> = Vec::new();
    for sample in trace.samples() {
        match sample.event {
            Event::Moved { dx, dy } => {
                moved += 1;
                if dx.abs() >= 1.0e300 || (dx != 0.0 && dx.abs() <= f64::MIN_POSITIVE) {
                    extreme += 1;
                }
                if dx.fract() != 0.0 || dy.fract() != 0.0 {
                    fractional += 1;
                }
            }
            Event::Pressed { control, .. } => {
                pressed += 1;
                if !controls.contains(&control.tag()) {
                    controls.push(control.tag());
                }
            }
            Event::Viewed { .. } => viewed += 1,
        }
    }
    println!(
        "telemetry part fixture: {moved} motion(s) ({extreme} at the ends of the \
         type, {fractional} fractional), {pressed} press(es) over {} control(s), \
         {viewed} anchor(s)",
        controls.len()
    );
    assert!(moved > 0 && pressed > 0 && viewed > 0);
    assert!(
        extreme > 0,
        "no delta near the ends of the f64 domain, so the encoding is checked only \
         where every value is easy"
    );
    assert!(fractional > 0, "every delta is a whole number");
    assert!(
        controls.len() >= 3,
        "only {} control(s) appear",
        controls.len()
    );
}
