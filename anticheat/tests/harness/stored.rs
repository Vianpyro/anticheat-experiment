//! Fabricating a *stored* match — a sealed replay and the session record beside
//! it — so that the calibration controls can be exercised rather than argued.
//!
//! # This is not a corpus and nothing here pretends it is
//!
//! Every match this builds is empty: nine seats, thirty ticks, no inputs. It
//! exists to drive [`anticheat::evaluate::Evaluation::basis`] through each of
//! its refusals and through the one path that succeeds, because a control that
//! has never been made to say yes is a control nobody has checked — the same
//! `docs/RISKS.md` R15 argument the exploit suite makes about an attack that has
//! never worked.
//!
//! What it must never become is a source of numbers. No detector's threshold is
//! fixed from anything here; `anticheat/tests/detectors.rs` asserts that every
//! shipped detector is uncalibrated, and this file's job is to show that the
//! gate which would let one through is real.

use replay::consent::ConsentVersion;
use replay::session::{
    Clock, Declared, Measured, Platform, SeatRecord, SessionRecord, Supervision,
};
use replay::split::{Split, split_of};
use replay::{MatchId, Pseudonym, Replay, SessionFacts, SigningKey};
use server::{Match, MatchConfig};
use sim::PLAYER_COUNT;

use anticheat::telemetry::MatchTelemetry;

/// Nine names, from the list of colours `docs/SCHEMA.md` §2 says an operator
/// chooses a pseudonym from.
pub const NINE: [&str; PLAYER_COUNT] = [
    "amber", "beryl", "cobalt", "damson", "ecru", "flax", "garnet", "hazel", "indigo",
];

/// A stored match: a sealed replay, a session record, and the telemetry the two
/// of them make.
#[must_use]
pub fn stored(
    identifier: MatchId,
    people: &[&str],
    supervision: Supervision,
    degraded: bool,
) -> MatchTelemetry {
    let mut game = Match::new(MatchConfig {
        seed: 0x5710_2ED0_0000_0000,
        players: people.len(),
    });
    for _ in 0..people.len() {
        let (seat, _) = game.join();
        let seat = seat.expect("a seat was granted");
        game.deliver(
            seat,
            protocol::ClientFrame::encode(&protocol::ClientMessage::Ready)
                .as_bytes()
                .as_slice(),
            0,
        )
        .expect("ready was accepted");
    }
    for _ in 0..30 {
        let _ = game.tick();
    }

    let mut participants = [const { None }; PLAYER_COUNT];
    for (index, name) in people.iter().enumerate() {
        participants[index] = Some(Pseudonym::parse(name).expect("a pseudonym"));
    }
    let key = SigningKey::from_seed(*b"moba m8 calibration harness key\0");
    let replay: Replay = replay::seal(
        &game.recording(),
        &SessionFacts {
            match_id: identifier,
            started_at_unix_ms: 1_786_000_000_000,
            participants,
            sim_commit: replay::SimCommit::Unknown,
            telemetry: replay::Commitment::Absent,
        },
        &key,
    );

    let declared = Declared {
        device_profile_id: replay::DeviceProfileId::parse("mouse-a").expect("a device label"),
        device_cpi: 800,
        device_polling_hz: 1000,
        pointer_acceleration: false,
    };
    let measured = Measured {
        platform: Platform::Linux,
        clock: Clock::Dequeue,
        world_units_per_count_e6: 50_000,
        samples: 12_000,
        motions: 11_000,
        coincident: 0,
        median_gap_ns: 8_000_000,
        budget_ns: 33_000_000,
        passes: 1000,
        // The one field that decides the degradation half of the stratum.
        passes_over_budget: u64::from(degraded),
        worst_overrun_ns: if degraded { 4_000_000 } else { 0 },
        worst_pass_ns: 2_000_000,
    };
    let mut seats = [const { SeatRecord::Empty }; PLAYER_COUNT];
    for slot in seats.iter_mut().take(people.len()) {
        *slot = SeatRecord::Human {
            declared: declared.clone(),
            measured,
            calibration: replay::calibration::SeatCalibration::absent(),
        };
    }

    let session = SessionRecord {
        match_id: identifier,
        consent_version: ConsentVersion::current(),
        recorded_on: "2026-08-14".to_owned(),
        supervision,
        seats,
    };

    MatchTelemetry::from_corpus(&replay, &session).expect("the two files describe one match")
}

/// A match this repository generated, wrapped as the exploit suite wraps one.
///
/// Nine bots sending one intention a tick for two hundred ticks. It has to
/// **score** rather than merely exist: a synthetic group nothing scored would
/// let the calibration control pass by refusing empty groups instead of
/// synthetic ones, which is the wrong refusal wearing the right message. The
/// bots do nothing interesting — an idle intention with an advancing clock is
/// enough for a rate to be measurable.
#[must_use]
pub fn synthetic_match(label: &'static str) -> MatchTelemetry {
    let mut game = Match::new(MatchConfig {
        seed: 0x5B07_5EED_0000_0000,
        players: PLAYER_COUNT,
    });
    for _ in 0..PLAYER_COUNT {
        let (seat, _) = game.join();
        let seat = seat.expect("a seat was granted");
        game.deliver(
            seat,
            protocol::ClientFrame::encode(&protocol::ClientMessage::Ready)
                .as_bytes()
                .as_slice(),
            0,
        )
        .expect("ready was accepted");
    }
    let mut bots: Vec<cheat_client::bot::Bot> = (0..PLAYER_COUNT)
        .map(|_| cheat_client::bot::Bot::new())
        .collect();
    for tick in 0..200u64 {
        let at = tick.saturating_mul(33);
        for seat in sim::Seat::ALL {
            let frame = bots[seat.index()].intend(protocol::Action::Idle, at);
            game.deliver(seat, frame.as_bytes().as_slice(), at)
                .expect("the server accepted a bot intention");
        }
        let _ = game.tick();
    }
    MatchTelemetry::synthetic(&game.recording(), label)
}

/// `count` match identifiers that all fall in the same half of the frozen
/// split.
///
/// The split is a hash of the identifier (`replay::split`), so a set of matches
/// that share a group has to be searched for rather than constructed — which is
/// the property that makes the split worth having: it cannot be steered by
/// whoever names the matches.
#[must_use]
pub fn identifiers_in(half: Split, count: usize) -> Vec<MatchId> {
    let mut found = Vec::new();
    let mut index = 0u64;
    while found.len() < count && index < 10_000 {
        // Sixteen bytes exactly: seven of prefix and nine of index. It was
        // seventeen and truncated to sixteen the first time, so ten
        // consecutive indices produced one identifier — twenty matches became
        // two, and the bound the report printed was `3/2`. A helper that
        // silently returns fewer distinct things than it was asked for is
        // `docs/RISKS.md` R15 in a fixture builder.
        let mut bytes = *b"stored-000000000";
        let text = format!("{index:09}");
        bytes[7..16].copy_from_slice(text.as_bytes());
        let identifier = MatchId(bytes);
        if split_of(identifier) == half {
            found.push(identifier);
        }
        index = index.saturating_add(1);
    }
    found
}
