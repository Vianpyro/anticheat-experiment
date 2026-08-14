//! Driving a real authority in-process, for the exploits whose claim is about
//! what a server does.
//!
//! # This is the half of M7 the attacker may not have
//!
//! `docs/ARCHITECTURE.md` forbids `cheat-client` a normal dependency on `server`
//! or `sim`, because an exploit that reaches into the victim is a test double.
//! This module links both, as dev-dependencies — and that is the design, not a
//! loophole. An exploit that asserts *the server rejected Y* is a claim about the
//! server, and the honest way to make it is to send Y to a real one. The attacker
//! (`src/`) produces the bytes; the judge (here) runs the authority; and the
//! boundary between them is that `cargo tree -p cheat-client --edges normal` shows
//! neither `server` nor `sim`.
//!
//! # Why this is a shared module and not one harness
//!
//! Seven exploit files include the harness by `#[path]`, and each is a separate
//! test binary that compiles the modules it names. This project's convention is
//! that a `#[path]`-shared harness is **fully consumed by every binary that
//! includes it** — that is why `client/tests/harness` has no `allow(dead_code)`.
//! So the harness is split by concern rather than pooled: every exploit that runs
//! a match includes this file and calls [`started_match`]; the vision ground
//! truth and the key registries live in siblings that only their own tests
//! include. No function here is dead in any binary, and none needs an allow.

use server::{Match, MatchConfig};

/// A match with `players` seats filled and ready, driven directly rather than
/// over the network.
///
/// `Match` is the authority `docs/ARCHITECTURE.md` describes as driven rather
/// than driving: no clock, no socket. That is exactly what an exploit suite
/// wants — advance the world a tick at a time, read the frames — and it is why
/// the traffic-shape and protocol-abuse exploits need no `quinn`.
#[must_use]
pub fn started_match(seed: u64, players: usize) -> Match {
    let mut game = Match::new(MatchConfig { seed, players });
    for _ in 0..players {
        let (seat, _) = game.join();
        let seat = seat.expect("a seat was granted");
        game.deliver(seat, ready_bytes().as_slice(), 0)
            .expect("ready was accepted");
    }
    game
}

/// The `Ready` frame's bytes, so the driver does not reach into the attacker
/// crate for one.
fn ready_bytes() -> Vec<u8> {
    protocol::ClientFrame::encode(&protocol::ClientMessage::Ready)
        .as_bytes()
        .to_vec()
}
