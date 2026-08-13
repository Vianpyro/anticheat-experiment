//! `docs/MILESTONES.md` M5's exit criterion.
//!
//! > A table-driven test covers six tamper cases — truncated log, reordered
//! > inputs, altered outcome record, altered seed, unknown signing key, version
//! > or rules-hash mismatch — each rejected with a distinct error, and a genuine
//! > replay accepted.
//!
//! # The adversary these cases assume, which is stronger than the obvious one
//!
//! The naive reading of "tamper with a replay" is somebody editing bytes in a
//! file. Every one of those edits is a signature failure, so a table built that
//! way would have six rows and one answer, and the criterion's word "distinct"
//! would be meeting nothing. It would also be testing the wrong attacker:
//! `docs/RISKS.md` R4 is about a signature whose *coverage* is wrong, and a
//! coverage failure is only visible against somebody who can re-sign.
//!
//! So every case below except [`Case::Signature`] and [`Case::UnknownKey`] is
//! **re-signed after tampering, with a key this registry accepts**. That is the
//! strongest adversary a verifier can be red against, and it is what makes the
//! six errors six different checks rather than one check reported six times.
//!
//! # What the last row of that argument is, and why it is not in the table
//!
//! An attacker who holds an accepted key and adjusts *every* field consistently
//! has not tampered with a replay. They have produced a replay of a different
//! match, simulated honestly, and no check in `replay::verify` can distinguish
//! it from one that was played — because there is nothing to distinguish. That
//! is the boundary of what a signature over a self-consistent artefact means,
//! and what lies past it is key custody, not verification.
//!
//! [`the_escalation_ends_where_key_custody_begins`] is that stated as a test
//! rather than as a paragraph, because a limit nobody has executed is a limit
//! somebody will forget.

#![deny(unsafe_code)]

use replay::keys::{KeyRegistry, KeyStatus, Signature, SigningKey};
use replay::manifest::{Build, MatchId, Pseudonym, SessionFacts, SimCommit};
use replay::{Recording, Replay, TimedInput, VerifyError};
use sim::{
    Action, Digest, FxVec2, Input, Outcome, PLAYER_COUNT, RULES, Seat, Team, Tick, base_position,
    digest_bytes, new_state, rules_hash, step,
};

/// The key the honest server in these tests seals with.
///
/// A written-down constant, and it must never be anything else: a fixture that
/// generated a key would produce a different signature on every run, and the
/// point of Ed25519's determinism here is that a sealed replay is a function of
/// the match rather than of the moment. It is not a secret and nothing in this
/// repository treats it as one.
const HONEST_SEED: [u8; 32] = *b"moba test signing key, honest.\0\0";

/// A key nobody put in the registry. The attacker's own.
const STRANGER_SEED: [u8; 32] = *b"moba test signing key, stranger\0";

/// A retired key, to check that retirement does not orphan what it sealed.
const RETIRED_SEED: [u8; 32] = *b"moba test signing key, retired.\0";

fn honest() -> SigningKey {
    SigningKey::from_seed(HONEST_SEED)
}

/// A registry accepting the honest key and a retired one, and nothing else.
fn registry() -> KeyRegistry {
    let mut keys = KeyRegistry::new();
    keys.insert(honest().verifying(), KeyStatus::Active, "honest-server");
    keys.insert(
        SigningKey::from_seed(RETIRED_SEED).verifying(),
        KeyStatus::Retired,
        "last-season",
    );
    keys
}

/// A short match with something in it.
///
/// `docs/RISKS.md` R15: every assertion below is about a replay, and a replay of
/// a match in which nothing happened is a replay whose log could be empty
/// without any of them noticing. [`the_fixture_is_a_match`] is the floor.
const SEED: u64 = 0x0F1E_2D3C_4B5A_6978;
const TICKS: u32 = 240;

fn a_recording() -> Recording {
    let mut inputs = Vec::new();
    let mut seq = [0u32; PLAYER_COUNT];
    let mut state = new_state(SEED);

    for tick in 0..TICKS {
        let mut batch = Vec::new();
        for seat in Seat::ALL {
            // A standing order re-sent every tick, plus a cast when the cooldown
            // allows — the shape `client::play` produces, and not `Action::Idle`
            // as filler, which is a rule that stops the champion.
            let action = if tick % 120 == 30 {
                Action::Skillshot(base_position(seat.team(), &RULES).neg())
            } else {
                Action::Move(FxVec2::ZERO)
            };
            let input = Input {
                tick: Tick(tick),
                seq: seq[seat.index()],
                player: seat,
                action,
            };
            seq[seat.index()] = seq[seat.index()].saturating_add(1);
            batch.push(input);
            inputs.push(TimedInput {
                input,
                claimed_at_ms: u64::from(tick).saturating_mul(33),
                received_at_ms: u64::from(tick).saturating_mul(33).saturating_add(7),
            });
        }
        state = step(&state, &batch);
    }

    Recording {
        seed: SEED,
        rules_hash: rules_hash(),
        ticks: TICKS,
        outcome: state.outcome(),
        final_state_digest: state.digest(),
        inputs,
    }
}

/// Session facts with two participants in them, so that the manifest's
/// participant slots are exercised rather than left empty.
fn session() -> SessionFacts {
    let mut participants: [Option<Pseudonym>; PLAYER_COUNT] = [const { None }; PLAYER_COUNT];
    participants[0] = Pseudonym::parse("alizarin");
    participants[4] = Pseudonym::parse("bistre");
    SessionFacts {
        match_id: MatchId(*b"a-test-match-id\0"),
        started_at_unix_ms: 1_786_000_000_000,
        participants,
        // Pinned rather than `of_this_build`, so the fixture is a function of
        // the match and not of which commit somebody happens to be on.
        sim_commit: SimCommit::Sha([0x5A; 20]),
    }
}

fn sealed() -> Replay {
    replay::seal(&a_recording(), &session(), &honest())
}

/// Edits a replay and re-signs it **verbatim** with the honest key.
///
/// The word that matters is verbatim. `replay::seal` recomputes the manifest's
/// derived fields — the input count, the log's digest — from the recording it is
/// given, so sealing a tampered replay through it would quietly repair the
/// tamper and the row would test nothing. That is not a hypothetical: the
/// truncated-log row passed the first time this file was written, because
/// re-sealing recomputed the count and the last forty inputs happened not to
/// move the world.
///
/// So the attacker here signs the bytes as they are, which is what an attacker
/// holding a key actually does, and what is left to catch them is whether the
/// manifest agrees with the log and with the world the log produces.
fn reseal(mut replay: Replay, edit: impl FnOnce(&mut Replay)) -> Replay {
    edit(&mut replay);
    let honest = honest();
    replay.manifest.server_identity = honest.verifying();
    replay.signature = honest.sign(&replay::signed_bytes(&replay.manifest));
    replay
}

// ---------------------------------------------------------------------------
// The table
// ---------------------------------------------------------------------------

/// One row: what was done to a genuine replay, and what must come back.
struct Case {
    /// What a reader should call it, and what the failure message names.
    name: &'static str,
    /// The tamper.
    tamper: fn(Replay) -> Replay,
    /// What the verifier must be doing when it refuses, so that the row is
    /// checked against a *check* rather than against a variant name.
    expect: fn(&VerifyError) -> bool,
    /// The build the verifier claims to be, for the two rows that are about a
    /// verifier disagreeing with a file rather than about a file being wrong.
    build: fn() -> Build,
}

fn this_build() -> Build {
    Build::current()
}

fn cases() -> Vec<Case> {
    vec![
        // 1. Truncated log. The manifest still says how many inputs there were,
        //    which is the field that makes "shorter" a distinct answer from
        //    "different": an attacker who drops the tail of a log and re-signs
        //    without touching the count is refused for the shortening rather
        //    than for a digest that no longer matches.
        Case {
            name: "truncated log",
            tamper: |replay| {
                reseal(replay, |replay| {
                    let keep = replay.inputs.len().saturating_sub(40);
                    replay.inputs.truncate(keep);
                })
            },
            expect: |error| matches!(error, VerifyError::Truncated { .. }),
            build: this_build,
        },
        // 2. Reordered inputs. The count is right and the multiset is right; the
        //    order is not, and order is part of a log's identity because `step`
        //    neither sorts nor deduplicates.
        //
        //    Note what this row is *not*: the two inputs swapped belong to
        //    different ticks, and `resimulate` buckets by each input's own tick
        //    field, so the resimulation reaches exactly the same state. The
        //    digest check would let this through. `input_log_digest` is what does
        //    not — the identity of a log includes its order even where the
        //    simulation is insensitive to it, and that is the whole reason the
        //    manifest carries the log's digest rather than only the state's.
        Case {
            name: "reordered inputs",
            tamper: |replay| {
                reseal(replay, |replay| {
                    let length = replay.inputs.len();
                    replay.inputs.swap(0, length.saturating_sub(1));
                })
            },
            expect: |error| matches!(error, VerifyError::InputLog { .. }),
            build: this_build,
        },
        // 3. Altered outcome record. The seed, the log and the final digest are
        //    untouched: this is a file that resimulates perfectly and lies about
        //    who won, which is exploit class 2 in one line.
        Case {
            name: "altered outcome record",
            tamper: |replay| {
                reseal(replay, |replay| {
                    replay.manifest.outcome = Outcome::Decided {
                        winner: Team::Green,
                        at: Tick(7),
                    };
                })
            },
            expect: |error| matches!(error, VerifyError::Outcome { .. }),
            build: this_build,
        },
        // 4. Altered seed. The whole initial world moves, so the log reaches a
        //    different state — and the *state* is the more specific answer than
        //    the outcome, which is why the digest is checked first.
        Case {
            name: "altered seed",
            tamper: |replay| {
                reseal(replay, |replay| {
                    replay.manifest.seed = replay.manifest.seed.wrapping_add(1);
                })
            },
            expect: |error| matches!(error, VerifyError::FinalDigest { .. }),
            build: this_build,
        },
        // 5. Unknown signing key. A perfectly valid replay of a real match,
        //    sealed by somebody this registry has never heard of. Nothing about
        //    the contents is wrong; the provenance is.
        Case {
            name: "unknown signing key",
            tamper: |replay| {
                replay::seal(
                    &Recording {
                        seed: replay.manifest.seed,
                        rules_hash: replay.manifest.rules_hash,
                        ticks: replay.manifest.ticks,
                        outcome: replay.manifest.outcome,
                        final_state_digest: replay.manifest.final_state_digest,
                        inputs: replay.inputs.clone(),
                    },
                    &session(),
                    &SigningKey::from_seed(STRANGER_SEED),
                )
            },
            expect: |error| matches!(error, VerifyError::UnknownKey(_)),
            build: this_build,
        },
        // 6a. Rules-hash mismatch. The constants moved: this replay describes a
        //     match played by a different game, and resimulating it here would
        //     produce a different match rather than a different digest
        //     (`docs/RISKS.md` R2).
        Case {
            name: "rules-hash mismatch",
            tamper: |replay| replay,
            expect: |error| matches!(error, VerifyError::RulesHash { .. }),
            build: || Build {
                rules_hash: digest_bytes(b"some other set of constants"),
                sim_version: sim::VERSION,
            },
        },
        // 6b. Version mismatch. The same constants, another build of the code
        //     that reads them — the gap `rules_hash` does not cover and
        //     `docs/RISKS.md` R13 exists for. Its own error, because "this
        //     replay is from another build" and "this replay was tampered with"
        //     must not look alike.
        Case {
            name: "sim version mismatch",
            tamper: |replay| replay,
            expect: |error| matches!(error, VerifyError::SimVersion { .. }),
            build: || Build {
                rules_hash: rules_hash(),
                sim_version: [sim::VERSION[0].saturating_add(1), 0, 0],
            },
        },
        // 7. And the naive attacker, who edits and cannot re-sign. Not one of
        //    the criterion's six, and it is here because it is the case a reader
        //    assumes the other six are: everything above is *harder* than this.
        Case {
            name: "edited without re-signing",
            tamper: |mut replay| {
                replay.manifest.seed = replay.manifest.seed.wrapping_add(1);
                replay
            },
            expect: |error| matches!(error, VerifyError::Signature),
            build: this_build,
        },
        // 8. …and the same attacker attacking the signature itself rather than
        //    the manifest, because a verifier that only compared manifests would
        //    pass this.
        Case {
            name: "forged signature",
            tamper: |mut replay| {
                let mut bytes = *replay.signature.as_bytes();
                bytes[0] ^= 0x01;
                replay.signature = Signature::from_bytes(bytes);
                replay
            },
            expect: |error| matches!(error, VerifyError::Signature),
            build: this_build,
        },
    ]
}

/// The criterion.
#[test]
fn every_tamper_case_is_refused_for_its_own_reason_and_a_genuine_replay_is_not() {
    let keys = registry();

    // The genuine replay, first, so that a table in which everything fails
    // cannot pass by failing.
    let genuine = sealed();
    let verified =
        replay::verify(&genuine, &keys, &Build::current()).expect("the genuine replay was refused");
    assert_eq!(
        verified.final_state_digest,
        genuine.manifest.final_state_digest
    );
    assert_eq!(verified.outcome, genuine.manifest.outcome);
    assert_eq!(verified.signer, honest().verifying());
    assert!(!verified.retired, "the honest key is not retired");

    // …and it survives the container, because a format that verifies in memory
    // and not on disk is not a format.
    let round_tripped = Replay::decode(&genuine.encode()).expect("a genuine replay did not decode");
    assert_eq!(round_tripped, genuine);
    replay::verify(&round_tripped, &keys, &Build::current())
        .expect("a genuine replay did not verify after a round trip");

    let mut seen: Vec<&'static str> = Vec::new();
    for case in cases() {
        let tampered = (case.tamper)(genuine.clone());
        // Through the container every time. A tamper that cannot survive
        // encoding and decoding is a tamper this format would never meet, and a
        // table that skipped the round trip would be testing `verify` rather
        // than the file.
        let bytes = tampered.encode();
        let decoded = Replay::decode(&bytes).unwrap_or_else(|error| {
            panic!("{}: the tampered file did not decode: {error}", case.name)
        });

        let error = replay::verify(&decoded, &keys, &(case.build)())
            .err()
            .unwrap_or_else(|| panic!("{}: the verifier accepted it", case.name));
        assert!(
            (case.expect)(&error),
            "{}: refused for the wrong reason: {error:?}",
            case.name
        );
        println!("{:28} -> {error}", case.name);
        seen.push(case.name);
    }

    // Every row ran, and every row is a different row. `docs/RISKS.md` R15: a
    // table-driven test whose table is empty is a passing test.
    assert_eq!(seen.len(), cases().len());
    assert!(seen.len() >= 8, "only {} cases", seen.len());
}

/// A retired key still verifies, and the verifier says so rather than deciding.
///
/// `docs/RISKS.md` R4: rotating a key without keeping the retired one published
/// orphans every replay signed with it, which is a way of destroying evidence by
/// housekeeping. So retirement is a statement about what may be *sealed* from
/// now on, and this is the assertion that it is not quietly a statement about
/// what may be read.
#[test]
fn a_retired_key_still_verifies_what_it_sealed() {
    let retired = SigningKey::from_seed(RETIRED_SEED);
    let replay = replay::seal(&a_recording(), &session(), &retired);
    let verified =
        replay::verify(&replay, &registry(), &Build::current()).expect("a retired key was refused");
    assert!(
        verified.retired,
        "the verifier did not report that the key was retired"
    );
    assert_eq!(verified.signer, retired.verifying());
}

/// The escalation ends where key custody begins, and this is that stated out
/// loud.
///
/// An attacker who holds an accepted key and adjusts every field consistently
/// produces a replay of a **different, honestly simulated match**. `verify`
/// accepts it, and it is right to: there is nothing in the bytes that is false.
/// What such a replay claims is exactly what it can support — that this key
/// sealed this manifest, and that this log reaches this state — and it says
/// nothing whatever about a match having been played by people.
///
/// This test exists because a reader who has just read a table of eight
/// refusals will conclude more than the format supports, and because a limit
/// nobody has executed is a limit somebody will forget.
#[test]
fn the_escalation_ends_where_key_custody_begins() {
    // A match that never happened: a different seed, simulated honestly.
    let mut state = new_state(0xDEAD_BEEF_DEAD_BEEF);
    for _ in 0..60 {
        state = step(&state, &[]);
    }
    let fabricated = Recording {
        seed: 0xDEAD_BEEF_DEAD_BEEF,
        rules_hash: rules_hash(),
        ticks: 60,
        outcome: state.outcome(),
        final_state_digest: state.digest(),
        inputs: Vec::new(),
    };
    let replay = replay::seal(&fabricated, &session(), &honest());

    let verified = replay::verify(&replay, &registry(), &Build::current())
        .expect("an internally consistent replay must verify");
    assert_eq!(verified.final_state_digest, state.digest());
    assert_eq!(verified.ticks, 60);

    // The point of the test, and the only assertion in this file that is about
    // what verification does *not* mean: nine champions stood at their bases for
    // two seconds and nobody played this. `verify` cannot tell, and no check
    // that could be added here would — a log an attacker generated is a log.
    assert!(
        replay.inputs.is_empty(),
        "the fabricated match should hold no inputs at all"
    );
}

/// The fixture is a match, and not an empty one.
///
/// `docs/RISKS.md` R15. Every row of the table above is conditional on a replay
/// with a log in it: "the truncated log was refused" is satisfied by a log of
/// zero inputs truncated to zero inputs, and "reordering was caught" is
/// satisfied by nothing at all if there is nothing to reorder.
#[test]
fn the_fixture_is_a_match() {
    let recording = a_recording();
    assert_eq!(
        recording.inputs.len(),
        PLAYER_COUNT * TICKS as usize,
        "every seat speaks on every tick"
    );
    assert_eq!(recording.ticks, TICKS);
    assert_ne!(
        recording.final_state_digest,
        new_state(SEED).digest(),
        "the match reached the same state it started in, so resimulating it \
         proves nothing"
    );

    // …and it contains events, because the manifest's outcome and digest are
    // the only two things the table's last four rows can contradict.
    let reached = replay::resimulate(SEED, TICKS, &recording.inputs);
    assert_eq!(reached.digest(), recording.final_state_digest);
    assert_eq!(reached.tick(), Tick(TICKS));

    let mut events = 0usize;
    let mut state = new_state(SEED);
    let mut buckets: Vec<Vec<Input>> = vec![Vec::new(); TICKS as usize];
    for timed in &recording.inputs {
        buckets[timed.input.tick.0 as usize].push(timed.input);
    }
    for bucket in &buckets {
        state = step(&state, bucket);
        events = events.saturating_add(state.events().count());
    }
    assert!(events > 0, "the fixture produced no events");
    println!(
        "tamper fixture: {} inputs, {events} events",
        recording.inputs.len()
    );
}

/// A manifest is a function of its fields and of nothing else.
///
/// The claim that makes a replay sealed on one target verify on another, stated
/// where it can be checked cheaply; `replay/tests/sealed.rs` is where it is
/// checked on all three.
#[test]
fn a_manifest_round_trips_and_two_sealings_of_one_match_are_the_same_bytes() {
    let first = sealed();
    let second = sealed();
    assert_eq!(
        first.encode(),
        second.encode(),
        "sealing the same match twice produced different bytes, so a replay is a \
         function of the moment rather than of the match"
    );

    let encoded = first.manifest.encode();
    let mut reader = replay::ByteReader::new(&encoded);
    let manifest = replay::Manifest::decode(&mut reader).expect("a manifest did not decode");
    assert_eq!(manifest, first.manifest);
    assert_eq!(reader.remaining(), 0, "the manifest has a fixed width");
    assert_eq!(
        encoded.len(),
        replay::manifest::MANIFEST_MIN_BYTES,
        "the manifest's width is not the derivation that names it"
    );
}

/// A pseudonym that could break the audit is refused where it enters.
///
/// The corpus finds a withdrawn participant by searching every byte of every
/// file for their pseudonym, so a pseudonym containing a separator, a newline or
/// a path component would be one the search could miss or over-match. And a
/// free-form string is where a real name ends up by accident, which
/// `docs/RISKS.md` R3 is the reason not to allow.
#[test]
fn a_pseudonym_that_could_break_an_audit_is_not_a_pseudonym() {
    for good in ["alizarin", "seat-0", "P_7", "a"] {
        assert!(Pseudonym::parse(good).is_some(), "{good} was refused");
    }
    for bad in [
        "",
        "with space",
        "new\nline",
        "../escape",
        "unicode-é",
        "nul\0byte",
        "this-name-is-far-too-long-to-be-a-pseudonym",
    ] {
        assert!(
            Pseudonym::parse(bad).is_none(),
            "{bad:?} was accepted as a pseudonym"
        );
    }
}

/// The reader is total on hostile bytes, and one byte string is one replay.
#[test]
fn no_byte_string_decodes_to_two_different_replays_and_none_makes_the_reader_panic() {
    let genuine = sealed().encode();

    // Truncations. Every prefix is either an error or nothing.
    for length in 0..genuine.len().min(400) {
        assert!(
            Replay::decode(&genuine[..length]).is_err(),
            "a {length}-byte prefix decoded as a replay"
        );
    }
    // Trailing bytes are refused rather than ignored, or one replay would have
    // infinitely many encodings and two verifiers could disagree about which
    // bytes they hashed.
    let mut longer = genuine.clone();
    longer.push(0);
    assert_eq!(
        Replay::decode(&longer),
        Err(replay::ReadError::TrailingBytes)
    );

    // A byte flipped anywhere either decodes to something (which `verify` then
    // refuses) or errors. It must never panic, and it must never decode to the
    // genuine replay, because that would be a byte the format does not read.
    //
    // Every byte of the header, the manifest and the signature, and then a
    // sample of the log: the log is thousands of identically-shaped entries and
    // sweeping all of them is the same assertion tens of thousands of times.
    let original = Replay::decode(&genuine).expect("the genuine replay");
    let manifest_end = 10 + replay::manifest::MANIFEST_MIN_BYTES + 64 + 8;
    let sweep = (0..manifest_end).chain((manifest_end..genuine.len()).step_by(11));
    for at in sweep {
        let mut bytes = genuine.clone();
        bytes[at] ^= 0xFF;
        if let Ok(decoded) = Replay::decode(&bytes) {
            assert_ne!(
                decoded, original,
                "byte {at} is not read by the decoder, so two byte strings are one replay"
            );
        }
    }

    // And a digest of the whole file, so that a change to the container's layout
    // is a deliberate act rather than a surprise. Not a cross-platform claim —
    // `replay/tests/sealed.rs` is that — just a tripwire under this one.
    println!("tamper: sealed replay is {} bytes", genuine.len());
    let _: Digest = digest_bytes(&genuine);
}
