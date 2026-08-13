//! Prediction is exact, or it is a rendering bug with a good excuse.
//!
//! `docs/MILESTONES.md` M4 asks for prediction and reconciliation. The claim
//! worth testing is not "the client draws something plausible" — it is that the
//! position the client renders *before* the server has answered is the position
//! the server is about to report, byte for byte, so that a player on a fast
//! connection never sees a correction at all.
//!
//! That is testable because both ends are deterministic and the `Match` is
//! driven rather than driving: the test sends an intention, asks the client
//! where it thinks it will be, runs exactly one tick, and requires the server's
//! own answer to be the same `Fx` pair. No tolerance, no distance threshold. A
//! prediction that agrees to within a rounding step is a prediction that has
//! stopped calling `sim`'s arithmetic, which is the failure this file exists to
//! catch and the one a tolerance would hide.
//!
//! The transport is not in the loop here, deliberately: what is under test is
//! the reconciliation rule, and running it over QUIC would add a source of lost
//! frames with nothing to say about whether the rule is right. `m4_exit.rs` is
//! where both are in the loop at once.

#![deny(unsafe_code)]

use client::predict::Prediction;
use protocol::{ClientFrame, ClientMessage, ServerFrame, ServerMessage};
use server::{Match, MatchConfig};
use sim::view::PlayerView;
use sim::{Action, EntityId, Fx, FxVec2, Seat};

/// A match with one team seated and ready — M4's own shape, three humans and
/// six empty seats.
fn seated() -> Match {
    let mut game = Match::new(MatchConfig {
        seed: 0x00C0_FFEE_0D15_EA5E,
        players: 3,
    });
    for _ in 0..3 {
        let (seat, _) = game.join();
        assert!(seat.is_some(), "the match refused a seat it had");
    }
    for seat in [Seat::Blue0, Seat::Blue1, Seat::Blue2] {
        game.deliver(
            seat,
            ClientFrame::encode(&ClientMessage::Ready).as_bytes(),
            0,
        )
        .expect("ready was refused");
    }
    game
}

/// The view and acknowledgement a seat was sent this tick.
fn frame_for(frames: &[(Seat, ServerFrame)], seat: Seat) -> (PlayerView, Option<u32>) {
    let frame = frames
        .iter()
        .find(|(who, _)| *who == seat)
        .map(|(_, frame)| frame)
        .expect("a seated player received no frame");
    match ServerFrame::decode(frame.as_bytes()) {
        Ok(ServerMessage::View {
            view,
            applied_through,
        }) => (view, applied_through),
        other => panic!("a tick produced {other:?} rather than a view"),
    }
}

/// Sends one intention and returns the frames of the tick that applies it.
fn round(game: &mut Match, seq: u32, action: Action) -> (PlayerView, Option<u32>) {
    game.deliver(
        Seat::Blue0,
        ClientFrame::encode(&ClientMessage::Input {
            seq,
            claimed_at_ms: 0,
            action,
        })
        .as_bytes(),
        0,
    )
    .expect("the server refused a well-sequenced input");
    let frames = game.tick();
    frame_for(&frames, Seat::Blue0)
}

/// The whole claim, in one loop: what the client renders is what the server is
/// about to say.
///
/// One intention per tick, which is the protocol's shape and the traffic the
/// playable client produces. The walk is long enough to leave Blue's base for
/// the middle of the map and then turn, so it covers a champion under way, a
/// champion arriving at a destination and stopping, a cast that must not disturb
/// the standing order, and a change of mind halfway.
#[test]
fn the_position_a_client_renders_is_the_one_the_server_is_about_to_report() {
    let mut game = seated();

    // The first view is the anchor. There is nothing to predict before it.
    let (first, _) = frame_for(&game.tick(), Seat::Blue0);
    let spawn = first.own.position;
    let mut prediction = Prediction::anchored(&first);

    let mut compared = 0u32;
    let mut ahead = 0u32;

    // The standing intention, re-sent every tick until the player changes it.
    // `Action::Idle` is not "nothing to say" — it is a rule that stops the
    // champion — so a client with nothing new to ask for repeats what it is
    // already asking for. Setting the same destination twice is idempotent in
    // `sim`, and the traffic stays one frame a tick either way.
    let mut standing = Action::Move(FxVec2::new(Fx::from_int(4), Fx::from_int(40)));

    for tick in 0..600u32 {
        let action = match tick {
            // A one-shot ability, which must not disturb the standing order —
            // in `sim` or in the prediction.
            120 => Action::Skillshot(FxVec2::new(Fx::ONE, Fx::ZERO)),
            300 => {
                standing = Action::Move(FxVec2::new(Fx::from_int(-20), Fx::from_int(60)));
                standing
            }
            _ => standing,
        };

        prediction.sent(tick, action);
        // The rendered position, decided before the server has answered.
        let rendered = prediction.position();
        if rendered != prediction.authoritative() {
            ahead = ahead.saturating_add(1);
        }

        let (view, applied_through) = round(&mut game, tick, action);
        assert_eq!(
            rendered, view.own.position,
            "tick {tick}: the client drew {rendered:?}, the server says {:?}",
            view.own.position
        );
        compared = compared.saturating_add(1);

        prediction.observe(&view, applied_through);
        assert_eq!(
            prediction.corrections().0,
            0,
            "tick {tick}: an exact prediction reported a correction"
        );
        assert_eq!(
            prediction.outstanding(),
            0,
            "tick {tick}: the acknowledgement did not drain the queue"
        );
    }

    assert_eq!(compared, 600, "the loop compared fewer ticks than it ran");
    assert!(
        ahead * 2 > compared,
        "the prediction was ahead of the authoritative position on only {ahead} of \
         {compared} ticks, so agreeing with the server is mostly not evidence"
    );
    assert_ne!(
        prediction.authoritative(),
        spawn,
        "the champion never left its spawn, so the walk proves nothing"
    );
    assert_eq!(prediction.corrections().1, 0, "the worst correction");
}

/// The order an acknowledged intention set stays behind when the intention
/// leaves the queue.
///
/// This is the half of reconciliation that is easy to get wrong in the quiet
/// direction. Dropping acknowledged intentions is obvious — keep them and the
/// client walks further every tick, which is visible immediately. Folding the
/// order they established into the anchor is not: forget it, and the client
/// predicts a champion standing still while the server walks it, which looks
/// like ordinary lag rather than like a bug.
///
/// The case that separates the two is a one-shot ability sent while a walk is
/// under way. The cast is outstanding, the walk has been acknowledged, and the
/// only way to predict the next tick's movement is to have kept the order the
/// acknowledged input set.
#[test]
fn an_acknowledged_intention_leaves_the_queue_and_the_order_it_set_stays_behind() {
    let mut game = seated();
    let (first, _) = frame_for(&game.tick(), Seat::Blue0);
    let mut prediction = Prediction::anchored(&first);

    let walk = Action::Move(FxVec2::ZERO);
    prediction.sent(0, walk);
    assert_eq!(prediction.outstanding(), 1);
    let (view, applied_through) = round(&mut game, 0, walk);
    assert_eq!(applied_through, Some(0), "the server applied it");
    let anchored_at = view.own.position;
    prediction.observe(&view, applied_through);
    assert_eq!(prediction.outstanding(), 0, "and the client dropped it");

    // A cast, which changes no order. The prediction must still advance, and it
    // can only do so from an order it no longer holds an intention for.
    let cast = Action::Skillshot(FxVec2::new(Fx::ONE, Fx::ZERO));
    prediction.sent(1, cast);
    let rendered = prediction.position();
    assert_ne!(
        rendered, anchored_at,
        "the client forgot the walk the moment the server acknowledged it"
    );

    let (view, applied_through) = round(&mut game, 1, cast);
    assert_eq!(
        rendered, view.own.position,
        "the standing order was folded into the anchor wrongly"
    );
    prediction.observe(&view, applied_through);
    assert_eq!(prediction.corrections().1, 0);
}

/// With nothing outstanding the prediction is the last view, and that is one
/// tick of staleness this design does not remove.
///
/// Written down as a test rather than as a caveat, because it is the boundary of
/// what `client::predict` claims: it hides the round trip and not the tick. A
/// future extrapolation would make this test fail, which is the point — it would
/// be a change of contract and not an improvement snuck in.
#[test]
fn with_nothing_outstanding_the_prediction_is_exactly_the_last_view() {
    let mut game = seated();
    let (first, _) = frame_for(&game.tick(), Seat::Blue0);
    let mut prediction = Prediction::anchored(&first);

    let walk = Action::Move(FxVec2::ZERO);
    prediction.sent(0, walk);
    let (view, applied_through) = round(&mut game, 0, walk);
    prediction.observe(&view, applied_through);

    assert_eq!(prediction.outstanding(), 0);
    assert_eq!(prediction.position(), view.own.position);

    // …and the server has since moved on, which is the staleness being named.
    let (later, _) = frame_for(&game.tick(), Seat::Blue0);
    assert_ne!(
        later.own.position, view.own.position,
        "the champion stopped walking, so there is no staleness to measure"
    );
    assert_eq!(prediction.position(), view.own.position);
}

/// An order the client cannot follow is not guessed at.
///
/// `Action::Attack` walks toward an entity's position, and the fog may be hiding
/// it. The prediction stops advancing rather than inventing a destination: a
/// champion that stands still until the server answers is visibly lagging, and
/// one that walks toward a guess is visibly wrong — and it is the second that
/// would put on the screen a position the client was never given.
#[test]
fn an_attack_order_stops_the_prediction_rather_than_guessing_where_it_leads() {
    let mut game = seated();
    let (first, _) = frame_for(&game.tick(), Seat::Blue0);
    let mut prediction = Prediction::anchored(&first);
    let standing = prediction.authoritative();

    // A handle this client has never been shown, which is the case that matters:
    // it cannot know where that entity is, and neither can the prediction.
    prediction.sent(0, Action::Attack(EntityId(4)));
    assert_eq!(
        prediction.position(),
        standing,
        "the prediction walked toward a position it was not given"
    );
}

/// A dead champion is not on the map, so there is nothing to predict about it.
#[test]
fn a_dead_champion_is_not_predicted_forward() {
    let mut game = seated();
    let (first, _) = frame_for(&game.tick(), Seat::Blue0);
    let mut dead = first.clone();
    dead.own.liveness = sim::Liveness::Dead {
        respawn_at: sim::Tick(450),
    };
    let mut prediction = Prediction::anchored(&dead);
    prediction.sent(0, Action::Move(FxVec2::ZERO));
    prediction.sent(1, Action::Move(FxVec2::ZERO));
    assert_eq!(prediction.position(), dead.own.position);
}
