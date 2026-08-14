//! The thin binary. `docs/ARCHITECTURE.md`: `server` is a library with a thin
//! binary, and the split exists for exactly one reason — the exploit suite at
//! M7 boots the authority in-process. Everything below is argument parsing and
//! a runtime.
//!
//! It prints its address and the DER of the certificate it generated, in hex,
//! on one line each. That is how a client is handed the certificate out of
//! band: there is no certificate authority here, one server process, and a
//! client that trusts exactly what it was told to trust rather than a verifier
//! that trusts anything.
//!
//! # Sealing, and why the key is an argument
//!
//! `docs/MILESTONES.md` M5 gives this project one artefact that reaches a disk
//! and it is a signed replay. The key that seals it is the operator's, not the
//! authority's: `Match` holds no secret, `replay::seal` is where the signature
//! happens, and this binary is where the two meet. Without a key this prints the
//! digest and writes nothing, which is the honest behaviour for a development
//! run — an unsigned file on disk is the artefact somebody would later hand you
//! as evidence.
//!
//! # And the telemetry companion, which has to be sealed first
//!
//! `docs/SCHEMA.md` §11: the device stream is a **second sealed file** and the
//! replay's manifest carries its digest. That fixes an order — the companion is
//! sealed, then the replay commits to it — and it puts the assembly here, because
//! this is the process that holds the key and the moment the recording exists.
//!
//! The parts come from the clients, which write them as they exit, which is
//! *after* this process has finished its ticks. So this waits for them: one part
//! per seat that spoke, with a deadline. If the deadline passes, **no companion
//! is written and the replay commits to `Absent`**, loudly and by seat. That is
//! the decision rather than a fallback: a companion covering six of nine seats
//! would be a corpus artefact whose coverage is a function of who managed to copy
//! a file, and `replay::manifest::Commitment::Absent` is a legitimate named state
//! that says exactly what happened.
//!
//! Usage:
//! `moba-server [ticks] [tick-ms] [signing-key] [replay-out] [parts-dir] [telemetry-out] [wait-s]`.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use replay::telemetry::TelemetryLog;
use replay::{Commitment, MatchId, Recording, SessionFacts, SigningKey};
use server::{MatchConfig, net::Listener};

/// How long to wait for the clients' telemetry parts, by default.
///
/// A minute, and an argument, because the two cases are very different: nine
/// clients writing into one directory on one machine are done in milliseconds,
/// and nine people copying a file off nine laptops are not. It is a wait rather
/// than a prompt so that the same binary runs unattended in a harness.
const DEFAULT_WAIT_SECONDS: u64 = 60;

/// The seats that sent something, which is what the authority knows about who
/// was playing.
///
/// Read out of the recording rather than out of the config, because the config
/// says how many seats were *offered* and this says which were used. A client
/// sends one intention per tick, so a seat that played appears many times and a
/// seat that did not appears never.
fn seats_that_spoke(recording: &Recording) -> Vec<usize> {
    let mut seats: Vec<usize> = Vec::new();
    for timed in &recording.inputs {
        let seat = timed.input.player.index();
        if !seats.contains(&seat) {
            seats.push(seat);
        }
    }
    seats.sort_unstable();
    seats
}

/// Waits for one telemetry part per seat that spoke, and assembles them.
///
/// `None` when the deadline passes with parts missing, having said which seats.
/// Nothing partial is ever returned: see this module's header.
fn collect(parts: &Path, expected: &[usize], wait: Duration) -> Option<TelemetryLog> {
    let began = Instant::now();
    let mut announced = false;
    loop {
        let mut found: Vec<(String, Vec<u8>)> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(parts) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|kind| kind != "telemetry-part") {
                    continue;
                }
                if let Ok(bytes) = std::fs::read(&path) {
                    found.push((path.display().to_string(), bytes));
                }
            }
        }
        match TelemetryLog::assemble(&found) {
            Ok(log) if log.occupied() == expected => return Some(log),
            Ok(log) => {
                if !announced {
                    println!(
                        "telemetry: waiting for seats {:?} in {}",
                        expected
                            .iter()
                            .filter(|seat| !log.occupied().contains(seat))
                            .collect::<Vec<_>>(),
                        parts.display()
                    );
                    announced = true;
                }
            }
            Err(message) => {
                eprintln!("telemetry: {message}");
                return None;
            }
        }
        if began.elapsed() >= wait {
            eprintln!(
                "telemetry: no companion written — {} did not hold a part for every \
                 seat that played within {} s. The replay records that this match \
                 collected no device stream, which is a state rather than a \
                 corruption (docs/SCHEMA.md §11).",
                parts.display(),
                wait.as_secs()
            );
            return None;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let ticks: u32 = arguments
        .next()
        .map_or(Ok(1000), |value| value.parse())
        .unwrap_or(1000);
    let period_ms: u64 = arguments
        .next()
        .map_or(Ok(33), |value| value.parse())
        .unwrap_or(33);
    let key_path = arguments.next().map(PathBuf::from);
    let replay_path = arguments.next().map(PathBuf::from);
    let parts_path = arguments.next().map(PathBuf::from);
    let telemetry_path = arguments.next().map(PathBuf::from);
    let wait = Duration::from_secs(
        arguments
            .next()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_WAIT_SECONDS),
    );

    // Loaded before the match rather than after it, because a run that plays a
    // thousand ticks and then discovers it cannot read its key has thrown away
    // the match it was supposed to seal.
    let signing = match key_path.as_ref().map(SigningKey::load) {
        None => None,
        Some(Ok(key)) => Some(key),
        Some(Err(error)) => {
            eprintln!(
                "moba-server: {}: {error}",
                key_path.expect("a path we just read").display()
            );
            return ExitCode::FAILURE;
        }
    };

    let listener = match Listener::bind(SocketAddr::from(([127, 0, 0, 1], 0))) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("moba-server: {error}");
            return ExitCode::FAILURE;
        }
    };
    let address = match listener.local_addr() {
        Ok(address) => address,
        Err(error) => {
            eprintln!("moba-server: {error}");
            return ExitCode::FAILURE;
        }
    };

    println!("address {address}");
    println!(
        "certificate {}",
        listener
            .certificate()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    println!("ticks {ticks}");

    match listener
        .host(
            MatchConfig {
                seed: 0x00C0_FFEE_0D15_EA5E,
                players: 3,
            },
            Duration::from_millis(period_ms),
            ticks,
        )
        .await
    {
        Ok(recording) => {
            println!("inputs {}", recording.inputs.len());
            println!(
                "digest {}",
                recording
                    .final_state_digest
                    .as_bytes()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            );

            let (Some(key), Some(path)) = (signing, replay_path) else {
                println!(
                    "replay not written: no signing key and no output path were given, \
                     and this build writes no unsigned file"
                );
                return ExitCode::SUCCESS;
            };
            // The match identifier is drawn from the clock and the seed rather
            // than from a random source, and it is worth saying why that is
            // enough: it distinguishes one match from another in a corpus a
            // single operator assembles, which is all `docs/RISKS.md` R4 asks it
            // for. It is not a secret and nothing may treat it as unguessable.
            let started_at_unix_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| {
                    u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
                });
            let mut id = [0u8; 16];
            id[..8].copy_from_slice(&started_at_unix_ms.to_be_bytes());
            id[8..].copy_from_slice(&recording.seed.to_be_bytes());
            let mut facts = SessionFacts::anonymous(MatchId(id), started_at_unix_ms);

            // The companion, first, because the replay's manifest commits to its
            // digest and a digest has to exist before something can commit to it.
            let companion = match (&parts_path, &telemetry_path) {
                (Some(parts), Some(_)) => {
                    let expected = seats_that_spoke(&recording);
                    println!("telemetry: {} seat(s) played", expected.len());
                    collect(parts, &expected, wait)
                        .map(|log| replay::telemetry::seal(&log, &facts, &key))
                }
                _ => None,
            };
            if let (Some(companion), Some(path)) = (&companion, &telemetry_path) {
                facts.telemetry = Commitment::Sealed(companion.digest());
                if let Err(error) = std::fs::write(path, companion.encode()) {
                    eprintln!("moba-server: {}: {error}", path.display());
                    return ExitCode::FAILURE;
                }
                println!("telemetry {}", path.display());
                println!("telemetry digest {}", companion.digest());
            } else {
                println!(
                    "telemetry none: this match records no device stream \
                     (docs/SCHEMA.md §11)"
                );
            }

            let sealed = replay::seal(&recording, &facts, &key);
            if let Err(error) = std::fs::write(&path, sealed.encode()) {
                eprintln!("moba-server: {}: {error}", path.display());
                return ExitCode::FAILURE;
            }
            println!("replay {}", path.display());
            println!("identity {}", sealed.manifest.server_identity);
            println!("match {}", sealed.manifest.match_id);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("moba-server: {error}");
            ExitCode::FAILURE
        }
    }
}
