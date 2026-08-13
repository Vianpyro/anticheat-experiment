//! Withdrawal of consent, exercised by breaking it.
//!
//! `docs/CONSENT.md` promises a participant that withdrawing destroys every
//! match they played in, their pseudonym mapping and their consent record, and
//! that the destruction is checkable. A promise nobody can check is a promise,
//! so the interesting tests here are not the ones where `withdraw` works — they
//! are the ones where a deliberately incomplete withdrawal has to be *caught*.
//!
//! The audit is the thing under test. It reads every byte of every file under
//! the corpus root rather than checking the places the pseudonym is supposed to
//! be, and this file is where that crudeness earns its keep: three of the tests
//! below plant a trace somewhere `withdraw` does not look, and the audit has to
//! find it anyway.

#![deny(unsafe_code)]

use std::path::{Path, PathBuf};

use replay::consent::ConsentVersion;
use replay::corpus::{ConsentRecord, Corpus};
use replay::keys::SigningKey;
use replay::manifest::{MatchId, Pseudonym, SessionFacts, SimCommit};
use replay::session::{Clock, Declared, Measured, Platform, SeatRecord, SessionRecord};
use replay::{Recording, Replay};
use sim::{Outcome, PLAYER_COUNT, new_state, rules_hash};

/// A corpus in a directory of its own, removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "moba-corpus-{}-{name}-{}",
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

fn consent(pseudonym: &str) -> ConsentRecord {
    ConsentRecord {
        pseudonym: pseudonym.to_owned(),
        consented_on: "2026-08-13".to_owned(),
        retention_until: "2028-08-13".to_owned(),
        publication: false,
        consent_version: ConsentVersion::current(),
    }
}

/// One seat's record, of the shape a client's part decodes into.
///
/// The numbers are a plausible session and are not the subject: what these tests
/// are about is which of them `Corpus::store` refuses and whether a withdrawal
/// reaches the file they live in.
fn a_seat() -> SeatRecord {
    SeatRecord::Human {
        declared: Declared {
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

/// The session record that goes with `a_replay(match_id, participants)`.
///
/// The seats it fills are the seats the manifest fills, because
/// [`Corpus::store`] refuses the two disagreeing and every test here that is not
/// about that refusal wants them to agree.
fn a_session(match_id: &str, participants: usize) -> SessionRecord {
    let mut seats = [SeatRecord::Empty; PLAYER_COUNT];
    for slot in seats.iter_mut().take(participants) {
        *slot = a_seat();
    }
    SessionRecord {
        match_id: a_replay(match_id, &[]).manifest.match_id,
        consent_version: ConsentVersion::current(),
        recorded_on: "2026-09-03".to_owned(),
        seats,
    }
}

/// The key the corpus in these tests is sealed with. A written-down constant,
/// and not a secret: see `replay/tests/tamper.rs`.
const SEAL_SEED: [u8; 32] = *b"moba test corpus signing key...\0";

/// A short match, sealed and naming its participants.
///
/// The log names **seats and not people**; the manifest names the people, inside
/// the signature. That is the M5 change these tests are written against — before
/// it there was a `participants` file beside the recording, which was a derived
/// index able to drift from and outlive the thing it described.
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
        },
        &SigningKey::from_seed(SEAL_SEED),
    )
}

/// The directory name `Corpus::store` files a match under, which is its
/// identifier and is inside the signature.
fn filed_as(match_id: &str) -> String {
    a_replay(match_id, &[]).manifest.match_id.to_string()
}

/// A short recording, of the shape a real one has.
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
    // Eight ticks of nine champions standing still, resimulated so that the
    // manifest's digest is the one the log actually reaches. A fixture whose
    // digest was a constant would be a corpus of files that do not verify, and
    // `a_stored_match_still_verifies` is what would have found that.
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

/// A corpus with three participants and three matches, two of which the first
/// participant played in.
fn populated(scratch: &Scratch) -> Corpus {
    let corpus = scratch.corpus();
    for pseudonym in ["alizarin", "bistre", "celadon"] {
        corpus
            .enrol(&consent(pseudonym), &format!("{pseudonym}@example.invalid"))
            .expect("enrol");
    }
    for (id, who) in [
        ("2026-09-03-a", ["alizarin", "bistre"]),
        ("2026-09-03-b", ["alizarin", "celadon"]),
        ("2026-09-11-a", ["bistre", "celadon"]),
    ] {
        corpus
            .store(&a_replay(id, &who), &a_session(id, who.len()))
            .expect("store");
    }
    corpus
}

/// The promise, kept: everything goes, and the audit agrees.
#[test]
fn withdrawing_destroys_every_match_the_participant_played_in() {
    let scratch = Scratch::new("honoured");
    let corpus = populated(&scratch);

    assert!(
        !corpus.audit("alizarin").expect("audit").is_empty(),
        "the corpus does not hold this participant, so destroying them proves nothing"
    );

    let destroyed = corpus.withdraw("alizarin", "2026-09-20").expect("withdraw");
    assert_eq!(destroyed.matches.len(), 2, "matches destroyed");
    assert!(
        destroyed.identity,
        "the pseudonym mapping was not destroyed"
    );
    assert!(destroyed.consent, "the consent record was not destroyed");

    assert_eq!(
        corpus.audit("alizarin").expect("audit"),
        Vec::<PathBuf>::new(),
        "something about the participant survived"
    );

    // …and the other two are untouched, which is the half a delete-everything
    // implementation would also pass the assertion above with.
    assert_eq!(
        corpus.matches().expect("matches"),
        vec![filed_as("2026-09-11-a")],
        "a match the participant was not in was destroyed"
    );
    assert!(
        !corpus.audit("celadon").expect("audit").is_empty(),
        "another participant's data went with them"
    );
}

/// The tombstone is the one thing that survives, and it names nobody.
#[test]
fn a_withdrawal_leaves_a_record_that_it_happened_and_nothing_else() {
    let scratch = Scratch::new("tombstone");
    let corpus = populated(&scratch);
    corpus.withdraw("alizarin", "2026-09-20").expect("withdraw");

    let tombstone = scratch
        .path()
        .join("withdrawals")
        .join("alizarin.withdrawn");
    let text = std::fs::read_to_string(&tombstone).expect("the tombstone was not written");
    assert!(text.contains("withdrawn_on: 2026-09-20"));
    assert!(text.contains("matches_destroyed: 2"));

    // The identity file is what made the pseudonym point at a person, and it is
    // gone, so the surviving line identifies nobody.
    assert!(
        !scratch
            .path()
            .join("identities")
            .join("alizarin.identity")
            .exists(),
        "the pseudonym mapping survived the withdrawal"
    );
    assert!(
        !text.contains("example.invalid"),
        "the tombstone carries contact information"
    );
}

/// Withdrawing twice is not an error.
///
/// A participant who is not sure their message landed will send a second one,
/// and `docs/CONSENT.md` promises no consequence and no reason asked. An
/// implementation that failed on the second request would be turning a
/// participant's uncertainty into an error message.
#[test]
fn withdrawing_twice_destroys_nothing_the_second_time_and_is_not_an_error() {
    let scratch = Scratch::new("twice");
    let corpus = populated(&scratch);

    let first = corpus.withdraw("alizarin", "2026-09-20").expect("withdraw");
    let second = corpus.withdraw("alizarin", "2026-09-21").expect("withdraw");
    assert_eq!(first.matches.len(), 2);
    assert!(second.matches.is_empty());
    assert!(!second.identity);
    assert!(!second.consent);
    assert_eq!(
        corpus.audit("alizarin").expect("audit"),
        Vec::<PathBuf>::new()
    );
}

/// **The audit catches a withdrawal that forgot the pseudonym mapping.**
///
/// The mapping is the single most sensitive file in the project: it is the one
/// that turns an opaque identifier back into a person. A withdrawal that deleted
/// every match and left it behind would look completely successful from the
/// outside — no telemetry, no consent record, and a file nobody would think to
/// look for still tying the two together.
///
/// Deliberately broken here by putting the mapping back after the withdrawal,
/// which is exactly what a `withdraw` that never unlinked it would leave.
#[test]
fn an_audit_catches_a_withdrawal_that_kept_the_pseudonym_mapping() {
    let scratch = Scratch::new("kept-identity");
    let corpus = populated(&scratch);
    corpus.withdraw("alizarin", "2026-09-20").expect("withdraw");
    assert_eq!(
        corpus.audit("alizarin").expect("audit"),
        Vec::<PathBuf>::new()
    );

    std::fs::create_dir_all(scratch.path().join("identities")).expect("directory");
    std::fs::write(
        scratch.path().join("identities").join("alizarin.identity"),
        "pseudonym: alizarin\nidentity: alizarin@example.invalid\n",
    )
    .expect("write");

    let traces = corpus.audit("alizarin").expect("audit");
    assert_eq!(traces.len(), 1, "the audit missed the mapping: {traces:?}");
    assert!(
        traces[0].ends_with("alizarin.identity"),
        "the audit named {traces:?}"
    );
}

/// **The audit catches a withdrawal that missed a match.**
///
/// One directory left behind is one match's worth of a participant's input
/// telemetry, which is the thing the consent text promised to destroy. Broken
/// here by restoring one match after the fact.
#[test]
fn an_audit_catches_a_withdrawal_that_left_one_match_behind() {
    let scratch = Scratch::new("kept-match");
    let corpus = populated(&scratch);
    corpus.withdraw("alizarin", "2026-09-20").expect("withdraw");

    corpus
        .enrol(&consent("alizarin"), "temporary")
        .expect("re-enrol so that store accepts it");
    corpus
        .store(
            &a_replay("2026-09-03-a", &["alizarin", "bistre"]),
            &a_session("2026-09-03-a", 2),
        )
        .expect("store");
    // Undo the re-enrolment, so that what is left is exactly the state a
    // withdrawal that skipped one match directory would leave.
    std::fs::remove_file(scratch.path().join("participants").join("alizarin.consent"))
        .expect("remove");
    std::fs::remove_file(scratch.path().join("identities").join("alizarin.identity"))
        .expect("remove");

    let traces = corpus.audit("alizarin").expect("audit");
    assert_eq!(traces.len(), 1, "the audit missed the match: {traces:?}");
    assert!(
        traces[0].ends_with("match.replay"),
        "the audit named {traces:?}"
    );
}

/// **And the audit catches the match nobody can account for.**
///
/// The orphan, which is the case a search for a name structurally cannot reach:
/// a *log* holds seats, not people, so telemetry with no readable manifest in
/// front of it is somebody's input record that no audit for any pseudonym would
/// ever match. It is reported unconditionally, because the question the operator
/// is asking is whether the corpus is in a state they can defend, and a match
/// nobody can account for is not.
///
/// M5 narrowed the case and did not close it. The participant list used to be a
/// separate file that could be deleted while the recording survived; it is
/// inside the signature now, so what is left is a replay that does not decode —
/// a truncated write, a half-finished copy, an edit. Broken here by truncating
/// one.
#[test]
fn an_audit_catches_a_match_nobody_can_account_for() {
    let scratch = Scratch::new("orphan");
    let corpus = populated(&scratch);

    let filed = filed_as("2026-09-11-a");
    let path = scratch
        .path()
        .join("matches")
        .join(&filed)
        .join("match.replay");
    let bytes = std::fs::read(&path).expect("read");
    std::fs::write(&path, &bytes[..bytes.len() / 2]).expect("truncate");

    for pseudonym in ["alizarin", "bistre", "celadon", "nobody-at-all"] {
        let traces = corpus.audit(pseudonym).expect("audit");
        assert!(
            traces.iter().any(|path| path.ends_with(&filed)),
            "auditing {pseudonym} did not report the unaccountable match: {traces:?}"
        );
    }
}

/// **A stored match still verifies**, which is the assertion that makes every
/// other test in this file about a corpus rather than about a directory.
///
/// A corpus of files that do not verify is not a corpus, it is a folder. The
/// withdrawal machinery would work perfectly on one and the promise it keeps
/// would be worth nothing, because the thing destroyed was never evidence of
/// anything (`docs/RISKS.md` R15).
#[test]
fn a_stored_match_still_verifies_and_names_the_people_the_corpus_thinks_it_does() {
    let scratch = Scratch::new("verifies");
    let corpus = populated(&scratch);
    let filed = filed_as("2026-09-03-a");

    let stored = corpus.replay_of(&filed).expect("the replay");
    let mut keys = replay::KeyRegistry::new();
    keys.insert(
        SigningKey::from_seed(SEAL_SEED).verifying(),
        replay::KeyStatus::Active,
        "corpus-test",
    );
    let verified = replay::verify(&stored, &keys, &replay::Build::current())
        .expect("a stored match did not verify");
    assert_eq!(
        verified.final_state_digest,
        stored.manifest.final_state_digest
    );

    // …and the corpus reads its participants out of that manifest rather than
    // out of anything beside it. This is the M5 change: there is one statement
    // of who played a match and it is the signed one.
    assert_eq!(
        corpus.participants_of(&filed).expect("participants"),
        vec!["alizarin".to_owned(), "bistre".to_owned()]
    );
    assert_eq!(
        corpus.sealed_by(&filed).expect("identity"),
        SigningKey::from_seed(SEAL_SEED).verifying()
    );
}

/// **The corpus builds no index, and the audit is what keeps it that way.**
///
/// `docs/CONSENT.md` promises destruction, and the way that promise fails is not
/// a match directory somebody forgot: it is a *derived* artefact that outlives
/// what it was derived from — a summary, a cache, a list of who played what.
/// Until M5 this corpus had exactly one, a `participants` file written beside
/// each recording, and it was deleted along with the match only because
/// `withdraw` removed the whole directory.
///
/// Two halves here. The first is that `store` writes nothing but the replay, so
/// there is no second place a pseudonym lives. The second is the guard: an index
/// added later, anywhere under the root, is reported by the audit — which is why
/// the audit reads every byte of every file rather than the places it knows
/// about.
#[test]
fn a_withdrawal_reaches_a_derived_index_because_the_audit_refuses_to_let_one_hide() {
    let scratch = Scratch::new("no-index");
    let corpus = populated(&scratch);
    let filed = filed_as("2026-09-03-a");

    // Half one: `store` wrote the replay and the session record, and nothing
    // else. Two files rather than one since M6, and the second is not an index —
    // it is primary, it names no pseudonym, and nothing in it is derived from
    // the replay. What this assertion is still for is the *third* file: a
    // summary, a cache, a participant list, anything computed from what is
    // already here.
    let directory = scratch.path().join("matches").join(&filed);
    let mut written: Vec<String> = std::fs::read_dir(&directory)
        .expect("read the match directory")
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    written.sort();
    assert_eq!(
        written,
        vec!["match.replay".to_owned(), "match.session".to_owned()],
        "`store` wrote something beside the replay and the session record, which \
         is an index by another name: {written:?}"
    );
    assert!(
        !std::fs::read_to_string(directory.join("match.session"))
            .expect("the session record")
            .contains("alizarin"),
        "the session record names a participant, which is the second naming M5 \
         removed arriving in a new file (docs/CONSENT.md)"
    );

    // Half two: somebody adds one anyway — the shape a later milestone's
    // convenience would take, outside every directory `withdraw` knows about.
    let index = scratch.path().join("index").join("by-participant.txt");
    std::fs::create_dir_all(index.parent().expect("parent")).expect("directory");
    std::fs::write(
        &index,
        format!("alizarin\t{filed}\nbistre\t{filed}\nceladon\t-\n"),
    )
    .expect("write");

    corpus.withdraw("alizarin", "2026-09-20").expect("withdraw");
    let traces = corpus.audit("alizarin").expect("audit");
    assert_eq!(
        traces.len(),
        1,
        "the audit did not report the derived index: {traces:?}"
    );
    assert!(traces[0].ends_with("by-participant.txt"));

    // …and once it is destroyed too, the corpus is defensible again. The audit
    // is the definition of "destroyed" this project uses, and a withdrawal that
    // leaves an index is a withdrawal that has not finished.
    std::fs::remove_file(&index).expect("remove");
    assert_eq!(
        corpus.audit("alizarin").expect("audit"),
        Vec::<PathBuf>::new()
    );
}

/// **The audit catches a trace in a place nobody thought of.**
///
/// This is why it reads every byte of every file rather than checking the four
/// directories it knows about. A stray export, an editor's backup, a directory a
/// later milestone adds — none of them are in `withdraw`'s list, and all of them
/// are personal information if they name a participant.
#[test]
fn an_audit_catches_a_trace_outside_every_directory_it_knows_about() {
    let scratch = Scratch::new("stray");
    let corpus = populated(&scratch);
    corpus.withdraw("alizarin", "2026-09-20").expect("withdraw");
    assert_eq!(
        corpus.audit("alizarin").expect("audit"),
        Vec::<PathBuf>::new()
    );

    let stray = scratch.path().join("exports").join("summary.txt");
    std::fs::create_dir_all(stray.parent().expect("parent")).expect("directory");
    std::fs::write(&stray, "alizarin: 42 matches, mean inter-arrival 31 ms\n").expect("write");

    let traces = corpus.audit("alizarin").expect("audit");
    assert_eq!(
        traces.len(),
        1,
        "the audit missed the stray file: {traces:?}"
    );
    assert!(traces[0].ends_with("summary.txt"));
}

/// A match nobody consented to cannot enter the corpus.
#[test]
fn a_recording_naming_someone_with_no_consent_record_is_refused() {
    let scratch = Scratch::new("unconsented");
    let corpus = scratch.corpus();
    corpus
        .enrol(&consent("alizarin"), "alizarin@example.invalid")
        .expect("enrol");

    let refused = corpus.store(
        &a_replay("2026-09-03-a", &["alizarin", "nobody"]),
        &a_session("2026-09-03-a", 2),
    );
    assert!(
        refused.is_err(),
        "a match with an unconsented player was stored"
    );
    assert!(
        corpus.matches().expect("matches").is_empty(),
        "the refusal left the match behind anyway"
    );
}

/// A consent record survives its own encoding, so a participant asking what is
/// held about them is shown the same fields the program reads.
#[test]
fn a_consent_record_reads_back_as_what_was_written() {
    let record = ConsentRecord {
        pseudonym: "celadon".to_owned(),
        consented_on: "2026-08-13".to_owned(),
        retention_until: "2028-08-13".to_owned(),
        publication: true,
        consent_version: ConsentVersion::current(),
    };
    assert_eq!(ConsentRecord::decode(&record.encode()), Some(record));
    assert_eq!(ConsentRecord::decode("nonsense"), None);
}

// ---------------------------------------------------------------------------
// M6: the refusals that make the consent regime mechanical, and the schema
// the corpus gained beside the replay.
// ---------------------------------------------------------------------------

/// **A consent record from another version of the document is not consent.**
///
/// `docs/CONSENT.md` is a text somebody signed on a day. A version of it that
/// gained a field — a new covariate, a new retention rule — has stopped being
/// that text, and a corpus of replays would say nothing about the difference.
/// This is the difference made mechanical.
#[test]
fn a_match_recorded_under_a_superseded_consent_document_is_refused() {
    let scratch = Scratch::new("stale-consent");
    let corpus = scratch.corpus();
    let mut stale = consent("alizarin");
    stale.consent_version = ConsentVersion::parse("2020-01-01").expect("a version");
    corpus
        .enrol(&stale, "alizarin@example.invalid")
        .expect("enrol");
    corpus
        .enrol(&consent("bistre"), "bistre@example.invalid")
        .expect("enrol");

    let refused = corpus
        .store(
            &a_replay("2026-09-03-a", &["alizarin", "bistre"]),
            &a_session("2026-09-03-a", 2),
        )
        .expect_err("a match under a superseded document was stored");
    assert_eq!(refused.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(
        refused.to_string().contains("2020-01-01"),
        "the refusal does not say which document was signed: {refused}"
    );
    assert!(corpus.matches().expect("matches").is_empty());
}

/// **And a record with no version at all fails the same way**, which is the half
/// that matters for a corpus assembled before this existed.
#[test]
fn a_consent_record_written_before_the_version_existed_is_not_a_consent_record() {
    let scratch = Scratch::new("versionless-consent");
    let corpus = scratch.corpus();
    corpus
        .enrol(&consent("alizarin"), "alizarin@example.invalid")
        .expect("enrol");
    corpus
        .enrol(&consent("bistre"), "bistre@example.invalid")
        .expect("enrol");

    // Exactly the four lines the M5 format had.
    std::fs::write(
        scratch.path().join("participants").join("alizarin.consent"),
        "pseudonym: alizarin\nconsented_on: 2026-08-13\nretention_until: \
         2028-08-13\npublication: false\n",
    )
    .expect("write");

    let refused = corpus
        .store(
            &a_replay("2026-09-03-a", &["alizarin", "bistre"]),
            &a_session("2026-09-03-a", 2),
        )
        .expect_err("a match with a versionless consent record was stored");
    assert_eq!(refused.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(
        refused.to_string().contains("no readable consent record"),
        "refused for the wrong reason: {refused}"
    );
}

/// **One person cannot fill several seats.**
///
/// `docs/MILESTONES.md` M6 says what must not be traded is the *people* count: a
/// corpus of forty matches from four people supports nothing, because the null
/// model a behavioural detector needs is a distribution over humans. A pseudonym
/// in two seats of one match is that failure in its smallest form, and it is the
/// one shape of synthetic contamination a file can actually see.
#[test]
fn one_pseudonym_cannot_occupy_two_seats_of_one_match() {
    let scratch = Scratch::new("two-seats");
    let corpus = scratch.corpus();
    corpus
        .enrol(&consent("alizarin"), "alizarin@example.invalid")
        .expect("enrol");

    let refused = corpus
        .store(
            &a_replay("2026-09-03-a", &["alizarin", "alizarin"]),
            &a_session("2026-09-03-a", 2),
        )
        .expect_err("one person filled two seats and the corpus took it");
    assert_eq!(refused.kind(), std::io::ErrorKind::InvalidInput);
    assert!(
        refused.to_string().contains("more than one seat"),
        "refused for the wrong reason: {refused}"
    );
}

/// **A seat with no device behind it is a script, and scripts are not in this
/// corpus.**
///
/// The one mechanical defence M6 has against synthetic play, and it is narrow by
/// construction: a headless client records no device event and is caught here; a
/// bot moving a real mouse records as many as a person and is not. That is
/// `docs/SCOPE.md`'s stated ceiling arriving early, and `docs/SCHEMA.md` says so
/// where it states the rule.
#[test]
fn a_seat_that_recorded_no_device_event_is_refused() {
    let scratch = Scratch::new("silent-seat");
    let corpus = scratch.corpus();
    for who in ["alizarin", "bistre"] {
        corpus
            .enrol(&consent(who), &format!("{who}@example.invalid"))
            .expect("enrol");
    }

    let mut session = a_session("2026-09-03-a", 2);
    if let SeatRecord::Human { measured, .. } = &mut session.seats[1] {
        measured.samples = 0;
        measured.motions = 0;
    }
    let refused = corpus
        .store(&a_replay("2026-09-03-a", &["alizarin", "bistre"]), &session)
        .expect_err("a seat with no device events was stored");
    assert_eq!(refused.kind(), std::io::ErrorKind::InvalidInput);
    assert!(
        refused.to_string().contains("no device event"),
        "refused for the wrong reason: {refused}"
    );
}

/// **A session played through the operating system's pointer acceleration is
/// refused rather than flagged.**
#[test]
fn a_seat_that_declares_pointer_acceleration_is_refused() {
    let scratch = Scratch::new("accelerated");
    let corpus = scratch.corpus();
    for who in ["alizarin", "bistre"] {
        corpus
            .enrol(&consent(who), &format!("{who}@example.invalid"))
            .expect("enrol");
    }

    let mut session = a_session("2026-09-03-a", 2);
    if let SeatRecord::Human { declared, .. } = &mut session.seats[0] {
        declared.pointer_acceleration = true;
    }
    let refused = corpus
        .store(&a_replay("2026-09-03-a", &["alizarin", "bistre"]), &session)
        .expect_err("an accelerated session was stored");
    assert!(
        refused.to_string().contains("pointer"),
        "refused for the wrong reason: {refused}"
    );
}

/// **The replay and the session record have to agree about who was playing.**
///
/// The failure this catches is the ordinary one: nine files collected from nine
/// machines, one of them missing, and a match filed as though a seat had been
/// empty. A corpus that accepted it would hold a match whose hardware covariates
/// are missing for one of the people in it and would not know.
#[test]
fn a_session_record_that_disagrees_with_the_replay_about_a_seat_is_refused() {
    let scratch = Scratch::new("seat-disagreement");
    let corpus = scratch.corpus();
    for who in ["alizarin", "bistre"] {
        corpus
            .enrol(&consent(who), &format!("{who}@example.invalid"))
            .expect("enrol");
    }

    // Two people in the replay, one part collected.
    let refused = corpus
        .store(
            &a_replay("2026-09-03-a", &["alizarin", "bistre"]),
            &a_session("2026-09-03-a", 1),
        )
        .expect_err("a match with a missing session part was stored");
    assert_eq!(refused.kind(), std::io::ErrorKind::InvalidInput);
    assert!(
        refused.to_string().contains("seat 1"),
        "the refusal does not name the seat: {refused}"
    );

    // …and a part collected for a seat nobody sat in fails the same way.
    let refused = corpus
        .store(
            &a_replay("2026-09-03-b", &["alizarin"]),
            &a_session("2026-09-03-b", 2),
        )
        .expect_err("a match with a session part for an empty seat was stored");
    assert!(refused.to_string().contains("seat 1"));
}

/// **A session record filed beside the wrong replay is refused.**
#[test]
fn a_session_record_naming_another_match_is_refused() {
    let scratch = Scratch::new("wrong-match");
    let corpus = scratch.corpus();
    corpus
        .enrol(&consent("alizarin"), "alizarin@example.invalid")
        .expect("enrol");

    let refused = corpus
        .store(
            &a_replay("2026-09-03-a", &["alizarin"]),
            &a_session("2026-09-03-b", 1),
        )
        .expect_err("a session record for another match was stored");
    assert_eq!(refused.kind(), std::io::ErrorKind::InvalidInput);
}

/// **The audit catches a match directory whose two files do not both read**, in
/// each direction separately.
///
/// This is the M6 shape of the orphan and the reason the audit's unaccountable
/// case had to grow. A session record names no pseudonym — deliberately, so that
/// the signed manifest stays the one naming of a person — so a search for a name
/// structurally cannot find one left behind. What is left is a description of
/// somebody's hardware and somebody's session with nothing to say whose.
///
/// **Two halves, and the second one is the reason this test is written this
/// way.** The obvious version deletes the replay and keeps the record, and it
/// passes whether or not the audit looks at session records at all — because the
/// missing replay trips the check that was already there. Removing the session
/// half of `Corpus::audit`'s condition left that version green, which is
/// `docs/RISKS.md` R15 with a fresh coat of paint. So the second half plants the
/// other direction: a replay that reads perfectly and a session record that does
/// not, audited for a pseudonym the corpus has never held, where the *only* thing
/// that can report the directory is the session check.
#[test]
fn an_audit_catches_a_match_whose_replay_and_session_record_do_not_both_read() {
    let scratch = Scratch::new("orphan-session");
    let corpus = populated(&scratch);

    // Half one: the replay is gone and the session record is left behind.
    let filed = filed_as("2026-09-03-a");
    let directory = scratch.path().join("matches").join(&filed);
    std::fs::remove_file(directory.join("match.replay")).expect("remove the replay");
    for pseudonym in ["alizarin", "bistre", "celadon", "nobody-at-all"] {
        let traces = corpus.audit(pseudonym).expect("audit");
        assert!(
            traces.iter().any(|path| path.ends_with(&filed)),
            "auditing {pseudonym} did not report the session record whose match is \
             gone: {traces:?}"
        );
    }
    std::fs::remove_dir_all(&directory).expect("remove");

    // Half two: the replay reads and the session record does not, audited for a
    // pseudonym nothing in the corpus mentions — so the name search finds
    // nothing and only the session check can speak.
    let other = filed_as("2026-09-11-a");
    let directory = scratch.path().join("matches").join(&other);
    assert_eq!(
        corpus.audit("nobody-at-all").expect("audit"),
        Vec::<PathBuf>::new(),
        "the corpus already reports something for a pseudonym it has never held"
    );
    std::fs::write(directory.join("match.session"), "not a session record\n").expect("corrupt");
    assert!(
        corpus.replay_of(&other).is_ok(),
        "the replay stopped reading too, so this half is the first half again"
    );

    let traces = corpus.audit("nobody-at-all").expect("audit");
    assert_eq!(
        traces.len(),
        1,
        "a match whose session record does not read was not reported: {traces:?}"
    );
    assert!(traces[0].ends_with(&other));

    // …and once the directory goes, the corpus is defensible again.
    std::fs::remove_dir_all(&directory).expect("remove");
    assert_eq!(
        corpus.audit("nobody-at-all").expect("audit"),
        Vec::<PathBuf>::new()
    );
}

/// **And a withdrawal reaches it**, which is the half the audit above only
/// checks the absence of.
///
/// Every field M6 added to the schema lives in this one file, so "the new fields
/// are destroyed on withdrawal" is the claim that the file goes — asserted by
/// reading the sensitivity out of it before the withdrawal and requiring the
/// path to be gone after.
#[test]
fn withdrawing_destroys_the_session_record_and_every_field_m6_added_to_it() {
    let scratch = Scratch::new("withdraw-session");
    let corpus = populated(&scratch);
    let filed = filed_as("2026-09-03-a");
    let record = scratch
        .path()
        .join("matches")
        .join(&filed)
        .join("match.session");

    let before = std::fs::read_to_string(&record).expect("the session record");
    for field in [
        "device_cpi",
        "device_polling_hz",
        "pointer_acceleration",
        "platform",
        "clock",
        "world_units_per_count_e6",
        "samples",
        "median_gap_ns",
        "budget_ns",
        "passes_over_budget",
        "worst_overrun_ns",
    ] {
        assert!(
            before.contains(field),
            "the schema has no {field}, so destroying it proves nothing"
        );
    }

    corpus.withdraw("alizarin", "2026-09-20").expect("withdraw");
    assert!(
        !record.exists(),
        "the session record survived the withdrawal of somebody who is in it"
    );
    assert_eq!(
        corpus.audit("alizarin").expect("audit"),
        Vec::<PathBuf>::new()
    );
}

/// A session record survives its own encoding, field for field.
#[test]
fn a_session_record_reads_back_as_what_was_written() {
    let mut record = a_session("2026-09-03-a", 3);
    if let SeatRecord::Human { measured, .. } = &mut record.seats[2] {
        measured.platform = Platform::Windows;
        measured.passes_over_budget = 7;
        measured.worst_overrun_ns = 12_345_678;
    }
    assert_eq!(
        SessionRecord::decode(&record.encode()),
        Some(record.clone())
    );
    assert!(SessionRecord::decode("nonsense").is_none());
    assert!(
        record.degraded(),
        "a seat over budget is a degraded session"
    );
    assert_eq!(record.occupied(), vec![0, 1, 2]);
}
