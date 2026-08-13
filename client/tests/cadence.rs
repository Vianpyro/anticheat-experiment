//! The tick budget, measured on a dense match instead of an empty one.
//!
//! # The gap this closes
//!
//! `docs/RISKS.md` R16, and the short version is that the only two numbers this
//! project ever had for the client's tick budget were both taken on fixtures.
//! `client/tests/m4_exit.rs` compressed the server's period to four
//! milliseconds, found that enough for a match in which three champions walked a
//! lane and nothing touched them, and found it **not** enough the moment the same
//! three walked under a tower and started receiving damage events — so the number
//! moved to ten, and the comment beside it records the lesson: *the period is a
//! budget for the client, and a fixture that reaches more of the game spends more
//! of it.*
//!
//! What nobody had run is the case a corpus is actually recorded under. Nine
//! occupied seats, every champion visible to every other, damage and cast events
//! at the frame's cap: each of those makes a view bigger to decode, more
//! expensive to reconcile and more expensive to draw, and all nine of them arrive
//! on the same tick. This file is that match, and it reports what one pass of the
//! capture loop costs in it.
//!
//! # What it asserts and what it prints, which are different lists
//!
//! The same split `client/tests/jitter.rs` makes, for the same reason: a
//! threshold on a shared CI runner is a check that goes red for reasons that have
//! nothing to do with this repository.
//!
//! - **Asserted, machine-independent.** That the fixture reached a dense match at
//!   all — nine champions visible, events delivered, the loop answering views.
//!   That [`Cadence`] counted every pass the loop made and agreed with an
//!   independently kept maximum. That the overrun it reports is the overrun a
//!   re-derived count finds in the same durations.
//! - **Printed, host-dependent.** The pass distribution, the worst overrun and
//!   the number of passes over budget, at the real 33.3 ms tick and at the 10 ms
//!   the M4 harness compresses to. `docs/RISKS.md` R16 carries the values against
//!   the run that produced them.
//!
//! # What is real here and what is not
//!
//! **Real:** nine sessions on one server over `quinn`, the server ticking at
//! 30 Hz, real datagrams and reassembly, the reconciliation, and — for the one
//! measured client — `compose` and `rasterize` over the actual mark list of an
//! actual view into an actual 1280×800 framebuffer.
//!
//! **Not:** the other eight clients do not render. They exist to fill seats and
//! to fight, which is what makes the measured client's views full. And the device
//! events are synthesised on a schedule, because CI has no display server — the
//! same substitution `client/tests/jitter.rs` makes and states.
//!
//! So the number below is a **lower bound** on what a real session costs: nine
//! renderers on nine machines are nine machines, but the measured client here
//! shares a container with eight other clients and a server, which pushes the
//! other way. It is the closest thing to the case that has never been run, and it
//! is reported as such rather than as a budget anybody may now rely on.

#![deny(unsafe_code)]

use std::net::SocketAddr;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use client::draw::{Scene, compose, rasterize};
use client::health::{BUDGET_NS, Cadence};
use client::play::Play;
use client::{Headless, net::Wire};
use protocol::{ClientFrame, SERVER_FRAME_BYTES};
use server::{MatchConfig, net::Listener};
use sim::view::MAX_EVENTS_PER_VIEW;
use sim::{Action, FxVec2, PLAYER_COUNT, RULES, Seat};

/// The device's report period. 125 Hz, an ordinary USB mouse, the same rate
/// `client/tests/jitter.rs` uses so the two runs are comparable.
const PERIOD: Duration = Duration::from_millis(8);

/// The window the measured client rasterises into.
const WINDOW: (u32, u32) = (1280, 800);

/// The redraw period. Sixty hertz, so the loop draws between views rather than
/// only on them.
const REDRAW: Duration = Duration::from_micros(16_667);

/// The server's period. The game's own rate, deliberately uncompressed: the load
/// on a capture loop *is* the rate views arrive at, and a compressed match would
/// be measuring a budget nobody records under.
const TICK: Duration = Duration::from_millis(33);

/// Ticks the match runs, and the walk is most of it.
///
/// Twenty-three seconds of wall clock at the game's own rate, which is a real
/// cost and is paid deliberately: the alternative is compressing the server's
/// period, and a compressed period is a compressed budget, which is exactly the
/// substitution that produced the number `docs/RISKS.md` R16 exists to
/// distrust.
const TICKS: u32 = 700;

/// The tick the nine of them stop walking and start fighting.
///
/// The bases are at the vertices of a triangle of circumradius **100** and a
/// champion covers `champion_speed` = 0.2 units a tick, so the walk to the
/// centroid is 500 ticks and no arithmetic makes it shorter — the centroid is
/// the point that minimises the furthest of the three walks. This is that plus a
/// margin, and what follows it is a hundred and eighty ticks of nine champions
/// standing inside one another's `attack_range`.
const FIGHT_FROM: u32 = 520;

/// The M4 harness's compressed budget, measured beside the real one.
///
/// Not because a corpus is recorded at ten milliseconds — it is not — but because
/// that number is the only tick budget this project has ever had evidence for,
/// and `docs/RISKS.md` R16 is the entry that says what that evidence was worth.
/// Reporting the same run against both is what turns "10 ms may be short" into a
/// number.
const M4_BUDGET_NS: u64 = 10_000_000;

/// What the run measured.
struct Measured {
    /// Every pass of the capture loop, in nanoseconds, in order.
    passes_ns: Vec<u64>,
    /// The same, as [`Cadence`] counted it at the real tick budget.
    cadence: client::health::CadenceReport,
    /// …and at the M4 harness's compressed budget.
    compressed: client::health::CadenceReport,
    /// Views the measured client reconciled.
    views: u32,
    /// Frames it rasterised.
    frames: u64,
    /// Device events it recorded.
    samples: usize,
    /// The most entities it was ever told about at once.
    most_entities: usize,
    /// Views that carried at least one derived event.
    views_with_events: u32,
    /// The most events one view carried.
    peak_events: usize,
    /// Views on which its own champion was below full health.
    hurt_views: u32,
}

/// Seats other than the measured one, each on its own connection.
///
/// They fill the roster and they fight. Nothing here renders: the load these
/// eight exist to create is on the *measured* client, whose views are full
/// because these eight are in them.
async fn filler(address: SocketAddr, certificate: Vec<u8>) -> Result<Seat, String> {
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

    while let Ok(frame) = wire.recv_state().await {
        if headless.receive(&frame).is_err() {
            break;
        }
        let Some(view) = headless.view() else {
            continue;
        };
        let action = script(seat, view.tick.0);
        if wire.send(&headless.intend(action, 0)).await.is_err() {
            break;
        }
    }
    Ok(seat)
}

/// What every seat does.
///
/// Walk to the origin — the centroid of the triangle, inside no tower's range —
/// and then attack an enemy, standing where they are. Nine champions inside one
/// another's `attack_range` of 2.5 units produce a damage event every
/// `attack_cooldown_ticks`, which is what fills a tick's event record; the
/// skillshot rides on top on its own cooldown and crosses the scrum from
/// wherever its caster is standing.
///
/// The target is the seat four along, which is on another team for **every** one
/// of the nine — the arithmetic is worth doing rather than trusting, since a
/// rotation that paired two teammates would produce an order the rules refuse and
/// a match with a quiet seat in it.
fn script(seat: Seat, tick: u32) -> Action {
    if tick < FIGHT_FROM {
        return Action::Move(FxVec2::ZERO);
    }
    let index = seat.index();
    let enemy = *Seat::ALL
        .get((index + 4) % PLAYER_COUNT)
        .unwrap_or(&Seat::Blue0);
    // One cast per seat per cooldown, staggered so the nine of them do not all
    // land on the same tick and then go quiet together.
    if (tick.wrapping_add(index as u32 * 7)).is_multiple_of(60) {
        let angle = index as i32;
        return Action::Skillshot(FxVec2::new(
            sim::Fx::from_int(angle - 4),
            sim::Fx::from_int(if index.is_multiple_of(2) { 3 } else { -3 }),
        ));
    }
    Action::Attack(sim::champion_entity_id(enemy))
}

/// Runs the dense match with one client that captures, renders and talks.
#[expect(
    clippy::too_many_lines,
    reason = "one harness: a server, nine clients and a capture loop"
)]
fn capture_under_a_dense_match() -> Measured {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("a runtime");

    let entered = runtime.enter();
    let listener = Listener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).expect("bind");
    drop(entered);
    let address = listener.local_addr().expect("local address");
    let certificate = listener.certificate().to_vec();

    let hosting = runtime.spawn(listener.host(
        MatchConfig {
            seed: 0x00C0_FFEE_0D15_EA5E,
            players: PLAYER_COUNT,
        },
        TICK,
        TICKS,
    ));

    // The measured client joins first, so it takes Blue0 and the eight fillers
    // take the rest. Which seat it is does not matter to the measurement; that it
    // is a fixed one keeps the run reproducible.
    let mut headless = Headless::new();
    let join = headless.join();
    let (mut wire, accepted) = runtime
        .block_on(async {
            let mut wire = Wire::connect(address, &certificate)
                .await
                .map_err(|error| error.to_string())?;
            wire.send(&join).await.map_err(|error| error.to_string())?;
            let accepted = wire
                .recv_session()
                .await
                .map_err(|error| error.to_string())?;
            Ok::<_, String>((wire, accepted))
        })
        .expect("the measured client could not join");
    headless.receive(&accepted).expect("the acceptance");
    let seat = headless.seat().expect("a seat");

    let mut fillers = Vec::new();
    for _ in 1..PLAYER_COUNT {
        fillers.push(runtime.spawn(filler(address, certificate.clone())));
    }

    let (inbound, views) = mpsc::channel::<[u8; SERVER_FRAME_BYTES]>();
    let (outbound, mut to_send) = tokio::sync::mpsc::channel::<ClientFrame>(64);
    outbound
        .try_send(headless.ready())
        .expect("the ready frame");
    runtime.spawn(async move {
        loop {
            tokio::select! {
                state = wire.recv_state() => {
                    let Ok(bytes) = state else { return };
                    if inbound.send(bytes).is_err() {
                        return;
                    }
                }
                sending = to_send.recv() => {
                    let Some(frame) = sending else { return };
                    if wire.send(&frame).await.is_err() {
                        return;
                    }
                }
            }
        }
    });

    // The device stream, on an absolute schedule, exactly as `jitter.rs`
    // produces it.
    let (device, motions) = mpsc::channel::<f64>();
    let producer = std::thread::spawn(move || {
        let start = Instant::now();
        for index in 0..u64::MAX {
            let due = start + PERIOD.saturating_mul(u32::try_from(index).unwrap_or(u32::MAX));
            let now = Instant::now();
            if due > now {
                std::thread::sleep(due - now);
            }
            if device.send(1.0 + (index % 7) as f64 * 0.125).is_err() {
                return;
            }
        }
    });

    let mut play = Play::new();
    play.resized(WINDOW.0, WINDOW.1);
    let mut pixels = vec![0u32; (WINDOW.0 as usize).saturating_mul(WINDOW.1 as usize)];
    let mut cadence = Cadence::new();
    let mut compressed = Cadence::with_budget(M4_BUDGET_NS);
    let mut passes_ns: Vec<u64> = Vec::new();
    let mut next_redraw = Instant::now();
    let mut stamp = 0u64;

    let mut views_applied = 0u32;
    let mut frames = 0u64;
    let mut most_entities = 0usize;
    let mut views_with_events = 0u32;
    let mut peak_events = 0usize;
    let mut hurt_views = 0u32;

    loop {
        let pass_began = Instant::now();

        // 1. The device queue, one sample per event, stamped as it is drained —
        //    `Session::device_event`'s shape. Blocking briefly rather than
        //    spinning, because a busy-wait would make a "pass" a few hundred
        //    nanoseconds of nothing and the distribution below a distribution of
        //    empty turns. Two milliseconds is well inside the 8 ms the device
        //    reports at, so no event waits for the timeout.
        match motions.recv_timeout(Duration::from_millis(2)) {
            Ok(delta) => {
                stamp = stamp.saturating_add(1_000_000);
                play.moved(stamp, delta, 0.0);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        // 2. Everything the server has sent, each answered with one intention.
        let mut disconnected = false;
        loop {
            match views.try_recv() {
                Ok(bytes) => {
                    if headless.receive(&bytes).is_err() {
                        disconnected = true;
                        break;
                    }
                    let Some(view) = headless.view() else {
                        continue;
                    };
                    views_applied = views_applied.saturating_add(1);
                    most_entities = most_entities.max(headless.world().len());
                    peak_events = peak_events.max(view.events.len());
                    if !view.events.is_empty() {
                        views_with_events = views_with_events.saturating_add(1);
                    }
                    if let sim::Liveness::Alive { hp } = view.own.liveness
                        && hp < RULES.champion_max_hp
                    {
                        hurt_views = hurt_views.saturating_add(1);
                    }
                    let action = script(seat, view.tick.0);
                    let frame = headless.intend(action, 0);
                    if outbound.try_send(frame).is_err() {
                        disconnected = true;
                        break;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }

        // 3. A frame, when one is due, composed from the real view.
        if Instant::now() >= next_redraw
            && let Some(view) = headless.view()
        {
            let scene = Scene {
                view,
                seat,
                own: view.own.position,
                aim: play.aim(),
            };
            rasterize(&compose(&scene), play.viewport(), &mut pixels);
            frames = frames.saturating_add(1);
            next_redraw = Instant::now() + REDRAW;
        }

        let took = u64::try_from(pass_began.elapsed().as_nanos()).unwrap_or(u64::MAX);
        passes_ns.push(took);
        cadence.pass(took);
        compressed.pass(took);

        if disconnected {
            break;
        }
    }

    drop(motions);
    let _ = producer.join();
    drop(outbound);
    for handle in fillers {
        handle.abort();
    }
    hosting.abort();

    Measured {
        passes_ns,
        cadence: cadence.report(),
        compressed: compressed.report(),
        views: views_applied,
        frames,
        samples: play.trace().len(),
        most_entities,
        views_with_events,
        peak_events,
        hurt_views,
    }
}

/// Order statistics of a slice, sorted in place.
fn percentiles(values: &mut [u64]) -> client::input::Percentiles {
    values.sort_unstable();
    client::input::Percentiles::of(values)
}

/// **The measurement `docs/RISKS.md` R16 is stated against.**
#[test]
fn the_capture_loop_is_measured_against_the_tick_on_a_match_with_nine_seats_in_it() {
    let measured = capture_under_a_dense_match();
    let ms = |ns: u64| (ns as f64) / 1e6;

    let mut sorted = measured.passes_ns.clone();
    let spread = percentiles(&mut sorted);
    println!(
        "cadence: {} passes over {} views, {} frames, {} device events",
        measured.passes_ns.len(),
        measured.views,
        measured.frames,
        measured.samples
    );
    println!(
        "cadence: pass ms  min {:.3}  p50 {:.3}  p95 {:.3}  p99 {:.3}  max {:.3}",
        ms(spread.min),
        ms(spread.p50),
        ms(spread.p95),
        ms(spread.p99),
        ms(spread.max)
    );
    println!(
        "cadence: at the game's tick ({:.3} ms): {} pass(es) over budget, worst \
         overrun {:.3} ms",
        ms(BUDGET_NS),
        measured.cadence.passes_over_budget,
        ms(measured.cadence.worst_overrun_ns)
    );
    println!(
        "cadence: at M4's compressed budget ({:.3} ms): {} pass(es) over budget, \
         worst overrun {:.3} ms",
        ms(M4_BUDGET_NS),
        measured.compressed.passes_over_budget,
        ms(measured.compressed.worst_overrun_ns)
    );
    println!(
        "cadence: reach — {} entities at most, {} view(s) with an event, {} events \
         on the busiest view against a frame budget of {MAX_EVENTS_PER_VIEW}, {} \
         view(s) under fire",
        measured.most_entities,
        measured.views_with_events,
        measured.peak_events,
        measured.hurt_views
    );

    // ---------------------------------------------------------------------
    // The match was dense. `docs/RISKS.md` R15: every assertion below this
    // point is about a loop under load, and a loop under no load satisfies all
    // of them trivially.
    // ---------------------------------------------------------------------
    assert!(
        measured.views > 100,
        "the client applied only {} views, so it was not communicating",
        measured.views
    );
    assert!(
        measured.frames > 100,
        "the client rasterised only {} frames, so it was not rendering",
        measured.frames
    );
    assert!(
        measured.samples > 100,
        "the client recorded only {} device events, so it was not capturing",
        measured.samples
    );
    // Nine champions and six towers is fifteen; a projectile in flight is more.
    // Below nine means somebody never saw the scrum, which is the fixture this
    // file exists to stop being.
    assert!(
        measured.most_entities >= PLAYER_COUNT,
        "the measured client never saw more than {} entities at once, so the nine \
         seats never met and this is the empty match the budget was already set \
         on (docs/RISKS.md R15)",
        measured.most_entities
    );
    assert!(
        measured.views_with_events > 0 && measured.peak_events > 1,
        "the busiest view carried {} event(s), so no tick in this match was busy \
         and the load the budget is about was never applied",
        measured.peak_events
    );
    assert!(
        measured.hurt_views > 0,
        "the measured client was never damaged, so the nine of them shared a map \
         and not a fight"
    );

    // ---------------------------------------------------------------------
    // The instrument agrees with an independently kept record of the same
    // durations. This is what makes the printed numbers evidence rather than
    // `Cadence` agreeing with itself: the loop pushes every pass into a vector
    // and the counters are re-derived from it here.
    // ---------------------------------------------------------------------
    assert_eq!(
        measured.cadence.passes,
        measured.passes_ns.len() as u64,
        "Cadence counted {} of the {} passes the loop made",
        measured.cadence.passes,
        measured.passes_ns.len()
    );
    assert_eq!(
        measured.cadence.worst_pass_ns, spread.max,
        "Cadence reports a longest pass of {} ns against an independently kept \
         maximum of {} ns",
        measured.cadence.worst_pass_ns, spread.max
    );
    let over = measured
        .passes_ns
        .iter()
        .filter(|took| **took > BUDGET_NS)
        .count() as u64;
    assert_eq!(
        measured.cadence.passes_over_budget, over,
        "Cadence counted {} passes over the tick and the durations hold {over}",
        measured.cadence.passes_over_budget
    );
    let worst = measured
        .passes_ns
        .iter()
        .map(|took| took.saturating_sub(BUDGET_NS))
        .max()
        .unwrap_or(0);
    assert_eq!(measured.cadence.worst_overrun_ns, worst);

    // …and the compressed budget is a strictly harder question than the real
    // one, which is the arithmetic that makes reporting both worth anything.
    assert!(
        measured.compressed.passes_over_budget >= measured.cadence.passes_over_budget,
        "a smaller budget reported fewer overruns, so one of the two counters is \
         not measuring what it says"
    );
}
