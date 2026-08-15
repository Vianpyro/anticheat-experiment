//! The destruction procedure, executed end to end through the tool an operator
//! actually types.
//!
//! # Why this exists beside `withdrawal.rs`
//!
//! `replay/tests/withdrawal.rs` exercises the *library*: it breaks the
//! destruction three ways and requires the audit to catch each. What it does not
//! establish is that the **procedure** works — that the sequence of commands
//! `docs/SCHEMA.md` tells an operator to run, run in that order against a corpus
//! built the way an operator builds one, ends with a corpus holding nothing about
//! the participant and exiting zero when asked.
//!
//! `docs/MILESTONES.md` M6 asks for exactly that and phrases it precisely: "a
//! written destruction procedure that has been executed once end to end on a
//! discarded test recording". This is the execution, it runs on every pull
//! request rather than once, and the recording it destroys is discarded in the
//! strongest sense — it is built in a temporary directory and never existed
//! anywhere else.
//!
//! # It drives the binary, not the crate
//!
//! For the reason `client/tests/m3_exit.rs` boots `replay verify` as a separate
//! process: a procedure checked by calling the functions it is a procedure *for*
//! is a procedure agreeing with itself. What an operator has is a shell and five
//! commands, and what can be wrong with those — an argument in the wrong place, a
//! command that reports success while doing nothing, an exit status nobody set —
//! is invisible from inside the library.

#![deny(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use replay::keys::SigningKey;
use replay::manifest::{MatchId, Pseudonym, SessionFacts, SimCommit};
use replay::{Recording, TimedInput};
use sim::{Outcome, PLAYER_COUNT, new_state, rules_hash};

/// The tool, as an operator invokes it.
fn replay_tool() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_replay"))
}

/// A corpus and a staging directory, removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("moba-destruction-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a scratch directory");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs the tool and prints what it said, so a failing run is readable in the
/// job log rather than only in a status code.
fn run(arguments: &[&str]) -> Output {
    let output = Command::new(replay_tool())
        .args(arguments)
        .output()
        .expect("run the replay tool");
    println!("$ replay {}", arguments.join(" "));
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    output
}

/// The key the discarded recording is sealed with. A written-down constant, and
/// not a secret: it seals one throwaway match in a temporary directory.
const SEAL_SEED: [u8; 32] = *b"moba destruction drill key.....\0";

/// A short recording, resimulated so that the manifest's digest is one the log
/// actually reaches.
fn a_recording() -> Recording {
    let inputs: Vec<TimedInput> = (0..8u32)
        .map(|index| TimedInput {
            input: sim::Input {
                tick: sim::Tick(index),
                seq: index,
                player: sim::Seat::Blue0,
                action: sim::Action::Idle,
            },
            claimed_at_ms: 1_786_000_000_000 + u64::from(index),
            received_at_ms: 1_786_000_000_007 + u64::from(index),
        })
        .collect();
    let mut state = new_state(11);
    for _ in 0..8 {
        state = sim::step(&state, &[]);
    }
    Recording {
        seed: 11,
        rules_hash: rules_hash(),
        ticks: 8,
        outcome: Outcome::InProgress,
        final_state_digest: state.digest(),
        inputs,
    }
}

/// Writes a sealed replay naming two seats, and returns its path.
fn seal_to(path: &Path, participants: [&str; 2]) -> MatchId {
    let mut slots: [Option<Pseudonym>; PLAYER_COUNT] = [const { None }; PLAYER_COUNT];
    for (slot, who) in slots.iter_mut().zip(participants) {
        *slot = Pseudonym::parse(who);
    }
    let replay = replay::seal(
        &a_recording(),
        &SessionFacts {
            match_id: MatchId(*b"m6-destruction!\0"),
            started_at_unix_ms: 1_786_000_000_000,
            participants: slots,
            sim_commit: SimCommit::Unknown,
            telemetry: replay::Commitment::Absent,
        },
        &SigningKey::from_seed(SEAL_SEED),
    );
    std::fs::write(path, replay.encode()).expect("write the replay");
    replay.manifest.match_id
}

/// One client's session part, of exactly the shape `client::health` writes.
///
/// Written out here rather than imported, because `replay` may not depend on
/// `client` — and the coupling that creates is closed on the other side, in
/// `client/tests/session_part.rs`, where a test binary links both and requires
/// the two to agree field for field.
fn a_part(seat: usize) -> String {
    format!(
        "format: moba/session-part/v1\nseat: {seat}\nprovenance: human\n\
         device_cpi: 800\ndevice_polling_hz: 1000\npointer_acceleration: off\n\
         platform: linux\nclock: dequeue\nworld_units_per_count_e6: 50000\n\
         samples: 91234\nmotions: 90880\ncoincident: 0\nmedian_gap_ns: 1000000\n\
         budget_ns: 33333333\npasses: 24010\npasses_over_budget: 0\n\
         worst_overrun_ns: 0\nworst_pass_ns: 5144000\n\
         device_profile_id: mouse-{seat}\n\
         calibration.reaches: 16\ncalibration.octants: 255\n\
         calibration.clamped: 0\ncalibration.min_distance_e3: 14100\n\
         calibration.max_distance_e3: 237500\n\
         calibration.sum_distance_e3: 1600000\n\
         calibration.sum_counts_e3: 32128000\n\
         calibration.sum_distance_sq_e3: 240000000\n\
         calibration.sum_distance_counts_e3: 4800800000\n\
         calibration.sum_counts_sq_e3: 96032000000\n\
         calibration.fast_reaches: 12\ncalibration.fast_motions: 900\n\
         calibration.fast_ns: 7200000000\ncalibration.quantum_e6: 1000000\n"
    )
}

/// **The procedure, run end to end on a recording that is thrown away.**
///
/// Every step is a command from `docs/SCHEMA.md`, in the order that document
/// gives them, and every one of them is checked for what it did rather than only
/// for what it printed.
#[test]
fn the_destruction_procedure_runs_end_to_end_on_a_discarded_recording() {
    let scratch = Scratch::new("procedure");
    let corpus = scratch.join("corpus");
    let staging = scratch.join("staging");
    let replay_path = scratch.join("match.replay");
    std::fs::create_dir_all(&staging).expect("a staging directory");

    let root = corpus.display().to_string();
    let staged = staging.display().to_string();
    let sealed = replay_path.display().to_string();

    // 1. Enrol. The consent record is stamped with the version of the document
    //    this build holds; the identity is the one string that maps back to a
    //    person and it lives in its own directory.
    for who in ["drill-one", "drill-two"] {
        let output = run(&[
            "enrol",
            &root,
            who,
            &format!("{who}@example.invalid"),
            "2026-09-01",
            "2028-09-01",
            "yes",
            "none",
        ]);
        assert!(output.status.success(), "enrol refused {who}");
    }

    // 2. Record and file. The parts are what nine clients would have written;
    //    two of them here, because this match has two seats in it.
    let match_id = seal_to(&replay_path, ["drill-one", "drill-two"]);
    for seat in 0..2usize {
        std::fs::write(
            staging.join(format!("seat-{seat}.session-part")),
            a_part(seat),
        )
        .expect("write a session part");
    }
    let output = run(&["store", &root, &sealed, &staged, "2026-09-01", "in-person"]);
    assert!(
        output.status.success(),
        "store refused a match the consent regime accounts for"
    );

    // The corpus really holds something, or every assertion below is about an
    // empty directory (`docs/RISKS.md` R15).
    let held = corpus.join("matches").join(match_id.to_string());
    assert!(held.join("match.replay").exists(), "no replay was filed");
    assert!(
        held.join("match.session").exists(),
        "no session record was filed"
    );
    let output = run(&["audit", &root, "drill-one"]);
    assert!(
        !output.status.success(),
        "the audit reports nothing about a participant the corpus is holding, so \
         the audit below proves nothing"
    );

    // 3. The census, which is what an operator reads before and after. It writes
    //    nothing, and it prints both bounds.
    let output = run(&["census", &root]);
    assert!(output.status.success(), "the census refused a sound corpus");
    let printed = String::from_utf8_lossy(&output.stdout);
    assert!(
        printed.contains("N = 2 people") && printed.contains("N = 1 matches"),
        "the census did not print both bounds:\n{printed}"
    );
    assert!(
        printed.contains("0% false positives"),
        "the census does not carry the sentence that refuses the claim:\n{printed}"
    );

    // 4. Withdraw. One message from a participant, one command, and the command
    //    audits itself on the way out.
    let output = run(&["withdraw", &root, "drill-one", "2026-09-20"]);
    assert!(
        output.status.success(),
        "withdraw exited non-zero, which is the command reporting that it left \
         something behind"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("destroyed 1 match(es)"),
        "withdraw did not report destroying the match"
    );

    // 5. Audit, separately, because a command that checks itself is a command
    //    that can be wrong twice in the same direction.
    let output = run(&["audit", &root, "drill-one"]);
    assert!(
        output.status.success(),
        "the audit found something after the withdrawal"
    );

    // …and the state on disk is what the procedure claims: the match is gone,
    // the identity is gone, the consent record is gone, and the tombstone names
    // nobody it can still point at.
    assert!(!held.exists(), "the match directory survived");
    assert!(
        !corpus
            .join("identities")
            .join("drill-one.identity")
            .exists(),
        "the pseudonym mapping survived"
    );
    assert!(
        !corpus
            .join("participants")
            .join("drill-one.consent")
            .exists(),
        "the consent record survived"
    );
    let tombstone = std::fs::read_to_string(corpus.join("withdrawals").join("drill-one.withdrawn"))
        .expect("no tombstone was written");
    assert!(tombstone.contains("withdrawn_on: 2026-09-20"));
    assert!(
        !tombstone.contains("example.invalid"),
        "the tombstone carries contact information"
    );

    // The other participant's *contribution to that match* went with it — which
    // is what the consent text says before anybody signs — while everything else
    // about them stayed, which is the half a delete-everything implementation
    // would also satisfy the assertions above with.
    //
    // So the check is on the shape of what is left rather than on an exit
    // status: `audit drill-two` is *supposed* to be non-zero here, because
    // drill-two has not withdrawn and their consent record and mapping are still
    // held. What must be gone is the telemetry.
    let output = run(&["audit", &root, "drill-two"]);
    let remaining = String::from_utf8_lossy(&output.stderr);
    assert!(
        !remaining.contains("match.replay") && !remaining.contains("match.session"),
        "drill-two's contribution to the destroyed match survived:\n{remaining}"
    );
    assert!(
        remaining.contains("drill-two.consent") && remaining.contains("drill-two.identity"),
        "the withdrawal took a participant who did not ask to be withdrawn:\n{remaining}"
    );

    // Withdrawing again is not an error. A participant who is not sure their
    // message landed sends a second one.
    assert!(
        run(&["withdraw", &root, "drill-one", "2026-09-21"])
            .status
            .success()
    );
}

/// **The granular half of the procedure, run end to end through the tool.**
///
/// `docs/CONSENT.md` promises four things a participant can refuse on their own,
/// and `replay/tests/permissions.rs` proves each gate refuses. What that file
/// cannot establish is the same thing this one exists for: that the **commands**
/// an operator types apply them — an argument in the wrong place, a command that
/// reports success while writing a match it should have withheld, an exit status
/// nobody set. All of that is invisible from inside the library.
///
/// Two participants, one match, and one of them refuses publication. Everything
/// below is checked for what it did rather than for what it printed.
#[test]
fn the_granular_procedure_runs_end_to_end_through_the_tool() {
    let scratch = Scratch::new("granular");
    let corpus = scratch.join("corpus");
    let staging = scratch.join("staging");
    let published = scratch.join("published");
    let replay_path = scratch.join("match.replay");
    std::fs::create_dir_all(&staging).expect("a staging directory");

    let root = corpus.display().to_string();
    let staged = staging.display().to_string();
    let sealed = replay_path.display().to_string();
    let destination = published.display().to_string();

    // 1. Enrol, with the permissions each participant gave. `drill-two` refused
    //    publication and everything else; `drill-one` granted the lot.
    assert!(
        run(&[
            "enrol",
            &root,
            "drill-one",
            "drill-one@example.invalid",
            "2026-09-01",
            "2028-09-01",
            "yes",
            "publication,bot-training,retention-after-project,named-attribution",
        ])
        .status
        .success()
    );
    assert!(
        run(&[
            "enrol",
            &root,
            "drill-two",
            "drill-two@example.invalid",
            "2026-09-01",
            "2028-09-01",
            "yes",
            "bot-training",
        ])
        .status
        .success()
    );

    // …and a participant under 18 is refused by the tool, before any record
    // exists. The exit status is what an operator sees.
    let refused = run(&[
        "enrol",
        &root,
        "drill-three",
        "drill-three@example.invalid",
        "2026-09-01",
        "2028-09-01",
        "no",
        "none",
    ]);
    assert!(
        !refused.status.success(),
        "the tool enrolled a participant recorded as under 18"
    );
    assert!(
        !corpus
            .join("participants")
            .join("drill-three.consent")
            .exists(),
        "a refused enrolment wrote a consent record anyway"
    );

    // 2. Record and file, exactly as the procedure above does.
    let match_id = seal_to(&replay_path, ["drill-one", "drill-two"]);
    for seat in 0..2usize {
        std::fs::write(
            staging.join(format!("seat-{seat}.session-part")),
            a_part(seat),
        )
        .expect("write a session part");
    }
    assert!(
        run(&["store", &root, &sealed, &staged, "2026-09-01", "in-person"])
            .status
            .success(),
        "store refused a match the consent regime accounts for"
    );

    // 3. `permits` reports, before anything is run on the corpus, what the
    //    evening's consent actually permits.
    let output = run(&["permits", &root]);
    assert!(output.status.success());
    let printed = String::from_utf8_lossy(&output.stdout);
    assert!(
        printed.contains("withheld") && printed.contains("drill-two"),
        "`permits` does not name who withheld what:\n{printed}"
    );

    // 4. Publish. The one match in this corpus has a refusal in it, so nothing is
    //    written — and the antecedent matters: the destination must not merely be
    //    empty because the corpus is (`docs/RISKS.md` R15).
    let output = run(&["publish", &root, &destination]);
    assert!(output.status.success(), "publish exited non-zero");
    let printed = String::from_utf8_lossy(&output.stdout);
    assert!(
        printed.contains("published 0 of 1 match(es)") && printed.contains("withheld"),
        "publish did not withhold a match a participant refused:\n{printed}"
    );
    assert!(
        !published.join("matches").exists(),
        "a match a participant refused publication for reached the destination"
    );

    // …and once they permit it, the same command writes it — which is what makes
    // the refusal above a refusal rather than a publish command that does
    // nothing.
    assert!(
        run(&[
            "enrol",
            &root,
            "drill-two",
            "drill-two@example.invalid",
            "2026-09-01",
            "2028-09-01",
            "yes",
            "publication,bot-training",
        ])
        .status
        .success()
    );
    let output = run(&["publish", &root, &destination]);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("published 1 of 1 match(es)"),
        "the same corpus with the refusal lifted still published nothing"
    );
    assert!(
        published
            .join("matches")
            .join(match_id.to_string())
            .join("match.replay")
            .exists(),
        "the publishable match did not reach the destination"
    );

    // 5. Withdraw one permission. The participation survives it, which is the
    //    whole difference, and the command audits itself on the way out.
    let output = run(&["withdraw", &root, "drill-one", "2026-09-20", "publication"]);
    assert!(
        output.status.success(),
        "the purpose audit found a use still reaching a participant who withdrew"
    );
    let printed = String::from_utf8_lossy(&output.stdout);
    assert!(printed.contains("participation unchanged"));
    assert!(
        corpus
            .join("matches")
            .join(match_id.to_string())
            .join("match.replay")
            .exists(),
        "a partial withdrawal destroyed a match"
    );
    assert!(
        corpus
            .join("participants")
            .join("drill-one.consent")
            .exists(),
        "a partial withdrawal destroyed the consent record"
    );

    // …and the next publication honours it. The destination already holds the
    // match from step 4, which is `docs/CONSENT.md`'s point about
    // irreversibility standing in front of a reader rather than beside them:
    // what the withdrawal reaches is every future publication and none of the
    // one that happened.
    let output = run(&[
        "publish",
        &root,
        &scratch.join("later").display().to_string(),
    ]);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("published 0 of 1 match(es)"),
        "a publication after the withdrawal published the match anyway"
    );
    assert!(
        published
            .join("matches")
            .join(match_id.to_string())
            .join("match.replay")
            .exists(),
        "the earlier publication was silently retracted, which this project \
         cannot do and must not claim to"
    );

    // 6. Conclude. `drill-two` re-enrolled without retention-after-project, so
    //    the project's end destroys everything of theirs — and takes the match
    //    with it, because a match is one interleaved log.
    let output = run(&["conclude", &root, "2027-03-01"]);
    assert!(output.status.success(), "conclude left something behind");
    let printed = String::from_utf8_lossy(&output.stdout);
    assert!(
        printed.contains("drill-two") && printed.contains("destroyed 1 match(es)"),
        "conclude did not destroy what a participant refused to have kept:\n{printed}"
    );
    assert!(
        !corpus.join("matches").join(match_id.to_string()).exists(),
        "the match survived the conclusion"
    );
    assert!(
        run(&["audit", &root, "drill-two"]).status.success(),
        "something about drill-two survived the conclusion"
    );
}

/// **`disclose` reads a part and writes nothing**, which is the property the
/// demonstration rests on.
///
/// `replay/tests/disclosure.rs` is where the page's arithmetic is checked. What
/// this adds is the operator's half: the command exists, it exits zero on a real
/// part file, and the directory it was pointed at is byte-for-byte what it was
/// before — because the whole argument for showing a participant their own data
/// before they sign is that the crossing it came from is never held.
#[test]
fn disclose_prints_a_participants_own_stream_and_writes_nothing() {
    let scratch = Scratch::new("disclose");
    let directory = scratch.join("parts");
    std::fs::create_dir_all(&directory).expect("a directory");
    let path = directory.join("seat-0.telemetry-part");

    let mut samples = Vec::new();
    for index in 0..64u64 {
        samples.push(replay::telemetry::Sample {
            at_ns: index * 1_000_000,
            event: replay::telemetry::Event::Moved {
                dx: if index % 2 == 0 { 5.0 } else { -2.0 },
                dy: 1.0,
            },
        });
    }
    samples.push(replay::telemetry::Sample {
        at_ns: 64_000_000,
        event: replay::telemetry::Event::Viewed {
            tick: sim::Tick(2),
            seq: 2,
        },
    });
    samples.push(replay::telemetry::Sample {
        at_ns: 64_000_000 + 187_000_000,
        event: replay::telemetry::Event::Pressed {
            control: replay::telemetry::Control::Move,
            down: true,
        },
    });
    let part = replay::TelemetryPart {
        seat: sim::Seat::Blue0,
        stream: replay::telemetry::SeatStream {
            clock: replay::session::Clock::Dequeue,
            platform: replay::session::Platform::Linux,
            world_units_per_count_e6: 50_000,
            dropped: 0,
            samples,
        },
    };
    std::fs::write(&path, part.encode()).expect("write a part");

    let before = listing(&scratch.join(""));
    let output = run(&["disclose", &path.display().to_string()]);
    assert!(output.status.success(), "disclose refused a real part");
    let printed = String::from_utf8_lossy(&output.stdout);
    assert!(
        printed.contains("This is your own recording"),
        "disclose printed nothing a participant would recognise:\n{printed}"
    );
    assert!(
        printed.contains("187 ms"),
        "the reaction was not derived from this part's own anchor:\n{printed}"
    );
    assert_eq!(
        listing(&scratch.join("")),
        before,
        "disclose wrote something. The whole argument for showing a participant \
         their own data before they sign is that the crossing is never held."
    );
}

/// Every path under a directory, sorted — the crudest possible statement that
/// nothing was written.
fn listing(root: &Path) -> Vec<String> {
    fn walk(directory: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            out.push(path.display().to_string());
            if path.is_dir() {
                walk(&path, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out.sort();
    out
}

/// **The pipeline refuses a session the consent regime cannot account for**, as
/// the operator meets it: through the tool, with an exit status.
#[test]
fn the_tool_refuses_a_match_with_no_session_parts_collected() {
    let scratch = Scratch::new("no-parts");
    let corpus = scratch.join("corpus");
    let staging = scratch.join("staging");
    let replay_path = scratch.join("match.replay");
    std::fs::create_dir_all(&staging).expect("a staging directory");

    for who in ["drill-one", "drill-two"] {
        assert!(
            run(&[
                "enrol",
                &corpus.display().to_string(),
                who,
                "someone",
                "2026-09-01",
                "2028-09-01",
                "yes",
                "none",
            ])
            .status
            .success()
        );
    }
    seal_to(&replay_path, ["drill-one", "drill-two"]);

    let output = run(&[
        "store",
        &corpus.display().to_string(),
        &replay_path.display().to_string(),
        &staging.display().to_string(),
        "2026-09-01",
        "in-person",
    ]);
    assert!(
        !output.status.success(),
        "a match with no session record at all was stored"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("session-part"),
        "the refusal does not say what was missing"
    );
    assert!(
        !corpus.join("matches").exists(),
        "the refusal filed the match anyway"
    );
}
