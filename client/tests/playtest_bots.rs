//! The playtest bot: that it plays a game worth testing, and that the match it
//! played cannot enter the corpus.
//!
//! # The two halves, and why the second is the one with teeth
//!
//! `client::bot` exists so that one or two people can start a nine-seat match
//! before nine of them are free. That is a convenience, and the first test below
//! is the ordinary check that the convenience works: nine bots, driven against
//! `sim` with no server and no clock, walk into each other and fight — damage,
//! deaths, and champions that finish the match somewhere other than their own
//! base. `docs/RISKS.md` R15 is why the counters are asserted rather than
//! printed: a bot that stood at its base for a thousand ticks would satisfy
//! every "it sent an intention" assertion anybody would think to write.
//!
//! The second is the constraint that has teeth. **A match one seat of which was
//! played by a program must never enter the corpus**, and `docs/SCOPE.md` is
//! explicit that what makes a corpus human is supervision rather than a property
//! of a file — so the refusal must be the shape this project gives its other
//! refusals, `docs/CONSENT.md`'s "the check is the only constructor of the value
//! the use needs", and not an `if` somebody has to remember. It is
//! `replay::Attested`: the only value `Corpus::store` accepts, with one
//! constructor, which refuses a match whose **input log** shows a seat playing
//! that no session record accounts for.
//!
//! # What the second test is careful to establish, beyond the refusal
//!
//! That the refusal is not one this corpus already had. `Corpus::store` compares
//! the session record against the manifest's participant list, and both of those
//! are written by the *operator*: an operator who files a playtest naming only
//! the seat a person sat in produces two files that agree perfectly, no silent
//! seat, and — before `Attested` — a stored match whose circumstances were eight
//! bots. So the test asserts that agreement holds and that the match is refused
//! anyway, which is the difference between a check over what somebody typed and
//! a check over what the authority observed.
//!
//! And the control, without which the refusal proves nothing (`docs/RISKS.md`
//! R15 again): the *same* pipeline, over the same transport, for a match with no
//! bot in it, and the corpus takes it.

#![deny(unsafe_code)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use client::Headless;
use client::bot::Bot;
use client::health::{Cadence, Declared, SessionPart};
use client::input::{Control, InputTrace};
use client::net::Wire;
use client::play::Play;
use replay::attest::Attested;
use replay::consent::{ConsentVersion, Permissions};
use replay::corpus::{ConsentRecord, Corpus};
use replay::manifest::{MatchId, Pseudonym, SessionFacts, SimCommit};
use replay::session::{SessionRecord, Supervision};
use replay::{Recording, Replay, SigningKey};
use server::{MatchConfig, net::Listener};
use sim::view::view_for;
use sim::{Action, EventKind, PLAYER_COUNT, RULES, Seat, Tick, base_position, new_state};

#[path = "harness/traversal.rs"]
mod traversal;

/// Ticks the transport-driven match runs for.
///
/// Short on purpose: what it has to establish is that a bot takes a seat, is
/// accepted, and appears in the authority's log. Whether the bots fight is the
/// first test's business, where it is a deterministic fact about `sim` rather
/// than a race with a scheduler.
const NETWORK_TICKS: u32 = 200;

/// Ticks the offline match runs for.
///
/// Long enough for the walk to the lane to finish and a fight to resolve: the
/// bases are 173 units apart, a lane's meeting point is half of that from each
/// end, and a champion covers `champion_speed` — a fifth of a unit — in a tick.
/// `bots_play_a_match_that_has_a_fight_in_it` fails loudly rather than silently
/// if that arithmetic stops holding.
const OFFLINE_TICKS: u32 = 900;

// ---------------------------------------------------------------------------
// 1. The bots play a game
// ---------------------------------------------------------------------------

/// Nine bots, no server, no clock: they walk to the lanes and fight.
///
/// Driven straight against `sim` — `view_for` in, one `Action` per seat per
/// tick out, `step` — which is the same loop the server runs with the transport
/// removed. That makes this deterministic: no scheduler, no dropped frame, no
/// timing. What it asserts is what a person would look for in a playtest before
/// calling the game testable, and it asserts it as counters because
/// `docs/RISKS.md` R15's failure is an assertion whose antecedent never happened.
#[test]
fn bots_play_a_match_that_has_a_fight_in_it() {
    let mut bots: Vec<Bot> = Seat::ALL.into_iter().map(Bot::new).collect();
    let mut state = new_state(0x00C0_FFEE_0D15_EA5E);
    let spawn: Vec<_> = Seat::ALL
        .into_iter()
        .map(|seat| state.champion(seat).position)
        .collect();
    // Ever left its spawn, rather than *ended* somewhere else: a champion that
    // walked to the lane, died and respawned is standing on its spawn point at
    // the last tick, and a check on the final position would report the three
    // most eventful seats as the ones that did nothing.
    let mut left_spawn = [false; PLAYER_COUNT];

    let mut damage = 0u32;
    let mut deaths = 0u32;
    let mut casts = 0u32;
    for tick in 0..OFFLINE_TICKS {
        let mut inputs: Vec<sim::Input> = Vec::with_capacity(PLAYER_COUNT);
        for (index, seat) in Seat::ALL.into_iter().enumerate() {
            let view = view_for(&state, seat);
            let Some(bot) = bots.get_mut(index) else {
                continue;
            };
            inputs.push(sim::Input {
                tick: Tick(tick),
                seq: tick,
                player: seat,
                action: bot.observe(&view),
            });
        }
        state = sim::step(&state, &inputs);
        for (index, seat) in Seat::ALL.into_iter().enumerate() {
            let away = spawn.get(index).is_some_and(|from| {
                !from.within_range(state.champion(seat).position, RULES.champion_radius)
            });
            if let Some(slot) = left_spawn.get_mut(index) {
                *slot = *slot || away;
            }
        }
        for event in state.events().iter() {
            match event.kind {
                EventKind::Damage { .. } => damage = damage.saturating_add(1),
                EventKind::Death { .. } => deaths = deaths.saturating_add(1),
                EventKind::Cast { .. } => casts = casts.saturating_add(1),
            }
        }
    }

    let moved = left_spawn.iter().filter(|away| **away).count();
    let fights: u32 = bots.iter().map(|bot| bot.counters().1).sum();
    println!(
        "playtest: {OFFLINE_TICKS} ticks — {moved} of {PLAYER_COUNT} champions left their spawn, \
         {fights} fighting intention(s), {casts} cast(s), {damage} damage event(s), {deaths} death(s)"
    );

    assert_eq!(
        moved, PLAYER_COUNT,
        "{moved} of {PLAYER_COUNT} champions moved at all, so the rest were not playing"
    );
    assert!(
        fights > 0,
        "no bot ever asked to attack or to cast, so this is nine champions walking past each other"
    );
    assert!(
        damage > 0,
        "nothing in this match damaged anything, so a playtest of it says nothing about a fight \
         (docs/RISKS.md R15)"
    );
    assert!(
        deaths > 0,
        "nobody died in {OFFLINE_TICKS} ticks, so the respawn a playtester would want to see was \
         never reached"
    );
}

// ---------------------------------------------------------------------------
// 2. The match they played cannot be filed
// ---------------------------------------------------------------------------

/// **A match with a bot in a seat is refused by the corpus, and the same
/// pipeline stores the match without one.**
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_match_a_bot_played_is_refused_by_the_corpus_and_one_it_did_not_is_stored() {
    let scratch = Scratch::new("playtest-bots");
    let corpus = scratch.corpus();
    corpus
        .enrol(&a_consent("alizarin"), "alizarin@example.invalid")
        .expect("enrol the one person in these matches");

    // ---- the control: one person, nobody else, filed and stored ----
    let alone = played(1, 0).await;
    let (replay, session) = filed("playtest-control", &alone);
    let attested = Attested::of(&replay, &session, None).expect(
        "a match one person played, whose one seat has a session record, is a match the corpus \
         is supposed to take",
    );
    corpus.store(&attested).expect("store the control");
    assert!(
        scratch
            .path()
            .join("matches")
            .join(replay.manifest.match_id.to_string())
            .join("match.replay")
            .exists(),
        "the control was not written, so the refusal below is not a refusal of anything"
    );

    // ---- the playtest: one person and two bots ----
    let played_with_bots = played(3, 2).await;
    let bot_seats: Vec<usize> = seats_that_spoke(&played_with_bots)
        .into_iter()
        .filter(|seat| *seat != Seat::Blue0.index())
        .collect();
    assert_eq!(
        bot_seats.len(),
        2,
        "the two bots did not both reach the authority's log, so what is refused below is not \
         what a playtest produces"
    );

    let (replay, session) = filed("playtest-bots", &played_with_bots);

    // The refusal the corpus already had is **satisfied**: the operator filed
    // one person, the session record holds one seat, and the two agree seat for
    // seat. This is the check `Corpus::store` makes over the two operator-side
    // files, and it is why a check over those files could not have caught this.
    for index in 0..PLAYER_COUNT {
        assert_eq!(
            replay.manifest.participants[index].is_some(),
            session.occupied().contains(&index),
            "seat {index} disagrees between the two files, so this match would have been refused \
             for a reason that is not the bot"
        );
    }

    let refused = Attested::of(&replay, &session, None)
        .expect_err("a match two bots played was accepted by the corpus's door");
    println!("playtest: refused — {refused}");
    assert_eq!(
        refused.seats, bot_seats,
        "the refusal names {:?} and the bots played {bot_seats:?}",
        refused.seats
    );

    // …and there is no other way in: `Corpus::store` takes an `Attested` and
    // `Attested::of` is its only constructor, so the sentence below is a
    // statement about the type rather than about this test's discipline.
    assert!(
        !scratch
            .path()
            .join("matches")
            .join(replay.manifest.match_id.to_string())
            .exists(),
        "the playtest match reached the corpus"
    );
}

/// One transport-driven match: `seats` clients, of which the last `bots` are
/// bots and the first is a person's.
///
/// The person is scripted the way `client/tests/m4_exit.rs` scripts one — a
/// standing order down the lane, re-sent every tick — because the point of this
/// harness is that the two kinds of seat are indistinguishable to the server,
/// and a person driven differently from a bot would be assuming the answer.
async fn played(seats: usize, bots: usize) -> Recording {
    let listener = Listener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).expect("bind");
    let address = listener.local_addr().expect("local address");
    let certificate = listener.certificate().to_vec();

    let hosting = tokio::spawn(listener.host(
        MatchConfig {
            seed: 0x00C0_FFEE_0D15_EA5E,
            players: seats,
        },
        Duration::from_millis(5),
        NETWORK_TICKS,
    ));

    // The person joins **before** the bots are launched, and this is a fixture
    // decision rather than a rule of the game: the server hands out the lowest
    // free seat, so a race between three connections would put the person
    // anywhere and the operator below files a pseudonym against seat 0. It is
    // also the advice `moba-bots` prints — start the people first if you care
    // which seats they get.
    let (seated, took_a_seat) = tokio::sync::oneshot::channel();
    let mut playing = vec![tokio::spawn(person(address, certificate.clone(), seated))];
    assert_eq!(
        took_a_seat.await.expect("the person never took a seat"),
        Seat::Blue0,
        "the person did not take the first seat"
    );
    for _ in 0..bots {
        playing.push(tokio::spawn(bot(address, certificate.clone())));
    }
    for handle in playing {
        handle
            .await
            .expect("a client task panicked")
            .expect("a client failed");
    }
    hosting
        .await
        .expect("the host task panicked")
        .expect("the host failed")
}

/// A seat driven the way a person's client drives one.
async fn person(
    address: SocketAddr,
    certificate: Vec<u8>,
    seated: tokio::sync::oneshot::Sender<Seat>,
) -> Result<(), String> {
    let mut wire = Wire::connect(address, &certificate)
        .await
        .map_err(|error| error.to_string())?;
    let mut session = Headless::new();
    let seat = join(&mut wire, &mut session).await?;
    let standing = Action::Move(base_position(Seat::Red0.team(), &RULES));
    seated.send(seat).map_err(|_| "nobody was listening")?;

    while let Ok(frame) = wire.recv_state().await {
        session.receive(&frame).map_err(|error| error.to_string())?;
        if wire.send(&session.intend(standing, 0)).await.is_err() {
            break;
        }
    }
    Ok(())
}

/// A seat driven by `client::bot`, which is what `moba-bots` does.
async fn bot(address: SocketAddr, certificate: Vec<u8>) -> Result<(), String> {
    let mut wire = Wire::connect(address, &certificate)
        .await
        .map_err(|error| error.to_string())?;
    let mut session = Headless::new();
    let seat = join(&mut wire, &mut session).await?;
    let mut bot = Bot::new(seat);

    while let Ok(frame) = wire.recv_state().await {
        session.receive(&frame).map_err(|error| error.to_string())?;
        let Some(view) = session.view() else {
            continue;
        };
        let action = bot.observe(view);
        if wire.send(&session.intend(action, 0)).await.is_err() {
            break;
        }
    }
    Ok(())
}

/// `Join`, `Ready`, and the seat the server granted.
async fn join(wire: &mut Wire, session: &mut Headless) -> Result<Seat, String> {
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
    Ok(seat)
}

/// What an operator files after the evening: the sealed replay, and the session
/// record assembled from the parts the clients wrote.
///
/// **One part, from the one seat that had a device behind it.** That is the
/// whole of what makes a playtest different on disk from a session — a bot
/// writes no part because `client::health::SessionPart` is built from an
/// `InputTrace`, and a trace is what a capture path produces. The pseudonym goes
/// on seat 0 for the same reason: it is the seat the person sat in, and an
/// operator filing honestly names exactly that one.
fn filed(name: &str, recording: &Recording) -> (Replay, SessionRecord) {
    let mut identifier = [b'-'; 16];
    for (slot, byte) in identifier.iter_mut().zip(name.bytes()) {
        *slot = byte;
    }
    let mut participants: [Option<Pseudonym>; PLAYER_COUNT] = [const { None }; PLAYER_COUNT];
    participants[Seat::Blue0.index()] = Pseudonym::parse("alizarin");

    let replay = replay::seal(
        recording,
        &SessionFacts {
            match_id: MatchId(identifier),
            started_at_unix_ms: 1_786_000_000_000,
            participants,
            sim_commit: SimCommit::Unknown,
            telemetry: replay::Commitment::Absent,
        },
        &SigningKey::from_seed(*b"moba playtest signing key......\0"),
    );

    let session = SessionRecord::assemble(
        replay.manifest.match_id,
        ConsentVersion::current(),
        "2026-09-03",
        Supervision::InPerson,
        &[(
            "blue0.session-part".to_owned(),
            a_part(Seat::Blue0).encode(),
        )],
    )
    .expect("assemble the session record from the one part a client wrote");
    (replay, session)
}

/// The part the person's client wrote.
///
/// A trace with device events in it, a cadence that kept up, and a crossing of
/// the lobby — the same fixture `client/tests/session_part.rs` builds, and for
/// its reason: a part of all zeroes would be refused by `Corpus::store`'s
/// silent-seat rule, and this test's subject is a different refusal.
fn a_part(seat: Seat) -> SessionPart {
    let mut trace = InputTrace::new();
    for index in 0..64u64 {
        let at_ns = index
            .saturating_mul(8_000_000)
            .saturating_add(index % 3 * 100_000);
        trace.moved(at_ns, 1.0 + (index % 5) as f64, -1.0);
    }
    trace.pressed(600_000_000, Control::Move, true);
    trace.pressed(600_500_000, Control::Move, false);

    let mut cadence = Cadence::with_budget(33_333_333);
    for _ in 0..500 {
        cadence.pass(1_200_000);
    }

    let mut play = Play::new();
    traversal::cross(&mut play, traversal::Hand::quick(), 12);

    SessionPart {
        seat,
        declared: Declared {
            device_profile_id: "mouse-a".to_owned(),
            device_cpi: 1600,
            device_polling_hz: 1000,
            pointer_acceleration: false,
        },
        trace: trace.stats(),
        cadence: cadence.report(),
        calibration: play.lobby().observations(),
    }
}

/// A consent record for the one person in these matches.
fn a_consent(pseudonym: &str) -> ConsentRecord {
    ConsentRecord {
        pseudonym: pseudonym.to_owned(),
        consented_on: "2026-09-01".to_owned(),
        retention_until: "2028-09-01".to_owned(),
        permissions: Permissions::none(),
        adult: true,
        consent_version: ConsentVersion::current(),
    }
}

/// A corpus in a directory of its own, removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("moba-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a scratch directory");
        Self(path)
    }

    fn corpus(&self) -> Corpus {
        Corpus::open(&self.0)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The seats the authority recorded an input from.
///
/// The same thing `replay::attest` reads out of a sealed replay, computed here
/// from the recording so that the test knows which seats it is expecting to be
/// refused rather than reading them back out of the refusal.
fn seats_that_spoke(recording: &Recording) -> Vec<usize> {
    let mut seats: Vec<usize> = Vec::new();
    for timed in &recording.inputs {
        let seat = timed.input.player.index();
        if !seats.contains(&seat) {
            seats.push(seat);
        }
    }
    seats.sort_unstable();
    seats
}
