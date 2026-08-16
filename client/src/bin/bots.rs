//! `moba-bots`: fills the seats nobody is sitting in, so that one or two people
//! can play a nine-seat match.
//!
//! ```text
//! moba-bots <address> <certificate-hex> [count]
//! ```
//!
//! The address and the certificate are printed by `moba-server` on startup, and
//! are the same two arguments `moba-client` takes — this connects the same way a
//! person's client does, because it *is* the same transport and the same session
//! state machine. `count` defaults to eight, which is a nine-seat match with one
//! human in it; the server hands out the lowest free seat, so start the people
//! first if you care which seats they get.
//!
//! A match of nine has to be a match of nine on the server too:
//! `moba-server --players 9`. The server will not run a tick until that many
//! seats are filled and ready, and it refuses a join after the first one.
//!
//! # This is a playtest tool and it is not three of the other things
//!
//! `docs/MILESTONES.md` says it in those terms and so does this file, because
//! the reader who reaches for it is the reader most likely to hope otherwise:
//!
//! - It **does not satisfy M4's exit criterion**, which asks for three humans on
//!   two operating systems. That clause is a fact about a calendar and no
//!   program stands in for it.
//! - It **produces no corpus data**. A bot seat writes no session part because
//!   there is no device behind it to write one about, and `replay::Attested` —
//!   the only value `Corpus::store` accepts — refuses a match whose input log
//!   shows a seat playing that no session record accounts for. Filing the
//!   evening a bot played is not a mistake to avoid; it is a value that cannot
//!   be built.
//! - It **calibrates nothing.** `client::lobby` measures a hand crossing a menu,
//!   and there is no hand here.
//!
//! # What it does, and the one thing it will never do
//!
//! One session per bot, each on its own task: `Join`, `Ready`, then one
//! intention per received frame for as long as the server keeps sending them.
//! The decision is `client::bot::Bot::observe`, which is a pure function of the
//! view the server chose to send — so a bot sees exactly what the fog leaves it,
//! and its intentions are ordinary `Action`s on the ordinary wire.
//!
//! **No device input is synthesised anywhere in this binary or in what it
//! calls.** `docs/RISKS.md` R7: a layer that moves a real mouse or presses real
//! keys through the operating system is the one part of a bot that generalises
//! to another game, and this project refuses to build it. There is no `uinput`,
//! no `SendInput`, no `XTest` and no pointer here; there is a socket and a
//! `match` over an enum.

use std::net::SocketAddr;
use std::process::ExitCode;

use client::Headless;
use client::bot::Bot;
use client::net::{Wire, certificate_from_hex};

/// Seats to fill when nobody says. Eight, which is a nine-seat match with one
/// person in it — the case this exists for.
const DEFAULT_BOTS: usize = 8;

/// What one bot came back with.
#[derive(Debug)]
struct Report {
    seat: sim::Seat,
    intentions: u32,
    fights: u32,
    views: u32,
    frames_lost: u32,
}

/// One bot: a session, a decision per frame, and one intention per frame.
///
/// The shape is `client/tests/m4_exit.rs`'s `play` with the person replaced,
/// deliberately: the criterion's harness is what established that this is the
/// input path a player drives, and a playtest that drove a different one would
/// be a playtest of something else.
async fn play(address: SocketAddr, certificate: Vec<u8>) -> Result<Report, String> {
    let mut wire = Wire::connect(address, &certificate)
        .await
        .map_err(|error| error.to_string())?;
    let mut session = Headless::new();

    wire.send(&session.join())
        .await
        .map_err(|error| error.to_string())?;
    let accepted = wire
        .recv_session()
        .await
        .map_err(|error| error.to_string())?;
    session
        .receive(&accepted)
        .map_err(|error| error.to_string())?;
    let seat = session.seat().ok_or("the server assigned no seat")?;
    wire.send(&session.ready())
        .await
        .map_err(|error| error.to_string())?;

    let mut bot = Bot::new(seat);
    let mut views = 0u32;
    loop {
        let Ok(frame) = wire.recv_state().await else {
            break;
        };
        session.receive(&frame).map_err(|error| error.to_string())?;
        let Some(view) = session.view() else {
            continue;
        };
        views = views.saturating_add(1);
        // The intention for this tick, and exactly one of them. `claimed_at_ms`
        // is zero for the reason the headless client's is: this is not a
        // participant and there is no clock here worth writing down. It is
        // recorded by the server and read by no rule.
        let action = bot.observe(view);
        if wire.send(&session.intend(action, 0)).await.is_err() {
            break;
        }
    }

    let (frames_lost, _) = wire.losses();
    let (intentions, fights) = bot.counters();
    Ok(Report {
        seat,
        intentions,
        fights,
        views,
        frames_lost,
    })
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [address, certificate, rest @ ..] = arguments.as_slice() else {
        eprintln!(
            "usage: moba-bots <address> <certificate-hex> [count]\n\n\
             Fills [count] seats with playtest bots, eight by default. The \
             address and the certificate are what `moba-server` prints; run it \
             with `--players 9` so that a nine-seat match starts.\n\n\
             A playtest tool: it is not M4's exit criterion, it produces no \
             corpus data, and it calibrates nothing (docs/MILESTONES.md)."
        );
        return ExitCode::from(2);
    };
    let Ok(address) = address.parse::<SocketAddr>() else {
        eprintln!("moba-bots: {address} is not an address");
        return ExitCode::from(2);
    };
    let Some(certificate) = certificate_from_hex(certificate) else {
        eprintln!("moba-bots: the certificate is not hex");
        return ExitCode::from(2);
    };
    let count = match rest.first() {
        None => DEFAULT_BOTS,
        Some(value) => match value.parse::<usize>() {
            Ok(count) if (1..=sim::PLAYER_COUNT).contains(&count) => count,
            _ => {
                eprintln!(
                    "moba-bots: {value} is not a number of seats between 1 and {}",
                    sim::PLAYER_COUNT
                );
                return ExitCode::from(2);
            }
        },
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("moba-bots: {error}");
            return ExitCode::FAILURE;
        }
    };

    runtime.block_on(async move {
        let mut playing = Vec::new();
        for _ in 0..count {
            playing.push(tokio::spawn(play(address, certificate.clone())));
        }
        let mut failures = 0u32;
        for handle in playing {
            match handle.await {
                Ok(Ok(report)) => println!(
                    "{:?}: {} intention(s), {} of them a fight, {} view(s), {} frame(s) lost",
                    report.seat, report.intentions, report.fights, report.views, report.frames_lost
                ),
                Ok(Err(error)) => {
                    eprintln!("moba-bots: {error}");
                    failures = failures.saturating_add(1);
                }
                Err(error) => {
                    eprintln!("moba-bots: a bot task panicked: {error}");
                    failures = failures.saturating_add(1);
                }
            }
        }
        // The counter, printed on the way out whether or not anything went
        // wrong. `docs/RISKS.md` R15's habit: a playtest in which every bot
        // reported zero fights is a playtest that told you nothing about a
        // fight, and the number is what makes that visible rather than
        // something an operator has to notice.
        println!("moba-bots: {count} seat(s) filled, {failures} failed");
        if failures == 0 {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    })
}
