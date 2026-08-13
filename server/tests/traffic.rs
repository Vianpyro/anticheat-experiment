//! The traffic-shape invariant, stated as properties.
//!
//! `docs/ARCHITECTURE.md`: **one message per player per tick, at a constant
//! cadence, of a constant encoded size, independent of content.** Culling is
//! worth nothing without it. An observer who cannot decrypt a single byte still
//! counts packets and measures their length, and without this either number is
//! close to a linear readout of how many entities a player can see — which is
//! exactly the thing `sim::view` exists to withhold.
//!
//! # What each half is carried by, and what that means for these tests
//!
//! The **size** half is carried by `protocol`'s types: a `ServerFrame` wraps a
//! fixed array, so no encoder can return a shorter one and no bucketing scheme
//! compiles. The tests below still measure it, and it is worth being honest that
//! those measurements cannot fail — they are here because a reader of this file
//! should be able to see the claim being checked, not because they have teeth.
//!
//! The halves with teeth are the two a type cannot state:
//!
//! - **Cadence.** `Match::tick` returns exactly one frame per occupied seat,
//!   every tick, whatever happened. A server that sent a view only when
//!   something changed, or an extra message when an event fired, would satisfy
//!   every size assertion and leak through message counts anyway.
//! - **The bytes are a function of entitlement.** Two states a player cannot
//!   tell apart produce byte-identical frames for that player. This is the
//!   transport's version of property 3 in `sim/tests/view_properties.rs`, and it
//!   is the one that covers everything between the projection and the socket:
//!   the padding, the framing, and the per-recipient handle space.
//!
//! # The specification is the same one M2 uses
//!
//! `entitled` comes from `sim/tests/spec/mod.rs`, included by path rather than
//! copied. That module is emphatic that there must be exactly one re-derivation
//! of the visibility rule — two that had drifted apart would each be green
//! against its own private idea of it — and a copy in this crate would be the
//! second. It compiles here because it uses nothing but `sim`'s public API.

#![deny(unsafe_code)]

use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::{FileFailurePersistence, TestRunner};

use protocol::{
    ClientFrame, ClientMessage, MAX_DATAGRAM_BYTES, SERVER_DATAGRAM_BYTES, SERVER_FRAME_BYTES,
    SERVER_SHARDS, ServerFrame, ServerMessage,
};
use server::{Match, MatchConfig};
use sim::{
    Action, EntityId, Fx, FxVec2, MAX_PROJECTILES, PLAYER_COUNT, PROJECTILE_ID_BASE, RULES, Seat,
    TOWER_COUNT,
};

#[path = "../../sim/tests/spec/mod.rs"]
mod spec;

use spec::{Entitled, entitled};

const REGRESSIONS: &str = "proptest-regressions/traffic.txt";

fn config() -> ProptestConfig {
    let default = ProptestConfig::default();
    ProptestConfig {
        max_global_rejects: default.cases.saturating_mul(4),
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(REGRESSIONS))),
        ..default
    }
}

// ---------------------------------------------------------------------------
// Driving a match
// ---------------------------------------------------------------------------

/// One tick's worth of what the nine clients ask for.
///
/// A batch rather than a per-seat action, because the thing under test is what
/// the *server* does with a tick's traffic, and a seat that says nothing this
/// tick is as much a case as one that says something.
#[derive(Clone, Debug)]
struct Batch(Vec<(usize, Action)>);

/// A match, expressed as the way to drive it.
#[derive(Clone, Debug)]
struct Script {
    seed: u64,
    batches: Vec<Batch>,
}

/// Six sessions, joined and ready, on a match that has not ticked yet.
fn seated(seed: u64) -> Match {
    let mut game = Match::new(MatchConfig {
        seed,
        players: PLAYER_COUNT,
    });
    for _ in 0..PLAYER_COUNT {
        let (seat, _) = game.join();
        assert!(seat.is_some(), "the match refused a seat it had");
    }
    for seat in Seat::ALL {
        game.deliver(
            seat,
            ClientFrame::encode(&ClientMessage::Ready).as_bytes(),
            0,
        )
        .expect("ready was refused");
    }
    game
}

/// Delivers one batch and runs one tick, returning the frames.
///
/// `seq` is carried by the caller because the server accepts only strictly
/// increasing sequence numbers per seat — which is itself part of what is under
/// test, so a driver that reset it would be hiding a rejection as a quiet
/// no-op.
fn advance(
    game: &mut Match,
    batch: &Batch,
    seq: &mut [u32; PLAYER_COUNT],
) -> Vec<(Seat, ServerFrame)> {
    for (index, action) in &batch.0 {
        let seat = Seat::ALL[*index];
        let frame = ClientFrame::encode(&ClientMessage::Input {
            seq: seq[*index],
            claimed_at_ms: 0,
            action: *action,
        });
        // A refusal is a legitimate outcome — the rate limit is real — so it is
        // ignored rather than asserted on. What matters here is the frames the
        // server emits, and it has to emit them either way.
        let _ = game.deliver(seat, frame.as_bytes(), 0);
        seq[*index] = seq[*index].saturating_add(1);
    }
    game.tick()
}

/// The frame a seat received this tick, if it received one.
fn frame_for(frames: &[(Seat, ServerFrame)], seat: Seat) -> Option<&ServerFrame> {
    frames
        .iter()
        .find(|(who, _)| *who == seat)
        .map(|(_, frame)| frame)
}

/// The projectile handles a frame names, in the order it names them.
fn projectiles_in(frame: &ServerFrame) -> Vec<u16> {
    match ServerFrame::decode(frame.as_bytes()) {
        Ok(ServerMessage::View(view)) => view
            .visible
            .iter()
            .filter_map(|entity| match entity {
                sim::view::EntityView::Projectile { id, .. } => Some(id.0),
                _ => None,
            })
            .collect(),
        other => panic!("a tick produced {other:?} rather than a view"),
    }
}

/// How many entities a seat was told about in a frame.
fn visible_count(frame: &ServerFrame) -> usize {
    match ServerFrame::decode(frame.as_bytes()) {
        Ok(ServerMessage::View(view)) => view.visible.len(),
        other => panic!("a tick produced {other:?} rather than a view"),
    }
}

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

const DOMAIN_RAW: i32 = 128 * 65536;

fn coordinate() -> impl Strategy<Value = Fx> {
    prop_oneof![
        7 => (-DOMAIN_RAW..=DOMAIN_RAW).prop_map(Fx::from_raw),
        1 => proptest::num::i32::ANY.prop_map(Fx::from_raw),
    ]
}

fn point() -> impl Strategy<Value = FxVec2> {
    (coordinate(), coordinate()).prop_map(|(x, y)| FxVec2::new(x, y))
}

fn handle() -> impl Strategy<Value = EntityId> {
    prop_oneof![
        4 => (0u16..PLAYER_COUNT as u16).prop_map(EntityId),
        3 => (10u16..10 + TOWER_COUNT as u16).prop_map(EntityId),
        2 => (1000u16..1000 + MAX_PROJECTILES as u16).prop_map(EntityId),
        2 => proptest::num::u16::ANY.prop_map(EntityId),
    ]
}

fn action() -> impl Strategy<Value = Action> {
    prop_oneof![
        2 => Just(Action::Idle),
        6 => point().prop_map(Action::Move),
        5 => point().prop_map(Action::Skillshot),
        4 => handle().prop_map(Action::Attack),
        3 => handle().prop_map(Action::Targeted),
    ]
}

/// A batch: each seat independently either says something or does not.
///
/// Constructed rather than filtered, for the reason `sim/tests/properties.rs`
/// records: a `prop_assume!` that discards half the draws turns a raised case
/// budget into an aborted test rather than a longer one.
fn batch() -> impl Strategy<Value = Batch> {
    proptest::collection::vec(proptest::option::weighted(0.4, action()), PLAYER_COUNT).prop_map(
        |per_seat| {
            Batch(
                per_seat
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, action)| action.map(|action| (index, action)))
                    .collect(),
            )
        },
    )
}

fn script(ticks: usize) -> impl Strategy<Value = Script> {
    (
        proptest::num::u64::ANY,
        proptest::collection::vec(batch(), 1..=ticks),
    )
        .prop_map(|(seed, batches)| Script { seed, batches })
}

// ---------------------------------------------------------------------------
// The properties
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(config())]

    /// **Cadence.** Exactly one frame per occupied seat, every tick, whatever
    /// happened.
    ///
    /// The half a type cannot carry, and the half padding does nothing about. A
    /// server that emitted a view only when something changed would pass every
    /// size assertion in this file and still report the number of visible
    /// entities through the count of packets.
    #[test]
    fn every_tick_produces_one_frame_per_seat_and_no_other(script in script(40)) {
        let mut game = seated(script.seed);
        let mut seq = [0u32; PLAYER_COUNT];
        let mut received = [0u32; PLAYER_COUNT];

        for (index, batch) in script.batches.iter().enumerate() {
            let frames = advance(&mut game, batch, &mut seq);
            prop_assert_eq!(
                frames.len(),
                PLAYER_COUNT,
                "tick {}: {} frames for {} seats",
                index,
                frames.len(),
                PLAYER_COUNT
            );
            // One each, in seat order, and not two for somebody and none for
            // somebody else.
            for (position, (seat, _)) in frames.iter().enumerate() {
                prop_assert_eq!(*seat, Seat::ALL[position]);
                received[seat.index()] += 1;
            }
        }

        for seat in Seat::ALL {
            prop_assert_eq!(
                received[seat.index()],
                script.batches.len() as u32,
                "{:?} received a number of frames that is not the tick count",
                seat
            );
        }
    }

    /// **Size.** Every frame is one bucket wide, it is cut into the same
    /// datagrams however much it carries, and neither follows the number of
    /// entities in it.
    ///
    /// The first two assertions cannot fail — `ServerFrame` is a fixed array and
    /// `shards` returns a fixed array of fixed arrays — and are here so the claim
    /// is visible where it is made. The one that carries something is the last:
    /// the run has to *reach* views of different content, or "the size did not
    /// vary" would be true of a match in which nothing varied either.
    #[test]
    fn a_frame_is_the_same_size_and_the_same_datagrams_whatever_it_carries(script in script(40)) {
        let mut game = seated(script.seed);
        let mut seq = [0u32; PLAYER_COUNT];
        let mut counts = std::collections::BTreeSet::new();

        for (number, batch) in script.batches.iter().enumerate() {
            for (_, frame) in advance(&mut game, batch, &mut seq) {
                prop_assert_eq!(frame.as_bytes().len(), SERVER_FRAME_BYTES);
                let shards = frame.shards(number as u32);
                prop_assert_eq!(shards.len(), SERVER_SHARDS);
                for shard in &shards {
                    prop_assert_eq!(shard.as_bytes().len(), SERVER_DATAGRAM_BYTES);
                    prop_assert!(SERVER_DATAGRAM_BYTES <= MAX_DATAGRAM_BYTES);
                }
                counts.insert(visible_count(&frame));
            }
        }
        prop_assert!(!counts.is_empty());
    }

    /// **The bytes are a function of what the recipient is entitled to know.**
    ///
    /// Two matches are driven identically and then forked with different
    /// batches. For a seat whose entitlement — as the specification derives it,
    /// not as `view_for` reports it — is the same in both, the frame it receives
    /// has to be the same bytes.
    ///
    /// This is the transport version of property 3 in
    /// `sim/tests/view_properties.rs`, and it covers everything that property
    /// could not: the padding, the framing, and the per-recipient handle space.
    /// A padding scheme that derived a byte from the content, a frame that
    /// carried a length, or a handle allocator that counted projectiles the
    /// recipient never saw all fail here and pass everything else in this file.
    #[test]
    fn two_states_a_player_cannot_tell_apart_produce_the_same_bytes(
        (prefix, left_batch, right_batch, tail) in
            (script(24), batch(), batch(), proptest::collection::vec(batch(), 0..=8)),
    ) {
        let mut left = seated(prefix.seed);
        let mut right = seated(prefix.seed);
        let mut left_seq = [0u32; PLAYER_COUNT];
        let mut right_seq = [0u32; PLAYER_COUNT];

        for batch in &prefix.batches {
            let a = advance(&mut left, batch, &mut left_seq);
            let b = advance(&mut right, batch, &mut right_seq);
            // The shared prefix has to be shared, or the fork below compares
            // two matches that had already diverged for another reason.
            prop_assert!(a.iter().zip(&b).all(|((_, x), (_, y))| x == y));
        }

        let mut left_frames = advance(&mut left, &left_batch, &mut left_seq);
        let mut right_frames = advance(&mut right, &right_batch, &mut right_seq);

        // The fork tick, and then several more driven identically. The tail is
        // not padding: some of what the fork hid is carried in state that only
        // reaches a frame later — a per-recipient handle counter advanced by a
        // cast nobody saw shows up on the tick a *visible* projectile is next
        // named, not on the tick of the cast. A property that looked only at
        // the fork tick would be blind to exactly the leak the handle space
        // exists to close.
        let mut step = 0usize;
        loop {
            for seat in Seat::ALL {
                let left_entitled: Entitled = entitled(left.world(), seat, &RULES);
                let right_entitled: Entitled = entitled(right.world(), seat, &RULES);
                if left_entitled != right_entitled {
                    continue;
                }
                // Looked up by seat rather than by position: this property must
                // report "different bytes", and it must not turn into an index
                // panic if the cadence half has been broken as well.
                let a = frame_for(&left_frames, seat);
                let b = frame_for(&right_frames, seat);
                prop_assert!(
                    a.is_some() && b.is_some(),
                    "{:?} was sent no frame at all, {} ticks after the fork",
                    seat,
                    step
                );
                prop_assert!(
                    a == b,
                    "{:?} was sent different bytes {} ticks after a fork it cannot \
                     tell apart",
                    seat,
                    step
                );
            }
            let Some(batch) = tail.get(step) else { break };
            left_frames = advance(&mut left, batch, &mut left_seq);
            right_frames = advance(&mut right, batch, &mut right_seq);
            step = step.saturating_add(1);
        }
    }
}

// ---------------------------------------------------------------------------
// The invariant as a measurement, and the guard that keeps it non-vacuous
// ---------------------------------------------------------------------------

/// The invariant said the way `docs/ARCHITECTURE.md` says it: nothing about the
/// traffic varies with the number of visible entities.
///
/// A plain test rather than a property, because the assertion is a *correlation*
/// over a whole run and the interesting part is the coverage: the run has to
/// reach ticks where a player sees nothing and ticks where it sees several, or
/// "the size did not depend on the count" is a statement about a constant.
/// Those floors are the reason this is not just the property above written
/// twice.
#[test]
fn nothing_about_the_traffic_follows_the_number_of_visible_entities() {
    let mut game = seated(0x7AFF_1C00_0000_0001);
    let mut seq = [0u32; PLAYER_COUNT];
    let mut sizes = std::collections::BTreeSet::new();
    let mut datagrams_per_tick = std::collections::BTreeSet::new();
    let mut datagram_sizes = std::collections::BTreeSet::new();
    let mut frames_per_tick = std::collections::BTreeSet::new();
    let mut counts = std::collections::BTreeSet::new();
    let mut bytes_to_blue0: u64 = 0;
    let mut ticks: u64 = 0;

    for tick in 0..400u32 {
        // All nine walk at the middle of the triangle and cast on cooldown, so
        // the run passes through a view with nothing but one's own team in it,
        // a view with two other teams in it, and the crossing in between. The
        // middle rather than "the enemy base", because on a three-team map
        // there is no such place — which is exactly the sort of sentence that
        // used to be written as `x = 120` and now has to be a position.
        let mut batch = Vec::new();
        if tick % 90 == 0 {
            for (index, _) in Seat::ALL.iter().enumerate() {
                batch.push((index, Action::Move(FxVec2::ZERO)));
            }
        }
        if tick % 240 == 30 {
            for (index, seat) in Seat::ALL.iter().enumerate() {
                // Aimed from its own base at the origin, so every skillshot
                // flies into the middle whichever vertex it started from.
                let home = sim::base_position(seat.team(), &RULES);
                batch.push((index, Action::Skillshot(home.neg())));
            }
        }

        let frames = advance(&mut game, &Batch(batch), &mut seq);
        frames_per_tick.insert(frames.len());
        ticks += 1;
        let mut datagrams_this_tick = 0usize;
        for (seat, frame) in &frames {
            sizes.insert(frame.as_bytes().len());
            counts.insert(visible_count(frame));
            for shard in frame.shards(tick) {
                datagrams_this_tick += 1;
                datagram_sizes.insert(shard.as_bytes().len());
            }
            if *seat == Seat::Blue0 {
                bytes_to_blue0 += frame.as_bytes().len() as u64;
            }
        }
        datagrams_per_tick.insert(datagrams_this_tick);
    }

    // The coverage the assertions below are conditional on.
    assert!(
        counts.len() >= 3,
        "the run only ever produced {} distinct visible counts, so 'the size does \
         not follow the count' is a claim about a constant",
        counts.len()
    );
    // Not "some tick showed a player nothing", which this game cannot produce:
    // vision is a team property and a living ally stands inside its own radius,
    // so a seat with a teammate alive always sees at least that teammate. The
    // variation that matters is what is on the far side of the fog, and what
    // this asks for is that the run crossed it — the busiest tick shows a
    // player several more entities than the quietest.
    let (fewest, most) = (
        *counts.iter().next().expect("no views at all"),
        *counts.iter().next_back().expect("no views at all"),
    );
    assert!(
        most.saturating_sub(fewest) >= 2,
        "the visible count only ever ranged over {counts:?}, which is too flat \
         for 'the traffic does not follow it' to be a claim about anything"
    );

    // And the invariant itself.
    assert_eq!(
        sizes.len(),
        1,
        "frames came in {} different sizes while the visible count took {} values",
        sizes.len(),
        counts.len()
    );
    assert_eq!(
        frames_per_tick,
        std::collections::BTreeSet::from([PLAYER_COUNT])
    );
    assert_eq!(bytes_to_blue0, ticks * SERVER_FRAME_BYTES as u64);

    // And the same statement one layer down, where the observer actually is.
    // A frame is not one write any more: it is `SERVER_SHARDS` datagrams of
    // `SERVER_DATAGRAM_BYTES`, and what an observer counts is those.
    assert_eq!(
        datagram_sizes,
        std::collections::BTreeSet::from([SERVER_DATAGRAM_BYTES]),
        "datagrams came in {} different sizes",
        datagram_sizes.len()
    );
    assert_eq!(
        datagrams_per_tick,
        std::collections::BTreeSet::from([PLAYER_COUNT * SERVER_SHARDS])
    );
}

/// The projectile-handle channel, closed at the frame the client actually
/// receives.
///
/// # Why this is a scripted scenario and not a property, and what changed
///
/// It was written because the property above could not catch this, and that was
/// established by breaking it rather than by reasoning about it: `localize` was
/// mutated to name every projectile in the arena instead of only the ones the
/// recipient was shown — the exact leak the handle space exists to close — and
/// at two teams `two_states_a_player_cannot_tell_apart_produce_the_same_bytes`
/// stayed green at 4096 cases with an eight-tick tail.
///
/// The reason was structural rather than a budget problem, and it was an
/// anti-correlation: that property's antecedent is *full* entitlement equality,
/// and on a two-team map a fork the recipient could not tell apart was almost
/// always a fork in which nothing had happened anywhere near anybody. The forks
/// that stayed indistinguishable were exactly the ones with no hidden activity
/// to leak.
///
/// **Three teams filled that hole rather than deepening it.** A whole enemy
/// team can now act at a vertex a hundred and seventy-three units away while
/// the observer's entitlement is untouched, so "hidden activity" and "equal
/// entitlement" stopped being opposites. Re-run at nine seats, the same
/// mutation fails the property on its **first case**, one tick after the fork:
/// `Blue0 was sent different bytes 1 ticks after a fork it cannot tell apart`.
///
/// This scenario stays anyway, and not out of sentiment. A property that
/// happens to reach a channel is evidence about this generator on this map; a
/// scripted state built to expose the channel — three casts out of sight before
/// one in sight — is evidence about the channel. It is the transport-level twin
/// of `protocol/tests/wire.rs::a_gap_in_the_global_counter_is_not_observable`,
/// and the two of them together are what makes the closure checkable rather
/// than sampled.
#[test]
fn a_frame_never_carries_the_count_of_casts_the_recipient_did_not_see() {
    let mut game = seated(0x0CA5_7000_0000_0001);
    let mut seq = [0u32; PLAYER_COUNT];

    // Tick 0: the Red seats cast, a lane away from anything Blue can see.
    // Drained in seat order, so these take the first three global handles.
    let red_casts = Batch(
        [3usize, 4, 5]
            .into_iter()
            .map(|index| (index, Action::Skillshot(FxVec2::new(Fx::NEG_ONE, Fx::ZERO))))
            .collect(),
    );
    let frames = advance(&mut game, &red_casts, &mut seq);
    assert!(
        projectiles_in(frame_for(&frames, Seat::Blue0).expect("no frame")).is_empty(),
        "Blue was shown a projectile it should not be able to see"
    );

    // Tick 1: Blue casts one, in its own vision. Its global handle is 1003.
    let blue_cast = Batch(vec![(
        0usize,
        Action::Skillshot(FxVec2::new(Fx::ONE, Fx::ZERO)),
    )]);
    let frames = advance(&mut game, &blue_cast, &mut seq);

    let projectiles = projectiles_in(frame_for(&frames, Seat::Blue0).expect("no frame"));

    // The state really does hold four projectiles with a gap before Blue's, or
    // this test is asserting something about a world that does not have the
    // shape it is supposed to have.
    assert_eq!(game.world().projectiles().count(), 4);
    assert!(
        game.world()
            .projectiles()
            .iter()
            .any(|projectile| projectile.id.0 == PROJECTILE_ID_BASE + 3),
        "the global counter did not reach the value this test is about"
    );

    assert_eq!(
        projectiles,
        vec![PROJECTILE_ID_BASE],
        "Blue0's frame names its first projectile {projectiles:?}; anything but \
         {PROJECTILE_ID_BASE} counts the casts it was not shown"
    );
}

/// The antecedent of the byte-equality property is reached often enough for it
/// to be about something.
///
/// The same argument, and the same shape of assertion, as the coverage counters
/// at the end of `sim/tests/view_properties.rs`: a property conditional on
/// "these two states look the same to this player" is satisfied by a generator
/// that never produces two such states, and a property that is never exercised
/// looks exactly like a property that holds.
#[test]
fn the_generators_reach_forks_a_player_cannot_tell_apart() {
    const SAMPLES: usize = 128;

    let mut runner = TestRunner::deterministic();
    let prefixes = script(24);
    let batches = batch();
    let mut hidden_forks = 0u32;
    let mut differing_states = 0u32;

    for _ in 0..SAMPLES {
        let prefix = prefixes
            .new_tree(&mut runner)
            .expect("the strategy produced no value")
            .current();
        let left_batch = batches
            .new_tree(&mut runner)
            .expect("the strategy produced no value")
            .current();
        let right_batch = batches
            .new_tree(&mut runner)
            .expect("the strategy produced no value")
            .current();

        let mut left = seated(prefix.seed);
        let mut right = seated(prefix.seed);
        let mut left_seq = [0u32; PLAYER_COUNT];
        let mut right_seq = [0u32; PLAYER_COUNT];
        for batch in &prefix.batches {
            let _ = advance(&mut left, batch, &mut left_seq);
            let _ = advance(&mut right, batch, &mut right_seq);
        }
        let _ = advance(&mut left, &left_batch, &mut left_seq);
        let _ = advance(&mut right, &right_batch, &mut right_seq);

        if left.digest() == right.digest() {
            continue;
        }
        differing_states += 1;
        if Seat::ALL.into_iter().any(|seat| {
            entitled(left.world(), seat, &RULES) == entitled(right.world(), seat, &RULES)
        }) {
            hidden_forks += 1;
        }
    }

    println!("forks: {differing_states} differing states, {hidden_forks} hidden from somebody");
    assert!(
        differing_states >= 40,
        "only {differing_states} of {SAMPLES} forks produced different states"
    );
    assert!(
        hidden_forks >= 20,
        "only {hidden_forks} of {SAMPLES} forks were hidden from anybody, so the \
         byte-equality property is drifting toward vacuity"
    );
}
