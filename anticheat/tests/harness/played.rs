//! Playing a whole match with nine bots, so that a detector has something to
//! read.
//!
//! # Why this exists on this side of the line
//!
//! `docs/ARCHITECTURE.md` gives `cheat-client` `server` and `sim` as
//! dev-dependencies so that an exploit asserting *the server accepted Y* can
//! send Y to a real authority. This is the mirror of that arrangement and the
//! reason is the same one read backwards: an assertion of the form *the detector
//! responded to this behaviour* is a claim about the detector, and the honest
//! way to make it is to play the behaviour through a real server and score the
//! log the server wrote.
//!
//! What that costs is one edge, and it points the safe way. **`anticheat` links
//! `cheat-client` as a dev-dependency and `cheat-client` links nothing of
//! `anticheat` at all** — not even in a test binary — because `docs/SCOPE.md`'s
//! reason for keeping the detectors out of `client` is that thresholds must not
//! reach a machine the project assumes is compromised, and `cheat-client` is
//! that machine. `ci` asserts both directions with `cargo tree`.
//!
//! # What the match is, and why it is shaped like that
//!
//! Nine bots walk to the middle of the map and then oscillate: in to the
//! centroid, back a quarter of the way to their own base, in again. The
//! oscillation is not decoration — a reaction is measured from the moment an
//! enemy **enters** vision, and a match in which everybody converges once
//! produces one appearance per pair of champions and therefore about one
//! reaction each. `docs/RISKS.md` R15 is the standing hazard here: a detector
//! that abstains for want of pairs and a detector that finds nothing look
//! identical from outside, so the fixture has to reach the case, and
//! `tests/detectors.rs` asserts the count it reached.

use cheat_client::bot::{ClaimedClock, Reactor, Reflexes};
use protocol::{PlayerView, ServerFrame, ServerMessage};
use replay::Recording;
use server::{Match, MatchConfig};
use sim::{Fx, FxVec2, PLAYER_COUNT, RULES, Seat, base_position};

/// How long a played match runs.
///
/// Four hundred ticks for nine champions to close from their bases — a base is
/// a hundred units from the centroid and a champion covers `champion_speed`,
/// which is 0.2 units a tick — and twelve hundred more of oscillation, which is
/// six cycles at the amplitude below.
pub const TICKS: u32 = 1600;

/// One tick of the server's clock, in milliseconds, as this harness stamps it.
///
/// The game's own rate. The clock the *bots* claim is a separate thing and is
/// the point of [`ClaimedClock`].
pub const TICK_MS: u64 = 33;

/// Ticks between one end of the oscillation and the other.
///
/// The amplitude below is 18 units and a champion covers 0.2 a tick, so ninety
/// ticks is one leg. A hundred leaves the slack the attack-holds spend.
const LEG_TICKS: u32 = 100;

/// How far back toward its own base a bot pulls before closing again, in units.
///
/// Larger than `champion_vision_radius`, which is 12, or the enemies never
/// leave vision and there is one appearance for the whole match.
const AMPLITUDE: i32 = 18;

/// A match one variant played, and what it reached.
#[derive(Debug)]
pub struct Played {
    /// What this match was: the exploit it demonstrates, or the control.
    pub label: &'static str,
    /// What the authority recorded.
    pub recording: Recording,
    /// How many appearances the nine bots answered between them.
    pub answers: u32,
    /// Frames the server refused. Must be zero: `docs/SCOPE.md` class 3's
    /// standing verdict is that a bot sends nothing illegal, and a variant the
    /// server rejected would be a class-5 exploit wearing a class-3 label.
    pub refused: u32,
}

/// Plays a whole match with nine bots of one variant and returns what the
/// authority recorded.
#[must_use]
pub fn play(label: &'static str, seed: u64, reflexes: Reflexes, clock: ClaimedClock) -> Played {
    let mut game = Match::new(MatchConfig {
        seed,
        players: PLAYER_COUNT,
    });
    for _ in 0..PLAYER_COUNT {
        let (seat, _) = game.join();
        let seat = seat.expect("a seat was granted");
        game.deliver(
            seat,
            protocol::ClientFrame::encode(&protocol::ClientMessage::Ready)
                .as_bytes()
                .as_slice(),
            0,
        )
        .expect("ready was accepted");
    }

    let mut reactors: Vec<Reactor> = Seat::ALL
        .into_iter()
        .map(|seat| Reactor::new(seat, reflexes))
        .collect();
    let mut views: Vec<Option<PlayerView>> = vec![None; PLAYER_COUNT];
    let mut refused = 0u32;

    for tick in 0..TICKS {
        // The two ends of the oscillation. In on the first leg, back out on the
        // second, so that an enemy crosses the vision boundary about once a
        // cycle rather than once a match.
        //
        // **The three teams are out of phase with each other**, and that is not
        // decoration either. A bot answers one appearance at a time (one
        // intention per tick is the protocol's shape), so nine champions
        // arriving together produce one answer between six enemies and five
        // dropped ones — which is a fixture that reaches a case *once* while
        // looking like it reached it six times. A third of a cycle apart, an
        // observer meets its two enemy teams at two different moments and
        // answers both.
        for (index, reactor) in reactors.iter_mut().enumerate() {
            let seat = Seat::ALL[index];
            let phase = team_phase(seat);
            let inbound = (tick.saturating_add(phase) / LEG_TICKS).is_multiple_of(2);
            let target = if inbound {
                FxVec2::ZERO
            } else {
                base_position(seat.team(), &RULES).scale(Fx::from_ratio(AMPLITUDE, 100))
            };
            reactor.walk_to(target);
        }

        let observed_ms = u64::from(tick).saturating_mul(TICK_MS);
        for (index, reactor) in reactors.iter_mut().enumerate() {
            let seat = Seat::ALL[index];
            let Some(view) = views[index].as_ref() else {
                continue;
            };
            let action = reactor.observe(view);
            let claimed = clock.claim(observed_ms);
            let frame = reactor.bot().intend(action, claimed);
            if game
                .deliver(seat, frame.as_bytes().as_slice(), observed_ms)
                .is_err()
            {
                refused = refused.saturating_add(1);
            }
        }

        for (seat, frame) in game.tick() {
            if let Ok(ServerMessage::View { view, .. }) = ServerFrame::decode(frame.as_bytes()) {
                views[seat.index()] = Some(view);
            }
        }
    }

    Played {
        label,
        recording: game.recording(),
        answers: reactors.iter().map(Reactor::answers).sum(),
        refused,
    }
}

/// How far into the oscillation a team starts, in ticks.
///
/// A third of a cycle apart, which is what staggers the appearances. `Team` has
/// three values and no index — deliberately, since `docs/ARCHITECTURE.md`
/// removed `Team::opponent()` when the third team arrived — so this is a match
/// rather than arithmetic on a discriminant.
fn team_phase(seat: Seat) -> u32 {
    let cycle = LEG_TICKS.saturating_mul(2);
    match seat.team() {
        sim::Team::Blue => 0,
        sim::Team::Red => cycle / 3,
        sim::Team::Green => cycle.saturating_mul(2) / 3,
    }
}
