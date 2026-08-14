//! Exploit class 2: result forgery — submitting the replay of a match nobody
//! played, and altering the replay of one somebody did.
//!
//! **This is the exploit `docs/MILESTONES.md` M7 calls the one that matters
//! most.** M5 delivered the format and stated its own limit; `docs/SCOPE.md`
//! reserves the word *delivered* for a defence with a matching exploit failing
//! against it in CI, so until this file existed the container was a format with a
//! table of hand-built structs beside it. Here that table is a program that
//! writes files, run by an attacker who does **not** hold a key the victim
//! accepts — which is where `docs/RISKS.md` R4 says the only interesting boundary
//! is.
//!
//! # The attacker built the container itself
//!
//! `cheat_client::forge` reimplements the replay file format from the published
//! documents. It links no `replay`: an exploit that used the victim's own writer
//! would be assuming the victim's cooperation. The first test here is therefore a
//! check on the reimplementation — the attacker's own bytes must decode, in the
//! victim's reader, into exactly the replay the victim's writer would produce —
//! because an exploit against a format nobody has independently read is an exploit
//! against a straw format.
//!
//! # What each exploit succeeds and fails against
//!
//! The weakened defence for class 2 is not a projection to switch off; it is a
//! **key registry that trusts the wrong key**, which `docs/RISKS.md` R4 argues is
//! exactly what the format's guarantee reduces to. So every exploit is run against
//! a compromised registry (which accepts the attacker) and the honest one (which
//! does not), and the test is red if either half is wrong.

#![deny(unsafe_code)]

use cheat_client::forge::{Commit, Commitment, Edit, ForgedInput, ForgedManifest, Forger, edit};
use protocol::{Outcome, Team, Tick};
use replay::manifest::Build;
use replay::{Replay, VerifyError};
use sim::{Action, FxVec2, Seat, digest_bytes, new_state, rules_hash, step};

#[path = "harness/authority.rs"]
mod authority;
#[path = "harness/registries.rs"]
mod registries;

use authority::started_match;
use registries::{compromised_registry, honest_registry};

/// The honest server's key. A written-down constant for the reason
/// `replay/tests/tamper.rs` gives about its own: deterministic signing makes a
/// sealed replay a function of the match rather than of the moment.
const HONEST_SEED: [u8; 32] = *b"moba cheat honest server key\0\0\0\0";

/// The forger's key. Nobody put it in a registry.
const FORGER_SEED: [u8; 32] = *b"moba cheat forger, no key here.\0";

fn honest() -> replay::SigningKey {
    replay::SigningKey::from_seed(HONEST_SEED)
}

/// A genuine recording of a match three seats actually played, sealed the way the
/// server would seal it.
///
/// The material the byte-surgery attacks start from: R4's tamper cases are about
/// altering a *genuine* replay, so there has to be a genuine one, and it has to
/// contain something (`docs/RISKS.md` R15) or "the truncated log was refused" is a
/// statement about a log of nothing.
fn a_genuine_replay() -> Replay {
    let mut game = started_match(0x0F1E_2D3C_4B5A_6978, 3);
    let target = sim::base_position(Seat::Red0.team(), &sim::RULES);
    for seat in [Seat::Blue0, Seat::Blue1, Seat::Blue2] {
        let frame = protocol::ClientFrame::encode(&protocol::ClientMessage::Input {
            seq: 0,
            claimed_at_ms: 0,
            action: Action::Move(target),
        });
        game.deliver(seat, frame.as_bytes().as_slice(), 0)
            .expect("the move was accepted");
    }
    for tick in 0..300u32 {
        if tick % 240 == 60 {
            for seat in [Seat::Blue0, Seat::Blue1, Seat::Blue2] {
                let frame = protocol::ClientFrame::encode(&protocol::ClientMessage::Input {
                    seq: 1,
                    claimed_at_ms: 0,
                    action: Action::Skillshot(target),
                });
                let _ = game.deliver(seat, frame.as_bytes().as_slice(), 0);
            }
        }
        let _ = game.tick();
    }
    let recording = game.recording();
    assert!(
        !recording.inputs.is_empty(),
        "the genuine match is empty (R15)"
    );
    replay::seal(
        &recording,
        &replay::SessionFacts::anonymous(replay::MatchId(*b"genuine-match-01"), 1_786_000_000_000),
        &honest(),
    )
}

/// A self-consistent forgery of a match that was never played, and the digests it
/// needs, computed with the published rules.
///
/// The forger has to assert a digest and an outcome its log actually reaches, or
/// resimulation contradicts it. The attacker in the wild does this by running the
/// rules — they are public — so the test runs them here rather than making the
/// forger reimplement `step` to prove a claim about a file. The log is **empty**:
/// nine champions stand at their bases for two seconds, and nobody played it.
fn a_forged_replay(forger: &Forger) -> Vec<u8> {
    let ticks = 60u32;
    let mut state = new_state(0xDEAD_BEEF_F00D_CAFE);
    for _ in 0..ticks {
        state = step(&state, &[]);
    }
    let manifest = ForgedManifest {
        match_id: *b"forged-match-001",
        seed: 0xDEAD_BEEF_F00D_CAFE,
        rules_hash: *rules_hash().as_bytes(),
        sim_version: sim::VERSION,
        sim_commit: Commit::Unknown,
        telemetry: Commitment::None,
        started_at_unix_ms: 1_786_000_000_000,
        participants: [const { None }; 9],
        ticks,
        input_log_digest: *sim::input_log_digest(&[]).as_bytes(),
        outcome: state.outcome(),
        final_state_digest: *state.digest().as_bytes(),
    };
    forger.seal(&manifest, &[])
}

/// The central exploit: a replay of a match nobody played verifies wherever the
/// forger's key is trusted, and is refused as soon as it is not.
#[test]
fn a_replay_of_a_match_nobody_played_verifies_where_the_forgers_key_is_trusted() {
    let honest = honest();
    let forger = Forger::with_seed(FORGER_SEED);
    let file = a_forged_replay(&forger);

    // It is a real file: the victim's reader accepts it as one.
    let replay = Replay::decode(&file).expect("the forged file is a well-formed replay");
    assert!(
        replay.inputs.is_empty(),
        "the forged match should carry no inputs at all — nobody played it"
    );

    // --- Works: against a registry that trusts the forger, the lie verifies ---
    let compromised = compromised_registry(&honest, forger.identity());
    let verified = replay::verify(&replay, &compromised, &Build::current())
        .expect("the forged replay was refused by a registry that trusts its key");
    assert_eq!(
        verified.final_state_digest, replay.manifest.final_state_digest,
        "the forgery verified to a digest other than the one it claims"
    );
    println!(
        "forgery: a replay of a match nobody played VERIFIED against a registry \
         that trusts the forger's key"
    );

    // --- Fails: against the honest registry, provenance stops it dead ---
    let honest_registry = honest_registry(&honest);
    let error = replay::verify(&replay, &honest_registry, &Build::current())
        .expect_err("the forged replay verified against the honest registry");
    assert!(
        matches!(error, VerifyError::UnknownKey(_)),
        "the forgery was refused for the wrong reason: {error:?}"
    );
    println!("forgery: the same file was refused by the honest registry — {error}");

    // The defence is provenance, and nothing else. Stated so the reader does not
    // conclude the contents saved anybody: they are internally perfect.
    assert!(
        replay::verify(&replay, &honest_registry, &Build::current()).is_err(),
        "provenance is the whole of the defence here"
    );
}

/// The attacker's hand-written container is the real format.
///
/// If it were not, the exploit above would be an attack on a straw format. So the
/// forger seals a manifest, and the same manifest sealed by `replay`'s own writer
/// over the same recording must produce byte-identical output — the attacker read
/// the format correctly, field for field and offset for offset.
#[test]
fn the_forgers_container_is_the_real_format() {
    let forger = Forger::with_seed(FORGER_SEED);

    // A recording the attacker and the victim both describe: a short empty match.
    let ticks = 60u32;
    let seed = 0x00C0_FFEE_0D15_EA5E;
    let mut state = new_state(seed);
    for _ in 0..ticks {
        state = step(&state, &[]);
    }
    let recording = replay::Recording {
        seed,
        rules_hash: rules_hash(),
        ticks,
        outcome: state.outcome(),
        final_state_digest: state.digest(),
        inputs: Vec::new(),
    };

    // The victim's writer, using the forger's key so the signatures are
    // comparable.
    let victim_key = replay::SigningKey::from_seed(FORGER_SEED);
    let victims_bytes = replay::seal(
        &recording,
        &replay::SessionFacts {
            match_id: replay::MatchId(*b"shared-match-001"),
            started_at_unix_ms: 1_786_000_000_000,
            participants: [const { None }; 9],
            sim_commit: replay::SimCommit::Unknown,
            telemetry: replay::Commitment::Absent,
        },
        &victim_key,
    )
    .encode();

    // The attacker's writer, over the same facts.
    let manifest = ForgedManifest {
        match_id: *b"shared-match-001",
        seed,
        rules_hash: *rules_hash().as_bytes(),
        sim_version: sim::VERSION,
        sim_commit: Commit::Unknown,
        telemetry: Commitment::None,
        started_at_unix_ms: 1_786_000_000_000,
        participants: [const { None }; 9],
        ticks,
        input_log_digest: *sim::input_log_digest(&[]).as_bytes(),
        outcome: state.outcome(),
        final_state_digest: *state.digest().as_bytes(),
    };
    let attackers_bytes = forger.seal(&manifest, &[]);

    assert_eq!(
        digest_bytes(&attackers_bytes),
        digest_bytes(&victims_bytes),
        "the attacker's container is not byte-identical to the victim's, so the \
         exploit is written against a format the victim does not use"
    );
    // And it round-trips through the victim's reader.
    let decoded = Replay::decode(&attackers_bytes).expect("the attacker's file decodes");
    assert_eq!(
        decoded.encode(),
        attackers_bytes,
        "the reader disagrees about the bytes"
    );
    println!("forgery: the attacker's container is byte-identical to the victim's");
}

/// The M5 tamper table, as byte-level attacks by an attacker who cannot re-sign,
/// and the two-layer defence that catches every one of them.
///
/// Each row edits a *genuine* replay's bytes and hands the result to the honest
/// registry. None gets through, and *which* check stops it is the finding —
/// because the attacker's edits fall into two kinds, and the format defends each
/// with a different layer:
///
/// - An edit **inside the manifest** — the outcome, the seed, the match id — dies
///   at the **signature**, because the manifest is what the signature covers and
///   the keyless attacker cannot repair it.
/// - An edit **to the log** — truncating it, reordering it — dies at the
///   **manifest's own commitment to the log**: the count and the digest the
///   manifest carries (`docs/RISKS.md` R4, "the manifest covers the log by
///   carrying its digest"). The signature is still valid — the manifest was not
///   touched — and the log is caught anyway.
///
/// So the keyless attacker is stopped whether they reach inside the signature or
/// outside it, and the "would have worked" half is asserted at each row: the edit
/// really did change what the file *says*, so a verifier that trusted the bytes
/// without either check would have believed it.
#[test]
fn a_keyless_attacker_is_caught_inside_the_signature_and_outside_it() {
    let genuine = a_genuine_replay();
    let file = genuine.encode();
    let registry = honest_registry(&honest());

    // The genuine file verifies, so the refusals below are about the edits.
    replay::verify(&genuine, &registry, &Build::current()).expect("the genuine replay verifies");

    struct Row {
        name: &'static str,
        edit: Edit,
        /// Which check must stop it, and where that check lives.
        expect: fn(&VerifyError) -> bool,
        /// The lie the edit wrote into the bytes, asserted so the row is not
        /// caught by a no-op edit (R15).
        lie_is_in_the_bytes: fn(&Replay),
    }
    let rows = [
        Row {
            name: "altered outcome (in the manifest)",
            edit: Edit::Outcome(Outcome::Decided {
                winner: Team::Green,
                at: Tick(7),
            }),
            expect: |error| matches!(error, VerifyError::Signature),
            lie_is_in_the_bytes: |tampered| {
                assert!(
                    matches!(
                        tampered.manifest.outcome,
                        Outcome::Decided {
                            winner: Team::Green,
                            ..
                        }
                    ),
                    "the outcome edit did not change what the file claims"
                );
            },
        },
        Row {
            name: "altered seed (in the manifest)",
            edit: Edit::Seed(0xFFFF_FFFF_FFFF_FFFF),
            expect: |error| matches!(error, VerifyError::Signature),
            lie_is_in_the_bytes: |tampered| {
                assert_eq!(
                    tampered.manifest.seed, 0xFFFF_FFFF_FFFF_FFFF,
                    "the seed edit did nothing"
                );
            },
        },
        Row {
            name: "resubmitted under another match id",
            edit: Edit::MatchId(*b"stolen-match-999"),
            expect: |error| matches!(error, VerifyError::Signature),
            lie_is_in_the_bytes: |tampered| {
                assert_eq!(
                    tampered.manifest.match_id.0, *b"stolen-match-999",
                    "the id edit did nothing"
                );
            },
        },
        Row {
            name: "truncated log (outside the signature)",
            edit: Edit::TruncateLog(2),
            expect: |error| matches!(error, VerifyError::Truncated { .. }),
            lie_is_in_the_bytes: |tampered| {
                assert!(
                    (tampered.inputs.len() as u64) < tampered.manifest.inputs,
                    "the truncation did not shorten the log below the manifest's count"
                );
            },
        },
        Row {
            name: "reordered inputs (outside the signature)",
            edit: Edit::SwapInputs(0, 3),
            expect: |error| matches!(error, VerifyError::InputLog { .. }),
            lie_is_in_the_bytes: |_tampered| {},
        },
        Row {
            name: "forged signature",
            edit: Edit::ForgeSignature,
            expect: |error| matches!(error, VerifyError::Signature),
            lie_is_in_the_bytes: |_tampered| {},
        },
    ];

    let mut seen = 0;
    for row in &rows {
        let tampered_bytes = edit(&file, row.edit);
        // The antecedent: the edit produced a different, still-well-formed file.
        // An edit that did nothing would be R15 wearing an attack.
        assert_ne!(
            tampered_bytes, file,
            "{}: the edit changed no bytes, so this attack is about nothing (R15)",
            row.name
        );
        let tampered = Replay::decode(&tampered_bytes).unwrap_or_else(|error| {
            panic!("{}: the edited file did not decode: {error}", row.name)
        });
        (row.lie_is_in_the_bytes)(&tampered);

        let error = replay::verify(&tampered, &registry, &Build::current())
            .err()
            .unwrap_or_else(|| {
                panic!("{}: the honest registry accepted a tampered file", row.name)
            });
        assert!(
            (row.expect)(&error),
            "{}: caught by the wrong check: {error:?}",
            row.name
        );
        println!("forgery, no key: {:40} -> {error}", row.name);
        seen += 1;
    }
    assert_eq!(seen, rows.len(), "not every row ran (R15)");
}

/// The same table, by an attacker who *does* hold a trusted key — which is what
/// makes the six errors six, and where the escalation ends.
///
/// This is `replay/tests/tamper.rs`'s distinct-error table reproduced from the
/// attacker's side and at the *byte* level: the forger edits a genuine file,
/// re-signs it with a key the compromised registry accepts, and each row is now
/// caught by a **different** check — the one that stopped one step short of the
/// next. Two rows are not errors and that is the point of R4: an edit the attacker
/// can make *consistent* (a new match id) verifies, because a self-consistent
/// replay of a different match is not a tampered one.
#[test]
fn with_a_trusted_key_each_edit_is_caught_by_its_own_check() {
    let forger = Forger::with_seed(FORGER_SEED);
    // The genuine file the forger starts from was sealed by the honest server;
    // the forger re-signs after each edit with its own key, which the compromised
    // registry accepts. This models an insider — R4's key-custody case.
    let genuine = a_genuine_replay().encode();
    let registry = compromised_registry(&honest(), forger.identity());

    // The forger has to re-point the manifest's identity at itself before signing,
    // or the signature it makes will not match the key the manifest names. That is
    // one more field in the manifest region, so it is another in-place edit.
    let reseal = |mut bytes: Vec<u8>| {
        forger.point_identity_at_self(&mut bytes);
        forger.reseal(&mut bytes);
        bytes
    };

    // A resigned but otherwise untouched genuine replay verifies: it is a real
    // match, now attributed to the forger. This is the floor the rows move off.
    let attributed = reseal(genuine.clone());
    let decoded = Replay::decode(&attributed).expect("resigned file decodes");
    replay::verify(&decoded, &registry, &Build::current())
        .expect("a genuine match re-signed by a trusted key must verify");

    struct Row {
        name: &'static str,
        edit: Edit,
        expect: fn(&VerifyError) -> bool,
        accepted: bool,
    }
    let rows = [
        Row {
            name: "altered outcome",
            edit: Edit::Outcome(Outcome::Decided {
                winner: Team::Green,
                at: Tick(7),
            }),
            expect: |error| matches!(error, VerifyError::Outcome { .. }),
            accepted: false,
        },
        Row {
            name: "altered seed",
            edit: Edit::Seed(0xFFFF_FFFF_FFFF_FFFF),
            expect: |error| matches!(error, VerifyError::FinalDigest { .. }),
            accepted: false,
        },
        Row {
            name: "truncated log",
            edit: Edit::TruncateLog(40),
            expect: |error| matches!(error, VerifyError::Truncated { .. }),
            accepted: false,
        },
        Row {
            name: "reordered inputs",
            edit: Edit::SwapInputs(0, 3),
            expect: |error| matches!(error, VerifyError::InputLog { .. }),
            accepted: false,
        },
        Row {
            name: "resubmitted under another match id",
            edit: Edit::MatchId(*b"stolen-match-999"),
            expect: |_| true,
            accepted: true,
        },
    ];

    for row in &rows {
        let bytes = reseal(edit(&genuine, row.edit));
        let tampered = Replay::decode(&bytes)
            .unwrap_or_else(|error| panic!("{}: did not decode: {error}", row.name));
        let result = replay::verify(&tampered, &registry, &Build::current());
        if row.accepted {
            result.unwrap_or_else(|error| {
                panic!(
                    "{}: a self-consistent replay was refused: {error:?}",
                    row.name
                )
            });
            println!(
                "forgery, trusted key: {:34} -> VERIFIES (a different match, honestly \
                 re-simulated — R4)",
                row.name
            );
        } else {
            let error = result
                .err()
                .unwrap_or_else(|| panic!("{}: verify accepted a tampered file", row.name));
            assert!(
                (row.expect)(&error),
                "{}: caught by the wrong check: {error:?}",
                row.name
            );
            println!("forgery, trusted key: {:34} -> {error}", row.name);
        }
    }

    // And the wall past the last row: a fully self-consistent forgery of a
    // different match verifies too, which is R4's boundary — the escalation ends
    // where key custody begins, and there is nothing in the bytes to catch it.
    let minted = a_forged_replay_for(&forger);
    let decoded = Replay::decode(&minted).expect("minted file decodes");
    replay::verify(&decoded, &registry, &Build::current())
        .expect("a self-consistent forgery under a trusted key must verify (R4)");
    println!(
        "forgery, trusted key: {:34} -> VERIFIES (nothing in the bytes is false — R4)",
        "minted from nothing"
    );
}

/// A minted forgery under a given forger, factored so the boundary test can use
/// it. Mirrors [`a_forged_replay`] but names the forger's own identity in the
/// manifest so the forger's signature matches it.
fn a_forged_replay_for(forger: &Forger) -> Vec<u8> {
    let ticks = 60u32;
    let mut state = new_state(0x1234_5678_9ABC_DEF0);
    for _ in 0..ticks {
        state = step(&state, &[]);
    }
    let manifest = ForgedManifest {
        match_id: *b"minted-match-002",
        seed: 0x1234_5678_9ABC_DEF0,
        rules_hash: *rules_hash().as_bytes(),
        sim_version: sim::VERSION,
        sim_commit: Commit::Unknown,
        telemetry: Commitment::None,
        started_at_unix_ms: 1_786_000_000_000,
        participants: [const { None }; 9],
        ticks,
        input_log_digest: *sim::input_log_digest(&[]).as_bytes(),
        outcome: state.outcome(),
        final_state_digest: *state.digest().as_bytes(),
    };
    forger.seal(&manifest, &[])
}

/// A forged log entry, so `ForgedInput` is exercised somewhere rather than only
/// declared.
///
/// The forger can put an input in the log too, not only an empty one; this checks
/// that a one-input forgery is self-consistent when its digest is computed over
/// that input. It is the smallest match with a move in it.
#[test]
fn a_forgery_can_carry_a_log_and_stay_consistent() {
    let forger = Forger::with_seed(FORGER_SEED);
    let seed = 0x0BAD_F00D_0BAD_F00D;

    let action = Action::Move(FxVec2::ZERO);
    let input = sim::Input {
        tick: Tick(0),
        seq: 0,
        player: Seat::Blue0,
        action,
    };
    let mut state = new_state(seed);
    state = step(&state, &[input]);
    for _ in 1..30u32 {
        state = step(&state, &[]);
    }

    let log = [ForgedInput {
        tick: 0,
        seq: 0,
        seat: Seat::Blue0,
        claimed_at_ms: 111,
        received_at_ms: 222,
        action,
    }];
    let manifest = ForgedManifest {
        match_id: *b"one-input-match0",
        seed,
        rules_hash: *rules_hash().as_bytes(),
        sim_version: sim::VERSION,
        sim_commit: Commit::Sha([0x11; 20]),
        telemetry: Commitment::None,
        started_at_unix_ms: 1_786_000_000_000,
        participants: [const { None }; 9],
        ticks: 30,
        input_log_digest: *sim::input_log_digest(&[input]).as_bytes(),
        outcome: state.outcome(),
        final_state_digest: *state.digest().as_bytes(),
    };
    let file = forger.seal(&manifest, &log);

    let replay = Replay::decode(&file).expect("the one-input forgery decodes");
    assert_eq!(
        replay.inputs.len(),
        1,
        "the log entry survived the round trip"
    );
    let registry = compromised_registry(&honest(), forger.identity());
    replay::verify(&replay, &registry, &Build::current())
        .expect("a self-consistent one-input forgery must verify where the key is trusted");
    println!("forgery: a one-input forgery is self-consistent and verifies under a trusted key");
}
