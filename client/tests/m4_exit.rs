//! `docs/MILESTONES.md` M4's exit criterion, and the honest account of which
//! half of it a machine can run.
//!
//! > Three humans play a match end to end on two operating systems — one team of
//! > the three, with the other six seats unoccupied […]; the match writes a
//! > replay; `replay verify` resimulates it to the server's final digest. The
//! > consent text exists, states the four points above, and was signed by all
//! > three before the match rather than after.
//!
//! # What this file runs, and what it cannot
//!
//! Four clauses, and they are not the same kind of thing:
//!
//! | Clause | Who can check it |
//! | --- | --- |
//! | one team of three, six seats unoccupied | this file |
//! | the match writes a replay | this file |
//! | `replay verify` resimulates it to the server's final digest | this file, in a separate process |
//! | **three humans, on two operating systems** | nobody but three humans on two operating systems |
//!
//! The fourth is a fact about a calendar and three people. Nothing in CI can
//! stand in for it and this file does not pretend to: what it substitutes is
//! three clients driving the **same input path a person drives** — intentions
//! composed the way `client::play` composes them, one per tick, a standing order
//! that persists and one-shot abilities that do not disturb it, through
//! `Prediction` and over the real QUIC transport. That establishes the machinery
//! is right. It establishes nothing about whether three people can sit down and
//! play, and `docs/MILESTONES.md` records M4 accordingly.
//!
//! The consent clause is checked here too, in the only way a test can: the
//! document exists and says the things it is required to say. Whether it was
//! *signed before the match* is a fact about paper.

#![deny(unsafe_code)]

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;

use client::predict::Prediction;
use client::{Headless, net::Wire};
use server::{MatchConfig, net::Listener};
use sim::{Action, Digest, Fx, FxVec2, Seat, Tick, base_position};

#[path = "harness/mod.rs"]
mod harness;

const TICKS: u32 = 1000;
const CHECKPOINT: u32 = 100;

/// The tick the team turns for home.
///
/// Late enough that they arrive under Red's tower first. The lane is 173 units,
/// Red's tower stands a quarter of the way down it from Red's base, and a
/// champion covers `champion_speed` a tick — so the first shot lands at about
/// tick 607 and this leaves a couple of hundred ticks of it before the walk
/// back. Nobody dies: a champion has 600 hit points and the whole exposure
/// costs about 135 of them, which is what makes this a match with damage in it
/// rather than a match with a respawn in it.
const TURN: u32 = 800;

/// What one client came back with.
#[derive(Debug)]
struct Report {
    seat: Seat,
    checkpoints: BTreeMap<u32, Digest>,
    /// The worst the prediction was ever wrong by, in raw fixed-point units.
    worst_correction: i32,
    /// Views on which the prediction was exact.
    exact: u32,
    /// Views applied in total.
    views: u32,
    /// How many ticks the client rendered a position ahead of the last view.
    predicted_ahead: u32,
    frames_lost: u32,
    /// What the match this client played actually contained.
    reach: harness::Reach,
}

/// A client driven the way `client::play` drives one.
///
/// Not the M3 script. The point of this harness is that the input path a person
/// uses is the path under test: a standing intention re-sent every tick, changed
/// when the player changes it, with one-shot abilities that leave it alone. The
/// prediction runs, and its corrections are reported, because a prediction that
/// is quietly wrong is the failure a person would notice first and a digest
/// comparison never would.
async fn play(address: SocketAddr, certificate: Vec<u8>) -> Result<Report, String> {
    let mut wire = Wire::connect(address, &certificate)
        .await
        .map_err(|error| error.to_string())?;
    let mut headless = Headless::new();

    wire.send(&headless.join())
        .await
        .map_err(|error| error.to_string())?;
    let accepted = wire
        .recv_session()
        .await
        .map_err(|error| error.to_string())?;
    headless
        .receive(&accepted)
        .map_err(|error| error.to_string())?;
    let seat = headless.seat().ok_or("the server assigned no seat")?;
    wire.send(&headless.ready())
        .await
        .map_err(|error| error.to_string())?;

    // Down the Blue–Red lane, three files abreast, which is what puts entities
    // into and out of the fog rather than leaving every view empty.
    let home = base_position(Seat::Blue0.team(), &sim::RULES);
    let target = base_position(Seat::Red0.team(), &sim::RULES);
    let along = target.sub(home);
    let across = FxVec2::new(along.y.neg(), along.x).normalize_or_zero();
    let file = match seat {
        Seat::Blue0 => -4,
        Seat::Blue1 => 0,
        _ => 4,
    };
    let mut standing = Action::Move(target.add(across.scale(Fx::from_int(file))));

    let mut prediction: Option<Prediction> = None;
    let mut checkpoints = BTreeMap::new();
    let mut predicted_ahead = 0u32;
    let mut exact = 0u32;
    let mut views = 0u32;
    let mut reach = harness::Reach::default();

    loop {
        let Ok(frame) = wire.recv_state().await else {
            break;
        };
        headless
            .receive(&frame)
            .map_err(|error| error.to_string())?;
        let Some(view) = headless.view().cloned() else {
            continue;
        };

        let seen = prediction.get_or_insert_with(|| Prediction::anchored(&view));
        seen.observe(&view, headless.applied_through());
        views = views.saturating_add(1);
        reach.observe(&headless);
        if seen.corrections().0 == 0 {
            exact = exact.saturating_add(1);
        }

        let Tick(tick) = view.tick;
        if tick.is_multiple_of(CHECKPOINT) {
            checkpoints.insert(tick, headless.world().digest());
        }

        // What the player would be asking for. A cast every eight seconds, which
        // is the skillshot's cooldown, and a change of destination once, so that
        // the standing order is exercised as an order rather than as a constant.
        //
        // The turn is at [`TURN`] and not at the halfway point it used to be at,
        // and the reason is `docs/RISKS.md` R15. A team that turns for home at
        // tick 500 covers a hundred of the lane's hundred and seventy-three
        // units, which stops it **thirty units short of Red's tower**: the match
        // this criterion ran over contained no damage at all, so the clause it
        // shares with M3 — three teammates agreeing about the world — was again
        // being checked on three teammates whose own hit points were equal by
        // accident. That is the exact condition that hid `LocalWorld::digest`'s
        // own-liveness bug for a milestone. Walking to `TURN` puts them under
        // fire from about tick 607, and the counters below refuse a run in which
        // it did not happen.
        let action = if tick % 240 == 60 {
            Action::Skillshot(along)
        } else {
            if tick == TURN {
                standing = Action::Move(home);
            }
            standing
        };

        let seq = headless.next_seq();
        seen.sent(seq, action);
        if seen.position() != seen.authoritative() {
            predicted_ahead = predicted_ahead.saturating_add(1);
        }
        if wire.send(&headless.intend(action, 0)).await.is_err() {
            break;
        }
    }

    let (frames_lost, _) = wire.losses();
    Ok(Report {
        seat,
        checkpoints,
        worst_correction: prediction.map_or(0, |seen| seen.corrections().1),
        exact,
        views,
        predicted_ahead,
        frames_lost,
        reach,
    })
}

/// The three machine-checkable clauses, end to end over the real transport.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_team_plays_a_match_that_writes_a_replay_which_verify_resimulates() {
    let listener = Listener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).expect("bind");
    let address = listener.local_addr().expect("local address");
    let certificate = listener.certificate().to_vec();

    // One team of the three, six seats unoccupied — a state the rules already
    // handle and the fixtures already cover.
    let hosting = tokio::spawn(listener.host(
        MatchConfig {
            seed: 0x00C0_FFEE_0D15_EA5E,
            players: 3,
        },
        // Four milliseconds a tick rather than M3's one, and the number is
        // measured rather than picked. At one millisecond three clients on one
        // machine cannot keep up: a third of the server's ticks run with no
        // input from a given client, the champion walks on its standing order
        // anyway, and the next view is two steps from the anchor where the
        // client drew one — 662 of 1000 views exact, worst correction exactly
        // one tick of movement. At four milliseconds and above, all three
        // clients are exact on every view of a thousand, and so they are at ten
        // and at the game's own thirty-three. The compression that costs
        // nothing is the one that keeps the criterion a statement about the
        // prediction rather than about the loopback's scheduler.
        Duration::from_millis(4),
        TICKS,
    ));

    let mut playing = Vec::new();
    for _ in 0..3 {
        playing.push(tokio::spawn(play(address, certificate.clone())));
    }
    let mut reports = Vec::new();
    for handle in playing {
        reports.push(
            handle
                .await
                .expect("a client task panicked")
                .expect("a client failed"),
        );
    }
    let recording = hosting
        .await
        .expect("the host task panicked")
        .expect("the host failed");

    reports.sort_by_key(|report| report.seat.index());
    assert_eq!(
        reports.iter().map(|report| report.seat).collect::<Vec<_>>(),
        vec![Seat::Blue0, Seat::Blue1, Seat::Blue2],
        "the three clients did not take one team's seats"
    );

    // One tick of movement, which is the unit every correction below is a
    // multiple of: a champion covers `champion_speed` in a tick and nothing in
    // the rules moves it by less.
    let step = sim::RULES.champion_speed.to_raw();

    for report in &reports {
        println!(
            "{:?}: {} checkpoints, prediction exact on {} of {} views, worst \
             correction {} raw units ({} tick(s) of movement), ahead on {} ticks, \
             {} frames lost",
            report.seat,
            report.checkpoints.len(),
            report.exact,
            report.views,
            report.worst_correction,
            report.worst_correction / step,
            report.predicted_ahead,
            report.frames_lost
        );
        println!("{:?}: reach — {}", report.seat, report.reach.summary());

        // `docs/RISKS.md` R15, before anything that is conditional on the match
        // having contained something.
        report.reach.assert_a_match_happened(report.seat);

        // Exact, on every view, over the real transport. `prediction.rs` makes
        // the same claim on a match driven one tick at a time, which is the test
        // of the *rule*; this one has QUIC, three sessions and a scheduler in
        // between, which is the test of the rule as a person meets it.
        //
        // The unit is deliberately named in the failure message. Every way this
        // can go wrong produces a multiple of one tick of movement — a client
        // that fell behind, an order folded in twice, a step applied that the
        // server did not — so a correction reported in raw fixed-point units and
        // in ticks tells a reader which of those it is looking at without
        // reaching for a calculator.
        assert_eq!(
            report.worst_correction,
            0,
            "{:?}'s prediction was corrected by {} raw units, {} tick(s) of \
             movement: the client and the server disagree about how a champion \
             moves, or the client fell behind the tick period",
            report.seat,
            report.worst_correction,
            report.worst_correction / step
        );
        assert_eq!(
            report.exact, report.views,
            "{:?}'s prediction was exact on only {} of {} views",
            report.seat, report.exact, report.views
        );
        assert!(
            report.predicted_ahead * 2 > TICKS,
            "{:?} rendered a predicted position on only {} of {TICKS} ticks, so \
             the prediction is not what is under test here",
            report.seat,
            report.predicted_ahead
        );
    }

    // …and they were on different hit points while they agreed, which is what
    // makes the agreement below evidence rather than a coincidence. A tower
    // shoots the lowest-numbered seat it can see, so this is one of the three
    // bleeding and two beside it untouched.
    let under_fire = reports
        .iter()
        .filter(|report| report.reach.hurt_views > 0)
        .count();
    assert!(
        under_fire > 0 && under_fire < reports.len(),
        "{under_fire} of the three clients were ever below full health, so the \
         agreement asserted below is agreement between teammates whose own liveness \
         never differed (docs/RISKS.md R15)"
    );

    // The three clients agree about the world, which is M3's criterion and still
    // has to hold when a person is driving.
    let first = &reports[0];
    let mut compared = 0u32;
    for report in &reports[1..] {
        for (tick, digest) in &report.checkpoints {
            let Some(theirs) = first.checkpoints.get(tick) else {
                continue;
            };
            assert_eq!(
                theirs, digest,
                "{:?} disagrees with {:?} about the world at tick {tick}",
                report.seat, first.seat
            );
            compared = compared.saturating_add(1);
        }
    }
    assert!(
        compared >= (TICKS / CHECKPOINT) * 3 / 2,
        "{compared} compared"
    );

    // ---------------------------------------------------------------
    // The match writes a replay, and `replay verify` resimulates it
    // ---------------------------------------------------------------

    assert!(
        recording.ticks >= TICKS && !recording.inputs.is_empty(),
        "the recording is {} ticks and {} inputs",
        recording.ticks,
        recording.inputs.len()
    );

    // Sealed, because since M5 there is one file format and it is signed: a
    // criterion that wrote an unsigned container would be exercising a format
    // this project no longer has. The key and the registry are the harness's.
    let (path, keys) = harness::seal_to_disk(&recording, "moba-m4");

    let output = std::process::Command::new(harness::replay_binary())
        .arg("verify")
        .arg(&path)
        .arg(&keys)
        .output()
        .expect("run the replay tool");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&keys);

    assert!(
        output.status.success(),
        "replay verify refused the recording.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains(&harness::hex(&recording.final_state_digest)),
        "replay verify did not report the server's own digest.\nstdout:\n{stdout}"
    );
}

/// The consent text exists and says the four things M4 requires of it.
///
/// A document test, and worth being clear about what it is: it checks that the
/// four required subjects are addressed, not that the words are good. Whether
/// the text was *signed before the match* is a fact about paper and about three
/// people, and no test reaches it.
///
/// It is here rather than nowhere because the failure it catches is real and
/// cheap: a `docs/CONSENT.md` that quietly loses its withdrawal section in an
/// edit about something else, months after anybody last read it, and before the
/// recording session that depends on it.
#[test]
fn the_consent_text_exists_and_states_the_four_required_points() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the client crate has a parent directory")
        .join("docs")
        .join("CONSENT.md");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let lower = text.to_lowercase();

    // 1. Declared purpose, and that the data serves nothing else.
    assert!(
        lower.contains("calibrate and evaluate") && lower.contains("only declared purpose"),
        "the consent text does not state a declared purpose"
    );
    // Publication of the raw corpus is a separate, refusable purpose.
    assert!(
        lower.contains("separate purpose") && lower.contains("refus"),
        "publication is not offered as a separate, refusable purpose"
    );
    // 2. Retention, and what triggers destruction.
    assert!(
        lower.contains("24 months") || lower.contains("twenty-four months"),
        "the consent text does not state a retention period"
    );
    // 3. Withdrawal: how, and on what timeline.
    assert!(
        lower.contains("withdraw") && lower.contains("7 days") && lower.contains("30"),
        "the consent text does not state how to withdraw or on what timeline"
    );
    // 4. What withdrawal actually destroys, including the part about other
    //    people's matches, which is the sentence a participant must read first.
    assert!(
        lower.contains("every match you played in, in full"),
        "the consent text does not say what withdrawal destroys"
    );
    assert!(
        lower.contains("eight players'"),
        "the consent text does not say that withdrawal destroys other \
         participants' contributions"
    );
    // …and the field-by-field account of what is collected, which
    // `docs/MILESTONES.md` M4 requires before the first recording session.
    for field in [
        "claimed_at_ms",
        "received_at_ms",
        "seq",
        "action",
        "rules_hash",
    ] {
        assert!(
            text.contains(field),
            "the consent text does not account for the `{field}` field"
        );
    }
    // Who holds it and where it lives.
    assert!(
        lower.contains("outside the git repository") || lower.contains("outside the repository"),
        "the consent text does not say where the data lives"
    );
}
