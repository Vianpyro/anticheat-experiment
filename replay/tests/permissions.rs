//! The granular consent regime, exercised by breaking it.
//!
//! `docs/CONSENT.md` offers four permissions a participant may refuse without
//! refusing to take part, and the claim this file has to substantiate is not
//! that the boxes are recorded — that is a struct field — but that **refusing one
//! stops the use it names**. A box recorded and not applied is decoration, and a
//! granular regime whose granularity lives in a paragraph is worse than a coarse
//! one, because it claims something it does not do.
//!
//! So the interesting tests here are the ones where a use is *attempted* against
//! a refusal and has to fail, and the ones where a fault is planted and the audit
//! has to find it. Each of the five things `docs/CONSENT.md` promises appears
//! twice: once as the promise kept, and once as the promise broken on purpose so
//! that the check has something to catch. `docs/RISKS.md` R15 is why the second
//! half exists — a refusal that was never reachable looks exactly like a refusal
//! that holds.

#![deny(unsafe_code)]

use std::path::{Path, PathBuf};

use replay::attest::Attested;
use replay::calibration::{CalibrationState, DeviceProfileId, Observations, SeatCalibration};
use replay::consent::{ConsentVersion, Permissions, Purpose};
use replay::corpus::{ConsentRecord, Corpus};
use replay::keys::SigningKey;
use replay::manifest::{MatchId, Pseudonym, SessionFacts, SimCommit};
use replay::permit::{PermitError, Publishable, TrainingSet};
use replay::session::{
    Clock, Declared, Measured, Platform, SeatRecord, SessionRecord, Supervision,
};
use replay::{Recording, Replay};
use sim::{Outcome, PLAYER_COUNT, new_state, rules_hash};

// ---------------------------------------------------------------------------
// The fixture: a corpus of three matches and three people, whose permissions
// the tests below vary.
// ---------------------------------------------------------------------------

/// A corpus in a directory of its own, removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "moba-permits-{}-{name}-{}",
            std::process::id(),
            name.len()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a scratch directory");
        Self(path)
    }

    fn corpus(&self) -> Corpus {
        Corpus::open(&self.0)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A consent record granting exactly these purposes.
fn consent(pseudonym: &str, granted: &[Purpose]) -> ConsentRecord {
    ConsentRecord {
        pseudonym: pseudonym.to_owned(),
        consented_on: "2026-08-17".to_owned(),
        retention_until: "2028-08-17".to_owned(),
        permissions: Permissions::granting(granted),
        adult: true,
        consent_version: ConsentVersion::current(),
    }
}

const SEAL_SEED: [u8; 32] = *b"moba test corpus signing key...\0";

fn a_seat() -> SeatRecord {
    let mut observations = Observations::new();
    observations.reaches = 24;
    observations.octants = 0xff;
    observations.min_distance_e3 = 40_000;
    observations.max_distance_e3 = 240_000;
    observations.sum_distance_e3 = 3_360_000;
    observations.sum_counts_e3 = 67_416_000;
    observations.sum_distance_sq_e3 = 560_000_000;
    observations.sum_distance_counts_e3 = 11_209_400_000;
    observations.sum_counts_sq_e3 = 224_400_000_000;
    observations.fast_reaches = 8;
    observations.fast_motions = 800;
    observations.fast_ns = 6_400_000_000;
    observations.quantum_e6 = 1_000_000;
    SeatRecord::Human {
        calibration: SeatCalibration {
            observations,
            state: CalibrationState::Sufficient,
        },
        declared: Declared {
            device_profile_id: DeviceProfileId::parse("mouse-a").expect("a device label"),
            device_cpi: 800,
            device_polling_hz: 1000,
            pointer_acceleration: false,
        },
        measured: Measured {
            platform: Platform::Linux,
            clock: Clock::Dequeue,
            world_units_per_count_e6: 50_000,
            samples: 118_233,
            motions: 117_980,
            coincident: 0,
            median_gap_ns: 1_000_000,
            budget_ns: 33_333_333,
            passes: 28_714,
            passes_over_budget: 0,
            worst_overrun_ns: 0,
            worst_pass_ns: 4_120_000,
        },
    }
}

fn a_session(match_id: &str, participants: usize) -> SessionRecord {
    let mut seats = [const { SeatRecord::Empty }; PLAYER_COUNT];
    for slot in seats.iter_mut().take(participants) {
        *slot = a_seat();
    }
    SessionRecord {
        match_id: a_replay(match_id, &[]).manifest.match_id,
        consent_version: ConsentVersion::current(),
        recorded_on: "2026-09-03".to_owned(),
        supervision: Supervision::InPerson,
        seats,
    }
}

fn a_recording() -> Recording {
    let inputs = (0..8u32)
        .map(|index| replay::TimedInput {
            input: sim::Input {
                tick: sim::Tick(index),
                seq: index,
                player: sim::Seat::Blue0,
                action: sim::Action::Idle,
            },
            claimed_at_ms: u64::from(index),
            received_at_ms: u64::from(index),
        })
        .collect();
    let mut state = new_state(7);
    for _ in 0..8 {
        state = sim::step(&state, &[]);
    }
    Recording {
        seed: 7,
        rules_hash: rules_hash(),
        ticks: 8,
        outcome: Outcome::InProgress,
        final_state_digest: state.digest(),
        inputs,
    }
}
/// The one way through [`Attested::of`], for tests whose subject is a different
/// refusal.
///
/// `Corpus::store` takes a value only `Attested::of` builds, and what that
/// constructor refuses is a seat the replay's input log shows playing that no
/// session record accounts for — a playtest bot, a headless client, a script
/// (`replay/src/attest.rs`). Every fixture here logs no seat the session record
/// leaves empty, so the gate opens; `client/tests/playtest_bots.rs` is where it
/// does not.
fn attested<'a>(replay: &'a Replay, session: &'a SessionRecord) -> Attested<'a> {
    Attested::of(replay, session, None).expect("every seat that played is a person")
}

fn a_replay(match_id: &str, participants: &[&str]) -> Replay {
    let mut slots: [Option<Pseudonym>; PLAYER_COUNT] = [const { None }; PLAYER_COUNT];
    for (slot, who) in slots.iter_mut().zip(participants) {
        *slot = Pseudonym::parse(who);
        assert!(slot.is_some(), "{who} is not a pseudonym");
    }
    let mut id = [b'-'; 16];
    for (slot, byte) in id.iter_mut().zip(match_id.bytes()) {
        *slot = byte;
    }
    replay::seal(
        &a_recording(),
        &SessionFacts {
            match_id: MatchId(id),
            started_at_unix_ms: 1_786_000_000_000,
            participants: slots,
            sim_commit: SimCommit::Unknown,
            telemetry: replay::Commitment::Absent,
        },
        &SigningKey::from_seed(SEAL_SEED),
    )
}

fn filed_as(match_id: &str) -> String {
    a_replay(match_id, &[]).manifest.match_id.to_string()
}

/// Three people and three matches, with each person's permissions given.
///
/// The matches overlap deliberately — `alizarin` is in two, `bistre` and
/// `celadon` in two each — because the rule under test is that **one** refusal
/// withholds a whole match, and a corpus of disjoint matches could not tell that
/// rule from a per-person one.
fn populated(scratch: &Scratch, permissions: &[(&str, &[Purpose])]) -> Corpus {
    let corpus = scratch.corpus();
    for (pseudonym, granted) in permissions {
        corpus
            .enrol(
                &consent(pseudonym, granted),
                &format!("{pseudonym} de la Fontaine <{pseudonym}@example.invalid>"),
            )
            .expect("enrol");
    }
    for (id, who) in [
        ("2026-09-03-a", ["alizarin", "bistre"]),
        ("2026-09-03-b", ["alizarin", "celadon"]),
        ("2026-09-11-a", ["bistre", "celadon"]),
    ] {
        corpus
            .store(&attested(&a_replay(id, &who), &a_session(id, who.len())))
            .expect("store");
    }
    corpus
}

/// Everybody grants everything, which is the corpus the refusals below are
/// varied against.
fn everyone_agrees(scratch: &Scratch) -> Corpus {
    populated(
        scratch,
        &[
            ("alizarin", &Purpose::ALL),
            ("bistre", &Purpose::ALL),
            ("celadon", &Purpose::ALL),
        ],
    )
}

// ---------------------------------------------------------------------------
// The consent record itself
// ---------------------------------------------------------------------------

/// A record survives its own encoding, with every purpose stated either way.
#[test]
fn a_consent_record_reads_back_with_every_purpose_it_was_written_with() {
    for granted in [
        Vec::new(),
        vec![Purpose::Publication],
        vec![Purpose::BotTraining, Purpose::NamedAttribution],
        Purpose::ALL.to_vec(),
    ] {
        let record = consent("celadon", &granted);
        assert_eq!(
            ConsentRecord::decode(&record.encode()),
            Some(record.clone()),
            "{granted:?} did not survive the round trip"
        );
        for purpose in Purpose::ALL {
            assert_eq!(
                record.permissions.granted(purpose),
                granted.contains(&purpose),
                "{purpose} was not written as it was granted"
            );
        }
    }
    assert_eq!(ConsentRecord::decode("nonsense"), None);
}

/// **The record from the version before this one is not a consent record.**
///
/// The mechanism `docs/CONSENT.md` describes as the reason a fifth box would
/// invalidate every signature: the old format's single `publication: true` line
/// answers one of four questions and says nothing about the other three, so it
/// does not decode, so every match its holder is in is refused until they are
/// asked again. Absent and stale fail alike, which is `docs/RISKS.md` R3's rule
/// one level down from the version.
#[test]
fn a_record_from_the_previous_consent_regime_does_not_decode() {
    let old = format!(
        "pseudonym: alizarin\nconsented_on: 2026-08-16\nretention_until: \
         2028-08-16\npublication: true\nconsent_version: {}\n",
        ConsentVersion::current()
    );
    assert!(
        ConsentRecord::decode(&old).is_none(),
        "a record answering one of four questions decoded anyway, so an old \
         signature would silently cover three purposes nobody was asked about"
    );

    // …and the refusal reaches the corpus, which is where it matters.
    let scratch = Scratch::new("old-regime");
    let corpus = everyone_agrees(&scratch);
    std::fs::write(
        scratch.path().join("participants").join("alizarin.consent"),
        &old,
    )
    .expect("write");
    let refused = corpus
        .store(&attested(
            &a_replay("2026-09-20-a", &["alizarin", "bistre"]),
            &a_session("2026-09-20-a", 2),
        ))
        .expect_err("a match with a previous-regime consent record was stored");
    assert_eq!(refused.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(
        refused.to_string().contains("no readable consent record"),
        "refused for the wrong reason: {refused}"
    );
    println!("store: {refused}");
}

/// **A participant under 18 is refused at the door of the corpus.**
///
/// `docs/CONSENT.md` L4: a minor's own consent is not sufficient, there is no
/// parental-consent procedure here, and the refusal names the human decision it
/// stands in for rather than inventing one.
#[test]
fn a_match_with_a_participant_under_eighteen_is_refused() {
    let scratch = Scratch::new("minor");
    let corpus = scratch.corpus();
    let mut minor = consent("alizarin", &Purpose::ALL);
    minor.adult = false;
    corpus
        .enrol(&minor, "alizarin@example.invalid")
        .expect("enrol");
    corpus
        .enrol(&consent("bistre", &Purpose::ALL), "bistre@example.invalid")
        .expect("enrol");

    let refused = corpus
        .store(&attested(
            &a_replay("2026-09-03-a", &["alizarin", "bistre"]),
            &a_session("2026-09-03-a", 2),
        ))
        .expect_err("a match with a minor in it was stored");
    assert_eq!(refused.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(
        refused.to_string().contains("under 18"),
        "refused for the wrong reason: {refused}"
    );
    assert!(
        corpus.matches().expect("matches").is_empty(),
        "the refusal left the match behind anyway"
    );
    println!("store: {refused}");

    // The antecedent, so that the refusal above is about the age answer and not
    // about anything else this record carries (`docs/RISKS.md` R15).
    let mut adult = minor.clone();
    adult.adult = true;
    corpus
        .enrol(&adult, "alizarin@example.invalid")
        .expect("enrol");
    corpus
        .store(&attested(
            &a_replay("2026-09-03-a", &["alizarin", "bistre"]),
            &a_session("2026-09-03-a", 2),
        ))
        .expect("the same match with the same record and one bit changed was refused");
}

/// A record silent about the age answer fails the same way a versionless one
/// does.
#[test]
fn a_record_with_no_age_answer_is_not_a_consent_record() {
    let record = consent("alizarin", &Purpose::ALL);
    let text = record.encode();
    assert!(ConsentRecord::decode(&text).is_some());
    let without: String = text
        .lines()
        .filter(|line| !line.starts_with("adult: "))
        .map(|line| format!("{line}\n"))
        .collect();
    assert!(
        ConsentRecord::decode(&without).is_none(),
        "a record that never answered the age question decoded anyway"
    );
}

// ---------------------------------------------------------------------------
// `publication` — the gate, and the mutation that has to be caught
// ---------------------------------------------------------------------------

/// **A match a participant refused publication for cannot be published, and the
/// refusal names them.**
///
/// The mutation is one bit in one consent record. Everything else — the matches,
/// the session records, the seals — is identical between the two halves, so a
/// difference in what is published is a difference the consent produced.
#[test]
fn publishing_a_match_someone_refused_is_refused_by_name() {
    let scratch = Scratch::new("publish-refused");
    let corpus = populated(
        &scratch,
        &[
            ("alizarin", &Purpose::ALL),
            // The one refusal in the corpus.
            (
                "bistre",
                &[Purpose::BotTraining, Purpose::RetentionAfterProject],
            ),
            ("celadon", &Purpose::ALL),
        ],
    );

    // The antecedent: something in this corpus *is* publishable, so a refusal
    // below is a refusal rather than a corpus that publishes nothing.
    let publishable = Publishable::of(&corpus, &filed_as("2026-09-03-b"))
        .expect("the match nobody refused is not publishable, so this test is about nothing");
    assert_eq!(publishable.match_id(), filed_as("2026-09-03-b"));

    // …and the two matches `bistre` is in are not, by name.
    for match_id in [filed_as("2026-09-03-a"), filed_as("2026-09-11-a")] {
        // Mapped to its identifier before the failure is unwrapped: a
        // `Publishable` prints a whole replay when an assertion fails, and a
        // failure message nobody can read is a failure message nobody reads.
        let refused = Publishable::of(&corpus, &match_id)
            .map(|publishable| publishable.match_id().to_owned())
            .expect_err("a match a participant refused publication for was publishable");
        assert_eq!(
            refused,
            PermitError::Refused {
                pseudonym: "bistre".to_owned(),
                purpose: Purpose::Publication,
            },
            "the refusal does not name who refused: {refused}"
        );
        assert!(refused.to_string().contains("bistre"));
        println!("publish: {match_id} withheld — {refused}");
    }
}

/// **One refusal withholds the whole match, including the seats of people who
/// agreed.**
///
/// `docs/SCHEMA.md` §10 states this in advance as the practical consequence and
/// `docs/CONSENT.md` repeats it to the participant. It is asserted here rather
/// than assumed, because the tempting implementation publishes the seats that
/// agreed and there is no such thing: a match is one interleaved log.
#[test]
fn a_single_refusal_withholds_a_match_that_two_other_people_agreed_to() {
    let scratch = Scratch::new("publish-one-refusal");
    let destination = scratch.path().join("published");
    let corpus = populated(
        &scratch,
        &[
            ("alizarin", &Purpose::ALL),
            ("bistre", &Purpose::ALL),
            ("celadon", &[]),
        ],
    );

    let mut published = 0u32;
    for match_id in corpus.matches().expect("matches") {
        if let Ok(publishable) = Publishable::of(&corpus, &match_id) {
            publishable.write_to(&destination).expect("write");
            published += 1;
        }
    }
    assert_eq!(
        published, 1,
        "celadon is in two of the three matches and refused publication, so \
         exactly one match is publishable"
    );

    // And what reached the disk names nobody who refused, which is the claim a
    // count cannot make. The byte search is `Corpus::audit`'s own crudeness,
    // borrowed for the same reason: a check that knew where a pseudonym is
    // supposed to be would be blind exactly where a bug would put it.
    let bytes = bytes_under(&destination);
    assert!(
        !bytes.contains("celadon"),
        "a participant who refused publication appears in the published set"
    );
    assert!(
        bytes.contains("alizarin") && bytes.contains("bistre"),
        "nothing was published at all, so the assertion above is about an empty \
         directory"
    );
    println!("publish: 1 of 3 match(es) published; one refusal withheld two");
}

/// **A revoked permission takes effect on the next publication, with nothing
/// recomputed.**
///
/// The partial withdrawal's whole claim: the gate reads the consent record at
/// the moment of use, so revoking is an edit to one file and there is no cache,
/// no index and no derived list to invalidate. Broken here in the only direction
/// that matters — the match is publishable, then it is not, and nothing else
/// changed.
#[test]
fn revoking_publication_stops_the_next_publication_and_destroys_nothing() {
    let scratch = Scratch::new("revoke-publication");
    let corpus = everyone_agrees(&scratch);
    let filed = filed_as("2026-09-03-a");

    assert!(
        Publishable::of(&corpus, &filed).is_ok(),
        "the match is not publishable before the revocation, so revoking proves \
         nothing"
    );
    assert_eq!(
        corpus
            .audit_purpose("alizarin", Purpose::Publication)
            .expect("audit"),
        vec![filed_as("2026-09-03-a"), filed_as("2026-09-03-b")],
        "the audit does not report the matches a publication would reach"
    );

    let revoked = corpus
        .withdraw_purpose("alizarin", Purpose::Publication, "2026-09-20")
        .expect("withdraw the purpose");
    assert!(revoked, "there was nothing to revoke");

    let refused = Publishable::of(&corpus, &filed)
        .map(|publishable| publishable.match_id().to_owned())
        .expect_err("a match whose participant revoked publication was publishable");
    assert_eq!(
        refused,
        PermitError::Refused {
            pseudonym: "alizarin".to_owned(),
            purpose: Purpose::Publication,
        }
    );

    // Nothing was destroyed: the participation is intact, which is the whole
    // difference between this and a withdrawal.
    assert_eq!(
        corpus.matches().expect("matches").len(),
        3,
        "a partial withdrawal destroyed a match"
    );
    assert!(
        corpus.consent_of("alizarin").is_some(),
        "a partial withdrawal destroyed the consent record"
    );
    assert!(
        scratch
            .path()
            .join("identities")
            .join("alizarin.identity")
            .exists(),
        "a partial withdrawal destroyed the pseudonym mapping"
    );
    // …and the other three permissions are untouched.
    for purpose in [
        Purpose::BotTraining,
        Purpose::RetentionAfterProject,
        Purpose::NamedAttribution,
    ] {
        assert!(
            corpus.permits("alizarin", purpose),
            "revoking publication also revoked {purpose}"
        );
    }
    assert_eq!(
        corpus
            .audit_purpose("alizarin", Purpose::Publication)
            .expect("audit"),
        Vec::<String>::new(),
        "a use of publication still reaches a participant who withdrew it"
    );
    println!("withdraw publication: participation intact, 3 match(es) still held");
}

/// **The purpose audit catches a revocation that did not take.**
///
/// The mirror of the test above and the reason `audit_purpose` runs the use's
/// own gate rather than reading back the record: a revocation that wrote a
/// tombstone and left the permission granted looks completely successful from
/// the outside. Broken here by restoring the record after the fact, which is
/// exactly what a `withdraw_purpose` that forgot to write would leave.
#[test]
fn the_purpose_audit_catches_a_revocation_that_left_the_permission_granted() {
    let scratch = Scratch::new("revocation-not-taken");
    let corpus = everyone_agrees(&scratch);

    corpus
        .withdraw_purpose("alizarin", Purpose::Publication, "2026-09-20")
        .expect("withdraw the purpose");
    assert_eq!(
        corpus
            .audit_purpose("alizarin", Purpose::Publication)
            .expect("audit"),
        Vec::<String>::new()
    );

    // The revocation undone, with the tombstone left in place.
    std::fs::write(
        scratch.path().join("participants").join("alizarin.consent"),
        consent("alizarin", &Purpose::ALL).encode(),
    )
    .expect("write");
    assert!(
        scratch
            .path()
            .join("withdrawals")
            .join("alizarin.publication.withdrawn")
            .exists(),
        "the tombstone is gone too, so the audit is catching the wrong thing"
    );

    let reached = corpus
        .audit_purpose("alizarin", Purpose::Publication)
        .expect("audit");
    assert_eq!(
        reached,
        vec![filed_as("2026-09-03-a"), filed_as("2026-09-03-b")],
        "the audit did not report the matches a publication would still reach"
    );
    println!(
        "audit publication: {} match(es) still reachable",
        reached.len()
    );
}

// ---------------------------------------------------------------------------
// `bot-training`
// ---------------------------------------------------------------------------

/// **A session whose participant refused training is not in the training set,
/// and asking for it by name says who refused.**
#[test]
fn training_on_a_session_that_refused_it_is_refused() {
    let scratch = Scratch::new("training-refused");
    let corpus = populated(
        &scratch,
        &[
            ("alizarin", &Purpose::ALL),
            ("bistre", &Purpose::ALL),
            ("celadon", &[Purpose::Publication]),
        ],
    );

    let set = TrainingSet::of(&corpus).expect("training set");
    assert_eq!(
        set.matches(),
        [filed_as("2026-09-03-a")],
        "the training set is not the matches celadon is absent from"
    );
    assert!(
        !set.matches().is_empty(),
        "the training set is empty, so excluding a refusal proves nothing"
    );

    // Named, the refusal says who and why.
    for match_id in [filed_as("2026-09-03-b"), filed_as("2026-09-11-a")] {
        let refusal = TrainingSet::refusal(&corpus, &match_id)
            .expect("a match a participant refused training for is in the set");
        assert_eq!(
            refusal,
            PermitError::Refused {
                pseudonym: "celadon".to_owned(),
                purpose: Purpose::BotTraining,
            }
        );
        println!("training: {match_id} excluded — {refusal}");
    }

    // And the only accessor that yields data yields only the permitted match,
    // which is what makes the exclusion mean something rather than being a
    // count.
    let loaded = set.load(&corpus).expect("load");
    assert_eq!(loaded.len(), 1);
    for (replay, _) in &loaded {
        for pseudonym in replay.manifest.participants() {
            assert!(
                corpus.permits(&pseudonym.to_string(), Purpose::BotTraining),
                "{pseudonym} refused training and their match reached a trainer"
            );
        }
    }
}

/// **And a model's provenance is reached by the withdrawal that destroys what it
/// learned from.**
///
/// A trained model is the derived artefact `docs/CONSENT.md` warns about: a
/// `remove_dir_all` over a match directory cannot reach it. The rule is that a
/// model carries the pseudonyms it learned from, and the check is the one that
/// already works — the audit reads every byte under the root for a name. Planted
/// here exactly as `withdrawal.rs` plants an index, because it is the same
/// failure in a new shape.
#[test]
fn a_trained_model_is_reported_by_the_audit_of_a_participant_it_learned_from() {
    let scratch = Scratch::new("model-provenance");
    let corpus = everyone_agrees(&scratch);

    let set = TrainingSet::of(&corpus).expect("training set");
    assert_eq!(set.matches().len(), 3, "nothing was trainable");
    let provenance = set.provenance(&corpus).expect("provenance");
    assert!(
        provenance.contains("alizarin") && provenance.contains("bistre"),
        "the provenance names nobody, so an audit could never reach a model: \
         {provenance}"
    );

    // The model, stored where the rule says: under the corpus root, beside the
    // provenance that names who is in it.
    let model = scratch.path().join("models").join("policy-0001");
    std::fs::create_dir_all(&model).expect("directory");
    std::fs::write(model.join("provenance"), &provenance).expect("write");
    std::fs::write(model.join("weights"), b"\x00\x01\x02not really weights").expect("write");

    corpus.withdraw("alizarin", "2026-09-20").expect("withdraw");
    let traces = corpus.audit("alizarin").expect("audit");
    assert_eq!(
        traces.len(),
        1,
        "the audit did not report a model trained on this participant: {traces:?}"
    );
    assert!(traces[0].ends_with("provenance"));

    // …and once the model goes with it, the corpus is defensible again. The
    // audit is this project's definition of destroyed, and a withdrawal that
    // leaves a model behind has not finished.
    std::fs::remove_dir_all(&model).expect("remove");
    assert_eq!(
        corpus.audit("alizarin").expect("audit"),
        Vec::<PathBuf>::new()
    );
    println!("audit: a model whose provenance names a withdrawn participant is reported");
}

// ---------------------------------------------------------------------------
// `named-attribution`
// ---------------------------------------------------------------------------

/// **The one path from a pseudonym to a person refuses without the permission.**
#[test]
fn a_name_is_only_obtainable_for_somebody_who_agreed_to_be_named() {
    let scratch = Scratch::new("attribution");
    let corpus = populated(
        &scratch,
        &[
            ("alizarin", &[Purpose::NamedAttribution]),
            ("bistre", &Purpose::ALL),
            ("celadon", &[Purpose::Publication, Purpose::BotTraining]),
        ],
    );

    let named = corpus.attribution("alizarin").expect("a name");
    assert!(
        named.contains("alizarin de la Fontaine"),
        "the identity mapping was not read: {named}"
    );

    let refused = corpus
        .attribution("celadon")
        .expect_err("a name was handed out for somebody who refused to be named");
    assert_eq!(refused.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(
        refused.to_string().contains("celadon"),
        "the refusal does not name the pseudonym: {refused}"
    );
    println!("attribution: {refused}");

    // Revoking it closes the path, and the audit for that purpose is the check.
    assert_eq!(
        corpus
            .audit_purpose("bistre", Purpose::NamedAttribution)
            .expect("audit"),
        vec!["bistre".to_owned()],
        "the audit does not report that a name is still obtainable"
    );
    corpus
        .withdraw_purpose("bistre", Purpose::NamedAttribution, "2026-09-20")
        .expect("withdraw");
    assert!(corpus.attribution("bistre").is_err());
    assert_eq!(
        corpus
            .audit_purpose("bistre", Purpose::NamedAttribution)
            .expect("audit"),
        Vec::<String>::new()
    );
}

// ---------------------------------------------------------------------------
// `retention-after-project`
// ---------------------------------------------------------------------------

/// **Concluding the project destroys exactly the data of the people who refused
/// indefinite retention, and nothing else.**
#[test]
fn concluding_the_project_destroys_what_may_not_be_kept_past_it() {
    let scratch = Scratch::new("conclude");
    let corpus = populated(
        &scratch,
        &[
            ("alizarin", &Purpose::ALL),
            ("bistre", &Purpose::ALL),
            // Refused: destroyed when the work concludes rather than at the
            // retention date.
            ("celadon", &[Purpose::Publication, Purpose::BotTraining]),
        ],
    );

    assert_eq!(
        corpus.due_at_conclusion().expect("due"),
        vec!["celadon".to_owned()],
        "the corpus does not know who refused, so concluding proves nothing"
    );
    assert_eq!(corpus.matches().expect("matches").len(), 3);

    let carried = corpus.conclude("2027-03-01").expect("conclude");
    assert_eq!(carried.len(), 1);
    let (pseudonym, destroyed) = &carried[0];
    assert_eq!(pseudonym, "celadon");
    assert_eq!(
        destroyed.matches.len(),
        2,
        "celadon is in two matches and both go"
    );
    assert!(destroyed.identity && destroyed.consent);

    // The audit is the definition, and the people who agreed keep everything.
    assert_eq!(
        corpus.audit("celadon").expect("audit"),
        Vec::<PathBuf>::new()
    );
    assert_eq!(
        corpus.matches().expect("matches"),
        vec![filed_as("2026-09-03-a")],
        "a match nobody who refused was in was destroyed"
    );
    assert!(!corpus.audit("alizarin").expect("audit").is_empty());

    // Idempotent, and quiet when there is nobody left to destroy.
    assert!(corpus.conclude("2027-03-01").expect("conclude").is_empty());
    println!("conclude: 1 participant, 2 match(es) destroyed, 1 match kept");
}

// ---------------------------------------------------------------------------
// The two audits, side by side
// ---------------------------------------------------------------------------

/// **A partial withdrawal leaves a tombstone that says the participation is
/// unchanged, and destroys nothing.**
///
/// The two tombstones are deliberately different documents: one records a
/// destruction and counts it, the other records a revocation and says explicitly
/// that nothing was destroyed. An operator reading a `withdrawals/` directory has
/// to be able to tell the two apart at a glance.
#[test]
fn a_partial_withdrawal_leaves_a_tombstone_naming_the_purpose_and_nothing_else() {
    let scratch = Scratch::new("partial-tombstone");
    let corpus = everyone_agrees(&scratch);
    corpus
        .withdraw_purpose("alizarin", Purpose::BotTraining, "2026-09-20")
        .expect("withdraw");

    let tombstone = scratch
        .path()
        .join("withdrawals")
        .join("alizarin.bot-training.withdrawn");
    let text = std::fs::read_to_string(&tombstone).expect("the tombstone was not written");
    assert!(text.contains("withdrawn_purpose: bot-training"));
    assert!(text.contains("withdrawn_on: 2026-09-20"));
    assert!(text.contains("participation: unchanged"));
    assert!(
        !text.contains("example.invalid"),
        "the tombstone carries contact information"
    );

    // Withdrawing the same purpose twice is not an error and revokes nothing the
    // second time, for the reason a total withdrawal is idempotent.
    assert!(
        !corpus
            .withdraw_purpose("alizarin", Purpose::BotTraining, "2026-09-21")
            .expect("withdraw twice"),
        "the second revocation claimed to revoke something"
    );
    // …and a participant this corpus has never held is not an error either.
    assert!(
        !corpus
            .withdraw_purpose("nobody-at-all", Purpose::Publication, "2026-09-21")
            .expect("withdraw for a stranger")
    );
}

/// **A total withdrawal still reaches everything, with the permissions in the
/// record.**
///
/// The regression guard for the whole of this change: the granular record is a
/// bigger file with more lines in it, and the thing that must not have moved is
/// that a withdrawal destroys it along with everything else.
#[test]
fn a_total_withdrawal_still_destroys_the_permissions_and_every_match() {
    let scratch = Scratch::new("total-still-total");
    let corpus = everyone_agrees(&scratch);

    let before = bytes_under(corpus.root());
    assert!(
        before.contains("permits.bot-training: yes"),
        "no permission line reached the corpus, so destroying one proves nothing"
    );

    let destroyed = corpus.withdraw("alizarin", "2026-09-20").expect("withdraw");
    assert_eq!(destroyed.matches.len(), 2);
    assert!(destroyed.identity && destroyed.consent);
    assert_eq!(
        corpus.audit("alizarin").expect("audit"),
        Vec::<PathBuf>::new()
    );
    assert!(
        corpus.consent_of("alizarin").is_none(),
        "the consent record and its four permissions survived"
    );
    // And every purpose now answers no, because a record that is gone grants
    // nothing — which is the safe direction and the only one.
    for purpose in Purpose::ALL {
        assert!(
            !corpus.permits("alizarin", purpose),
            "{purpose} is still permitted for a participant whose record is gone"
        );
    }
    assert!(
        Publishable::of(&corpus, &filed_as("2026-09-11-a")).is_ok(),
        "a match the withdrawing participant was not in stopped being publishable"
    );
}

/// A use is refused for somebody with no readable record, and the refusal says
/// which of the two things went wrong.
#[test]
fn a_use_is_refused_for_a_participant_whose_record_does_not_read() {
    let scratch = Scratch::new("unconsented-use");
    let corpus = everyone_agrees(&scratch);
    std::fs::write(
        scratch.path().join("participants").join("bistre.consent"),
        "not a consent record\n",
    )
    .expect("write");

    let refused = Publishable::of(&corpus, &filed_as("2026-09-03-a"))
        .map(|publishable| publishable.match_id().to_owned())
        .expect_err("a match with an unreadable consent record was publishable");
    assert_eq!(
        refused,
        PermitError::Unconsented {
            pseudonym: "bistre".to_owned()
        },
        "an unreadable record was reported as a refusal, which is a different \
         thing for whoever has to fix it"
    );
    println!("publish: {refused}");
}

/// **A match this corpus cannot account for is published by nothing**, which is
/// the same rule the audit applies for every pseudonym at once.
#[test]
fn a_match_that_does_not_read_is_neither_publishable_nor_trainable() {
    let scratch = Scratch::new("unaccountable-use");
    let corpus = everyone_agrees(&scratch);
    let filed = filed_as("2026-09-03-a");
    let path = scratch
        .path()
        .join("matches")
        .join(&filed)
        .join("match.session");
    std::fs::write(&path, "not a session record\n").expect("corrupt");

    assert_eq!(
        Publishable::of(&corpus, &filed).err(),
        Some(PermitError::Unaccountable {
            match_id: filed.clone()
        })
    );
    assert_eq!(
        TrainingSet::refusal(&corpus, &filed),
        Some(PermitError::Unaccountable {
            match_id: filed.clone()
        })
    );
    assert!(
        !TrainingSet::of(&corpus)
            .expect("training set")
            .matches()
            .contains(&filed)
    );
}

/// Every byte of every file under a directory, as one string.
fn bytes_under(root: &Path) -> String {
    fn walk(directory: &Path, out: &mut String) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if let Ok(bytes) = std::fs::read(&path) {
                out.push_str(&String::from_utf8_lossy(&bytes));
                out.push('\n');
            }
        }
    }
    let mut out = String::new();
    walk(root, &mut out);
    out
}
