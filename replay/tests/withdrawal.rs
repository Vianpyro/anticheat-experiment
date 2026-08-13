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

use replay::corpus::{ConsentRecord, Corpus};
use replay::keys::SigningKey;
use replay::manifest::{MatchId, Pseudonym, SessionFacts, SimCommit};
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
    corpus
        .store(&a_replay("2026-09-03-a", &["alizarin", "bistre"]))
        .expect("store");
    corpus
        .store(&a_replay("2026-09-03-b", &["alizarin", "celadon"]))
        .expect("store");
    corpus
        .store(&a_replay("2026-09-11-a", &["bistre", "celadon"]))
        .expect("store");
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
        .store(&a_replay("2026-09-03-a", &["alizarin", "bistre"]))
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

    // Half one: the only file `store` wrote is the replay itself.
    let directory = scratch.path().join("matches").join(&filed);
    let mut written: Vec<String> = std::fs::read_dir(&directory)
        .expect("read the match directory")
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    written.sort();
    assert_eq!(
        written,
        vec!["match.replay".to_owned()],
        "`store` wrote something beside the replay, which is an index by another \
         name: {written:?}"
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

    let refused = corpus.store(&a_replay("2026-09-03-a", &["alizarin", "nobody"]));
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
    };
    assert_eq!(ConsentRecord::decode(&record.encode()), Some(record));
    assert_eq!(ConsentRecord::decode("nonsense"), None);
}
