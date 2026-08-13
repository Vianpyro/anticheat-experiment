//! The replay tool: verify a recording, honour a withdrawal, audit the result.
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
//! # Why the corpus commands are in the same binary
//!
//! `docs/ENGINEERING.md`'s rule is five automations understood over fifteen
//! endured, and `docs/ARCHITECTURE.md` refuses a crate for a handful of
//! commands. `withdraw` and `audit` operate on directories of recordings, which
//! is what this crate defines, so they are subcommands here rather than a second
//! binary or an eighth crate.
//!
//! ```text
//! replay verify <recording>            # resimulate and report the digest
//! replay withdraw <corpus> <pseudonym> <date>
//! replay audit <corpus> <pseudonym>    # non-zero if anything is left
//! ```
//!
//! Exit status: 0 on success, 1 when the thing being checked is wrong, 2 when
//! the arguments or the file are.

use std::path::PathBuf;
use std::process::ExitCode;

use replay::corpus::Corpus;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = arguments.first() else {
        return usage();
    };
    match (command.as_str(), arguments.len()) {
        ("verify", 2) => verify(PathBuf::from(&arguments[1])),
        ("withdraw", 4) => withdraw(&arguments[1], &arguments[2], &arguments[3]),
        ("audit", 3) => audit(&arguments[1], &arguments[2]),
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: replay verify <recording>");
    eprintln!("       replay withdraw <corpus> <pseudonym> <date>");
    eprintln!("       replay audit <corpus> <pseudonym>");
    ExitCode::from(2)
}

/// Resimulates a recording and reports the digest it ends on.
fn verify(path: PathBuf) -> ExitCode {
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("replay: {}: {error}", path.display());
            return ExitCode::from(2);
        }
    };
    let recording = match replay::Recording::decode(&bytes) {
        Ok(recording) => recording,
        Err(error) => {
            eprintln!("replay: {}: {error}", path.display());
            return ExitCode::from(2);
        }
    };

    println!("replay: seed {:#018x}", recording.seed);
    println!("replay: ticks {}", recording.ticks);
    println!("replay: inputs {}", recording.inputs.len());
    println!("replay: rules {}", hex(recording.rules_hash.as_bytes()));

    match replay::resimulate(&recording) {
        Ok(digest) => {
            println!("replay: digest {}", hex(digest.as_bytes()));
            println!("replay: ok");
            ExitCode::SUCCESS
        }
        Err(replay::VerifyError::RulesHash { recorded, local }) => {
            eprintln!("replay: recorded under {}", hex(recorded.as_bytes()));
            eprintln!("replay: this build plays {}", hex(local.as_bytes()));
            eprintln!("replay: refusing to resimulate a match played by other rules");
            ExitCode::FAILURE
        }
        Err(replay::VerifyError::Digest { claimed, computed }) => {
            eprintln!("replay: claimed  {}", hex(claimed.as_bytes()));
            eprintln!("replay: computed {}", hex(computed.as_bytes()));
            eprintln!("replay: the log does not reproduce the digest it claims");
            ExitCode::FAILURE
        }
    }
}

/// Destroys everything a participant's withdrawal reaches, and then checks.
///
/// The audit runs here rather than being left to the operator, because a
/// destruction command that reports success without looking is the failure mode
/// the whole mechanism exists to avoid. If anything is left, this exits non-zero
/// having *said what*, and the corpus is in the state the next run repairs.
fn withdraw(root: &str, pseudonym: &str, on: &str) -> ExitCode {
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

fn hex(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}
