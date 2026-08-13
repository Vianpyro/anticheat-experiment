//! The client binary: playable by default, scripted on request.
//!
//! `docs/MILESTONES.md` M4 is the playable client, and this is where a person
//! meets it. The window and the event loop are `client::gfx`; the parts of them
//! that can be wrong are elsewhere and are pure — `client::draw` decides what
//! goes on the screen, `client::input` and `client::play` decide what is
//! captured, `client::predict` decides what is predicted.
//!
//! The headless mode M3 shipped is still here behind `--headless`, because it is
//! what a second pair of hands uses to fill a seat when there are eight people
//! and nine seats, and because it is the only way to drive this binary where
//! there is no display — a container, a CI job, an exit-criterion harness.
//!
//! Usage:
//!
//! ```text
//! moba-client <address> <certificate-hex>              # play
//! moba-client <address> <certificate-hex> --headless   # idle, print digests
//! moba-client --probe-input [seconds]                  # measure the capture path
//! ```
//!
//! The address and the certificate are printed by `moba-server` on startup. The
//! certificate is passed in rather than fetched because the client trusts
//! exactly that one and nothing else; see `client::net`.
//!
//! `--probe-input` needs no server. It opens a window, records every device
//! event for a few seconds, and prints the inter-arrival distribution and the
//! finest motion it saw — the measurement behind the claim that this client's
//! sampling rate no longer follows the pointer's speed.

use std::net::SocketAddr;
use std::process::ExitCode;

use client::{Headless, net::Wire};
use sim::{Action, Tick};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let headless = arguments.iter().any(|argument| argument == "--headless");
    let probing = arguments.iter().any(|argument| argument == "--probe-input");
    let positional: Vec<&String> = arguments
        .iter()
        .filter(|argument| !argument.starts_with("--"))
        .collect();

    if probing {
        let seconds = positional
            .first()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(10);
        return finish(client::gfx::probe(seconds));
    }

    let [address, certificate] = positional.as_slice() else {
        eprintln!(
            "usage: moba-client <address> <certificate-hex> [--headless]\n       \
             moba-client --probe-input [seconds]"
        );
        return ExitCode::from(2);
    };
    let Ok(address) = address.parse::<SocketAddr>() else {
        eprintln!("moba-client: {address} is not an address");
        return ExitCode::from(2);
    };
    let Some(certificate) = unhex(certificate) else {
        eprintln!("moba-client: the certificate is not hex");
        return ExitCode::from(2);
    };

    if headless {
        // The headless client is the only one that wants a runtime of its own:
        // the playable one is driven by `winit`, which owns the thread it runs
        // on and keeps the transport on a thread beside it.
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                eprintln!("moba-client: {error}");
                return ExitCode::FAILURE;
            }
        };
        return finish(runtime.block_on(idle(address, &certificate)));
    }
    finish(client::gfx::play(address, &certificate))
}

fn finish(played: Result<(), String>) -> ExitCode {
    match played {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("moba-client: {error}");
            ExitCode::FAILURE
        }
    }
}

/// M3's client, unchanged: idle every tick, print a digest every hundredth.
///
/// It sends `Action::Idle`, which is a rule that means *stop* rather than a way
/// of saying nothing — that is exactly what this mode wants, since it is filling
/// a seat rather than playing it.
async fn idle(address: SocketAddr, certificate: &[u8]) -> Result<(), String> {
    let mut wire = Wire::connect(address, certificate)
        .await
        .map_err(|error| error.to_string())?;
    let mut headless = Headless::new();

    wire.send(&headless.join())
        .await
        .map_err(|error| error.to_string())?;
    let accepted = wire
        .recv_session()
        .await
        .map_err(|error| error.to_string())?;
    headless
        .receive(&accepted)
        .map_err(|error| error.to_string())?;
    println!("seat {:?}", headless.seat().ok_or("no seat")?);
    wire.send(&headless.ready())
        .await
        .map_err(|error| error.to_string())?;

    while let Ok(frame) = wire.recv_state().await {
        headless
            .receive(&frame)
            .map_err(|error| error.to_string())?;
        let Tick(tick) = headless.world().tick();
        if tick.is_multiple_of(100) {
            println!("checkpoint {tick} {}", hex(headless.world().digest()));
        }
        if wire.send(&headless.intend(Action::Idle, 0)).await.is_err() {
            break;
        }
    }
    let (incomplete, stale) = wire.losses();
    println!("frames lost {incomplete} shards late {stale}");
    Ok(())
}

fn hex(digest: sim::Digest) -> String {
    digest
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
