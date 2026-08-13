//! What the state channel does when the network eats it.
//!
//! # The debt this pays
//!
//! State travels in QUIC datagrams since M3, so a client can miss a tick, and
//! the M3 exit criterion had to be weakened to say so: no two clients ever
//! disagree about a checkpoint they *both received*, plus a floor of nine tenths
//! of the checkpoints reached per client. On loopback the observed loss is zero.
//! The floor had therefore never once been approached, its calibration was
//! unknown, and every claim about the loss path rested on `ShardAssembler` unit
//! tests that hand it shards in an order chosen by hand.
//!
//! A criterion whose interesting condition has never occurred is a criterion
//! nobody has verified. `server::net::LossProfile` produces the condition — in
//! the real transport, on the send path, per shard — and this file runs the
//! criterion under it.
//!
//! # What is asserted at each rate, and what is only reported
//!
//! The **invariant** is asserted everywhere and is the one the criterion is
//! actually making: two clients that both received a checkpoint agree about it.
//! Loss must cost a client ticks and must never cost it a *different world*. If
//! reassembly ever sewed two frames together, or a partially applied view
//! survived, this is where it shows.
//!
//! The **floor** is reported rather than asserted at the injected rates, because
//! it is a statement about a healthy transport and not about this one: at a
//! per-shard rate of *p* a frame needs both shards, so frames arrive at
//! `(1 - p)²` — 81% at 10%, 49% at 30% — and requiring nine tenths of the
//! checkpoints under deliberate loss would be requiring the injection not to
//! work. What is asserted instead is that the reach is in the neighbourhood the
//! arithmetic predicts, in both directions: too high means the loss is not
//! reaching the wire, too low means something other than the injection is
//! eating frames.
//!
//! The **event-loss counter** is asserted to be zero everywhere, and that is not
//! a formality — see `docs/ARCHITECTURE.md`. A non-zero count there is a client
//! that permanently missed a game event, which is a correctness defect and not a
//! capacity statistic.

#![deny(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::time::Duration;

use client::{Headless, net::Wire};
use server::MatchConfig;
use server::net::{Listener, LossProfile};
use sim::{Action, Digest, Seat, Tick};

const TICKS: u32 = 1000;

/// Every twentieth tick rather than M3's every hundredth.
///
/// Fifty checkpoints instead of ten, because at 30% loss a client reaches about
/// half of them and ten samples of a coin that lands heads half the time is not
/// a measurement. The invariant does not care; the reported reach does.
const CHECKPOINT: u32 = 20;

/// What one client came back with.
#[derive(Debug)]
struct Report {
    seat: Seat,
    checkpoints: BTreeMap<u32, Digest>,
    ticks_seen: u32,
    incomplete_frames: u32,
    late_shards: u32,
    stale_views: u32,
}

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

    let mut checkpoints = BTreeMap::new();
    let mut ticks_seen = 0u32;
    // The standing intention, re-sent every tick. `Action::Idle` is a rule that
    // stops the champion, so a client with nothing new to ask for repeats what
    // it is already asking for — see `client::predict`.
    let standing = Action::Move(sim::base_position(Seat::Red0.team(), &sim::RULES));

    loop {
        let Ok(frame) = wire.recv_state().await else {
            break;
        };
        headless
            .receive(&frame)
            .map_err(|error| error.to_string())?;
        ticks_seen = ticks_seen.saturating_add(1);

        let Tick(tick) = headless.world().tick();
        if tick.is_multiple_of(CHECKPOINT) {
            checkpoints.insert(tick, headless.world().digest());
        }
        if wire.send(&headless.intend(standing, 0)).await.is_err() {
            break;
        }
    }

    let (incomplete, late_shards) = wire.losses();
    Ok(Report {
        seat,
        checkpoints,
        ticks_seen,
        incomplete_frames: incomplete,
        late_shards,
        stale_views: headless.stale(),
    })
}

/// Runs one match at the loss rate given and returns what the three clients saw.
async fn match_at(loss: LossProfile) -> Vec<Report> {
    let listener = Listener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).expect("bind");
    let address = listener.local_addr().expect("local address");
    let certificate = listener.certificate().to_vec();

    let hosting = tokio::spawn(listener.host_lossy(
        MatchConfig {
            seed: 0x00C0_FFEE_0D15_EA5E,
            players: 3,
        },
        Duration::from_millis(1),
        TICKS,
        loss,
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
    hosting
        .await
        .expect("the host task panicked")
        .expect("the host failed");

    reports.sort_by_key(|report| report.seat.index());
    reports
}

/// The invariant, checked over the checkpoints a pair of clients both hold.
///
/// Returns how many digests were actually compared, so that the caller can
/// refuse a run in which the three clients shared nothing and "they never
/// disagreed" is a statement about the empty set.
fn agree(reports: &[Report]) -> u32 {
    let mut compared = 0u32;
    for (index, first) in reports.iter().enumerate() {
        for second in reports.iter().skip(index.saturating_add(1)) {
            for (tick, digest) in &first.checkpoints {
                let Some(theirs) = second.checkpoints.get(tick) else {
                    continue;
                };
                assert_eq!(
                    digest, theirs,
                    "{:?} disagrees with {:?} about the world at tick {tick}",
                    first.seat, second.seat
                );
                compared = compared.saturating_add(1);
            }
        }
    }
    compared
}

/// The whole thing, at 0%, 10% and 30%.
///
/// One test rather than three, because the interesting output is the comparison
/// between the rows and a reader should see them together. Each rate is a
/// separate match on a separate endpoint.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_state_channel_loses_ticks_and_never_loses_agreement() {
    let expected = TICKS / CHECKPOINT;
    let mut reached_at = Vec::new();

    for percent in [0u8, 10, 30] {
        let reports = match_at(LossProfile {
            datagram_percent: percent,
            seed: 0x1055_0000_0000 + u64::from(percent),
        })
        .await;

        assert_eq!(
            reports.iter().map(|r| r.seat).collect::<Vec<_>>(),
            vec![Seat::Blue0, Seat::Blue1, Seat::Blue2],
            "the three clients are one team, which is what makes them comparable"
        );

        for report in &reports {
            println!(
                "{percent}% loss  {:?}: {} of {expected} checkpoints, {} ticks applied, \
                 {} frames abandoned, {} shards late, {} views stale",
                report.seat,
                report.checkpoints.len(),
                report.ticks_seen,
                report.incomplete_frames,
                report.late_shards,
                report.stale_views
            );
        }

        // The invariant, at every rate. This is the claim the criterion makes.
        //
        // The floor under the comparison count is what stops "they never
        // disagreed" from being a statement about the empty set, and it has to
        // follow the rate rather than the criterion: three pairs, each sharing a
        // checkpoint when both received it, is `3 × 50 × (1 - p)⁴` — 150 with
        // nothing injected, about 98 at 10%, about 36 at 30%. Half of that,
        // rounded down, is the floor.
        let delivery = 100u32.saturating_sub(u32::from(percent));
        let both = delivery.pow(4) / 100u32.pow(3);
        let floor = expected.saturating_mul(3).saturating_mul(both) / 200;
        let compared = agree(&reports);
        assert!(
            compared >= floor,
            "{percent}% loss: only {compared} checkpoint digests were compared across \
             the three clients against a floor of {floor}, which is too few for their \
             agreement to mean anything"
        );

        // …and it is not agreement about a world that never changed.
        let distinct: BTreeSet<&Digest> = reports[0].checkpoints.values().collect();
        assert!(
            distinct.len() > 1,
            "{percent}% loss: every checkpoint produced the same digest"
        );

        let worst = reports
            .iter()
            .map(|report| report.checkpoints.len())
            .min()
            .unwrap_or(0);
        let abandoned: u32 = reports.iter().map(|r| r.incomplete_frames).sum();
        reached_at.push((percent, worst, abandoned));
    }

    // What the three rates say when they are put beside each other.
    for (percent, worst, abandoned) in &reached_at {
        println!(
            "{percent}% loss: worst client reached {worst}/{expected} checkpoints, \
             {abandoned} frames abandoned across the three"
        );
    }

    let (_, lossless_worst, lossless_abandoned) = reached_at[0];
    let (_, ten_worst, ten_abandoned) = reached_at[1];
    let (_, thirty_worst, thirty_abandoned) = reached_at[2];

    // At zero the criterion's own floor is the assertion, and it is the only
    // rate at which that floor is a claim about this project rather than about
    // the injection.
    assert!(
        lossless_worst as u32 * 10 >= expected * 9,
        "with nothing injected the worst client reached {lossless_worst} of {expected} \
         checkpoints, which is a transport failure rather than an occasional drop"
    );
    assert_eq!(
        lossless_abandoned, 0,
        "the loopback abandoned {lossless_abandoned} frames with no loss injected"
    );

    // The injection reaches the wire, and the assembler notices.
    assert!(
        ten_abandoned > 0 && thirty_abandoned > ten_abandoned,
        "frames abandoned did not follow the injected rate: {ten_abandoned} at 10%, \
         {thirty_abandoned} at 30%"
    );
    // A frame needs both of its shards, so delivery is (1 - p)^2: 81% and 49%.
    // Generous bands on both sides — too high means the loss never reached the
    // wire, too low means something else is eating frames.
    for (percent, worst, delivery_numerator, delivery_denominator) in [
        (10u8, ten_worst, 81u32, 100u32),
        (30, thirty_worst, 49, 100),
    ] {
        let predicted = expected.saturating_mul(delivery_numerator) / delivery_denominator;
        assert!(
            (worst as u32) * 2 >= predicted,
            "{percent}% loss: the worst client reached {worst} of {expected} checkpoints \
             against a prediction of {predicted}; something other than the injection is \
             eating frames"
        );
        assert!(
            (worst as u32) < expected,
            "{percent}% loss: the worst client reached {worst} of {expected} checkpoints, \
             so the injected loss never reached the wire"
        );
    }
}

/// The counter that must never move, over a match that gives it every chance to.
///
/// `EventBacklog` defers events a frame cannot carry and drops them only if the
/// queue itself overflows. A non-zero drop count is a client that will never be
/// told about something that happened to it — a correctness defect, not a
/// capacity number — and `docs/ARCHITECTURE.md` says it is a tripwire rather
/// than a budget.
///
/// # This test was asleep, and it is one of `docs/RISKS.md` R15's four
///
/// It used to seat three clients, take them ready, and tick a thousand times
/// **without sending a single input**. Nine champions therefore stood at their
/// bases for the whole match and the run produced **zero events of any kind**,
/// so "nothing was dropped" was a statement about a match in which there was
/// nothing to drop. Every mutation of `EventBacklog` passes it: one that drops
/// on the first overflow, one that never defers, one that throws the queue away
/// each tick.
///
/// What it does now is force the condition the assertion is about. Nine seats
/// walk to the middle of the map, where one observer can see all of the others,
/// and then every seat casts both of its abilities on the same tick as often as
/// the cooldowns allow — which produces more events in a tick than a frame's
/// [`sim::view::MAX_EVENTS_PER_VIEW`] budget can carry, so the overflow has to
/// be deferred and the queue has to be exercised. The floor below is on the
/// deferral, not on the drop: a run in which nothing was ever deferred is a run
/// in which the drop counter was never asked a question.
#[test]
fn nothing_a_client_was_entitled_to_is_ever_dropped_rather_than_deferred() {
    use protocol::{ClientFrame, ClientMessage, ServerFrame, ServerMessage};
    use sim::view::MAX_EVENTS_PER_VIEW;
    use sim::{FxVec2, PLAYER_COUNT, champion_entity_id};

    let mut game = server::Match::new(MatchConfig {
        seed: 0x00C0_FFEE_0D15_EA5E,
        players: PLAYER_COUNT,
    });
    for _ in 0..PLAYER_COUNT {
        assert!(game.join().0.is_some());
    }
    for seat in Seat::ALL {
        game.deliver(
            seat,
            ClientFrame::encode(&ClientMessage::Ready).as_bytes(),
            0,
        )
        .expect("ready was refused");
    }

    let mut seq = 0u32;
    let send = |game: &mut server::Match, seat: Seat, seq: u32, action: Action| {
        let _ = game.deliver(
            seat,
            ClientFrame::encode(&ClientMessage::Input {
                seq,
                claimed_at_ms: 0,
                action,
            })
            .as_bytes(),
            0,
        );
    };

    // The walk in. The middle of the triangle is inside no tower's range, which
    // is what lets the nine of them stand together long enough to fight.
    let middle = Action::Move(FxVec2::ZERO);
    for _ in 0..640u32 {
        for seat in Seat::ALL {
            send(&mut game, seat, seq, middle);
        }
        seq = seq.saturating_add(1);
        let _ = game.tick();
    }

    // …and then everything they have, whenever they have it. Two inputs a seat
    // a tick is inside `server::MAX_INPUTS_PER_TICK`; the cooldowns turn most of
    // them into no-ops and let the rest through together, which is what produces
    // a tick busier than one frame can carry.
    let mut produced = 0u64;
    let mut delivered = 0u64;
    let mut ticks_over_budget = 0u32;
    let mut peak_deferred = 0usize;
    for _ in 0..TICKS {
        for seat in Seat::ALL {
            let victim = if seat.team() == Seat::Red0.team() {
                champion_entity_id(Seat::Blue0)
            } else {
                champion_entity_id(Seat::Red0)
            };
            send(
                &mut game,
                seat,
                seq,
                Action::Skillshot(sim::base_position(seat.team(), &sim::RULES).neg()),
            );
            send(
                &mut game,
                seat,
                seq.saturating_add(1),
                Action::Targeted(victim),
            );
            // And a standing order to keep hitting somebody, so the tick's event
            // count is a rate rather than four bursts of it: three inputs a seat
            // a tick, inside `server::MAX_INPUTS_PER_TICK`.
            send(
                &mut game,
                seat,
                seq.saturating_add(2),
                Action::Attack(victim),
            );
        }
        seq = seq.saturating_add(3);

        let frames = game.tick();
        let count = game.world().events().count();
        produced = produced.saturating_add(count as u64);
        if count > MAX_EVENTS_PER_VIEW {
            ticks_over_budget = ticks_over_budget.saturating_add(1);
        }
        for (_, frame) in &frames {
            match ServerFrame::decode(frame.as_bytes()) {
                Ok(ServerMessage::View { view, .. }) => {
                    delivered = delivered.saturating_add(view.events.len() as u64);
                }
                other => panic!("a tick produced {other:?} rather than a view"),
            }
        }
        for seat in Seat::ALL {
            peak_deferred = peak_deferred.max(game.events_owed(seat).0);
        }
        assert_eq!(
            game.events_lost(),
            0,
            "the match dropped an event a client was entitled to"
        );
    }

    println!(
        "backlog: {produced} events produced, {delivered} delivered across nine \
         seats, {ticks_over_budget} ticks over the {MAX_EVENTS_PER_VIEW}-event frame \
         budget, {peak_deferred} the deepest the queue ever got"
    );

    // The floors. Each one is an antecedent the assertion above needs.
    assert!(
        produced > 0,
        "the match produced no events at all, so 'nothing was dropped' is a \
         statement about an empty match (docs/RISKS.md R15)"
    );
    assert!(
        ticks_over_budget > 0,
        "no tick of the match produced more than {MAX_EVENTS_PER_VIEW} events, so \
         the frame budget was never exceeded and the backlog was never used"
    );
    assert!(
        peak_deferred > 0,
        "nothing was ever deferred, so the drop counter was never asked a question"
    );
    assert!(
        delivered > 0,
        "no frame carried a single event, so deferring everything for ever would \
         also pass this test"
    );
    assert_eq!(game.events_lost(), 0, "the match lost events");
}
