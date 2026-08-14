//! Exploit class 5: protocol abuse — replay, reordering, out-of-sequence,
//! unvalidated handles, and messages that do not belong in a session's state.
//!
//! # Which layer, stated because `docs/SCOPE.md` requires it
//!
//! QUIC defeats the *packet*-level attack beneath the application: a duplicated or
//! reordered datagram is rejected before this code sees it, and this crate cannot
//! reach that layer to test it. What is left is the **application-level residue**
//! `Match::deliver` owns — two distinct packets carrying one intention, a sequence
//! number that went backwards, a second `Join`, a handle that names nothing — and
//! that is what every exploit here sends.
//!
//! # The works/fails pair for a rule, not a projection
//!
//! For classes 1 and 2 the weakened defence is a projection or a registry to
//! swap. Class 5's defences are the sequence and session rules in
//! `Match::deliver`, which cannot be switched off without editing the server. So
//! each exploit establishes the "it would have worked" half differently and the
//! test says so: the attacker's frame is **well-formed and would be acted upon by
//! a server without the rule** — it decodes to a legal `ClientMessage`, it names a
//! real seat — and then the rule refuses it. An exploit whose frame did not even
//! decode would be `docs/RISKS.md` R15 with a protocol error: a refusal one step
//! short of the check it is about.

#![deny(unsafe_code)]

use cheat_client::abuse;
use protocol::{Action, ClientFrame, ClientMessage, EntityId, FxVec2};
use server::Violation;
use sim::{Seat, champion_entity_id};

#[path = "harness/authority.rs"]
mod authority;

use authority::started_match;

/// The seat the attacker occupies. `started_match` fills it and eight others.
const ATTACKER: Seat = Seat::Blue0;

/// A replayed input — the same intention in two distinct packets — is applied
/// once and refused once.
#[test]
fn a_replayed_input_is_applied_once() {
    let mut game = started_match(0x00C0_FFEE_0D15_EA5E, 9);

    let [first, second] = abuse::replayed_input(0, Action::Move(FxVec2::ZERO));

    // The antecedent: both are the same well-formed intention. QUIC would pass
    // them both; there is nothing at the packet layer to tell them apart.
    assert_eq!(
        ClientFrame::decode(first.as_bytes()).expect("first decodes"),
        ClientFrame::decode(second.as_bytes()).expect("second decodes"),
        "the two packets are not the same intention, so this is not a replay"
    );

    // The first is accepted…
    game.deliver(ATTACKER, first.as_bytes().as_slice(), 0)
        .expect("the first input was accepted");
    // …and the second, same sequence number, is refused as a sequence violation.
    let error = game
        .deliver(ATTACKER, second.as_bytes().as_slice(), 1)
        .expect_err("the replayed input was applied a second time");
    assert_eq!(
        error,
        Violation::Sequence,
        "the replay was refused for the wrong reason"
    );

    // And it was counted, so an operator can tell a replaying client from a
    // stalling one.
    assert_eq!(game.refused(ATTACKER), 1, "the refusal was not recorded");
    println!("abuse: an application-level replay is applied once and refused once");
}

/// An input whose sequence number went backwards is refused.
#[test]
fn an_out_of_order_input_is_refused() {
    let mut game = started_match(0x00C0_FFEE_0D15_EA5E, 9);

    let [high, low] = abuse::out_of_order(3, 9, Action::Move(FxVec2::ZERO));

    // The higher sequence number lands first.
    game.deliver(ATTACKER, high.as_bytes().as_slice(), 0)
        .expect("the higher sequence number was accepted");
    // Now the lower one: a legal frame, a real intention, refused only because the
    // server accepts strictly increasing sequence numbers.
    assert!(
        matches!(
            ClientFrame::decode(low.as_bytes()),
            Ok(ClientMessage::Input { .. })
        ),
        "the reordered frame is not even an input (R15)"
    );
    let error = game
        .deliver(ATTACKER, low.as_bytes().as_slice(), 1)
        .expect_err("an input with a stale sequence number was accepted");
    assert_eq!(error, Violation::Sequence);
    println!("abuse: an out-of-order input is refused once the higher number has landed");
}

/// A second `Join` on an established session is refused as out of order.
#[test]
fn a_second_join_is_refused() {
    let mut game = started_match(0x00C0_FFEE_0D15_EA5E, 9);

    let join = abuse::second_join();
    assert!(
        matches!(
            ClientFrame::decode(join.as_bytes()),
            Ok(ClientMessage::Join)
        ),
        "the frame is not a Join (R15)"
    );
    let error = game
        .deliver(ATTACKER, join.as_bytes().as_slice(), 0)
        .expect_err("a second Join was accepted on an established session");
    assert_eq!(error, Violation::OutOfOrder);
    println!("abuse: a second Join on an established session is refused");
}

/// An `Attack` naming a handle that resolves to nothing, or to an ally, is
/// discarded — it does not become the champion's standing order.
///
/// This is the exploit `docs/ARCHITECTURE.md`'s `apply_inputs` guards against:
/// storing an arbitrary handle would let an attacker write it into the state. The
/// frame *is accepted* — it is a legal input — but the order does not take, which
/// the test confirms by watching what the champion does next.
#[test]
fn an_attack_on_a_bad_handle_does_not_become_an_order() {
    let mut game = started_match(0x00C0_FFEE_0D15_EA5E, 9);

    // First, a real move order, so the champion has a known standing order to
    // watch. It walks toward the centre.
    let move_frame = ClientFrame::encode(&ClientMessage::Input {
        seq: 0,
        claimed_at_ms: 0,
        action: Action::Move(FxVec2::ZERO),
    });
    game.deliver(ATTACKER, move_frame.as_bytes().as_slice(), 0)
        .expect("the move was accepted");
    let _ = game.tick();
    let after_move = game.world().champion(ATTACKER).position;

    // Now an attack on a handle that resolves to nothing. `deliver` accepts the
    // frame — it is a legal intention — but the rule discards the order, leaving
    // the move in force.
    let bad = abuse::attack_nonexistent(1);
    game.deliver(ATTACKER, bad.as_bytes().as_slice(), 1)
        .expect("the attack frame is a legal input and is accepted");
    let _ = game.tick();
    let after_bad = game.world().champion(ATTACKER).position;

    // The champion kept walking on its move order rather than freezing to attack
    // a phantom: the bad handle never became an order.
    assert_ne!(
        after_move, after_bad,
        "the champion stopped moving, so the attack on a non-existent handle took \
         hold as an order"
    );

    // And an attack on the attacker's own ally is discarded the same way, by the
    // team check rather than the resolution check.
    let ally = champion_entity_id(Seat::Blue1);
    let friendly_fire = abuse::attack_own_ally(2, EntityId(ally.0));
    game.deliver(ATTACKER, friendly_fire.as_bytes().as_slice(), 2)
        .expect("the frame is legal and accepted");
    let before = game.world().champion(Seat::Blue1).liveness;
    for _ in 0..30 {
        let _ = game.tick();
    }
    let after = game.world().champion(Seat::Blue1).liveness;
    assert_eq!(before, after, "an ally took damage from friendly fire");
    println!(
        "abuse: an attack on a bad or friendly handle is accepted and discarded, never stored"
    );
}

/// A byte string that is not a frame is refused, and does not panic the server.
#[test]
fn garbage_is_refused_without_a_panic() {
    let mut game = started_match(0x00C0_FFEE_0D15_EA5E, 9);

    for bytes in [
        abuse::garbage(vec![]),
        abuse::garbage(vec![0xFF; 3]),
        abuse::garbage(vec![0x00; protocol::CLIENT_FRAME_BYTES]),
        abuse::garbage(vec![0xAB; protocol::CLIENT_FRAME_BYTES + 40]),
        abuse::garbage((0..=255).collect()),
    ] {
        // The only acceptable outcomes are "accepted as a legal frame" or "refused
        // as a violation". A panic — an index out of bounds, an unbounded
        // allocation — would be the vulnerability `SECURITY.md` asks to hear about.
        let _: Result<(), Violation> = game.deliver(ATTACKER, bytes.as_slice(), 0);
    }
    // The match is still alive and still authoritative after all of it.
    let _ = game.tick();
    println!("abuse: hostile byte strings are refused at the frontier, and the server ticks on");
}
