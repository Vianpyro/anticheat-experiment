//! What two clients on one team are, and are not, supposed to agree about.
//!
//! `docs/MILESTONES.md` M3's exit criterion compares the reconciled local worlds
//! of three clients on one team. The comparison is only meaningful over what the
//! three are entitled to the same view of, and getting that boundary wrong is
//! silent in both directions: too much in the digest and the criterion fails on
//! a legitimate match, too little and it passes on a leak.
//!
//! This file pins the boundary from both sides on a match driven in process, so
//! that the case it is about — a teammate on different hit points — is produced
//! rather than waited for.

#![deny(unsafe_code)]

use client::Headless;
use protocol::{ClientFrame, ClientMessage, ServerFrame};
use server::{Match, MatchConfig};
use sim::{Action, Liveness, Seat, base_position};

const TEAM: [Seat; 3] = [Seat::Blue0, Seat::Blue1, Seat::Blue2];

fn seated() -> Match {
    let mut game = Match::new(MatchConfig {
        seed: 0x00C0_FFEE_0D15_EA5E,
        players: 3,
    });
    for _ in 0..3 {
        assert!(game.join().0.is_some(), "the match refused a seat it had");
    }
    for seat in TEAM {
        game.deliver(
            seat,
            ClientFrame::encode(&ClientMessage::Ready).as_bytes(),
            0,
        )
        .expect("ready was refused");
    }
    game
}

fn frame_for(frames: &[(Seat, ServerFrame)], seat: Seat) -> ServerFrame {
    frames
        .iter()
        .find(|(who, _)| *who == seat)
        .map(|(_, frame)| frame.clone())
        .expect("a seated player received no frame")
}

/// Three teammates agree about the world even when one of them is bleeding.
///
/// The bug this pins, found by M4's loss work and not by M3's own criterion:
/// `LocalWorld::digest` hashed the client's **own** liveness, which is this
/// player's own hit points and its own respawn timer. Two teammates on different
/// hit points therefore reported different digests for a world they were seeing
/// identically, and the criterion said `Blue0 disagrees with Blue1 about the
/// world at tick 620`.
///
/// It went unnoticed for a whole milestone because M3's scripted match never
/// produces damage: the three clients walk a lane, nothing touches them, and
/// their own hit points stay equal by accident. This walks them into a tower.
#[test]
fn three_teammates_agree_about_the_world_while_one_of_them_is_being_shot() {
    let mut game = seated();
    let mut clients: Vec<Headless> = TEAM.iter().map(|_| Headless::new()).collect();
    for (index, headless) in clients.iter_mut().enumerate() {
        let (_, accepted) = {
            let mut fresh = Match::new(MatchConfig {
                seed: 0x00C0_FFEE_0D15_EA5E,
                players: 3,
            });
            for _ in 0..index {
                let _ = fresh.join();
            }
            fresh.join()
        };
        headless
            .receive(accepted.as_bytes())
            .expect("the acceptance was refused");
    }

    // Straight down the Blue–Red lane and into the range of Red's tower, which
    // shoots the lowest-numbered seat it can see. That is what puts exactly one
    // of the three on different hit points from the other two.
    let destination = Action::Move(base_position(Seat::Red0.team(), &sim::RULES));

    let mut hurt_ticks = 0u32;
    let mut compared = 0u32;

    for tick in 0..760u32 {
        for seat in TEAM {
            game.deliver(
                seat,
                ClientFrame::encode(&ClientMessage::Input {
                    seq: tick,
                    claimed_at_ms: 0,
                    action: destination,
                })
                .as_bytes(),
                0,
            )
            .expect("the server refused a well-sequenced input");
        }
        let frames = game.tick();
        for (index, seat) in TEAM.into_iter().enumerate() {
            clients[index]
                .receive(frame_for(&frames, seat).as_bytes())
                .expect("a client refused a frame the server sent it");
        }

        let hp: Vec<Option<i32>> = TEAM
            .iter()
            .map(|seat| match game.world().champion(*seat).liveness {
                Liveness::Alive { hp } => Some(hp.to_raw()),
                Liveness::Dead { .. } => None,
            })
            .collect();
        if hp.iter().any(|value| *value != hp[0]) {
            hurt_ticks = hurt_ticks.saturating_add(1);
        }

        let first = clients[0].world().digest();
        for (index, seat) in TEAM.into_iter().enumerate().skip(1) {
            assert_eq!(
                clients[index].world().digest(),
                first,
                "tick {tick}: {seat:?} disagrees with Blue0, own hit points {hp:?}"
            );
            compared = compared.saturating_add(1);
        }
    }

    assert!(
        hurt_ticks > 100,
        "the three teammates were on equal hit points for all but {hurt_ticks} ticks, \
         so agreeing is not evidence that the digest excludes what it should"
    );
    assert_eq!(
        compared,
        760 * 2,
        "the loop compared fewer ticks than it ran"
    );
}

/// …and the exclusion is narrow: everything a teammate *can* see is still in the
/// digest.
///
/// The other half of the boundary. Dropping `own_liveness` would be the wrong
/// fix if it took a champion's position or hit points with it, because then the
/// criterion would pass on a client whose world had drifted. A client's own
/// champion is folded into the entity list at exactly the fidelity a teammate is
/// given for it, so a change to its position moves every one of the three
/// digests together — and moves them at all, which is what this checks.
#[test]
fn the_digest_still_follows_everything_a_teammate_can_see() {
    let mut game = seated();
    let mut headless = Headless::new();
    let (_, accepted) = Match::new(MatchConfig {
        seed: 0x00C0_FFEE_0D15_EA5E,
        players: 3,
    })
    .join();
    headless
        .receive(accepted.as_bytes())
        .expect("the acceptance was refused");

    let destination = Action::Move(base_position(Seat::Red0.team(), &sim::RULES));
    game.deliver(
        Seat::Blue0,
        ClientFrame::encode(&ClientMessage::Input {
            seq: 0,
            claimed_at_ms: 0,
            action: destination,
        })
        .as_bytes(),
        0,
    )
    .expect("the server refused the input");

    let frames = game.tick();
    headless
        .receive(frame_for(&frames, Seat::Blue0).as_bytes())
        .expect("a client refused a frame");
    let standing = headless.world().digest();

    let frames = game.tick();
    headless
        .receive(frame_for(&frames, Seat::Blue0).as_bytes())
        .expect("a client refused a frame");
    assert_ne!(
        headless.world().digest(),
        standing,
        "the champion moved and the digest did not"
    );
}
