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
//!
//! # and, for a recording session (docs/MILESTONES.md M6):
//! moba-client <address> <certificate-hex> \
//!     --record <directory> --profile <id> --cpi <n> --polling <hz> \
//!     --acceleration off
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
//!
//! # The recording flags, and why the client asks rather than measures
//!
//! `--cpi` and `--polling` are what the participant was asked, because no
//! process can read them: a mouse reports counts, not the inch it crossed to
//! produce them. `--acceleration` is asked and required to be `off`, and a
//! session that says otherwise refuses to start rather than recording something
//! the corpus cannot use. `docs/SCHEMA.md` is the field-by-field account,
//! including what is measured beside each of these and what stays unknown.
//!
//! `--profile` is the fourth, and it is the one that makes a participant's
//! sessions poolable: an opaque label the operator keeps stable for as long as
//! the hardware does not change, so that the device profile `client::lobby`
//! measures accumulates across evenings instead of starting again every time.
//! It names a device rather than a person and it is not the pseudonym; a
//! participant who changes mouse gets a new one, which is the point.
//!
//! Without `--record` the client writes nothing at all. A person playing for fun
//! is not a recording session, and a flag is the difference.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use client::Headless;
use client::health::{Declared, Recorded};
use client::net::{Wire, certificate_from_hex};
use sim::{Action, Tick};

/// The value of `--name`, if it was given.
fn flag<'a>(arguments: &'a [String], name: &str) -> Option<&'a str> {
    let at = arguments.iter().position(|argument| argument == name)?;
    arguments.get(at.checked_add(1)?).map(String::as_str)
}

/// What the participant declared, or a message saying which answer is missing.
///
/// All three are required together: a corpus that held some sessions' hardware
/// and not others' would have a covariate present on part of the data, which is
/// worse than not having it at all — a detector fitted on the subset that has it
/// is fitted on a subset chosen by whoever remembered a flag.
fn declared(arguments: &[String]) -> Result<Declared, String> {
    let number = |name: &str| -> Result<u32, String> {
        flag(arguments, name)
            .ok_or_else(|| format!("--record needs {name} <n>"))?
            .parse::<u32>()
            .map_err(|_| format!("{name} takes a whole number"))
    };
    let acceleration = match flag(arguments, "--acceleration") {
        Some("off") => false,
        Some("on") => true,
        _ => return Err("--record needs --acceleration on|off".to_owned()),
    };
    // Constrained here rather than at the corpus, so that an operator finds out
    // before the evening rather than when they file it. The character set is
    // `replay::Pseudonym`'s, and for its reason: these strings are written into a
    // record `replay audit` reads byte by byte.
    let profile = flag(arguments, "--profile").ok_or(
        "--record needs --profile <id>: an opaque label for the device \
                this participant is playing on, stable across their sessions \
                (docs/SCHEMA.md §4a)",
    )?;
    if profile.is_empty()
        || profile.len() > 32
        || !profile
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(format!(
            "--profile {profile:?} is not a device profile label: at most 32 bytes \
             of letters, digits, `_` and `-`"
        ));
    }
    Ok(Declared {
        device_profile_id: profile.to_owned(),
        device_cpi: number("--cpi")?,
        device_polling_hz: number("--polling")?,
        pointer_acceleration: acceleration,
    })
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let headless = arguments.iter().any(|argument| argument == "--headless");
    let probing = arguments.iter().any(|argument| argument == "--probe-input");
    let recording = flag(&arguments, "--record").map(PathBuf::from);
    // The flags and their values both start with no `--`, so the values have to
    // come out of the positional list explicitly rather than by prefix.
    let flagged: Vec<String> = [
        "--record",
        "--profile",
        "--cpi",
        "--polling",
        "--acceleration",
    ]
    .into_iter()
    .filter_map(|name| flag(&arguments, name).map(str::to_owned))
    .collect();
    let positional: Vec<&String> = arguments
        .iter()
        .filter(|argument| !argument.starts_with("--") && !flagged.contains(argument))
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
             moba-client <address> <certificate-hex> --record <dir> --profile \
             <id> --cpi <n> --polling <hz> --acceleration off\n       \
             moba-client --probe-input [seconds]"
        );
        return ExitCode::from(2);
    };
    let Ok(address) = address.parse::<SocketAddr>() else {
        eprintln!("moba-client: {address} is not an address");
        return ExitCode::from(2);
    };
    let Some(certificate) = certificate_from_hex(certificate) else {
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

    let recorded = match recording {
        None => None,
        Some(directory) => match declared(&arguments) {
            Ok(declared) => Some(Recorded {
                directory,
                declared,
            }),
            Err(message) => {
                eprintln!("moba-client: {message}");
                return ExitCode::from(2);
            }
        },
    };
    finish(client::gfx::play(address, &certificate, recorded.as_ref()))
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
