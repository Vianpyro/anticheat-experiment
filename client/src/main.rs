//! The headless client's binary: input scripts in, digests out.
//!
//! `docs/MILESTONES.md` M3 asks for headless clients only, and this is one. It
//! plays `Idle` every tick, which is a script, and prints the digest of its
//! reconciled local world at every hundredth tick. Anything a human would
//! recognise as playing is M4's.
//!
//! Usage: `moba-client <address> <certificate-hex>`, both printed by
//! `moba-server` on startup. The certificate is passed in rather than fetched
//! because the client trusts exactly that one; see `client::net`.

use std::net::SocketAddr;
use std::process::ExitCode;

use client::{Headless, net::Wire};
use sim::{Action, Tick};

#[tokio::main]
async fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let (Some(address), Some(certificate)) = (arguments.next(), arguments.next()) else {
        eprintln!("usage: moba-client <address> <certificate-hex>");
        return ExitCode::from(2);
    };
    let Ok(address) = address.parse::<SocketAddr>() else {
        eprintln!("moba-client: {address} is not an address");
        return ExitCode::from(2);
    };
    let Some(certificate) = unhex(&certificate) else {
        eprintln!("moba-client: the certificate is not hex");
        return ExitCode::from(2);
    };

    match play(address, &certificate).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("moba-client: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn play(address: SocketAddr, certificate: &[u8]) -> Result<(), String> {
    let mut wire = Wire::connect(address, certificate)
        .await
        .map_err(|error| error.to_string())?;
    let mut headless = Headless::new();

    wire.send(&headless.join())
        .await
        .map_err(|error| error.to_string())?;
    let accepted = wire.recv().await.map_err(|error| error.to_string())?;
    headless
        .receive(&accepted)
        .map_err(|error| error.to_string())?;
    println!("seat {:?}", headless.seat().ok_or("no seat")?);
    wire.send(&headless.ready())
        .await
        .map_err(|error| error.to_string())?;

    while let Ok(frame) = wire.recv().await {
        headless
            .receive(&frame)
            .map_err(|error| error.to_string())?;
        let Tick(tick) = headless.world().tick();
        if tick.is_multiple_of(100) {
            println!(
                "checkpoint {tick} {}",
                headless
                    .world()
                    .digest()
                    .as_bytes()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            );
        }
        if wire.send(&headless.intend(Action::Idle, 0)).await.is_err() {
            break;
        }
    }
    Ok(())
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok())
        .collect()
}
