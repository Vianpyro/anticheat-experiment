//! What a recording records: what the rules **produced**, never what a client
//! was **delivered**.
//!
//! # The question, and why it costs thirty seconds now
//!
//! The frame's event budget is smaller than a tick's capacity, so
//! `protocol::EventBacklog` defers the overflow: an event the rules produced on
//! tick *T* can reach a client in the frame for tick *T + 1*. Two sequences
//! therefore exist and they are not the same sequence.
//!
//! If a replay recorded the *delivered* one, M5's resimulation would compare a
//! log written in delivery order against a `sim` that produces events in rule
//! order, and the two would be offset by exactly one tick on every busy tick of
//! every match. The symptom would be a digest mismatch that says "these bytes do
//! not describe that match" about a recording that is perfectly faithful, in a
//! milestone whose whole subject is telling tampering from honest disagreement.
//!
//! # The decision, and why it is structural rather than a rule somebody follows
//!
//! **Produced.** And the recording does not carry events at all: it carries the
//! seed and the input log, and resimulation *derives* the events by running the
//! same `step` the server ran. There is no event field for delivery order to get
//! into. `Match::recording` is built from the seed and the log; the backlog
//! lives in a `Session`, downstream of it, and is fed from an already-culled
//! `PlayerView` — it never sees a `State` and nothing it does can reach the
//! recording.
//!
//! That is the strongest available answer and it is worth naming why it is
//! stronger than a convention: there is no code path that could record delivery
//! order, so there is nothing to review for and nothing to regress. What this
//! file adds is the demonstration that the two sequences really do differ — that
//! the decision is about a live case and not a hypothetical one — and that
//! resimulation reproduces the produced one on the tick that produced it.

#![deny(unsafe_code)]

use protocol::{ClientFrame, ClientMessage, ServerFrame, ServerMessage};
use server::{Match, MatchConfig};
use sim::view::MAX_EVENTS_PER_VIEW;
use sim::{Action, FxVec2, Input, PLAYER_COUNT, Seat, State, champion_entity_id, new_state, step};

/// Nine seats, joined and ready.
///
/// `expected` is how many the match waits for before its first tick, which is
/// not always nine: the second test frees six of the seats before play starts,
/// and a match still waiting for them would never run.
fn seated(seed: u64, expected: usize) -> Match {
    let mut game = Match::new(MatchConfig {
        seed,
        players: expected,
    });
    for _ in 0..PLAYER_COUNT {
        assert!(game.join().0.is_some(), "the match refused a seat it had");
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

fn send(game: &mut Match, seat: Seat, seq: u32, action: Action) {
    game.deliver(
        seat,
        ClientFrame::encode(&ClientMessage::Input {
            seq,
            claimed_at_ms: 0,
            action,
        })
        .as_bytes(),
        0,
    )
    .expect("the server refused a well-sequenced input");
}

/// The events a seat's frame carried this tick.
fn delivered(frames: &[(Seat, ServerFrame)], seat: Seat) -> usize {
    let frame = frames
        .iter()
        .find(|(who, _)| *who == seat)
        .map(|(_, frame)| frame)
        .expect("a seated player received no frame");
    match ServerFrame::decode(frame.as_bytes()) {
        Ok(ServerMessage::View { view, .. }) => view.events.len(),
        other => panic!("a tick produced {other:?} rather than a view"),
    }
}

/// The state a log reaches after `ticks` ticks, re-derived rather than borrowed.
///
/// `replay::resimulate` reports only the final digest, and what this file needs
/// is the state *at* a tick. Bucketing the log by each input's own tick field is
/// the same rule `replay` applies and is re-derived here for the reason
/// `sim/tests/spec` is: a check that called the function it is checking would be
/// a function agreeing with itself.
fn replay_to(recording: &replay::Recording, ticks: u32) -> State {
    let mut buckets: Vec<Vec<Input>> = vec![Vec::new(); ticks as usize];
    for timed in &recording.inputs {
        if let Some(bucket) = buckets.get_mut(timed.input.tick.0 as usize) {
            bucket.push(timed.input);
        }
    }
    let mut state = new_state(recording.seed);
    for bucket in &buckets {
        state = step(&state, bucket);
    }
    state
}

/// The whole thing, on a tick busy enough that the two sequences differ.
///
/// Producing that tick is most of the work and none of it is incidental: the
/// frame budget is sixteen events and a tick has to beat it, which needs nine
/// champions close enough together for one observer to see all of them act. So
/// they walk to the middle of the map first — a few hundred ticks at six units a
/// second — and then every seat casts both of its abilities on one tick, which
/// the input rate limit allows and the cooldowns permit exactly once.
#[test]
fn a_recording_reproduces_the_events_on_the_tick_the_rules_produced_them() {
    let mut game = seated(0x9E6D_0000_0000_0001, PLAYER_COUNT);
    let mut seq = 0u32;

    // Everyone to the origin, which is the centroid of the triangle and inside
    // no tower's range. The standing order carries the walk; the intention is
    // re-sent every tick because `Action::Idle` is a rule that stops the
    // champion rather than a way of saying nothing.
    let middle = Action::Move(FxVec2::ZERO);
    for _ in 0..640u32 {
        for seat in Seat::ALL {
            send(&mut game, seat, seq, middle);
        }
        seq = seq.saturating_add(1);
        let _ = game.tick();
    }

    // Close enough that one observer sees every one of the other eight.
    let observer = Seat::Blue0;
    let sighted = sim::view::view_for(game.world(), observer).visible.len();
    assert!(
        sighted >= PLAYER_COUNT - 1,
        "only {sighted} entities are in {observer:?}'s view, so a busy tick would \
         not be busy for anybody"
    );

    // The busy tick: every seat casts both abilities. Two inputs per seat, which
    // is inside `server::MAX_INPUTS_PER_TICK`, and each cast is permitted
    // because no cooldown has been spent.
    let target = champion_entity_id(Seat::Red0);
    let other_target = champion_entity_id(Seat::Blue0);
    for seat in Seat::ALL {
        send(
            &mut game,
            seat,
            seq,
            Action::Skillshot(FxVec2::new(sim::Fx::ONE, sim::Fx::ZERO)),
        );
        let victim = if seat.team() == Seat::Red0.team() {
            other_target
        } else {
            target
        };
        send(
            &mut game,
            seat,
            seq.saturating_add(1),
            Action::Targeted(victim),
        );
    }
    let busy_tick = game.tick_number().0;
    let frames = game.tick();

    // ---------------------------------------------------------------
    // Produced, and delivered, are two different sequences
    // ---------------------------------------------------------------

    let produced = game.world().events().count();
    assert!(
        produced > MAX_EVENTS_PER_VIEW,
        "the busy tick produced {produced} events, which one frame could have \
         carried, so this test is not about anything"
    );

    let carried = delivered(&frames, observer);
    assert_eq!(
        carried, MAX_EVENTS_PER_VIEW,
        "the frame carried {carried} of a tick that produced {produced}"
    );
    let (deferred, dropped) = game.events_owed(observer);
    assert!(deferred > 0, "the overflow was not deferred");
    assert_eq!(dropped, 0, "the overflow was dropped rather than deferred");

    // …and the rest arrives on the next tick, which is the offset the decision
    // is about: an event this observer is told about at tick `busy_tick + 2` was
    // produced at `busy_tick`.
    let next = game.tick();
    let later = delivered(&next, observer);
    assert!(
        later > 0,
        "nothing was delivered on the following tick, so nothing was deferred"
    );

    // ---------------------------------------------------------------
    // The recording reproduces the produced sequence, on its own tick
    // ---------------------------------------------------------------

    let recording = game.recording();
    let replayed = replay_to(&recording, recording.ticks);
    assert_eq!(
        replayed.digest(),
        recording.final_state_digest,
        "the log does not reproduce the match at all"
    );

    // The state one tick after the busy one is the state that carries the busy
    // tick's events: `step` clears the record at the top of every tick, so the
    // events of the transition into tick `n` live in the state labelled `n`.
    let at_busy = replay_to(&recording, busy_tick.saturating_add(1));
    assert_eq!(
        at_busy.events().count(),
        produced,
        "resimulating the log produced {} events on the tick the server produced \
         {produced}",
        at_busy.events().count()
    );

    // And it is the produced count rather than the delivered one — the number a
    // manifest recording delivery would have had to write down.
    assert_ne!(
        at_busy.events().count(),
        carried,
        "the produced and delivered counts coincide, so this comparison is empty"
    );

    println!(
        "tick {busy_tick}: {produced} events produced, {carried} delivered to \
         {observer:?} on the tick, {later} on the next, {deferred} still owed"
    );
}

/// The recording is the same bytes whatever any client was told.
///
/// The structural half, and the one that would still hold if the test above
/// stopped reaching a busy tick. Two matches are driven with the *same* accepted
/// inputs and told to *different* audiences: in one, six of the nine sessions
/// leave halfway through, so from that point on six seats receive no frames, own
/// no handle space and own no event backlog. The recordings have to be
/// byte-identical, because a recording is a function of the seed and the inputs
/// the authority accepted and of nothing downstream of them.
///
/// The six seats speak *before* they leave, which is the half that gives this
/// teeth: their inputs are in both logs, and a recording that had learned to
/// consult the session table would drop them from one.
#[test]
fn what_a_client_was_told_does_not_reach_the_recording() {
    let mut watched = seated(0x9E6D_0000_0000_0002, PLAYER_COUNT);
    let mut unwatched = seated(0x9E6D_0000_0000_0002, PLAYER_COUNT);

    let middle = Action::Move(FxVec2::ZERO);

    // All nine speak, in both matches.
    for tick in 0..100u32 {
        for seat in Seat::ALL {
            send(&mut watched, seat, tick, middle);
            send(&mut unwatched, seat, tick, middle);
        }
        assert_eq!(watched.tick().len(), PLAYER_COUNT);
        assert_eq!(unwatched.tick().len(), PLAYER_COUNT);
    }

    // Six of them leave one of the two matches. Their champions stay on the map
    // taking no orders, exactly as an unoccupied seat's does, so the *world*
    // does not fork — only the audience does.
    for seat in Seat::ALL.into_iter().skip(3) {
        unwatched
            .deliver(
                seat,
                ClientFrame::encode(&ClientMessage::Surrender).as_bytes(),
                0,
            )
            .expect("surrender was refused");
    }

    // From here only the remaining three speak, in both.
    for tick in 100..200u32 {
        for seat in [Seat::Blue0, Seat::Blue1, Seat::Blue2] {
            send(&mut watched, seat, tick, middle);
            send(&mut unwatched, seat, tick, middle);
        }
        assert_eq!(watched.tick().len(), PLAYER_COUNT);
        assert_eq!(
            unwatched.tick().len(),
            3,
            "the seats that left are still being sent frames"
        );
    }

    assert_eq!(
        watched.digest(),
        unwatched.digest(),
        "the two worlds forked, so comparing their recordings proves nothing"
    );
    assert!(
        watched.recording().inputs.len() > 900,
        "the log is too short to contain the inputs of the seats that left"
    );
    // Compared as sealed bytes rather than as structs, and the difference is
    // the point: a `Recording` has no encoding since M5 — there is one file
    // format and it is signed — so what has to be identical is the artefact that
    // reaches a disk. Ed25519 signing is deterministic, so two seals of one
    // match are the same 64 bytes and this is a byte comparison rather than a
    // field-by-field one that could miss a field.
    let seal = |recording: &replay::Recording| {
        replay::seal(
            recording,
            &replay::SessionFacts {
                match_id: replay::MatchId([7u8; 16]),
                started_at_unix_ms: 1_786_000_000_000,
                participants: [const { None }; PLAYER_COUNT],
                // Pinned, so that the comparison is about the two matches rather
                // than about which commit this was built from.
                sim_commit: replay::SimCommit::Unknown,
                telemetry: replay::Commitment::Absent,
            },
            &replay::SigningKey::from_seed(*b"moba produced-not-delivered key\0"),
        )
        .encode()
    };
    assert_eq!(
        seal(&watched.recording()),
        seal(&unwatched.recording()),
        "the recording followed who was listening"
    );
}
