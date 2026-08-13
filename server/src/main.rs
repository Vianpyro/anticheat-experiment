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
//! Usage: `moba-server [ticks] [tick-ms] [signing-key] [replay-out]`.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use replay::{MatchId, SessionFacts, SigningKey};
use server::{MatchConfig, net::Listener};

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
            let facts = SessionFacts::anonymous(MatchId(id), started_at_unix_ms);

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
