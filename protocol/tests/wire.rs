//! The trust boundary, tested as one.
//!
//! Everything a client can send arrives here first, and `docs/SCOPE.md` assumes
//! the sender is hostile. So these tests are in two halves: a round trip, which
//! says the codec is a codec, and a much longer list of byte strings that are
//! not messages, which says the decoder is a decoder rather than a parser that
//! happens to work on well-formed input.

#![deny(unsafe_code)]

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;

use protocol::{
    CLIENT_FRAME_BYTES, ClientFrame, ClientMessage, DecodeError, HEADER_BYTES, HandleSpace,
    RejectReason, SERVER_FRAME_BYTES, ServerFrame, ServerMessage, VERSION,
};
use sim::view::{EntityView, PlayerView, view_for};
use sim::{
    Action, EntityId, Fx, FxVec2, Input, PROJECTILE_ID_BASE, Seat, State, Tick, champion_entity_id,
    new_state, rules_hash, step, tower_entity_id,
};

const REGRESSIONS: &str = "proptest-regressions/wire.txt";

fn config() -> ProptestConfig {
    let default = ProptestConfig::default();
    ProptestConfig {
        // Scaled to the case count for the reason `sim/tests/properties.rs`
        // gives: proptest's fixed default of 1024 turns a raised budget into an
        // aborted test rather than a longer one. Nothing here rejects.
        max_global_rejects: default.cases.saturating_mul(4),
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(REGRESSIONS))),
        ..default
    }
}

// ---------------------------------------------------------------------------
// Round trips
// ---------------------------------------------------------------------------

fn client_messages() -> Vec<ClientMessage> {
    vec![
        ClientMessage::Join,
        ClientMessage::Ready,
        ClientMessage::Surrender,
        ClientMessage::Input {
            seq: 0,
            claimed_at_ms: 0,
            action: Action::Idle,
        },
        ClientMessage::Input {
            seq: u32::MAX,
            claimed_at_ms: u64::MAX,
            action: Action::Move(FxVec2::new(Fx::MAX, Fx::MIN)),
        },
        ClientMessage::Input {
            seq: 7,
            claimed_at_ms: 1_700_000_000_000,
            action: Action::Skillshot(FxVec2::new(Fx::ONE, Fx::NEG_ONE)),
        },
        ClientMessage::Input {
            seq: 8,
            claimed_at_ms: 1,
            action: Action::Targeted(EntityId(u16::MAX)),
        },
        ClientMessage::Input {
            seq: 9,
            claimed_at_ms: 2,
            action: Action::Attack(EntityId(0)),
        },
    ]
}

#[test]
fn every_client_message_survives_a_round_trip() {
    for message in client_messages() {
        let frame = ClientFrame::encode(&message);
        assert_eq!(
            ClientFrame::decode(frame.as_bytes()),
            Ok(message),
            "{message:?}"
        );
    }
}

#[test]
fn every_server_message_survives_a_round_trip() {
    let mut messages = vec![ServerMessage::Accepted {
        seat: Seat::Red2,
        seed: 0x00C0_FFEE_0D15_EA5E,
        rules_hash: rules_hash(),
    }];
    for reason in [
        RejectReason::UnsupportedVersion,
        RejectReason::MatchFull,
        RejectReason::AlreadyStarted,
        RejectReason::ProtocolViolation,
    ] {
        messages.push(ServerMessage::Rejected(reason));
    }
    for seat in Seat::ALL {
        messages.push(ServerMessage::View(view_for(&busy_state(), seat)));
    }

    for message in messages {
        let frame = ServerFrame::encode(&message);
        assert_eq!(
            ServerFrame::decode(frame.as_bytes()),
            Ok(message.clone()),
            "{message:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// The frame is one size, and that is the traffic invariant's first half
// ---------------------------------------------------------------------------

/// The size half of the traffic-shape invariant, asserted where it can still
/// be asserted at all.
///
/// It is carried by the *type* — `ServerFrame` wraps `[u8; SERVER_FRAME_BYTES]`
/// — so this cannot fail without the crate failing to compile first, and it is
/// worth being honest that a test which cannot go red is documentation. It is
/// here because `SERVER_FRAME_BYTES` is a derivation from
/// `PlayerView::MAX_ENCODED_BYTES`, and a derivation can be wrong: the
/// assertion with teeth is that the *widest* message still fits, which is the
/// next test.
#[test]
fn every_frame_is_its_direction_s_one_size() {
    for message in client_messages() {
        assert_eq!(
            ClientFrame::encode(&message).as_bytes().len(),
            CLIENT_FRAME_BYTES
        );
    }
    for seat in Seat::ALL {
        let view = ServerMessage::View(view_for(&busy_state(), seat));
        assert_eq!(
            ServerFrame::encode(&view).as_bytes().len(),
            SERVER_FRAME_BYTES
        );
    }
}

/// The bucket is wide enough for the worst case the *type* allows, not for the
/// worst case a match happened to produce.
///
/// This is the one that can fail: `pad` refuses a payload that does not fit,
/// and a view type that grew a field without the bound growing with it would
/// panic here rather than truncate a message in production.
#[test]
fn the_widest_possible_view_still_fits_one_frame() {
    let view = widest_view();
    assert_eq!(view.encode().len(), PlayerView::MAX_ENCODED_BYTES);
    let frame = ServerFrame::encode(&ServerMessage::View(view.clone()));
    assert_eq!(frame.as_bytes().len(), SERVER_FRAME_BYTES);
    assert_eq!(
        ServerFrame::decode(frame.as_bytes()),
        Ok(ServerMessage::View(view))
    );
}

// ---------------------------------------------------------------------------
// Hostile input
// ---------------------------------------------------------------------------

/// The seventh seat, refused at the one place it can arrive.
///
/// `sim::Seat` has six values, so this is the frontier the type exists for:
/// `sim/tests/properties.rs` used to assert that `PlayerId(200)` was ignored by
/// the rules, and that claim moved here when the value stopped existing. Every
/// byte from 6 to 255 is a `DecodeError::Seat` and never a seat.
#[test]
fn a_seat_byte_outside_the_match_is_refused_rather_than_folded_in() {
    let accepted = ServerFrame::encode(&ServerMessage::Accepted {
        seat: Seat::Blue0,
        seed: 1,
        rules_hash: rules_hash(),
    });
    for byte in 6u8..=u8::MAX {
        let mut bytes = *accepted.as_bytes();
        bytes[HEADER_BYTES] = byte;
        assert_eq!(
            ServerFrame::decode(&bytes),
            Err(DecodeError::Seat(byte)),
            "seat byte {byte}"
        );
    }
    // …and the six that do name a seat still do.
    for seat in Seat::ALL {
        let mut bytes = *accepted.as_bytes();
        bytes[HEADER_BYTES] = seat.index() as u8;
        assert!(matches!(
            ServerFrame::decode(&bytes),
            Ok(ServerMessage::Accepted { seat: decoded, .. }) if decoded == seat
        ));
    }
}

/// Padding is checked rather than skipped.
///
/// A receiver that ignores the bytes after a payload gives the sender a channel
/// to write into, and gives one message two encodings — which is what would let
/// two verifiers disagree about a recorded log.
#[test]
fn a_byte_hidden_in_the_padding_is_refused() {
    let frame = ClientFrame::encode(&ClientMessage::Join);
    // `Join` has no payload, so everything after the header is padding.
    for index in HEADER_BYTES..CLIENT_FRAME_BYTES {
        let mut bytes = *frame.as_bytes();
        bytes[index] = 0xAB;
        assert_eq!(
            ClientFrame::decode(&bytes),
            Err(DecodeError::Padding),
            "byte {index}"
        );
    }

    let view = ServerFrame::encode(&ServerMessage::View(view_for(&busy_state(), Seat::Blue0)));
    let mut bytes = *view.as_bytes();
    let last = bytes.len() - 1;
    bytes[last] = 1;
    assert_eq!(ServerFrame::decode(&bytes), Err(DecodeError::Padding));
}

#[test]
fn a_frame_from_another_protocol_is_refused_before_it_is_parsed() {
    let frame = ClientFrame::encode(&ClientMessage::Ready);
    for other in [0u16, VERSION.wrapping_sub(1), VERSION + 1, u16::MAX] {
        if other == VERSION {
            continue;
        }
        let mut bytes = *frame.as_bytes();
        bytes[0..2].copy_from_slice(&other.to_be_bytes());
        assert_eq!(
            ClientFrame::decode(&bytes),
            Err(DecodeError::Version {
                expected: VERSION,
                found: other
            })
        );
    }
}

#[test]
fn a_frame_of_the_wrong_length_is_refused() {
    let frame = ClientFrame::encode(&ClientMessage::Ready);
    for len in 0..CLIENT_FRAME_BYTES {
        assert_eq!(
            ClientFrame::decode(&frame.as_bytes()[..len]),
            Err(DecodeError::Length {
                expected: CLIENT_FRAME_BYTES,
                actual: len
            })
        );
    }
    let mut long = frame.as_bytes().to_vec();
    long.push(0);
    assert_eq!(
        ClientFrame::decode(&long),
        Err(DecodeError::Length {
            expected: CLIENT_FRAME_BYTES,
            actual: CLIENT_FRAME_BYTES + 1
        })
    );
}

#[test]
fn a_kind_byte_that_names_no_message_is_refused() {
    let frame = ClientFrame::encode(&ClientMessage::Join);
    for kind in 4u8..=u8::MAX {
        let mut bytes = *frame.as_bytes();
        bytes[2] = kind;
        assert_eq!(ClientFrame::decode(&bytes), Err(DecodeError::Kind(kind)));
    }
}

// ---------------------------------------------------------------------------
// The handle space
// ---------------------------------------------------------------------------

/// Reaches a state in which Blue has seen exactly one projectile while Red has
/// cast several out of Blue's sight.
///
/// Built by simulation from the public API, for the reason
/// `sim/tests/view_properties.rs` gives: a state assembled field by field can
/// be unreachable, and a property that holds only on unreachable states holds
/// about nothing. The two spawn lines are two hundred units apart against a
/// vision radius of twelve, so a cast at Red's spawn is a cast Blue cannot see.
fn state_with_casts_out_of_sight() -> State {
    let state = new_state(0x5EED_0F0F);
    let mut inputs = Vec::new();
    // Red casts first, so the global counter is well past its base before Blue
    // is shown anything at all.
    for seat in [Seat::Red0, Seat::Red1, Seat::Red2] {
        inputs.push(Input {
            tick: Tick(0),
            seq: 0,
            player: seat,
            action: Action::Skillshot(FxVec2::new(Fx::NEG_ONE, Fx::ZERO)),
        });
    }
    inputs.push(Input {
        tick: Tick(0),
        seq: 0,
        player: Seat::Blue0,
        action: Action::Skillshot(FxVec2::new(Fx::ONE, Fx::ZERO)),
    });
    step(&state, &inputs)
}

fn projectile_ids(view: &PlayerView) -> Vec<u16> {
    view.visible
        .iter()
        .filter_map(|entity| match entity {
            EntityView::Projectile { id, .. } => Some(id.0),
            _ => None,
        })
        .collect()
}

/// The channel this whole module exists to close.
///
/// Blue is shown one projectile. Its *global* handle is 1003, because three
/// skillshots it never saw were allocated first, and a client that knows the
/// rules reads three hidden casts straight off that number. In Blue's own
/// handle space it is 1000.
#[test]
fn a_gap_in_the_global_counter_is_not_observable() {
    let state = state_with_casts_out_of_sight();
    let global = view_for(&state, Seat::Blue0);
    assert_eq!(
        projectile_ids(&global),
        vec![PROJECTILE_ID_BASE + 3],
        "the global handle is supposed to carry the gap; if it does not, this \
         test is no longer about anything"
    );

    let mut space = HandleSpace::new();
    let local = space.localize(&global, state.projectiles());
    assert_eq!(projectile_ids(&local), vec![PROJECTILE_ID_BASE]);
    assert_eq!(space.issued(), 1);
}

/// Public handles pass through unchanged: a champion's handle is its seat, and
/// renaming it would cost the client the only identity it has.
#[test]
fn champion_and_tower_handles_are_not_renamed() {
    let mut state = new_state(3);
    for _ in 0..30 {
        state = step(&state, &[]);
    }
    let view = view_for(&state, Seat::Blue0);
    let mut space = HandleSpace::new();
    let local = space.localize(&view, state.projectiles());

    let expected: Vec<u16> = view
        .visible
        .iter()
        .filter_map(|entity| match entity {
            EntityView::Champion { id, .. } | EntityView::Tower { id, .. } => Some(id.0),
            EntityView::Projectile { .. } => None,
        })
        .collect();
    let seen: Vec<u16> = local
        .visible
        .iter()
        .filter_map(|entity| match entity {
            EntityView::Champion { id, .. } | EntityView::Tower { id, .. } => Some(id.0),
            EntityView::Projectile { .. } => None,
        })
        .collect();
    assert_eq!(seen, expected);
    assert!(expected.contains(&champion_entity_id(Seat::Blue1).0));
    assert!(expected.contains(&tower_entity_id(0).0));
    assert_eq!(space.issued(), 0, "no projectile was in sight");
}

/// A local handle is issued once and never again, across the whole match.
///
/// This is the property that rules out the rejected design — a map with a free
/// list — and it is the one that has to hold over a long run rather than over
/// one tick, because recycling only becomes observable after something expires.
#[test]
fn a_local_handle_is_never_issued_twice() {
    let mut state = new_state(0xB0_0B);
    let mut space = HandleSpace::new();
    let mut ever_issued: Vec<u16> = Vec::new();
    let mut alive_last_tick: Vec<u16> = Vec::new();

    for tick in 0..600u32 {
        // Everybody casts whenever the cooldown allows, so the arena fills,
        // drains and fills again — which is exactly when a free list would
        // start handing a number back out.
        let inputs: Vec<Input> = if tick % 60 == 0 {
            Seat::ALL
                .iter()
                .map(|seat| Input {
                    tick: Tick(tick),
                    seq: tick,
                    player: *seat,
                    action: Action::Skillshot(FxVec2::new(
                        if seat.team() == sim::Team::Blue {
                            Fx::ONE
                        } else {
                            Fx::NEG_ONE
                        },
                        Fx::ZERO,
                    )),
                })
                .collect()
        } else {
            Vec::new()
        };
        state = step(&state, &inputs);

        let local = space.localize(&view_for(&state, Seat::Blue0), state.projectiles());
        let now = projectile_ids(&local);
        for id in &now {
            if !alive_last_tick.contains(id) && ever_issued.contains(id) {
                panic!("local handle {id} came back after leaving sight, at tick {tick}");
            }
            if !ever_issued.contains(id) {
                ever_issued.push(*id);
            }
        }
        alive_last_tick = now;
    }

    assert!(
        ever_issued.len() >= 8,
        "only {} handles were ever issued, so the run never exercised reuse",
        ever_issued.len()
    );
    // Dense from the base, with no gaps: the count is the only thing readable.
    let mut sorted = ever_issued.clone();
    sorted.sort_unstable();
    for (offset, id) in sorted.iter().enumerate() {
        assert_eq!(*id, PROJECTILE_ID_BASE + offset as u16);
    }
}

/// Two recipients that were shown the same projectiles name them the same way.
///
/// This is what M3's exit criterion rests on — three clients on one team
/// comparing digests of the worlds they reconciled — and it is not free: it
/// holds because vision is a team property and because the entity list is
/// ordered by handle, so the two allocate in the same order. A recipient that
/// joined later would legitimately disagree.
#[test]
fn two_recipients_with_the_same_history_agree_on_their_handles() {
    let mut state = new_state(0xFACE);
    let mut spaces = [HandleSpace::new(), HandleSpace::new(), HandleSpace::new()];
    let team = [Seat::Blue0, Seat::Blue1, Seat::Blue2];

    // Every 240 ticks, because that is the skillshot's cooldown: asking more
    // often would not produce more projectiles, it would produce a test that
    // looks busier than it is.
    for tick in 0..800u32 {
        let inputs: Vec<Input> = if tick % 240 == 0 {
            vec![Input {
                tick: Tick(tick),
                seq: tick,
                player: Seat::Blue1,
                action: Action::Skillshot(FxVec2::new(Fx::ONE, Fx::ZERO)),
            }]
        } else {
            Vec::new()
        };
        state = step(&state, &inputs);

        let localized: Vec<Vec<u16>> = team
            .iter()
            .zip(spaces.iter_mut())
            .map(|(seat, space)| {
                projectile_ids(&space.localize(&view_for(&state, *seat), state.projectiles()))
            })
            .collect();
        assert_eq!(localized[0], localized[1], "tick {tick}");
        assert_eq!(localized[1], localized[2], "tick {tick}");
    }
    assert!(
        spaces[0].issued() >= 3,
        "the run showed nobody a projectile"
    );
}

// ---------------------------------------------------------------------------
// Totality
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(config())]

    /// No byte string is a panic.
    ///
    /// The decoder is the first code an attacker reaches, so "it returns an
    /// error on bad input" is not the claim — the claim is that there is no
    /// input for which it does something other than return. Drawn as arbitrary
    /// bytes of arbitrary length, and separately as a *mutated valid frame*,
    /// because uniform random bytes almost never get past the version check and
    /// would otherwise leave the body unexercised.
    #[test]
    fn no_byte_string_can_make_a_decoder_panic(bytes in prop::collection::vec(any::<u8>(), 0..64)) {
        let _ = ClientFrame::decode(&bytes);
        let _ = ServerFrame::decode(&bytes);
    }

    #[test]
    fn a_mutated_client_frame_decodes_or_errors(
        message_index in 0usize..8,
        index in 0usize..CLIENT_FRAME_BYTES,
        value in any::<u8>(),
    ) {
        let messages = client_messages();
        let mut bytes = *ClientFrame::encode(&messages[message_index]).as_bytes();
        bytes[index] = value;
        // Either it is a message or it is an error; there is no third outcome
        // and in particular no panic.
        let _ = ClientFrame::decode(&bytes);
    }

    #[test]
    fn a_mutated_server_frame_decodes_or_errors(
        index in 0usize..SERVER_FRAME_BYTES,
        value in any::<u8>(),
    ) {
        let view = ServerMessage::View(view_for(&busy_state(), Seat::Blue0));
        let mut bytes = *ServerFrame::encode(&view).as_bytes();
        bytes[index] = value;
        let _ = ServerFrame::decode(&bytes);
    }
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A state with something in every part of a view: entities in sight, a
/// projectile in flight, and an event from the tick that produced it.
fn busy_state() -> State {
    let state = new_state(0xB005);
    let mut state = step(
        &state,
        &[Input {
            tick: Tick(0),
            seq: 0,
            player: Seat::Blue0,
            action: Action::Skillshot(FxVec2::new(Fx::ONE, Fx::ZERO)),
        }],
    );
    for _ in 0..2 {
        state = step(&state, &[]);
    }
    state
}

/// The worst case the *type* allows: every entity slot and every event slot
/// full. Not reachable in a match, which is the point — the bucket is sized for
/// what the encoding can produce, not for what a run happened to produce.
fn widest_view() -> PlayerView {
    let mut visible = Vec::new();
    for seat in [Seat::Blue1, Seat::Blue2, Seat::Red0, Seat::Red1, Seat::Red2] {
        visible.push(EntityView::Champion {
            id: champion_entity_id(seat),
            position: FxVec2::ZERO,
            hp: Fx::ONE,
        });
    }
    for index in 0..sim::TOWER_COUNT {
        visible.push(EntityView::Tower {
            id: tower_entity_id(index),
            position: FxVec2::ZERO,
            hp: Fx::ONE,
        });
    }
    for slot in 0..sim::MAX_PROJECTILES {
        visible.push(EntityView::Projectile {
            id: EntityId(PROJECTILE_ID_BASE + slot as u16),
            position: FxVec2::ZERO,
            velocity: FxVec2::ZERO,
        });
    }
    PlayerView {
        tick: Tick(u32::MAX),
        outcome: sim::Outcome::Decided {
            winner: sim::Team::Red,
            at: Tick(1),
        },
        own: sim::view::OwnView {
            id: champion_entity_id(Seat::Blue0),
            position: FxVec2::ZERO,
            liveness: sim::Liveness::Alive { hp: Fx::ONE },
            cooldowns: sim::Cooldowns::default(),
        },
        visible,
        events: vec![
            sim::view::VisibleEvent::Damage {
                target: EntityId(0),
                amount: Fx::ONE,
                at: FxVec2::ZERO,
            };
            sim::MAX_EVENTS
        ],
    }
}
