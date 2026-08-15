//! Exploit class 3: synthetic input and botting.
//!
//! `docs/SCOPE.md` and `docs/MILESTONES.md` M6 both say this in advance, and this
//! file is where it stops being a paragraph: **there is a bot, no delivered
//! defence catches it, and that is correct.** `docs/MILESTONES.md` M7 is explicit
//! that an exploit which does not fall is worth keeping — "un exploit non attrape
//! documente une limite reelle et vaut mieux qu'une defense supposee" — so this
//! exploit is green on purpose, and the green documents a limit rather than a
//! defence.
//!
//! # What "not caught" means concretely, so the green is not empty
//!
//! Two things M7 could deliver, and neither catches the bot:
//!
//! - **The server does not reject it.** A bot's frames are well-formed
//!   intentions; `Match::deliver` accepts every one. There is no frame a bot sends
//!   that a person's client could not.
//! - **Resimulation does not catch it.** `docs/SCOPE.md`'s standing note on class
//!   2 is exactly this: a bot's inputs are in the log and resimulate perfectly, so
//!   the replay verifies. The artefact is indistinguishable from a human match.
//!
//! # The one mechanical thing a file can say, and how narrow it is
//!
//! `replay`'s corpus refuses a seat that recorded **zero device events**, which
//! catches a *headless* bot — one that drives the protocol and never touches an
//! input device. This file demonstrates that too, and its own narrowness: the
//! same refusal does not touch a bot that moves a real mouse, which records as
//! many samples as a person. That is `docs/SCOPE.md`'s stated ceiling of
//! behavioural detection arriving early, and what keeps it closed at M6 is the
//! operator in the room, not a property of any file. The behavioural detectors
//! that narrow the gap statistically are M8's, and they carry their own error
//! bounds; nothing here claims one.

#![deny(unsafe_code)]

use cheat_client::bot::Bot;
use protocol::Action;
use replay::manifest::Build;
use replay::session::{
    Clock, Declared, Measured, Platform, SeatRecord, SessionRecord, Supervision,
};
use sim::{RULES, Seat, base_position};

#[path = "harness/authority.rs"]
mod authority;

use authority::started_match;

const HONEST_SEED: [u8; 32] = *b"moba cheat honest server key\0\0\0\0";

/// Nine bots play a whole match, and the server cannot tell.
///
/// Each bot walks its seat toward the centre and casts down its lane on the
/// cooldown — the shape a person's client produces, because a bot that produced a
/// different traffic shape would be caught by the traffic invariant rather than by
/// anything behavioural, and that is class 1, not class 3.
#[test]
fn a_bot_plays_a_whole_match_and_nothing_delivered_catches_it() {
    let mut game = started_match(0x0F1E_2D3C_4B5A_6978, 9);
    let mut bots: Vec<Bot> = (0..9).map(|_| Bot::new()).collect();

    // Everyone toward the centre.
    for seat in Seat::ALL {
        let frame = bots[seat.index()].intend(Action::Move(sim::FxVec2::ZERO), 0);
        game.deliver(seat, frame.as_bytes().as_slice(), 0)
            .expect("the server accepted a bot's opening move");
    }

    let mut accepted = 0u64;
    let mut refused = 0u64;
    for tick in 0..400u32 {
        // A cast every eight seconds from every seat that can.
        if tick % 240 == 60 {
            for seat in Seat::ALL {
                let direction = base_position(seat.team(), &RULES).neg();
                let frame = bots[seat.index()].intend(Action::Skillshot(direction), 0);
                match game.deliver(seat, frame.as_bytes().as_slice(), 0) {
                    Ok(()) => accepted += 1,
                    Err(_) => refused += 1,
                }
            }
        }
        let _ = game.tick();
    }

    // Works, half one: the server accepted the bot's play. There was no frame it
    // sent that the authority treated as anything but a legal intention.
    println!("botting: the server accepted {accepted} bot casts and refused {refused}");
    assert_eq!(
        refused, 0,
        "the server refused a bot frame; a bot sent nothing illegal"
    );

    // R15: the bot's match actually happened — a replay of nine champions standing
    // still would verify without meaning anything.
    let recording = game.recording();
    assert!(!recording.inputs.is_empty(), "the bot match is empty (R15)");
    let mut events = 0usize;
    {
        let mut state = sim::new_state(recording.seed);
        let mut buckets: Vec<Vec<sim::Input>> = vec![Vec::new(); recording.ticks as usize];
        for timed in &recording.inputs {
            if let Some(bucket) = buckets.get_mut(timed.input.tick.0 as usize) {
                bucket.push(timed.input);
            }
        }
        for bucket in &buckets {
            state = sim::step(&state, bucket);
            events = events.saturating_add(state.events().count());
        }
    }
    assert!(events > 0, "the bot match produced no events (R15)");
    println!(
        "botting: the bot match ran {} ticks and produced {events} events",
        recording.ticks
    );

    // Works, half two: the bot's replay verifies. Resimulation — the class-2
    // defence — passes the bot's log exactly, because every input in it is real.
    // The artefact is indistinguishable from a human one, which is the whole
    // point: **this exploit is not caught, and that is documented rather than
    // fixed.**
    let key = replay::SigningKey::from_seed(HONEST_SEED);
    let sealed = replay::seal(
        &recording,
        &replay::SessionFacts::anonymous(replay::MatchId(*b"a-bot-played-th0"), 1_786_000_000_000),
        &key,
    );
    let mut registry = replay::KeyRegistry::new();
    registry.insert(key.verifying(), replay::KeyStatus::Active, "server");
    replay::verify(&sealed, &registry, &Build::current())
        .expect("the bot's replay did not verify — but a bot's inputs resimulate perfectly");
    println!(
        "botting: the bot's replay VERIFIED — no delivered defence distinguishes it \
         from a human match (docs/SCOPE.md ceiling, docs/MILESTONES.md M6)"
    );
}

/// The one mechanical thing a file says about synthetic play, and how narrow it
/// is.
///
/// `replay`'s corpus refuses a seat whose client recorded zero device events. A
/// headless bot records none, so it is refused; a bot moving a real mouse records
/// as many as a person, so it is not. This asserts both, which is the honest shape
/// of the defence: it is a floor against the crudest bot and nothing against the
/// one `docs/SCOPE.md` names as the ceiling.
#[test]
fn the_corpus_refuses_a_silent_bot_and_cannot_touch_a_mouse_moving_one() {
    let declared = Declared {
        device_profile_id: replay::DeviceProfileId::parse("bot-mouse").expect("a device label"),
        device_cpi: 800,
        device_polling_hz: 1000,
        pointer_acceleration: false,
    };
    let measured = |samples: u64| Measured {
        platform: Platform::Linux,
        clock: Clock::Dequeue,
        world_units_per_count_e6: 50_000,
        samples,
        motions: samples,
        coincident: 0,
        median_gap_ns: 8_000_000,
        budget_ns: 33_000_000,
        passes: 1000,
        passes_over_budget: 0,
        worst_overrun_ns: 0,
        worst_pass_ns: 2_000_000,
    };

    // A headless bot: zero samples. The corpus's silent-seat check catches it.
    let headless = SessionRecord {
        match_id: replay::MatchId(*b"headless-bot-000"),
        consent_version: replay::consent::ConsentVersion::current(),
        recorded_on: "2026-08-13".to_owned(),
        supervision: Supervision::InPerson,
        seats: {
            let mut seats = [const { SeatRecord::Empty }; sim::PLAYER_COUNT];
            seats[Seat::Blue0.index()] = SeatRecord::Human {
                declared: declared.clone(),
                measured: measured(0),
                calibration: replay::calibration::SeatCalibration::absent(),
            };
            seats
        },
    };
    assert_eq!(
        headless.silent_seats(),
        vec![Seat::Blue0.index()],
        "the corpus did not flag a seat that recorded no device event"
    );

    // A bot moving a real mouse: samples like a person. The same check finds
    // nothing, which is the ceiling stated as code.
    let mouse_bot = SessionRecord {
        match_id: replay::MatchId(*b"mouse-moving-bot"),
        consent_version: replay::consent::ConsentVersion::current(),
        recorded_on: "2026-08-13".to_owned(),
        supervision: Supervision::InPerson,
        seats: {
            let mut seats = [const { SeatRecord::Empty }; sim::PLAYER_COUNT];
            seats[Seat::Blue0.index()] = SeatRecord::Human {
                declared,
                measured: measured(12_000),
                calibration: replay::calibration::SeatCalibration::absent(),
            };
            seats
        },
    };
    assert!(
        mouse_bot.silent_seats().is_empty(),
        "the silent-seat check flagged a bot that moved a real mouse, which it \
         cannot distinguish from a person (docs/SCOPE.md ceiling)"
    );
    println!(
        "botting: the corpus refuses a headless bot (0 samples) and is blind to a \
         mouse-moving one (docs/SCOPE.md ceiling)"
    );
}
