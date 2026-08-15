//! The demonstration a participant is shown before they sign.
//!
//! `docs/CONSENT.md` §L3 makes one claim no paragraph on that page can make for
//! itself: that a participant is shown **their own** movements and the four
//! things this project works out from them, computed from their numbers. This
//! file is what keeps that from being a promise about a command nobody ran.
//!
//! Two things are under test and the second is the one that would rot quietly.
//! The page has to *say* the four things — which is a formatting assertion and
//! cheap. And it has to have **derived** them, which is not: a disclosure that
//! printed "not enough movement to read a report rate" for every stream would
//! satisfy every assertion about the words while demonstrating nothing at all.
//! So every derived quantity is asserted against a number this test constructed
//! the stream to produce (`docs/RISKS.md` R15).

#![deny(unsafe_code)]

use replay::disclosure::{self, EXCERPT};
use replay::session::{Clock, Platform};
use replay::telemetry::{Control, Event, Sample, SeatStream, TelemetryPart};

/// The report period this stream is built at, in nanoseconds: 1 kHz, the fastest
/// mouse `docs/CONSENT.md` describes and the one whose numbers are least
/// comfortable.
const PERIOD_NS: u64 = 1_000_000;
/// Motions in the stream.
const MOTIONS: usize = 300;
/// How long after a frame arrives the fixture's hand answers it.
const REACTION_NS: u64 = 214_000_000;

/// A crossing of the lobby, as a client would have recorded it.
///
/// Built rather than replayed from a fixture because what this test is about is
/// the arithmetic over a stream whose answers are known: a constant report
/// period so the median gap is exact, a sawtooth so the path is longer than the
/// displacement by a knowable amount, whole-count deltas so the quantum is one,
/// and exactly one frame-then-press pair at a fixed delay so the reaction is the
/// number below and not an accident of ordering.
fn a_crossing() -> TelemetryPart {
    let mut samples = Vec::new();
    for index in 0..MOTIONS as u64 {
        // A hand that goes out and comes back: eight counts right, then four
        // left. Net is four per pair, path is twelve, so the correction is a
        // third of the travel and the page has something to report.
        let dx = if index % 2 == 0 { 8.0 } else { -4.0 };
        samples.push(Sample {
            at_ns: index * PERIOD_NS,
            event: Event::Moved { dx, dy: 1.0 },
        });
    }
    // A frame arrives, and the hand answers it. The release afterwards is not an
    // answer to anything and the page must not read it as one.
    let shown = MOTIONS as u64 * PERIOD_NS;
    samples.push(Sample {
        at_ns: shown,
        event: Event::Viewed {
            tick: sim::Tick(30),
            seq: 30,
        },
    });
    samples.push(Sample {
        at_ns: shown + REACTION_NS,
        event: Event::Pressed {
            control: Control::Move,
            down: true,
        },
    });
    samples.push(Sample {
        at_ns: shown + REACTION_NS + 90_000_000,
        event: Event::Pressed {
            control: Control::Move,
            down: false,
        },
    });

    TelemetryPart {
        seat: sim::Seat::Blue0,
        stream: SeatStream {
            clock: Clock::Dequeue,
            platform: Platform::Linux,
            world_units_per_count_e6: 50_000,
            dropped: 0,
            samples,
        },
    }
}

/// **The page shows the participant their own records, and derives the four
/// things the document says can be worked out from them.**
///
/// Each derived assertion names the number the fixture was built to produce, so
/// a change that quietly turns a derivation into "not enough data" fails here
/// rather than in a room with nine people in it.
#[test]
fn the_disclosure_shows_the_stream_and_derives_what_the_document_claims() {
    let part = a_crossing();
    let page = disclosure::of(&part);
    println!("{page}");

    // 1. The excerpt is the participant's own records, verbatim, with a
    //    timestamp relative to the first — never the absolute stopwatch reading,
    //    which means nothing outside the session.
    assert!(
        page.contains("0.000 ms"),
        "the excerpt is not anchored at the first record"
    );
    assert!(
        page.contains("+8.00") && page.contains("-4.00"),
        "the excerpt does not carry the deltas the stream holds"
    );
    let excerpt_lines = page.lines().filter(|line| line.contains(" ms  ")).count();
    assert_eq!(
        excerpt_lines, EXCERPT,
        "the page printed {excerpt_lines} record(s) rather than {EXCERPT}"
    );
    assert!(
        page.contains(&format!("and {} more", MOTIONS - EXCERPT)),
        "the page does not say how many records it did not print, so a \
         participant would read {EXCERPT} as the whole of it"
    );

    // 2. The report rate, read off the stream rather than off the declaration.
    //    One millisecond exactly, which is the 1 kHz case docs/RISKS.md R14
    //    treats as the live one.
    assert!(
        page.contains("every 1.00 ms") && page.contains("roughly 1000 times a"),
        "the report rate was not derived from the stream"
    );

    // 3. The quantum: whole counts, so one.
    assert!(
        page.contains("is 1 count(s)"),
        "the device's resolution was not derived"
    );

    // 4. Path against displacement. 150 pairs of (+8, +1) and (-4, +1): the path
    //    is 150·(√65 + √17) ≈ 1828 counts and the net is (600, 300) ≈ 671, so
    //    about 63% of the travel was correction. The assertion is on the shape of
    //    the sentence and on the numbers being present, because the point is that
    //    they are *this* participant's.
    assert!(
        page.contains("Your hand travelled 1828 counts to move the cursor 671"),
        "the travel was not derived from the stream: the page says something \
         else, and a participant would be shown a number that is not theirs"
    );
    assert!(
        page.contains("63%"),
        "the overshoot fraction was not derived"
    );

    // 5. The reaction, which is the one that lands. Measured from the frame to
    //    the press, and never to the release.
    assert!(
        page.contains("was\n  214 ms"),
        "the reaction was not derived from the frame anchor: this is the \
         quantity the whole demonstration is for"
    );

    // …and the sentences the document promises are on the page.
    for promise in [
        "handwriting",
        "the same person",
        "Nothing on this page is a score",
        "wrote nothing",
        "no key",
    ] {
        assert!(
            page.to_lowercase().contains(&promise.to_lowercase()),
            "the disclosure page never says {promise:?}"
        );
    }
    println!("disclosure: {MOTIONS} motions, {EXCERPT} shown, four quantities derived");
}

/// **A crossing with nothing in it says so, in each of the four places, rather
/// than printing a number it does not have.**
///
/// The other half of the antecedent. A page that answered "1000 times a second"
/// for an empty stream would be worse than one that answered nothing, because a
/// participant has no way to tell a derivation from a template.
#[test]
fn a_crossing_with_nothing_in_it_derives_nothing_and_says_so() {
    let part = TelemetryPart {
        seat: sim::Seat::Blue0,
        stream: SeatStream {
            clock: Clock::Dequeue,
            platform: Platform::Linux,
            world_units_per_count_e6: 50_000,
            dropped: 0,
            samples: Vec::new(),
        },
    };
    let page = disclosure::of(&part);
    println!("{page}");
    assert!(page.contains("Not enough movement to read a report rate"));
    assert!(page.contains("there is no resolution to report"));
    assert!(page.contains("Nothing here to measure a reaction from"));
    assert!(
        !page.contains("times a second"),
        "an empty stream produced a report rate"
    );
}

/// A motion stream with no frame anchors cannot produce a reaction, and the page
/// says that rather than reaching for the nearest press.
#[test]
fn a_stream_with_no_frame_anchor_reports_no_reaction() {
    let mut part = a_crossing();
    part.stream
        .samples
        .retain(|sample| !matches!(sample.event, Event::Viewed { .. }));
    let page = disclosure::of(&part);
    assert!(
        page.contains("Nothing here to measure a reaction from"),
        "a reaction was computed with nothing to measure it from"
    );
    // …and the three quantities that do not need an anchor are still there, so
    // this test is about the anchor rather than about a page that gave up.
    assert!(page.contains("every 1.00 ms"));
    assert!(page.contains("is 1 count(s)"));
}
