//! The replay tool: seal a match, verify one, honour a withdrawal, audit the
//! result.
//!
//! # Why verification runs as a separate process
//!
//! `docs/MILESTONES.md` M3's exit criterion asks that the server's
//! authoritative digest be reproduced by "an offline resimulation of the
//! recorded input log, run as a separate process", and M4's asks that
//! `replay verify` resimulate a human match to the server's final digest. The
//! separation is the point rather than a packaging preference: a check that ran
//! inside the server would share the server's memory, its arithmetic and its
//! bugs, and would agree with it for reasons that have nothing to do with the
//! log. Booting a second process from the file alone is the cheapest thing that
//! is not that.
//!
//! # Why `verify` refuses to run without a key registry
//!
//! Because a verification with no registry establishes nothing. A signature is
//! internally consistent by construction — anybody can produce one — so
//! "verified" without "verified as *whose*" is a word doing no work.
//! `docs/RISKS.md` R4 is where that lives, and the shape of it here is that the
//! registry is a required argument rather than an optional one with a permissive
//! default. There is deliberately no `--insecure`.
//!
//! # Why the corpus commands are in the same binary
//!
//! `docs/ENGINEERING.md`'s rule is five automations understood over fifteen
//! endured, and `docs/ARCHITECTURE.md` refuses a crate for a handful of
//! commands. `withdraw` and `audit` operate on directories of replays, which is
//! what this crate defines, so they are subcommands here rather than a second
//! binary or an eighth crate.
//!
//! ```text
//! replay keygen <name>                  # <name>.signing-key and <name>.public-key
//! replay verify <replay> <keys> [<telemetry>]   # resimulate, check the seal, report
//! replay inspect <replay>               # print the manifest, check nothing
//! replay disclose <telemetry-part>      # show a participant their own device stream
//! replay enrol <corpus> <pseudonym> <identity> <consented-on> <retention-until> <adult> <permits>
//! replay store <corpus> <replay> <parts-dir> <recorded-on> <supervision> [<telemetry>]
//! replay census <corpus>                # what the corpus is, and what it supports
//! replay permits <corpus> [<purpose>]   # what each separable purpose reaches
//! replay publish <corpus> <destination> # the publishable subset, and nothing else
//! replay conclude <corpus> <date>       # destroy what may not be kept past the project
//! replay withdraw <corpus> <pseudonym> <date> [<purpose>]
//! replay audit <corpus> <pseudonym> [<purpose>]  # non-zero if anything is left
//! ```
//!
//! Exit status: 0 on success, 1 when the thing being checked is wrong, 2 when
//! the arguments or the file are.
//!
//! # The three M6 commands, and what each of them refuses
//!
//! `enrol` writes a consent record and the pseudonym mapping. It is the one
//! command whose input is a conversation — `docs/ENGINEERING.md` lists admitting
//! a participant among the things that stay manual — and what it mechanises is
//! only the filing. It stamps the consent record with the version of
//! `docs/CONSENT.md` this build holds, so the operator cannot record somebody
//! against a text that is not the one in front of them.
//!
//! # The commands the granular regime added, and why each is a command
//!
//! `docs/CONSENT.md` offers four permissions a participant may refuse on their
//! own. A permission with no command behind it is enforced by whoever remembers
//! it, so there is one entry point per purpose and each of them goes through
//! `replay::permit`:
//!
//! | Command | The purpose it applies | What it refuses |
//! | --- | --- | --- |
//! | `publish` | `publication` | a match any participant of which refused it, by name, before anything is written |
//! | `permits` | all four | nothing — it *reports*, which is how an operator finds out what an evening's consent actually permits before running anything |
//! | `conclude` | `retention-after-project` | nothing — it destroys, on a date, exactly what a participant who refused indefinite retention asked to have destroyed |
//! | `withdraw … <purpose>` | one | nothing — it revokes, and `audit … <purpose>` is the separate check |
//!
//! `disclose` is not a corpus command and takes no corpus. It prints a
//! participant's **own** device stream back at them — a few dozen records of
//! `dx`, `dy` and a timestamp, and the four things a page of prose says can be
//! worked out from them. It writes nothing, ever: it is a reading of a file the
//! client already produced, shown to the person who produced it, and
//! `docs/CONSENT.md` puts it in the procedure before the signature rather than
//! beside it as an illustration.
//!
//! `store` is the pipeline. It takes the sealed replay and the directory of
//! session parts the nine clients wrote, assembles them into one session record,
//! and refuses the match if the consent regime cannot account for it — see
//! `replay::corpus`'s table of refusals. Nothing is written until every check has
//! passed.
//!
//! `census` prints what the corpus is and what it supports. It writes **nothing**:
//! a stored summary is the derived index M5 removed wearing a friendly name, and
//! the number it would carry is a number that can disagree with the corpus it
//! came from. Its output is a page rather than a line because `docs/RISKS.md` R8
//! requires the two confidence bounds to travel together — a reader shown the
//! friendlier one has been handled.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use replay::calibration::{DeviceProfileId, Profile, rate_seats};
use replay::consent::{ConsentVersion, Permissions, Purpose};
use replay::corpus::{ConsentRecord, Corpus};
use replay::manifest::Commitment;
use replay::permit::Publishable;
use replay::session::{SeatRecord, SessionRecord, Supervision};
use replay::split::{HOLDOUT_IN, Split, split_of};
use replay::telemetry::{Telemetry, TelemetryError};
use replay::{Build, KeyRegistry, KeyStatus, Replay, SigningKey, VerifyError};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = arguments.first() else {
        return usage();
    };
    match (command.as_str(), arguments.len()) {
        ("keygen", 2) => keygen(Path::new(&arguments[1])),
        ("verify", 3) => verify(Path::new(&arguments[1]), Path::new(&arguments[2]), None),
        ("verify", 4) => verify(
            Path::new(&arguments[1]),
            Path::new(&arguments[2]),
            Some(Path::new(&arguments[3])),
        ),
        ("inspect", 2) => inspect(Path::new(&arguments[1])),
        ("disclose", 2) => disclose(Path::new(&arguments[1])),
        ("enrol", 8) => enrol(
            &arguments[1],
            &arguments[2],
            &arguments[3],
            &arguments[4],
            &arguments[5],
            &arguments[6],
            &arguments[7],
        ),
        ("store", 6) => store(
            &arguments[1],
            Path::new(&arguments[2]),
            Path::new(&arguments[3]),
            &arguments[4],
            &arguments[5],
            None,
        ),
        ("store", 7) => store(
            &arguments[1],
            Path::new(&arguments[2]),
            Path::new(&arguments[3]),
            &arguments[4],
            &arguments[5],
            Some(Path::new(&arguments[6])),
        ),
        ("census", 2) => census(&arguments[1]),
        ("permits", 2) => permits(&arguments[1], None),
        ("permits", 3) => permits(&arguments[1], Some(&arguments[2])),
        ("publish", 3) => publish(&arguments[1], Path::new(&arguments[2])),
        ("conclude", 3) => conclude(&arguments[1], &arguments[2]),
        ("withdraw", 4) => withdraw(&arguments[1], &arguments[2], &arguments[3], None),
        ("withdraw", 5) => withdraw(
            &arguments[1],
            &arguments[2],
            &arguments[3],
            Some(&arguments[4]),
        ),
        ("audit", 3) => audit(&arguments[1], &arguments[2]),
        ("audit", 4) => audit_purpose(&arguments[1], &arguments[2], &arguments[3]),
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: replay keygen <name>");
    eprintln!("       replay verify <replay> <keys> [<telemetry>]");
    eprintln!("       replay inspect <replay>");
    eprintln!("       replay disclose <telemetry-part>");
    eprintln!(
        "       replay enrol <corpus> <pseudonym> <identity> <consented-on> \
         <retention-until> <adult:yes|no> <permits>"
    );
    eprintln!(
        "       replay store <corpus> <replay> <parts-dir> <recorded-on> \
         <in-person|remote|unsupervised> [<telemetry>]"
    );
    eprintln!("       replay census <corpus>");
    eprintln!("       replay permits <corpus> [<purpose>]");
    eprintln!("       replay publish <corpus> <destination>");
    eprintln!("       replay conclude <corpus> <date>");
    eprintln!("       replay withdraw <corpus> <pseudonym> <date> [<purpose>]");
    eprintln!("       replay audit <corpus> <pseudonym> [<purpose>]");
    eprintln!();
    eprintln!(
        "       <permits> is a comma-separated list of the purposes a participant \
         granted, or `none`:"
    );
    for purpose in Purpose::ALL {
        eprintln!(
            "         {:<24} refusing it means {}",
            purpose.tag(),
            purpose.refusing_means()
        );
    }
    ExitCode::from(2)
}

/// Records one participant's consent and their pseudonym mapping.
///
/// # What it prints that it did not used to, and why that is the point
///
/// A participant re-signing because the document moved is entitled to read the
/// **difference** rather than the whole page; a re-signature against a text
/// somebody skimmed for the second time is administrative rather than informed.
/// So this reads whatever record it is about to replace and, if that record was
/// signed against an older version, prints `replay::consent::since` — every
/// change between the two, in the participant's own words — before writing
/// anything. On an ordinary first enrolment it prints nothing, because a command
/// that emits a paragraph every run teaches its reader to skip it.
fn enrol(
    root: &str,
    pseudonym: &str,
    identity: &str,
    consented_on: &str,
    retention_until: &str,
    adult: &str,
    permits: &str,
) -> ExitCode {
    let Some(adult) = yes_or_no(adult) else {
        eprintln!("replay: adult is yes or no, not {adult}");
        return ExitCode::from(2);
    };
    let Some(permissions) = parse_permits(permits) else {
        eprintln!(
            "replay: {permits:?} is not a permission list. A comma-separated \
             selection of the purposes below, or `none`:"
        );
        for purpose in Purpose::ALL {
            eprintln!("replay:   {}", purpose.tag());
        }
        return ExitCode::from(2);
    };
    if replay::Pseudonym::parse(pseudonym).is_none() {
        eprintln!(
            "replay: {pseudonym} is not a pseudonym: letters, digits, '_' and '-', \
             at most 32 bytes (docs/SCHEMA.md)"
        );
        return ExitCode::from(2);
    }
    // The age question, refused here as well as at `store`. Two refusals rather
    // than one because they answer different questions: this one stops an
    // operator writing a record that could never be used, and `store`'s stops a
    // record written by hand or before this check existed from admitting a
    // match. `docs/CONSENT.md` states the regime; neither of them decides it.
    if !adult {
        eprintln!(
            "replay: refused: this project's consent regime covers adults only. A \
             participant under 18 cannot give sufficient consent on their own \
             under Quebec's Law 25, and there is no parental-consent procedure, \
             no second text and nobody to review one here. This is a human \
             decision and not a flag to override (docs/CONSENT.md)."
        );
        return ExitCode::FAILURE;
    }

    let corpus = Corpus::open(root);
    // The difference, before anything is written.
    if let Some(previous) = corpus.consent_of(pseudonym) {
        let changes = replay::consent::since(&previous.consent_version);
        if !changes.is_empty() {
            println!(
                "replay: {pseudonym} last signed consent document {}. What changed \
                 since, newest first — this is what they are re-signing for, and \
                 they are entitled to read it rather than the whole page:",
                previous.consent_version
            );
            for change in changes {
                println!("replay:   {} — {}", change.version, change.summary);
            }
        }
    }

    let record = ConsentRecord {
        pseudonym: pseudonym.to_owned(),
        consented_on: consented_on.to_owned(),
        retention_until: retention_until.to_owned(),
        permissions,
        adult,
        // Stamped from this build rather than typed by the operator: the version
        // is a fact about which document was on the table, and a field somebody
        // types is a field somebody types wrong.
        consent_version: ConsentVersion::current(),
    };
    if let Err(error) = corpus.enrol(&record, identity) {
        eprintln!("replay: {root}: {error}");
        return ExitCode::from(2);
    }
    println!(
        "replay: enrolled {pseudonym} under consent document {}, retained until \
         {retention_until}",
        record.consent_version
    );
    // Every purpose, granted or refused, with what the refusal means beside it.
    // The refusals are the half worth printing: an operator who reads them here
    // finds out what this evening's corpus will and will not permit, before the
    // evening rather than at `publish`.
    for purpose in Purpose::ALL {
        if permissions.granted(purpose) {
            println!("replay:   {:<24} granted", purpose.tag());
        } else {
            println!(
                "replay:   {:<24} REFUSED — {}",
                purpose.tag(),
                purpose.refusing_means()
            );
        }
    }
    eprintln!(
        "replay: the signed consent text is kept with the corpus and outside this \
         repository. This record is only the machine's note that one exists."
    );
    ExitCode::SUCCESS
}

/// `yes` or `no`, and nothing else.
fn yes_or_no(text: &str) -> Option<bool> {
    match text {
        "yes" => Some(true),
        "no" => Some(false),
        _ => None,
    }
}

/// A comma-separated permission list, or `none`.
///
/// Total: an unknown purpose refuses the whole list rather than being skipped,
/// because a typo that silently grants nothing is a participant's decision lost
/// in a shell.
fn parse_permits(text: &str) -> Option<Permissions> {
    if text == "none" {
        return Some(Permissions::none());
    }
    let mut permissions = Permissions::none();
    for tag in text.split(',') {
        permissions.set(Purpose::parse(tag.trim())?, true);
    }
    Some(permissions)
}

/// What each separable purpose currently reaches, and what it does not.
///
/// Prints and writes nothing, in the register `census` prints and writes
/// nothing: every answer here is recomputed from the consent records on disk, so
/// it cannot drift from them and a withdrawal changes it the moment it happens.
fn permits(root: &str, purpose: Option<&str>) -> ExitCode {
    let selected = match purpose {
        None => Purpose::ALL.to_vec(),
        Some(tag) => match Purpose::parse(tag) {
            Some(purpose) => vec![purpose],
            None => {
                eprintln!("replay: {tag:?} is not a purpose. One of:");
                for purpose in Purpose::ALL {
                    eprintln!("replay:   {}", purpose.tag());
                }
                return ExitCode::from(2);
            }
        },
    };
    let corpus = Corpus::open(root);
    let matches = match corpus.matches() {
        Ok(matches) => matches,
        Err(error) => {
            eprintln!("replay: {root}: {error}");
            return ExitCode::from(2);
        }
    };

    for purpose in selected {
        println!();
        println!(
            "{} — refusing it means {}",
            purpose.tag(),
            purpose.refusing_means()
        );
        if purpose == Purpose::NamedAttribution {
            // No matches to list: this purpose is about a person rather than
            // about a recording, so the report is the roster.
            let mut named = 0u32;
            let mut pseudonymous = 0u32;
            for match_id in &matches {
                let Ok(participants) = corpus.participants_of(match_id) else {
                    continue;
                };
                for pseudonym in participants {
                    if corpus.attribution(&pseudonym).is_ok() {
                        named = named.saturating_add(1);
                    } else {
                        pseudonymous = pseudonymous.saturating_add(1);
                    }
                }
            }
            println!("  {named} seat-appearance(s) may be named, {pseudonymous} may not");
            println!(
                "  This gate reaches the corpus and not a sentence somebody writes. \
                 It is the one permission here kept by a promise as well as by a \
                 control (docs/CONSENT.md)."
            );
            continue;
        }
        if purpose == Purpose::RetentionAfterProject {
            // Not a gate over matches but a destruction date over people, so the
            // report is the roster rather than the corpus — the same shape
            // `named-attribution` takes, one line up.
            let due = corpus.due_at_conclusion().unwrap_or_default();
            println!(
                "  {} participant(s) refused it{}",
                due.len(),
                if due.is_empty() {
                    String::new()
                } else {
                    format!(": {}", due.join(", "))
                }
            );
            println!(
                "  `replay conclude <corpus> <date>` is what carries it out, and it \
                 destroys their matches in full."
            );
            continue;
        }
        let mut permitted = 0u32;
        for match_id in &matches {
            match replay::permit::everyone_in(&corpus, match_id, purpose) {
                Ok(()) => permitted = permitted.saturating_add(1),
                Err(error) => println!("  {match_id}: withheld — {error}"),
            }
        }
        println!(
            "  {permitted} of {} match(es) permitted. One refusal withholds a whole \
             match: a match is one interleaved log and there is no way to use one \
             seat of it (docs/SCHEMA.md §10).",
            matches.len()
        );
    }
    ExitCode::SUCCESS
}

/// Writes the publishable subset of a corpus, and nothing else.
///
/// Every match goes through `replay::Publishable`, which is the only value this
/// workspace can write to a publication directory and has no constructor that
/// skips the consent records. So the interesting line below is not the check —
/// there is no check here — it is that there is no other way to get a match into
/// the destination.
fn publish(root: &str, destination: &Path) -> ExitCode {
    let corpus = Corpus::open(root);
    let matches = match corpus.matches() {
        Ok(matches) => matches,
        Err(error) => {
            eprintln!("replay: {root}: {error}");
            return ExitCode::from(2);
        }
    };

    let mut published = Vec::new();
    let mut withheld = 0u32;
    for match_id in &matches {
        match Publishable::of(&corpus, match_id) {
            Ok(publishable) => {
                if let Err(error) = publishable.write_to(destination) {
                    eprintln!("replay: {}: {error}", destination.display());
                    return ExitCode::from(2);
                }
                published.push(match_id.clone());
            }
            Err(error) => {
                println!("replay: withheld {match_id}: {error}");
                withheld = withheld.saturating_add(1);
            }
        }
    }

    println!(
        "replay: published {} of {} match(es) to {}, withheld {withheld}",
        published.len(),
        matches.len(),
        destination.display()
    );
    // The irreversibility, said once and where it lands. `docs/RISKS.md` R3 is
    // about a publication nobody can recall, and a participant who withdraws
    // this permission tomorrow is withdrawing it from every future publication
    // and from none of this one.
    eprintln!(
        "replay: a publication cannot be recalled. Every refusal in force at this \
         moment was honoured; a refusal arriving after this command is a \
         conversation with a person, and docs/CONSENT.md says so beside the box."
    );
    ExitCode::SUCCESS
}

/// Destroys everything a participant who refused indefinite retention asked to
/// have destroyed when the project's work ends.
fn conclude(root: &str, on: &str) -> ExitCode {
    let corpus = Corpus::open(root);
    let due = match corpus.due_at_conclusion() {
        Ok(due) => due,
        Err(error) => {
            eprintln!("replay: {root}: {error}");
            return ExitCode::from(2);
        }
    };
    if due.is_empty() {
        println!(
            "replay: nobody in this corpus refused {}; nothing to destroy",
            Purpose::RetentionAfterProject.tag()
        );
        return ExitCode::SUCCESS;
    }
    println!(
        "replay: {} participant(s) refused {}: {}",
        due.len(),
        Purpose::RetentionAfterProject.tag(),
        due.join(", ")
    );
    let carried = match corpus.conclude(on) {
        Ok(carried) => carried,
        Err(error) => {
            eprintln!("replay: {root}: {error}");
            return ExitCode::from(2);
        }
    };
    let mut status = ExitCode::SUCCESS;
    for (pseudonym, destroyed) in carried {
        println!(
            "replay: {pseudonym} — destroyed {} match(es), mapping {}, consent \
             record {}",
            destroyed.matches.len(),
            if destroyed.identity {
                "destroyed"
            } else {
                "not present"
            },
            if destroyed.consent {
                "destroyed"
            } else {
                "not present"
            }
        );
        if audit(root, &pseudonym) != ExitCode::SUCCESS {
            status = ExitCode::FAILURE;
        }
    }
    status
}

/// Prints a participant their own device stream, and derives from it what a page
/// of prose can only assert.
///
/// # Why this is a command and not a screen
///
/// The demonstration has to show the participant *their own* movements, and the
/// only place text can be rendered in this project is a terminal:
/// `client::draw` has no font stack and `docs/MILESTONES.md` M4 is explicit that
/// a glyph atlas is not in scope for a fixture's UI. So the operator runs this
/// beside them, on the part their own client just wrote.
///
/// **It writes nothing.** There is no argument for a destination and no path
/// through this function to a file, which is what keeps the demonstration
/// crossing from becoming a recording the corpus holds: the operator points this
/// at the part, the participant reads it, and `store` is never run on it.
fn disclose(path: &Path) -> ExitCode {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("replay: {}: {error}", path.display());
            return ExitCode::from(2);
        }
    };
    let Some(part) = replay::TelemetryPart::decode(&bytes) else {
        eprintln!(
            "replay: {} is not a telemetry part (docs/SCHEMA.md §11)",
            path.display()
        );
        return ExitCode::from(2);
    };
    print!("{}", replay::disclosure::of(&part));
    ExitCode::SUCCESS
}

/// Files a sealed match and the session it was recorded in.
fn store(
    root: &str,
    replay_path: &Path,
    parts: &Path,
    recorded_on: &str,
    supervision: &str,
    telemetry_path: Option<&Path>,
) -> ExitCode {
    let Some(supervision) = Supervision::parse(supervision) else {
        eprintln!(
            "replay: {supervision:?} is not a supervision condition. One of \
             in-person, remote, unsupervised — what makes a match human is a fact \
             about a person rather than a property of the file, so the fact is \
             recorded (docs/SCHEMA.md)."
        );
        return ExitCode::from(2);
    };

    let Some(replay) = read(replay_path) else {
        return ExitCode::from(2);
    };

    let mut collected: Vec<(String, String)> = Vec::new();
    let entries = match std::fs::read_dir(parts) {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!("replay: {}: {error}", parts.display());
            return ExitCode::from(2);
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|kind| kind != "session-part") {
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => collected.push((path.display().to_string(), text)),
            Err(error) => {
                eprintln!("replay: {}: {error}", path.display());
                return ExitCode::from(2);
            }
        }
    }
    if collected.is_empty() {
        eprintln!(
            "replay: {} holds no *.session-part file. A match with no session \
             record is a match whose hardware nobody wrote down (docs/SCHEMA.md).",
            parts.display()
        );
        return ExitCode::from(2);
    }

    let mut session = match SessionRecord::assemble(
        replay.manifest.match_id,
        ConsentVersion::current(),
        recorded_on,
        supervision,
        &collected,
    ) {
        Ok(session) => session,
        Err(message) => {
            eprintln!("replay: {message}");
            return ExitCode::from(2);
        }
    };

    // How well each seat's device is known, decided **here** and frozen into the
    // record. It is a decision rather than a measurement — it reads the
    // participant's earlier sessions on the same device — and freezing it is what
    // lets a distribution stratify later without recomputing anything, which is
    // what `docs/SCHEMA.md` §8 requires of a stratum a published number rests on.
    //
    // It refuses nothing. A seat nobody has calibrated is filed as `partial` and
    // the match is stored: `docs/SCHEMA.md` §4e, and `docs/SCOPE.md`'s standing
    // decision that an anti-cheat which degrades honest play has cost more than
    // it caught.
    let corpus = Corpus::open(root);
    let filed = replay.manifest.match_id.to_string();
    let participants = replay.manifest.participants.clone();
    rate_seats(&mut session, &|seat: usize, device: &DeviceProfileId| {
        let Some(Some(pseudonym)) = participants.get(seat) else {
            return Profile::empty(device.clone());
        };
        corpus
            .profile_of(pseudonym.as_str(), device, Some(&filed))
            .unwrap_or_else(|_| Profile::empty(device.clone()))
    });

    let telemetry = match telemetry_path {
        None => None,
        Some(path) => match read_telemetry(path) {
            Some(companion) => Some(companion),
            None => return ExitCode::from(2),
        },
    };

    match corpus.store(&replay, &session, telemetry.as_ref()) {
        Ok(()) => {
            println!(
                "replay: stored {} — {} seat(s) occupied, {}, supervision {}, {}",
                replay.manifest.match_id,
                session.occupied().len(),
                split_of(replay.manifest.match_id).tag(),
                session.supervision.tag(),
                if session.degraded() {
                    "DEGRADED: a client fell behind the tick"
                } else {
                    "every client kept the tick"
                }
            );
            // The calibration strata, printed on the way out for the reason
            // `docs/RISKS.md` R15 gives about counters: an operator who reads
            // `4 sufficient, 5 partial` knows what the evening bought, and one
            // who reads `1 mismatched` knows to ask whose mouse changed before
            // the answer is six months old.
            let mut states: BTreeMap<&'static str, u32> = BTreeMap::new();
            for seat in &session.seats {
                if matches!(seat, SeatRecord::Human { .. }) {
                    *states.entry(seat.calibration().tag()).or_insert(0) += 1;
                }
            }
            println!(
                "replay: calibration — {}; an insufficiently calibrated seat is \
                 marked and never refused, and a detector that reads a distance \
                 or a speed answers None for it (docs/SCHEMA.md §4e)",
                if states.is_empty() {
                    "-".to_owned()
                } else {
                    states
                        .iter()
                        .map(|(state, seats)| format!("{seats} {state}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            );
            match &telemetry {
                None => println!(
                    "replay: no telemetry companion — this match recorded no device \
                     stream, which is a state rather than a gap (docs/SCHEMA.md §11)"
                ),
                Some(companion) => {
                    let (samples, motions) =
                        companion.manifest.seats.iter().flatten().fold(
                            (0u64, 0u64),
                            |(samples, motions), seat| {
                                (samples + seat.samples, motions + seat.motions)
                            },
                        );
                    println!(
                        "replay: telemetry {} — {samples} device event(s), {motions} \
                         motion(s), {} seat(s)",
                        companion.digest(),
                        companion.manifest.occupied().len()
                    );
                }
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("replay: refused: {error}");
            ExitCode::FAILURE
        }
    }
}

/// What the corpus is, and what a claim made on it may say.
///
/// Prints and stores nothing. Every number here is recomputed from the matches
/// on disk, so it cannot drift from them and a withdrawal changes it the moment
/// it happens.
#[expect(
    clippy::too_many_lines,
    reason = "one report, printed in the order a reader needs it"
)]
fn census(root: &str) -> ExitCode {
    let corpus = Corpus::open(root);
    let matches = match corpus.matches() {
        Ok(matches) => matches,
        Err(error) => {
            eprintln!("replay: {root}: {error}");
            return ExitCode::from(2);
        }
    };

    let mut people: Vec<String> = Vec::new();
    let mut full = 0u32;
    let mut partial = 0u32;
    let mut degraded = 0u32;
    let mut held = 0u32;
    let mut unaccountable: Vec<String> = Vec::new();
    let mut ticks = 0u64;
    let mut worst_overrun_ns = 0u64;
    let mut supervision: BTreeMap<Supervision, u32> = BTreeMap::new();
    let mut with_telemetry = 0u32;
    let mut device_events = 0u64;
    let mut telemetry_bytes = 0u64;
    let mut polling: BTreeMap<u32, u32> = BTreeMap::new();
    let mut calibration: BTreeMap<&'static str, u32> = BTreeMap::new();
    let mut profiles: BTreeMap<(String, String), Profile> = BTreeMap::new();

    for match_id in &matches {
        let (Ok(replay), Ok(session)) = (corpus.replay_of(match_id), corpus.session_of(match_id))
        else {
            unaccountable.push(match_id.clone());
            continue;
        };
        if !corpus.accountable(match_id) {
            unaccountable.push(match_id.clone());
            continue;
        }
        if let Ok(Some(companion)) = corpus.telemetry_of(match_id) {
            with_telemetry = with_telemetry.saturating_add(1);
            telemetry_bytes = telemetry_bytes.saturating_add(companion.encode().len() as u64);
            for seat in companion.manifest.seats.iter().flatten() {
                device_events = device_events.saturating_add(seat.samples);
            }
        }
        for (index, seat) in session.seats.iter().enumerate() {
            let SeatRecord::Human {
                declared,
                calibration: seat_calibration,
                ..
            } = seat
            else {
                continue;
            };
            *polling.entry(declared.device_polling_hz).or_insert(0) += 1;
            *calibration.entry(seat.calibration().tag()).or_insert(0) += 1;
            // Folded here rather than through `Corpus::profile_of` because a
            // census walks the corpus once and asking per seat would walk it
            // once per seat. The answer is the same fold.
            if let Some(Some(pseudonym)) = replay.manifest.participants.get(index) {
                profiles
                    .entry((
                        pseudonym.to_string(),
                        declared.device_profile_id.to_string(),
                    ))
                    .or_insert_with(|| Profile::empty(declared.device_profile_id.clone()))
                    .fold(seat_calibration.observations);
            }
        }
        for pseudonym in replay.manifest.participants() {
            let name = pseudonym.to_string();
            if !people.contains(&name) {
                people.push(name);
            }
        }
        ticks = ticks.saturating_add(u64::from(replay.manifest.ticks));
        if session.occupied().len() == sim::PLAYER_COUNT {
            full = full.saturating_add(1);
        } else {
            partial = partial.saturating_add(1);
        }
        if session.degraded() {
            degraded = degraded.saturating_add(1);
        }
        *supervision.entry(session.supervision).or_insert(0) += 1;
        for seat in &session.seats {
            if let SeatRecord::Human { measured, .. } = seat {
                worst_overrun_ns = worst_overrun_ns.max(measured.worst_overrun_ns);
            }
        }
        if split_of(replay.manifest.match_id) == Split::Holdout {
            held = held.saturating_add(1);
        }
    }
    people.sort();

    let recorded = matches.len().saturating_sub(unaccountable.len());
    println!("corpus: {} match(es) at {root}", recorded);
    println!("corpus: {} distinct participant(s): {}", people.len(), {
        if people.is_empty() {
            "-".to_owned()
        } else {
            people.join(", ")
        }
    });
    println!(
        "corpus: {full} at nine seats, {partial} partially filled — counted \
         separately and never pooled into one distribution (docs/SCHEMA.md)"
    );
    println!(
        "corpus: {degraded} session(s) degraded; worst tick-budget overrun {:.3} ms \
         (docs/RISKS.md R16)",
        (worst_overrun_ns as f64) / 1e6
    );
    // The supervision strata, printed beside the counts they qualify. What makes
    // a match human is the operator having been there, not anything in the file
    // (docs/SCHEMA.md, docs/SCOPE.md's ceiling) — so a corpus that mixes the
    // three has a covariate in it, and this is the line that says so before
    // anybody builds a distribution over the whole of it.
    println!(
        "corpus: supervision — {} in person, {} remote, {} unsupervised; a \
         distribution over more than one of these has a provenance covariate in it \
         (docs/SCHEMA.md)",
        supervision
            .get(&Supervision::InPerson)
            .copied()
            .unwrap_or(0),
        supervision.get(&Supervision::Remote).copied().unwrap_or(0),
        supervision
            .get(&Supervision::Unsupervised)
            .copied()
            .unwrap_or(0)
    );
    println!(
        "corpus: {held} held out, {} for training, one in {HOLDOUT_IN} by \
         replay::split (frozen)",
        recorded.saturating_sub(held as usize)
    );
    println!(
        "corpus: {} tick(s) recorded, {:.1} minute(s) of play",
        ticks,
        (ticks as f64) / f64::from(sim::TICKS_PER_SECOND) / 60.0
    );
    println!(
        "corpus: {with_telemetry} of {recorded} match(es) carry a telemetry \
         companion — {device_events} device event(s), {:.1} MiB on disk \
         (docs/SCHEMA.md §11)",
        (telemetry_bytes as f64) / (1024.0 * 1024.0)
    );
    // The polling rates, printed for the reason the supervision strata are: a
    // corpus that mixes them has a covariate in it that nobody can remove
    // afterwards, and it is the covariate an inter-arrival detector reads
    // directly. `docs/RISKS.md` R14 carries the arithmetic — at 1 kHz the gap
    // between two device events is 1 ms, which is the scale of the residual the
    // capture path itself adds, and at 125 Hz it is eight times that.
    println!(
        "corpus: declared polling rates — {}; a distribution over more than one of \
         these has the device's own report rate in it (docs/RISKS.md R14)",
        if polling.is_empty() {
            "-".to_owned()
        } else {
            polling
                .iter()
                .map(|(hz, seats)| format!("{hz} Hz x{seats}"))
                .collect::<Vec<_>>()
                .join(", ")
        }
    );

    // What the lobby has bought so far, per participant and per device. The
    // measurement is the answer to `docs/RISKS.md` R17 — nine people on nine
    // mice, person and device perfectly confounded — and the honest form of a
    // report on it is the shortfall rather than the estimate, because a scale
    // estimated from too few directions is a number with the shape of a
    // calibration and none of the basis.
    println!(
        "corpus: calibration — {}; an insufficiently calibrated seat is marked \
         and never refused, and a detector that reads a distance or a speed \
         answers None for it (docs/SCHEMA.md §4e)",
        if calibration.is_empty() {
            "no seat records a lobby crossing".to_owned()
        } else {
            calibration
                .iter()
                .map(|(state, seats)| format!("{seats} seat(s) {state}"))
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    for ((pseudonym, device), profile) in &profiles {
        let shortfall = profile.shortfall();
        match (profile.estimate(), shortfall.is_empty()) {
            (Some(estimate), true) => println!(
                "corpus: {pseudonym} on {device} — {} session(s), {} reach(es): \
                 {:.3} device count(s) per world unit, arrival cost {:.1} count(s), \
                 fit {:.3}, {:.0} Hz measured, quantum {:.4} count(s)",
                profile.sessions,
                profile.observations.reaches,
                estimate.counts_per_unit,
                estimate.arrival_counts,
                estimate.fit,
                estimate.report_hz,
                estimate.quantum
            ),
            (_, _) => println!(
                "corpus: {pseudonym} on {device} — {} session(s), {} reach(es): not \
                 yet sufficient ({})",
                profile.sessions,
                profile.observations.reaches,
                if shortfall.is_empty() {
                    "no fit over these distances".to_owned()
                } else {
                    shortfall.join(", ")
                }
            ),
        }
    }

    // What the four separable purposes actually reach, printed beside the counts
    // they qualify. A corpus of forty matches of which two may be published is a
    // different artefact from one of forty that may all be, and the difference is
    // a fact about consent rather than about recording — so it belongs in the
    // census for the same reason the supervision strata do. One refusal withholds
    // a whole match, so these numbers fall much faster than a reader expects and
    // that is the number worth seeing before anything is planned on it.
    println!();
    for purpose in Purpose::ALL {
        if purpose == Purpose::NamedAttribution {
            let named = people
                .iter()
                .filter(|who| corpus.permits(who, purpose))
                .count();
            println!(
                "corpus: {} of {} participant(s) may be named; the rest appear as \
                 their pseudonym (docs/CONSENT.md)",
                named,
                people.len()
            );
            continue;
        }
        if purpose == Purpose::RetentionAfterProject {
            let due = corpus.due_at_conclusion().unwrap_or_default();
            println!(
                "corpus: {} participant(s) refused {} — `replay conclude` destroys \
                 everything of theirs when the work ends{}",
                due.len(),
                purpose.tag(),
                if due.is_empty() {
                    String::new()
                } else {
                    format!(": {}", due.join(", "))
                }
            );
            continue;
        }
        let reached = matches
            .iter()
            .filter(|match_id| replay::permit::everyone_in(&corpus, match_id, purpose).is_ok())
            .count();
        println!(
            "corpus: {reached} of {recorded} match(es) permit {} — one refusal \
             withholds a whole match (docs/SCHEMA.md §10)",
            purpose.tag()
        );
    }

    if !unaccountable.is_empty() {
        eprintln!(
            "replay: {} match(es) do not read and are in nobody's account:",
            unaccountable.len()
        );
        for match_id in &unaccountable {
            eprintln!("replay:   {match_id}");
        }
    }

    // The two bounds, together, always. `docs/RISKS.md` R8 and
    // `docs/MILESTONES.md` M6: nine seats of one match are not nine independent
    // observations and several matches share people, so a detector has two
    // honest denominators depending on what it reads, and showing one of them is
    // showing the friendlier one.
    let people_bound = bound(people.len());
    let match_bound = bound(recorded);
    println!();
    println!("what this corpus can support, at 95% confidence and zero observed false positives:");
    println!(
        "  what a person's style drives  : N = {} people  -> upper bound about {people_bound}",
        people.len()
    );
    println!(
        "  what a match's circumstances drive: N = {recorded} matches -> upper bound about {match_bound}"
    );
    println!(
        "  Both belong in every detector document. The rule is 3/N; the 9 x {recorded} = {} scored",
        recorded.saturating_mul(9)
    );
    println!("  player-matches are NOT independent and must never be used as N. No claim of");
    println!("  the form \"0% false positives\" is supportable at any size this project reaches.");

    if unaccountable.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The rule of three, rendered.
///
/// Zero observations in `n` independent trials supports roughly a `3/n` upper
/// bound at 95% confidence. `n = 0` has no bound at all and says so rather than
/// dividing.
fn bound(n: usize) -> String {
    if n == 0 {
        return "nothing at all (no observations)".to_owned();
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "a percentage printed in a report"
    )]
    let percent = 300.0 / (n as f64);
    format!("{percent:.1}%")
}

/// Generates a signing key and the registry line that accepts it.
///
/// Two files, and the split is the whole of the operational advice this tool
/// gives: `<name>.signing-key` is the secret and never leaves the machine —
/// `.gitignore` refuses it and `ci` fails on a tracked one — while
/// `<name>.public-key` is meant to be published beside a release and kept
/// published for ever after the key is retired, because a retired key that
/// stops being published orphans every replay it sealed
/// (`docs/RISKS.md` R4).
fn keygen(name: &Path) -> ExitCode {
    let key = match SigningKey::generate() {
        Ok(key) => key,
        Err(error) => {
            eprintln!("replay: {error}");
            return ExitCode::from(2);
        }
    };
    let secret = name.with_extension("signing-key");
    let public = name.with_extension("public-key");
    let label = name
        .file_name()
        .map_or_else(|| "server".to_owned(), |name| name.to_string_lossy().into());

    let mut registry = KeyRegistry::new();
    registry.insert(key.verifying(), KeyStatus::Active, label);

    if let Err(error) = std::fs::write(&secret, hex(&key.to_bytes())) {
        eprintln!("replay: {}: {error}", secret.display());
        return ExitCode::from(2);
    }
    if let Err(error) = std::fs::write(&public, registry.encode()) {
        eprintln!("replay: {}: {error}", public.display());
        return ExitCode::from(2);
    }
    println!("replay: signing key  {}", secret.display());
    println!("replay: registry line {}", public.display());
    println!("replay: identity {}", key.verifying());
    eprintln!(
        "replay: the signing key is a secret. It belongs outside this repository, \
         and `.gitignore` and `ci` both refuse a tracked one."
    );
    ExitCode::SUCCESS
}

/// Resimulates a replay, checks its seal, and reports what that establishes.
///
/// The companion is an argument rather than a file this goes looking for, and
/// that is the decision rather than an omission. **A replay is verifiable
/// without one**, so a verifier that searched a directory would turn a
/// legitimate absence into a question about where somebody put a file; and a
/// verifier that accepted a companion it found beside a replay would be
/// accepting a binding the replay never made. Handed one, this checks it against
/// the digest the manifest committed to; handed none, it says which of the two
/// legitimate states the replay is in and checks nothing else.
fn verify(path: &Path, keys: &Path, telemetry_path: Option<&Path>) -> ExitCode {
    let Some(replay) = read(path) else {
        return ExitCode::from(2);
    };
    let registry = match KeyRegistry::load(keys) {
        Ok(registry) => registry,
        Err(error) => {
            eprintln!("replay: {}: {error}", keys.display());
            return ExitCode::from(2);
        }
    };

    describe(&replay);
    println!("replay: accepting {} key(s)", registry.entries().len());

    match replay::verify(&replay, &registry, &Build::current()) {
        Ok(verified) => {
            println!(
                "replay: digest {}",
                hex(verified.final_state_digest.as_bytes())
            );
            println!("replay: outcome {:?}", verified.outcome);
            if verified.retired {
                println!(
                    "replay: sealed by a retired key, which still verifies \
                     (docs/RISKS.md R4)"
                );
            }
            // The one line a reader should take away, and it is deliberately
            // narrower than "this match was played fairly": `docs/SCOPE.md` is
            // explicit that resimulating an authoritative server's own inputs
            // catches a broken server and not a cheating client.
            println!(
                "replay: ok — {} sealed this manifest, the log is the one it names, \
                 and it reaches the state and result it claims. This says nothing \
                 about how anybody played.",
                verified.signer
            );
            telemetry(&replay, verified.telemetry, telemetry_path, &registry)
        }
        Err(error) => {
            eprintln!("replay: {error}");
            match error {
                VerifyError::UnknownKey(key) => {
                    eprintln!("replay: {key} is not in {}", keys.display());
                }
                VerifyError::Signature => {
                    eprintln!("replay: the manifest is not what that key signed");
                }
                VerifyError::RulesHash { recorded, local } => {
                    eprintln!("replay: recorded under {}", hex(recorded.as_bytes()));
                    eprintln!("replay: this build plays {}", hex(local.as_bytes()));
                }
                VerifyError::SimVersion { .. } => {
                    eprintln!(
                        "replay: this is another build of the same rules, not a \
                         tampered file (docs/RISKS.md R13)"
                    );
                }
                VerifyError::Truncated { claimed, found } => {
                    eprintln!("replay: the log is {} input(s) short", claimed - found);
                }
                VerifyError::InputLog { claimed, computed } => {
                    eprintln!("replay: claimed  {}", hex(claimed.as_bytes()));
                    eprintln!("replay: computed {}", hex(computed.as_bytes()));
                }
                VerifyError::FinalDigest { claimed, computed } => {
                    eprintln!("replay: claimed  {}", hex(claimed.as_bytes()));
                    eprintln!("replay: computed {}", hex(computed.as_bytes()));
                }
                VerifyError::Outcome { .. } => {
                    eprintln!(
                        "replay: the result this file asserts is not the one its own log produces"
                    );
                }
            }
            ExitCode::FAILURE
        }
    }
}

/// Prints a replay's manifest without checking anything about it.
///
/// Separate from `verify` on purpose. A tool that printed a manifest and a
/// verdict in one breath would invite a reader to believe the manifest because
/// the verdict was there; this one prints a manifest and says, in the last line,
/// that it has checked nothing.
fn inspect(path: &Path) -> ExitCode {
    let Some(replay) = read(path) else {
        return ExitCode::from(2);
    };
    describe(&replay);
    println!(
        "replay: inspect checks nothing. The signature, the log and the outcome \
         are unverified; run `replay verify <replay> <keys>`."
    );
    ExitCode::SUCCESS
}

/// The companion half of `verify`, and the three states it distinguishes.
///
/// Absence is the first of them and it is not a failure: `docs/SCHEMA.md` §11 and
/// `replay::manifest::Commitment` both say a replay with no companion is a
/// complete replay, and a verifier that reported it as an error would teach its
/// reader to ignore the error. So this prints what the replay says and returns
/// success — the only thing it will not do is stay silent about it, because a
/// reader who is told nothing about the telemetry concludes there was some.
fn telemetry(
    replay: &Replay,
    commitment: Commitment,
    path: Option<&Path>,
    registry: &KeyRegistry,
) -> ExitCode {
    match (commitment, path) {
        (Commitment::Absent, None) => {
            println!(
                "replay: telemetry none — this match recorded no device stream. That \
                 is a state and not a gap: the replay above is complete \
                 (docs/SCHEMA.md §11)."
            );
            ExitCode::SUCCESS
        }
        (Commitment::Absent, Some(given)) => {
            eprintln!(
                "replay: {} was given, and this replay commits to no telemetry \
                 companion. {}",
                given.display(),
                TelemetryError::NotCommitted
            );
            ExitCode::FAILURE
        }
        (Commitment::Sealed(digest), None) => {
            println!(
                "replay: telemetry {digest} — committed to and NOT checked. Pass the \
                 companion as a third argument to check it."
            );
            ExitCode::SUCCESS
        }
        (Commitment::Sealed(_), Some(given)) => {
            let Some(companion) = read_telemetry(given) else {
                return ExitCode::from(2);
            };
            match replay::telemetry::verify(replay, &companion, registry) {
                Ok(verified) => {
                    println!(
                        "replay: telemetry ok — {} device event(s) across {} seat(s), \
                         {} of them motions, sealed by {} and named by this replay and \
                         no other.",
                        verified.samples,
                        companion.manifest.occupied().len(),
                        verified.motions,
                        verified.signer
                    );
                    if verified.retired {
                        println!(
                            "replay: the companion was sealed by a retired key, which \
                             still verifies (docs/RISKS.md R4)"
                        );
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("replay: telemetry refused: {error}");
                    if let TelemetryError::Substituted { claimed, computed } = error {
                        eprintln!("replay: the replay names {claimed}");
                        eprintln!("replay: this companion is {computed}");
                    }
                    ExitCode::FAILURE
                }
            }
        }
    }
}

fn read_telemetry(path: &Path) -> Option<Telemetry> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("replay: {}: {error}", path.display());
            return None;
        }
    };
    match Telemetry::decode(&bytes) {
        Ok(companion) => Some(companion),
        Err(error) => {
            eprintln!("replay: {}: {error}", path.display());
            None
        }
    }
}

fn read(path: &Path) -> Option<Replay> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("replay: {}: {error}", path.display());
            return None;
        }
    };
    match Replay::decode(&bytes) {
        Ok(replay) => Some(replay),
        Err(error) => {
            eprintln!("replay: {}: {error}", path.display());
            None
        }
    }
}

fn describe(replay: &Replay) {
    let manifest = &replay.manifest;
    println!("replay: match {}", manifest.match_id);
    println!("replay: sealed by {}", manifest.server_identity);
    println!("replay: seed {:#018x}", manifest.seed);
    println!("replay: ticks {}", manifest.ticks);
    println!("replay: inputs {}", replay.inputs.len());
    println!("replay: rules {}", hex(manifest.rules_hash.as_bytes()));
    println!(
        "replay: sim {}.{}.{} at {}",
        manifest.sim_version[0],
        manifest.sim_version[1],
        manifest.sim_version[2],
        manifest.sim_commit
    );
    println!("replay: started at {} ms", manifest.started_at_unix_ms);
    let participants = manifest.participants();
    println!(
        "replay: participants {}",
        if participants.is_empty() {
            "-".to_owned()
        } else {
            participants
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    println!("replay: claims outcome {:?}", manifest.outcome);
    println!(
        "replay: telemetry {}",
        match manifest.telemetry {
            Commitment::Absent => "none — this match recorded no device stream".to_owned(),
            Commitment::Sealed(digest) => format!("{digest}"),
        }
    );
}

/// Destroys everything a participant's withdrawal reaches, and then checks.
///
/// The audit runs here rather than being left to the operator, because a
/// destruction command that reports success without looking is the failure mode
/// the whole mechanism exists to avoid. If anything is left, this exits non-zero
/// having *said what*, and the corpus is in the state the next run repairs.
fn withdraw(root: &str, pseudonym: &str, on: &str, purpose: Option<&str>) -> ExitCode {
    if let Some(tag) = purpose {
        return withdraw_purpose(root, pseudonym, on, tag);
    }
    let corpus = Corpus::open(root);
    let destroyed = match corpus.withdraw(pseudonym, on) {
        Ok(destroyed) => destroyed,
        Err(error) => {
            eprintln!("replay: {root}: {error}");
            return ExitCode::from(2);
        }
    };
    println!(
        "replay: destroyed {} match(es): {}",
        destroyed.matches.len(),
        if destroyed.matches.is_empty() {
            "-".to_owned()
        } else {
            destroyed.matches.join(", ")
        }
    );
    println!(
        "replay: pseudonym mapping {}, consent record {}",
        if destroyed.identity {
            "destroyed"
        } else {
            "not present"
        },
        if destroyed.consent {
            "destroyed"
        } else {
            "not present"
        }
    );
    audit(root, pseudonym)
}

/// Revokes one permission and leaves the participation standing.
///
/// **Nothing is destroyed here, and that is the whole difference.** A partial
/// withdrawal takes back a *use*; the recordings stay, the participant stays in
/// the corpus, and what changes is that the gate for that purpose stops letting
/// their matches through. The audit that follows is not this command reading
/// back what it wrote — it runs the use's own gate over the matches they are in,
/// which is the question they actually asked.
fn withdraw_purpose(root: &str, pseudonym: &str, on: &str, tag: &str) -> ExitCode {
    let Some(purpose) = Purpose::parse(tag) else {
        eprintln!("replay: {tag:?} is not a purpose. One of:");
        for purpose in Purpose::ALL {
            eprintln!("replay:   {}", purpose.tag());
        }
        return ExitCode::from(2);
    };
    let corpus = Corpus::open(root);
    let revoked = match corpus.withdraw_purpose(pseudonym, purpose, on) {
        Ok(revoked) => revoked,
        Err(error) => {
            eprintln!("replay: {root}: {error}");
            return ExitCode::from(2);
        }
    };
    println!(
        "replay: {} {} for {pseudonym} on {on}",
        purpose.tag(),
        if revoked {
            "withdrawn"
        } else {
            "was already refused or unrecorded; nothing to revoke"
        }
    );
    println!(
        "replay: participation unchanged — no match destroyed, nothing else \
         touched. From now on, {}.",
        purpose.refusing_means()
    );
    audit_purpose(root, pseudonym, tag)
}

/// Fails, by name, if any use of a withdrawn purpose would still reach this
/// participant.
fn audit_purpose(root: &str, pseudonym: &str, tag: &str) -> ExitCode {
    let Some(purpose) = Purpose::parse(tag) else {
        eprintln!("replay: {tag:?} is not a purpose. One of:");
        for purpose in Purpose::ALL {
            eprintln!("replay:   {}", purpose.tag());
        }
        return ExitCode::from(2);
    };
    match Corpus::open(root).audit_purpose(pseudonym, purpose) {
        Ok(reached) if reached.is_empty() => {
            println!(
                "replay: no use of {} reaches {pseudonym} in this corpus",
                purpose.tag()
            );
            ExitCode::SUCCESS
        }
        Ok(reached) => {
            eprintln!(
                "replay: {} of {pseudonym}'s record(s) would still be used for {}:",
                reached.len(),
                purpose.tag()
            );
            for what in reached {
                eprintln!("replay:   {what}");
            }
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("replay: {root}: {error}");
            ExitCode::from(2)
        }
    }
}

/// Fails, loudly and by name, if anything about a pseudonym is left.
fn audit(root: &str, pseudonym: &str) -> ExitCode {
    let corpus = Corpus::open(root);
    match corpus.audit(pseudonym) {
        Ok(traces) if traces.is_empty() => {
            println!("replay: no trace of {pseudonym} outside its withdrawal record");
            ExitCode::SUCCESS
        }
        Ok(traces) => {
            eprintln!(
                "replay: {} file(s) still mention {pseudonym}:",
                traces.len()
            );
            for path in traces {
                eprintln!("replay:   {}", path.display());
            }
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("replay: {root}: {error}");
            ExitCode::from(2)
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}
