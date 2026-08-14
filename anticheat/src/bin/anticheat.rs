//! The operator's entry point: score a corpus and print what it supports.
//!
//! ```console
//! anticheat report <corpus>
//! ```
//!
//! Exit status: 0 on success, 1 when the corpus holds a match that does not
//! read, 2 when the arguments or the directory are wrong.
//!
//! # Why the filesystem is here and not in the library
//!
//! `docs/ARCHITECTURE.md` puts `anticheat` outside I/O: it is a pure function
//! from telemetry to scores, which is what makes a detector reproducible from a
//! stored match rather than from a server that happened to be running. The
//! directory walking is twenty lines of `replay::Corpus` and it lives here, in a
//! binary that nothing links.
//!
//! # Why it exists before there is a corpus to point it at
//!
//! `replay census` is the precedent and `docs/MILESTONES.md` M6 records the
//! judgement: on an empty corpus it correctly prints that the corpus supports
//! nothing at all, and that is a working instrument rather than a satisfied
//! criterion. The same is true here — pointed at an empty directory this prints
//! three detectors, three null models, three `UNCALIBRATED` lines and the
//! sentence saying no threshold can be fixed. That is the milestone's actual
//! state, rendered by the thing that would render a real one.

use std::path::Path;
use std::process::ExitCode;

use anticheat::evaluate::evaluate;
use anticheat::telemetry::MatchTelemetry;
use replay::corpus::Corpus;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match (arguments.first().map(String::as_str), arguments.len()) {
        (Some("report"), 2) => report(&arguments[1]),
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: anticheat report <corpus>");
    ExitCode::from(2)
}

/// Scores every match in a corpus and prints the page.
fn report(root: &str) -> ExitCode {
    let corpus = Corpus::open(Path::new(root));
    let identifiers = match corpus.matches() {
        Ok(found) => found,
        Err(error) => {
            eprintln!("anticheat: {root}: {error}");
            return ExitCode::from(2);
        }
    };

    let mut telemetry = Vec::new();
    let mut unreadable: Vec<String> = Vec::new();
    for match_id in &identifiers {
        // A match whose replay or session record does not read is reported
        // rather than skipped, for the reason `Corpus::audit` reports the same
        // pair unconditionally: a seat record with no manifest in front of it
        // describes somebody's session and nobody can say whose.
        let (Ok(replay), Ok(session)) = (corpus.replay_of(match_id), corpus.session_of(match_id))
        else {
            unreadable.push(match_id.clone());
            continue;
        };
        match MatchTelemetry::from_corpus(&replay, &session) {
            Ok(one) => telemetry.push(one),
            Err(error) => {
                eprintln!("anticheat: {match_id}: {error}");
                unreadable.push(match_id.clone());
            }
        }
    }

    let detectors = anticheat::all();
    println!("{}", evaluate(&detectors, &telemetry));

    if unreadable.is_empty() {
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "anticheat: {} match(es) do not read and were scored by nothing:",
            unreadable.len()
        );
        for match_id in &unreadable {
            eprintln!("anticheat:   {match_id}");
        }
        ExitCode::FAILURE
    }
}
