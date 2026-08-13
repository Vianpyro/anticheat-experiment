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

use replay::Recording;
use replay::corpus::{ConsentRecord, Corpus};
use sim::{Digest, rules_hash};

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

/// A short recording, of the shape a real one has.
///
/// It names **seats and not people**, because that is what a recording holds:
/// the only thing tying a seat to a person is the match's participant list. That
/// is why the audit has a second job — see the orphan case in
/// `Corpus::audit` — and why the tests below can distinguish "the match went"
/// from "the pointer to it went".
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
    Recording {
        seed: 7,
        rules_hash: rules_hash(),
        ticks: 8,
        final_state_digest: Digest::from_bytes([0u8; 32]),
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
        .store(
            "2026-09-03-a",
            &a_recording(),
            &["alizarin".to_owned(), "bistre".to_owned()],
            "2026-09-03",
        )
        .expect("store");
    corpus
        .store(
            "2026-09-03-b",
            &a_recording(),
            &["alizarin".to_owned(), "celadon".to_owned()],
            "2026-09-03",
        )
        .expect("store");
    corpus
        .store(
            "2026-09-11-a",
            &a_recording(),
            &["bistre".to_owned(), "celadon".to_owned()],
            "2026-09-11",
        )
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
        vec!["2026-09-11-a".to_owned()],
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
            "2026-09-03-a",
            &a_recording(),
            &["alizarin".to_owned(), "bistre".to_owned()],
            "2026-09-03",
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
        traces[0].ends_with("participants"),
        "the audit named {traces:?}"
    );
}

/// **And the audit catches the match whose participant list went instead.**
///
/// The orphan, which is the case a search for a name structurally cannot reach:
/// a recording holds seats, not people, so a match directory that has lost its
/// participant list is somebody's input telemetry that no audit for any
/// pseudonym would ever match. It is reported unconditionally, because the
/// question the operator is asking is whether the corpus is in a state they can
/// defend, and a recording nobody can account for is not.
#[test]
fn an_audit_catches_a_match_that_lost_the_list_of_who_played_it() {
    let scratch = Scratch::new("orphan");
    let corpus = populated(&scratch);

    std::fs::remove_file(
        scratch
            .path()
            .join("matches")
            .join("2026-09-11-a")
            .join("participants"),
    )
    .expect("remove");

    for pseudonym in ["alizarin", "bistre", "celadon", "nobody-at-all"] {
        let traces = corpus.audit(pseudonym).expect("audit");
        assert!(
            traces.iter().any(|path| path.ends_with("2026-09-11-a")),
            "auditing {pseudonym} did not report the orphaned match: {traces:?}"
        );
    }
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
        "2026-09-03-a",
        &a_recording(),
        &["alizarin".to_owned(), "nobody".to_owned()],
        "2026-09-03",
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
    };
    assert_eq!(ConsentRecord::decode(&record.encode()), Some(record));
    assert_eq!(ConsentRecord::decode("nonsense"), None);
}
