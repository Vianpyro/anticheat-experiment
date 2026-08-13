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
//! Usage: `moba-server [ticks] [tick-ms]`.

use std::net::SocketAddr;
use std::process::ExitCode;
use std::time::Duration;

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
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("moba-server: {error}");
            ExitCode::FAILURE
        }
    }
}
