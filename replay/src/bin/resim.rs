//! Resimulates a recorded match and reports the digest it ends on.
//!
//! A separate process on purpose. `docs/MILESTONES.md` M3's exit criterion asks
//! that the server's authoritative digest be reproduced by "an offline
//! resimulation of the recorded input log, run as a separate process", and the
//! separation is the point rather than a packaging preference: a check that ran
//! inside the server would share the server's memory, its arithmetic and its
//! bugs, and would agree with it for reasons that have nothing to do with the
//! log. Booting a second process from the file alone is the cheapest thing that
//! is not that.
//!
//! Usage: `resim <recording>`. Exit status 0 if the log reproduces the digest
//! the recording claims, 1 otherwise, 2 if the file could not be read.

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut arguments = std::env::args_os().skip(1);
    let Some(path) = arguments.next() else {
        eprintln!("usage: resim <recording>");
        return ExitCode::from(2);
    };
    if arguments.next().is_some() {
        eprintln!("usage: resim <recording>");
        return ExitCode::from(2);
    }

    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("resim: {}: {error}", path.to_string_lossy());
            return ExitCode::from(2);
        }
    };
    let recording = match replay::Recording::decode(&bytes) {
        Ok(recording) => recording,
        Err(error) => {
            eprintln!("resim: {}: {error}", path.to_string_lossy());
            return ExitCode::from(2);
        }
    };

    println!("resim: seed {:#018x}", recording.seed);
    println!("resim: ticks {}", recording.ticks);
    println!("resim: inputs {}", recording.inputs.len());
    println!("resim: rules {}", hex(recording.rules_hash.as_bytes()));

    match replay::resimulate(&recording) {
        Ok(digest) => {
            println!("resim: digest {}", hex(digest.as_bytes()));
            println!("resim: ok");
            ExitCode::SUCCESS
        }
        Err(replay::VerifyError::RulesHash { recorded, local }) => {
            eprintln!("resim: recorded under {}", hex(recorded.as_bytes()));
            eprintln!("resim: this build plays {}", hex(local.as_bytes()));
            eprintln!("resim: refusing to resimulate a match played by other rules");
            ExitCode::FAILURE
        }
        Err(replay::VerifyError::Digest { claimed, computed }) => {
            eprintln!("resim: claimed  {}", hex(claimed.as_bytes()));
            eprintln!("resim: computed {}", hex(computed.as_bytes()));
            eprintln!("resim: the log does not reproduce the digest it claims");
            ExitCode::FAILURE
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
