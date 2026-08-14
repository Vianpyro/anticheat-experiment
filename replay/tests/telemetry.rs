//! The telemetry companion, and the nine ways it is refused.
//!
//! # What this file is the criterion for
//!
//! `docs/MILESTONES.md` M5 asked for a table of tamper cases each rejected with a
//! distinct error, and `replay/tests/tamper.rs` is that table for the replay. The
//! companion is a second sealed file with a second signature and a commitment
//! between them, so it gets its own table — and the two rows that do not exist
//! for a replay are the interesting ones:
//!
//! - **A companion attached to a replay that named none.** Absence is a *signed*
//!   state (`replay::manifest::Commitment::Absent`), which is what turns "this
//!   match recorded no telemetry" from a missing file into a claim an attacker
//!   cannot quietly upgrade.
//! - **A second companion for the same match, honestly sealed by a key the
//!   registry accepts.** Nothing in that file is false. Every check but the last
//!   passes. What refuses it is that the replay named thirty-two other bytes
//!   before the attacker arrived, and
//!   [`the_substitution_the_commitment_exists_for`] is that case executed.
//!
//! # The attacker holds a key, for the reason `tamper.rs` gives
//!
//! An attacker who cannot re-sign is caught by the signature every time, so a
//! table built that way would have nine rows and one answer. Every row below
//! except the two signature rows is therefore **re-sealed with a key the registry
//! accepts**, and what catches each is a different check.

#![deny(unsafe_code)]

use replay::keys::{KeyRegistry, KeyStatus, SigningKey};
use replay::manifest::{Commitment, MatchId, Pseudonym, SessionFacts, SimCommit};
use replay::session::{Clock, Platform};
use replay::telemetry::{
    Control, Event, Sample, SeatStream, Telemetry, TelemetryError, TelemetryLog, TelemetryManifest,
};
use replay::{Build, Recording, Replay, TimedInput};
use sim::{Action, Digest, FxVec2, Input, PLAYER_COUNT, Seat, Tick, rules_hash};

/// The operator's key. Written down rather than generated: Ed25519 signing is
/// deterministic, so a sealed file is a function of its contents and two runs of
/// this suite produce the same bytes.
const OPERATOR: [u8; 32] = *b"moba telemetry operator key\0\0\0\0\0";

/// A second key the registry also accepts, for the row where the companion and
/// the replay were sealed by different accepted keys — which is a real mistake
/// (two servers, one corpus) and not only an attack.
const OTHER: [u8; 32] = *b"moba telemetry other operator\0\0\0";

fn operator() -> SigningKey {
    SigningKey::from_seed(OPERATOR)
}

fn other() -> SigningKey {
    SigningKey::from_seed(OTHER)
}

/// A registry that accepts both keys, so that `UnknownKey` is reachable only by
/// a third one and `Identity` is reachable at all.
fn registry() -> KeyRegistry {
    let mut keys = KeyRegistry::new();
    keys.insert(operator().verifying(), KeyStatus::Active, "operator");
    keys.insert(other().verifying(), KeyStatus::Active, "other-operator");
    keys
}

fn match_id() -> MatchId {
    MatchId(*b"telemetry-match!")
}

/// Two seats, three record kinds each.
///
/// `docs/RISKS.md` R15: the assertions below are about an encoding and a set of
/// checks, and a fixture holding one kind of record would exercise a third of
/// them. [`the_fixture_reaches_every_record_kind_and_both_seats`] is the floor.
fn a_log() -> TelemetryLog {
    let mut log = TelemetryLog::new();
    let stream = |seat: usize, dropped: u64| SeatStream {
        clock: Clock::Dequeue,
        platform: Platform::Linux,
        world_units_per_count_e6: 50_000,
        dropped,
        samples: (0..12u64)
            .map(|index| Sample {
                at_ns: 1_000_000 * (index + 1) + seat as u64,
                event: match index % 3 {
                    0 => Event::Moved {
                        dx: (index as f64) * 0.25,
                        dy: -(index as f64) * 0.5,
                    },
                    1 => Event::Pressed {
                        control: Control::Move,
                        down: index % 2 == 1,
                    },
                    _ => Event::Viewed {
                        tick: Tick(index as u32),
                        seq: index as u32,
                    },
                },
            })
            .collect(),
    };
    log.seats[1] = Some(stream(1, 0));
    log.seats[4] = Some(stream(4, 3));
    log
}

/// A recording nobody needs to look at: this file is about the companion, and a
/// replay is here to be the thing that commits to one.
fn a_recording() -> Recording {
    let inputs: Vec<TimedInput> = (0..10u32)
        .map(|tick| TimedInput {
            input: Input {
                tick: Tick(tick),
                seq: tick,
                player: Seat::Blue1,
                action: Action::Move(FxVec2::new(sim::Fx::from_int(10), sim::Fx::from_int(0))),
            },
            claimed_at_ms: 1_786_000_000_000 + u64::from(tick),
            received_at_ms: 1_786_000_000_007 + u64::from(tick),
        })
        .collect();
    let reached = replay::resimulate(0x5EED, 10, &inputs);
    Recording {
        seed: 0x5EED,
        rules_hash: rules_hash(),
        ticks: 10,
        outcome: reached.outcome(),
        final_state_digest: reached.digest(),
        inputs,
    }
}

/// The seats the companion covers are the seats the replay names, because
/// `replay::telemetry::verify` requires it.
fn facts(telemetry: Commitment) -> SessionFacts {
    let mut participants: [Option<Pseudonym>; PLAYER_COUNT] = [const { None }; PLAYER_COUNT];
    participants[1] = Pseudonym::parse("alizarin");
    participants[4] = Pseudonym::parse("bistre");
    SessionFacts {
        match_id: match_id(),
        started_at_unix_ms: 1_786_000_000_000,
        participants,
        sim_commit: SimCommit::Unknown,
        telemetry,
    }
}

/// A sealed pair: the companion, and the replay that commits to it.
fn a_pair() -> (Replay, Telemetry) {
    let key = operator();
    let companion = replay::telemetry::seal(&a_log(), &facts(Commitment::Absent), &key);
    let replay = replay::seal(
        &a_recording(),
        &facts(Commitment::Sealed(companion.digest())),
        &key,
    );
    (replay, companion)
}

/// Re-seals a companion whose manifest was edited, with a key the registry
/// accepts. Without this every row below is `Signature`.
fn reseal(
    mut companion: Telemetry,
    key: &SigningKey,
    edit: impl FnOnce(&mut Telemetry),
) -> Telemetry {
    edit(&mut companion);
    companion.manifest.server_identity = key.verifying();
    companion.signature = key.sign(&replay::telemetry::signed_bytes(&companion.manifest));
    companion
}

// ---------------------------------------------------------------------------
// The criterion
// ---------------------------------------------------------------------------

/// **A genuine companion verifies, and the replay verifies with or without it.**
///
/// Asserted before the table runs, in the same spirit `replay/tests/tamper.rs`
/// accepts a genuine replay first: a suite in which everything is refused would
/// otherwise pass by refusing everything.
#[test]
fn a_genuine_companion_verifies_against_the_replay_that_names_it() {
    let (replay, companion) = a_pair();

    let verified =
        replay::verify(&replay, &registry(), &Build::current()).expect("the replay did not verify");
    assert_eq!(
        verified.telemetry,
        Commitment::Sealed(companion.digest()),
        "the replay does not report the companion it committed to"
    );

    let telemetry = replay::telemetry::verify(&replay, &companion, &registry())
        .expect("the companion did not verify");
    assert_eq!(telemetry.match_id, match_id());
    assert_eq!(telemetry.signer, operator().verifying());
    assert!(!telemetry.retired);
    assert_eq!(telemetry.samples, 16, "eight device events per seat, twice");
    assert_eq!(telemetry.motions, 8);

    // …and the round trip through the reader, because every claim above is about
    // a file somebody hands you.
    let bytes = companion.encode();
    let read = Telemetry::decode(&bytes).expect("the companion did not decode");
    assert_eq!(read, companion, "the reader disagrees with the writer");
    assert_eq!(read.encode(), bytes);

    println!(
        "telemetry: {} bytes, {} device event(s), {} motion(s), {} seat(s)",
        bytes.len(),
        telemetry.samples,
        telemetry.motions,
        companion.manifest.occupied().len()
    );
}

/// **A replay with no companion is a complete replay, and says so.**
///
/// The state `replay::manifest::Commitment::Absent` names. A verifier that
/// treated it as a failure would be reporting an error on every development run
/// and on every match nobody was recording, which teaches a reader to ignore the
/// error — and `docs/SCHEMA.md` §11 is explicit that the absence is legitimate.
#[test]
fn a_replay_that_recorded_no_telemetry_verifies_and_names_the_absence() {
    let replay = replay::seal(&a_recording(), &facts(Commitment::Absent), &operator());
    let verified = replay::verify(&replay, &registry(), &Build::current())
        .expect("a replay with no companion did not verify");
    assert_eq!(verified.telemetry, Commitment::Absent);
    println!("telemetry: absent, and the replay verifies: {verified:?}");
}

/// One row of the table.
struct Row {
    /// What the attacker did.
    what: &'static str,
    /// The file they produced.
    make: fn(Telemetry, Replay) -> (Telemetry, Replay),
    /// The check that catches them.
    caught: fn(&TelemetryError) -> bool,
}

/// **The table.** Nine attackers, nine answers.
///
/// Each row is a *different check*, and the ordering in
/// `replay::telemetry::verify` is what makes them nine rather than one repeated:
/// every one catches the attacker who stopped one step short of the next.
#[test]
#[expect(clippy::too_many_lines, reason = "one table, read as one table")]
fn every_tamper_case_is_refused_by_a_different_check() {
    let rows = [
        Row {
            what: "attaches a companion to a replay that names none",
            make: |companion, _| {
                let replay = replay::seal(&a_recording(), &facts(Commitment::Absent), &operator());
                (companion, replay)
            },
            caught: |error| matches!(error, TelemetryError::NotCommitted),
        },
        Row {
            what: "seals the companion with a key the registry does not hold",
            make: |companion, replay| {
                let stranger = SigningKey::from_seed(*b"a stranger nobody has heard of!\0");
                (reseal(companion, &stranger, |_| {}), replay)
            },
            caught: |error| matches!(error, TelemetryError::UnknownKey(_)),
        },
        Row {
            what: "edits the manifest and cannot re-sign",
            make: |mut companion, replay| {
                companion.manifest.started_at_unix_ms += 1;
                (companion, replay)
            },
            caught: |error| matches!(error, TelemetryError::Signature),
        },
        Row {
            what: "re-signs a stream that is not the one the manifest names",
            make: |companion, replay| {
                (
                    reseal(companion, &operator(), |companion| {
                        if let Some(stream) = companion.log.seats[1].as_mut()
                            && let Some(sample) = stream.samples.first_mut()
                        {
                            sample.at_ns += 1;
                        }
                    }),
                    replay,
                )
            },
            caught: |error| matches!(error, TelemetryError::Stream { .. }),
        },
        Row {
            what: "re-signs a seat entry that miscounts its own records",
            make: |companion, replay| {
                (
                    reseal(companion, &operator(), |companion| {
                        // The digest still covers the body, so the body is left
                        // alone: what moves is the *claim* about it. One fewer
                        // motion and one more press keeps the record count — and
                        // therefore the layout — intact, which is what makes this
                        // a distinct answer from `Stream` rather than a second
                        // way of reaching it.
                        if let Some(seat) = companion.manifest.seats[1].as_mut() {
                            seat.motions -= 1;
                        }
                    }),
                    replay,
                )
            },
            caught: |error| matches!(error, TelemetryError::Counts { seat: 1, .. }),
        },
        Row {
            what: "seals the companion with the other operator's accepted key",
            make: |companion, replay| (reseal(companion, &other(), |_| {}), replay),
            caught: |error| matches!(error, TelemetryError::Identity { .. }),
        },
        Row {
            what: "re-signs a companion that names another match",
            make: |companion, replay| {
                (
                    reseal(companion, &operator(), |companion| {
                        companion.manifest.match_id = MatchId(*b"another-match!!!");
                    }),
                    replay,
                )
            },
            caught: |error| matches!(error, TelemetryError::Match { .. }),
        },
        Row {
            what: "seals a companion covering a seat the replay does not name",
            make: |_, replay| {
                // Sealed rather than edited, so that the stream digest and the
                // counts are the ones this companion's own manifest should
                // carry: an attacker who only *edited* a seat in would be caught
                // by `Stream` one step earlier, which is the check working and
                // not the case under test.
                let mut log = a_log();
                log.seats[7] = log.seats[1].clone();
                (
                    replay::telemetry::seal(&log, &facts(Commitment::Absent), &operator()),
                    replay,
                )
            },
            caught: |error| matches!(error, TelemetryError::Seats { .. }),
        },
        Row {
            what: "seals a second, smoother companion for the same match",
            make: |_, replay| {
                let mut log = a_log();
                if let Some(stream) = log.seats[1].as_mut()
                    && let Some(sample) = stream
                        .samples
                        .iter_mut()
                        .find(|sample| matches!(sample.event, Event::Moved { .. }))
                {
                    sample.event = Event::Moved { dx: 0.0, dy: 0.0 };
                }
                let smoothed =
                    replay::telemetry::seal(&log, &facts(Commitment::Absent), &operator());
                (smoothed, replay)
            },
            caught: |error| matches!(error, TelemetryError::Substituted { .. }),
        },
    ];

    let (genuine_replay, genuine) = a_pair();
    let mut seen: Vec<String> = Vec::new();
    for row in rows {
        let (companion, replay) = (row.make)(genuine.clone(), genuine_replay.clone());
        let error = replay::telemetry::verify(&replay, &companion, &registry())
            .map(|_| ())
            .expect_err(&format!("an attacker who {} was accepted", row.what));
        assert!(
            (row.caught)(&error),
            "an attacker who {} was refused by the wrong check: {error:?}",
            row.what
        );
        let name = format!("{error:?}");
        let variant = name.split_once(' ').map_or(name.clone(), |(head, _)| {
            head.trim_end_matches('{').to_owned()
        });
        assert!(
            !seen.contains(&variant),
            "two rows are caught by {variant}, so the table has fewer answers than rows"
        );
        seen.push(variant);
        println!("telemetry: an attacker who {} — {error}", row.what);
    }
    assert_eq!(
        seen.len(),
        9,
        "the table does not exercise nine distinct checks"
    );
}

/// **The substitution the commitment exists for**, stated on its own because it
/// is the reason the replay carries a digest at all.
///
/// The attacker holds the operator's key. The companion they produce is sealed
/// correctly, names this match, covers these seats, and every number in it agrees
/// with every other. Nothing in that file is false. It is refused because the
/// replay committed to different bytes, which is a claim made before the attacker
/// arrived and one no amount of internal consistency reaches.
#[test]
fn the_substitution_the_commitment_exists_for() {
    let (replay, genuine) = a_pair();

    let mut log = a_log();
    if let Some(stream) = log.seats[4].as_mut() {
        for sample in &mut stream.samples {
            if let Event::Moved { dx, dy } = sample.event {
                // A hand that never overshot: the shape a curvature detector at
                // M8 would be reading, replaced with one that scores better.
                sample.event = Event::Moved {
                    dx: dx * 0.5,
                    dy: dy * 0.5,
                };
            }
        }
    }
    let smoothed = replay::telemetry::seal(&log, &facts(Commitment::Absent), &operator());

    // It is internally perfect: it verifies against a replay that commits to
    // *it*, which is what makes the refusal below about the binding rather than
    // about the file.
    let its_own_replay = replay::seal(
        &a_recording(),
        &facts(Commitment::Sealed(smoothed.digest())),
        &operator(),
    );
    replay::telemetry::verify(&its_own_replay, &smoothed, &registry())
        .expect("the smoothed companion is not internally consistent");

    let error = replay::telemetry::verify(&replay, &smoothed, &registry())
        .expect_err("a smoothed companion was accepted for a replay that named another");
    let TelemetryError::Substituted { claimed, computed } = error else {
        panic!("refused for the wrong reason: {error:?}");
    };
    assert_eq!(claimed, genuine.digest());
    assert_ne!(computed, claimed);
    println!("telemetry: {claimed} was named, {computed} was offered");
}

/// A companion whose match, key and seats are all somebody else's is refused
/// before the digest is reached, which is what makes the errors useful.
#[test]
fn the_companion_of_another_match_is_named_as_such() {
    let (replay, _) = a_pair();
    let mut elsewhere = facts(Commitment::Absent);
    elsewhere.match_id = MatchId(*b"a different one!");
    let companion = replay::telemetry::seal(&a_log(), &elsewhere, &operator());

    let error = replay::telemetry::verify(&replay, &companion, &registry())
        .expect_err("the telemetry of another match was accepted");
    assert!(matches!(error, TelemetryError::Match { .. }));
    println!("telemetry: {error}");
}

/// The reader is total on every byte string.
///
/// `replay::Replay::decode`'s obligation, one file over: a companion is something
/// a third party hands you, and this milestone's whole subject is what happens
/// when they hand you a tampered one.
#[test]
fn the_reader_refuses_every_kind_of_byte_string_it_should() {
    let (_, companion) = a_pair();
    let genuine = companion.encode();

    assert!(matches!(
        Telemetry::decode(b"not a telemetry file at all"),
        Err(replay::telemetry::ReadError::NotTelemetry)
    ));
    assert!(matches!(
        Telemetry::decode(&[]),
        Err(replay::telemetry::ReadError::Malformed)
    ));

    let mut wrong_format = genuine.clone();
    wrong_format[9] = 99;
    assert!(matches!(
        Telemetry::decode(&wrong_format),
        Err(replay::telemetry::ReadError::UnsupportedFormat(99))
    ));

    let mut trailing = genuine.clone();
    trailing.push(0);
    assert!(matches!(
        Telemetry::decode(&trailing),
        Err(replay::telemetry::ReadError::TrailingBytes)
    ));

    let mut truncated = genuine.clone();
    truncated.truncate(genuine.len() - 1);
    assert!(matches!(
        Telemetry::decode(&truncated),
        Err(replay::telemetry::ReadError::Malformed)
    ));

    // A non-finite delta. A device does not report one, so a file that holds one
    // is not a file this build reads — and it is refused at the *reader* rather
    // than filtered at the writer, because a predicate on the contents of a
    // record is `docs/RISKS.md` R14's mistake with a better excuse.
    let mut nan = genuine.clone();
    let body_at = genuine.len() - 24 * replay::telemetry::SAMPLE_BYTES;
    nan[body_at..body_at + 1].copy_from_slice(&[0]);
    nan[body_at + 9..body_at + 17].copy_from_slice(&f64::NAN.to_bits().to_be_bytes());
    assert!(
        matches!(
            Telemetry::decode(&nan),
            Err(replay::telemetry::ReadError::Malformed)
        ),
        "a NaN device delta decoded"
    );

    // And a non-zero byte in a record's padding, which would give one sample two
    // encodings and stop the stream digest being a function of the stream.
    let mut padded = genuine.clone();
    let last = genuine.len() - 1;
    padded[last] = 1;
    assert!(matches!(
        Telemetry::decode(&padded),
        Err(replay::telemetry::ReadError::Malformed)
    ));

    println!("telemetry: the reader refused seven byte strings and accepted one");
}

/// A manifest that decodes is one this build wrote, field for field.
#[test]
fn the_manifest_round_trips() {
    let (_, companion) = a_pair();
    let bytes = companion.manifest.encode();
    assert_eq!(bytes.len(), replay::telemetry::TELEMETRY_MANIFEST_BYTES);
    let read = TelemetryManifest::decode(&mut replay::ByteReader::new(&bytes))
        .expect("the manifest did not decode");
    assert_eq!(read, companion.manifest);

    // The width does not report how many seats were traced, which is the same
    // reasoning the replay manifest's participant slots carry.
    let empty = replay::telemetry::seal(
        &TelemetryLog::new(),
        &facts(Commitment::Absent),
        &operator(),
    );
    assert_eq!(
        empty.manifest.encode().len(),
        bytes.len(),
        "the manifest's length follows how many seats it covers"
    );
}

/// `docs/RISKS.md` R15: the fixture reaches every case the assertions are about.
#[test]
fn the_fixture_reaches_every_record_kind_and_both_seats() {
    let log = a_log();
    let mut moved = 0u32;
    let mut pressed = 0u32;
    let mut viewed = 0u32;
    for stream in log.seats.iter().flatten() {
        for sample in &stream.samples {
            match sample.event {
                Event::Moved { .. } => moved += 1,
                Event::Pressed { .. } => pressed += 1,
                Event::Viewed { .. } => viewed += 1,
            }
        }
    }
    println!(
        "telemetry fixture: {moved} motion(s), {pressed} press(es), {viewed} view anchor(s), {} seat(s)",
        log.occupied().len()
    );
    assert!(moved > 0 && pressed > 0 && viewed > 0);
    assert_eq!(log.occupied(), vec![1, 4]);
    assert!(
        log.seats.iter().flatten().any(|seat| seat.dropped > 0),
        "no seat in the fixture dropped anything"
    );
}

/// A view anchor is not a device event, and the count that guards the corpus
/// against a headless client knows the difference.
///
/// `docs/SCHEMA.md` §6 refuses a seat that recorded zero device events, and a
/// headless client **receives views**. If the anchors were counted among the
/// device events, a client that touched no mouse would produce a stream of thirty
/// records a second and walk through the one mechanical defence the corpus has.
#[test]
fn a_view_anchor_is_not_a_device_event() {
    let mut log = TelemetryLog::new();
    log.seats[0] = Some(SeatStream {
        clock: Clock::Dequeue,
        platform: Platform::Linux,
        world_units_per_count_e6: 50_000,
        dropped: 0,
        samples: (0..100u64)
            .map(|index| Sample {
                at_ns: index * 33_333_333,
                event: Event::Viewed {
                    tick: Tick(index as u32),
                    seq: index as u32,
                },
            })
            .collect(),
    });
    let facts = log.seats[0]
        .as_ref()
        .expect("the seat was just filled")
        .facts();
    assert_eq!(
        facts.samples, 0,
        "a hundred view anchors counted as device events, so a headless client \
         reads as a person"
    );
    assert_eq!(facts.views, 100);
    assert_eq!(facts.records(), 100);
    println!(
        "telemetry: {} anchors, {} device events — a silent seat stays silent",
        facts.views, facts.samples
    );
}

/// The commitment is inside the signature, so a replay's telemetry claim cannot
/// be edited for free.
#[test]
fn the_commitment_is_inside_the_replay_signature() {
    let (replay, companion) = a_pair();
    let mut edited = replay.clone();
    edited.manifest.telemetry = Commitment::Absent;
    let error = replay::verify(&edited, &registry(), &Build::current())
        .expect_err("a replay whose telemetry commitment was edited verified");
    assert!(matches!(error, replay::VerifyError::Signature));

    // And with a re-seal, the claim is a different claim rather than a free one:
    // the companion is then refused as one nobody named.
    let resealed = replay::seal(&a_recording(), &facts(Commitment::Absent), &operator());
    assert!(matches!(
        replay::telemetry::verify(&resealed, &companion, &registry()),
        Err(TelemetryError::NotCommitted)
    ));
    println!("telemetry: {error}, and then NotCommitted");
}

/// A retired key still verifies what it sealed, exactly as it does for a replay.
///
/// `docs/RISKS.md` R4: rotating without keeping the retired key published orphans
/// every file it sealed, and the companion is a file it sealed.
#[test]
fn a_retired_key_still_verifies_the_companion_it_sealed() {
    let (replay, companion) = a_pair();
    let mut keys = KeyRegistry::new();
    keys.insert(operator().verifying(), KeyStatus::Retired, "rotated-away");
    let verified = replay::telemetry::verify(&replay, &companion, &keys)
        .expect("a retired key's companion was refused");
    assert!(verified.retired);
    println!("telemetry: sealed by a retired key, and it still verifies");
}

/// The digest a replay commits to covers the companion's *manifest* as well as
/// its stream.
///
/// A commitment over the stream alone would let an attacker keep every record and
/// change what the file says about them — its match, its identity, its counts —
/// while the replay's commitment still held. This is the assertion that the
/// digest is over the whole file.
#[test]
fn the_commitment_covers_the_companion_manifest_and_not_only_its_stream() {
    let (_, companion) = a_pair();
    let body = companion.log.body();

    let mut elsewhere = facts(Commitment::Absent);
    elsewhere.started_at_unix_ms += 1;
    let moved = replay::telemetry::seal(&a_log(), &elsewhere, &operator());

    assert_eq!(
        moved.log.body(),
        body,
        "the two companions do not share a stream, so this proves nothing"
    );
    assert_ne!(
        moved.digest(),
        companion.digest(),
        "two companions with the same stream and different manifests hash alike, \
         so a replay's commitment does not cover what the file says about its own \
         records"
    );
    println!(
        "telemetry: one stream, two manifests, two digests: {} and {}",
        companion.digest(),
        moved.digest()
    );
}

/// A record is twenty-five bytes whatever it holds, so a file's length is a
/// function of how many events it carries rather than of which ones.
#[test]
fn every_record_is_the_same_width() {
    let widths: Vec<usize> = [
        Event::Moved { dx: 1.0, dy: -1.0 },
        Event::Pressed {
            control: Control::Stop,
            down: true,
        },
        Event::Viewed {
            tick: Tick(7),
            seq: 9,
        },
    ]
    .into_iter()
    .map(|event| {
        let mut log = TelemetryLog::new();
        log.seats[0] = Some(SeatStream {
            clock: Clock::Dequeue,
            platform: Platform::Linux,
            world_units_per_count_e6: 50_000,
            dropped: 0,
            samples: vec![Sample { at_ns: 1, event }],
        });
        log.body().len()
    })
    .collect();
    assert_eq!(
        widths,
        vec![
            replay::telemetry::SAMPLE_BYTES,
            replay::telemetry::SAMPLE_BYTES,
            replay::telemetry::SAMPLE_BYTES
        ],
        "a record's width follows its kind"
    );

    // …and the arithmetic `docs/SCHEMA.md` §11's budget is computed from.
    let per_second_at_125_hz = (125 + 30 + 10) * replay::telemetry::SAMPLE_BYTES * PLAYER_COUNT;
    println!(
        "telemetry: {} bytes per record; nine seats at 125 Hz cost {} bytes a \
         second, {:.1} MiB in twenty minutes",
        replay::telemetry::SAMPLE_BYTES,
        per_second_at_125_hz,
        (per_second_at_125_hz * 1200) as f64 / (1024.0 * 1024.0)
    );
}

/// The stream digest is a function of the whole log and of the order in it.
#[test]
fn the_stream_digest_follows_the_order_of_the_records() {
    let mut log = a_log();
    let before = Digest::from_bytes(*sim::digest_bytes(&log.body()).as_bytes());
    if let Some(stream) = log.seats[1].as_mut() {
        stream.samples.swap(0, 1);
    }
    let after = sim::digest_bytes(&log.body());
    assert_ne!(before, after, "swapping two records left the digest alone");
    println!("telemetry: {before} became {after} when two records swapped places");
}
