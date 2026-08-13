//! What the dequeue timestamp costs, measured while the client is doing its job.
//!
//! # The residual this is about
//!
//! `docs/RISKS.md` R14 closed its aim-resolution half and left its timestamp
//! half open. No platform in `docs/ENGINEERING.md`'s matrix hands this client a
//! device timestamp through `winit`, so [`client::input::CLOCK`] is
//! `Clock::Dequeue`: the stamp is read in the callback that receives the event,
//! and it carries whatever delay sits between the device producing the event and
//! this process reading it.
//!
//! The number R14 recorded for that was taken **on an idle container, with no
//! renderer and no socket** — a standard deviation of 0.247 ms on a stream
//! emitted at 8 ms. That is the measurement under the conditions that do not
//! matter. The conditions that matter are the ones a corpus is recorded under:
//! a client rasterising a frame, reconciling a view, and writing to a socket, on
//! the same thread that stamps the events. This file is that measurement.
//!
//! # What is real here and what is synthesised, stated rather than glossed
//!
//! **Real:** the rasteriser (`client::draw::compose` and `rasterize`, over the
//! actual mark list of an actual view, into an actual 1280×800 framebuffer), the
//! transport (a `quinn` endpoint, a real server ticking at 30 Hz, real datagrams
//! reassembled by `ShardAssembler`), the reconciliation, and the structure of
//! the loop — one thread drains device events and draws, a `tokio` runtime on
//! another thread carries the socket, exactly as `client::gfx::play` arranges
//! them.
//!
//! **Synthesised:** the device events. There is no display server in CI and
//! `winit` cannot open a window without one, so the events are produced by a
//! thread emitting on an absolute schedule at 125 Hz and delivered through a
//! channel. That reproduces every source of delay this *process* contributes —
//! the loop's own latency between drains, the scheduler contention from
//! rasterising, and the runtime beside it — and it does **not** reproduce the
//! kernel input stack or the compositor. So the number below is a lower bound on
//! the real residual, and R14 says so where it records it.
//!
//! The mechanism the measurement is actually aimed at is in the covered half. A
//! long frame does not delay an event on its way through the kernel; it delays
//! the *drain*, so the events that arrived during it are all stamped when it
//! finishes. That is the fifteen-millisecond tail that would look like a
//! hesitation, and it is produced here if it is produced anywhere.

#![deny(unsafe_code)]

use std::net::SocketAddr;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use client::draw::{Scene, compose, rasterize};
use client::input::WORLD_UNITS_PER_COUNT;
use client::play::Play;
use client::{Headless, net::Wire};
use protocol::{ClientFrame, SERVER_FRAME_BYTES};
use server::{MatchConfig, net::Listener};
use sim::{Action, RULES, base_position};

/// The device's report period. 125 Hz is an ordinary USB mouse and is the rate
/// R14's first measurement used, so the two numbers are comparable.
const PERIOD: Duration = Duration::from_millis(8);

/// Device events emitted. 1200 at 125 Hz is 9.6 seconds, which is enough
/// samples for a 99th percentile to be the twelfth largest rather than the
/// largest, and the same count R14's first measurement used.
const EVENTS: u64 = 1200;

/// The window the client rasterises into, in pixels. The playable client's own
/// default, because the load is the point and a smaller buffer would be a
/// cheaper frame than a player draws.
const WINDOW: (u32, u32) = (1280, 800);

/// The redraw period. Sixty hertz, which is more often than a 30 Hz server sends
/// anything, so the loop is drawing between views rather than only on them.
const REDRAW: Duration = Duration::from_micros(16_667);

/// One device count per event, which over 1200 events is 60 world units — well
/// inside `map_half_extent`, so the aim never reaches its clamp.
///
/// `docs/RISKS.md` R15's fourth instance was a capture fixture that saturated
/// this clamp and made an equality true of anything. Nothing here compares two
/// aims, but the arithmetic is written down so that a later edit to `EVENTS`
/// has to look at it.
const COUNTS_PER_EVENT: f64 = 1.0;

/// What the run measured.
struct Measured {
    stats: client::input::TraceStats,
    /// Device events the producer emitted.
    emitted: u64,
    /// Distinct timestamps among the samples. Equal to the sample count unless
    /// something is stamping more than one event at a time.
    distinct_stamps: usize,
    /// Views the client reconciled while it was capturing.
    views: u32,
    /// Frames the client rasterised while it was capturing.
    frames: u64,
}

/// Runs a match with one client that captures, renders and talks, and returns
/// what its trace measured.
fn capture_under_load() -> Measured {
    assert!(
        (EVENTS as f64 * COUNTS_PER_EVENT * WORLD_UNITS_PER_COUNT)
            < f64::from(RULES.map_half_extent.to_raw()) / 65536.0,
        "the fixture would saturate the aim's clamp"
    );

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("a runtime");

    // `quinn` binds its endpoint on whatever runtime is current, so the bind
    // happens inside the runtime's context even though this thread is not one
    // of its workers — the same shape `client::gfx::play` has, where `winit`
    // owns this thread and the transport lives beside it.
    let entered = runtime.enter();
    let listener = Listener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).expect("bind");
    drop(entered);
    let address = listener.local_addr().expect("local address");
    let certificate = listener.certificate().to_vec();

    // The server runs at the game's own rate. Compressing it, as the exit
    // criteria do, would change the thing being measured: the load on the
    // capture loop is the arrival rate of views, and 30 Hz is the rate a corpus
    // is recorded at.
    let hosting = runtime.spawn(listener.host(
        MatchConfig {
            seed: 0x00C0_FFEE_0D15_EA5E,
            players: 1,
        },
        Duration::from_millis(33),
        // Long enough to outlast the device stream: the client stops when the
        // producer does, not when the server does.
        400,
    ));

    let mut headless = Headless::new();
    let join = headless.join();
    let (mut wire, accepted) = runtime
        .block_on(async move {
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
        .expect("the client could not join");
    headless.receive(&accepted).expect("the acceptance");
    let seat = headless.seat().expect("a seat");

    // The transport lives on the runtime and meets the capture loop over two
    // channels, which is `client::gfx::play`'s arrangement rather than a
    // simplification of it.
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

    // The device stream: an absolute schedule, so a late wake-up does not push
    // the whole run later. What the producer sends is the *delta*; the stamp is
    // read by the consumer, which is where the client reads it.
    let (device, motions) = mpsc::channel::<f64>();
    let producer = std::thread::spawn(move || {
        let start = Instant::now();
        for index in 0..EVENTS {
            let due = start + PERIOD.saturating_mul(u32::try_from(index).unwrap_or(u32::MAX));
            let now = Instant::now();
            if due > now {
                std::thread::sleep(due - now);
            }
            if device.send(COUNTS_PER_EVENT).is_err() {
                return index;
            }
        }
        EVENTS
    });

    let mut play = Play::new();
    play.resized(WINDOW.0, WINDOW.1);
    let mut pixels = vec![0u32; (WINDOW.0 as usize).saturating_mul(WINDOW.1 as usize)];
    let epoch = Instant::now();
    let mut applied = 0u32;
    let mut frames = 0u64;
    let mut next_redraw = Instant::now();
    let standing = Action::Move(base_position(seat.team(), &RULES));

    loop {
        // 1. The device queue, stamped per event in the callback's position.
        //    This is `Session::device_event`: read the clock first, then decide
        //    what the event means.
        match motions.recv_timeout(Duration::from_millis(2)) {
            Ok(delta) => {
                let at_ns = u64::try_from(epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
                play.moved(at_ns, delta, 0.0);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        // 2. Everything that has arrived from the server, answered with one
        //    intention each — the network half of the load.
        while let Ok(bytes) = views.try_recv() {
            if headless.receive(&bytes).is_err() {
                break;
            }
            if headless.view().is_some() {
                applied = applied.saturating_add(1);
                let frame = headless.intend(standing, 0);
                if outbound.try_send(frame).is_err() {
                    break;
                }
            }
        }

        // 3. A frame, when one is due — the rendering half. Composed from the
        //    real view and rasterised into a real buffer, because a `compose`
        //    over an empty scene is not the work a player's machine does.
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
    }

    let emitted = producer.join().expect("the producer thread panicked");
    drop(outbound);
    hosting.abort();
    let stats = play.trace().stats();
    let mut stamps: Vec<u64> = play
        .trace()
        .samples()
        .iter()
        .map(|sample| sample.at_ns)
        .collect();
    stamps.sort_unstable();
    stamps.dedup();

    Measured {
        stats,
        emitted,
        distinct_stamps: stamps.len(),
        views: applied,
        frames,
    }
}

/// The measurement, and the two claims it is allowed to make.
///
/// The numbers are printed on every run, because a distribution that drifts is
/// something a reader should be able to see in a log rather than in a bisect.
/// What is *asserted* is deliberately narrower than what is printed, and the
/// split is between claims about the mechanism and claims about the machine:
///
/// - **Mechanism, asserted.** Every emitted event produces exactly one sample;
///   no two samples share a timestamp; none is a coincident duplicate; the
///   record is monotone. A client that stamped once per frame, or that folded a
///   frame's worth of motion into one sample, fails all of these on a busy
///   machine and none of them on an idle one — which is exactly why they are the
///   assertions rather than a threshold.
/// - **Machine, printed and bounded loosely.** The standard deviation and the
///   99th percentile are what `docs/RISKS.md` R14 is closed against, and their
///   values are a property of the host. The ceiling asserted here is four times
///   the emission period, which no scheduling noise on a working machine
///   approaches and which per-frame stamping at 60 Hz exceeds immediately. The
///   strict number R14 quotes — a standard deviation under a millisecond — is
///   reported here and recorded there against the run that produced it, rather
///   than asserted against every runner this will ever meet.
#[test]
fn the_dequeue_stamp_survives_the_client_rendering_and_talking() {
    let measured = capture_under_load();
    let stats = measured.stats;
    let ms = |ns: u64| (ns as f64) / 1e6;

    println!(
        "jitter: {} of {} device events recorded, {} views applied, {} frames \
         rasterised over {:.2} s",
        stats.samples,
        measured.emitted,
        measured.views,
        measured.frames,
        ms(stats.span_ns) / 1e3
    );
    println!(
        "jitter: inter-arrival ms  min {:.3}  p05 {:.3}  p50 {:.3}  p95 {:.3}  \
         p99 {:.3}  max {:.3}",
        ms(stats.gaps_ns.min),
        ms(stats.gaps_ns.p05),
        ms(stats.gaps_ns.p50),
        ms(stats.gaps_ns.p95),
        ms(stats.gaps_ns.p99),
        ms(stats.gaps_ns.max)
    );
    println!(
        "jitter: mean {:.3} ms, standard deviation {:.3} ms, against an emission \
         period of {:.3} ms",
        stats.gap_mean_ns / 1e6,
        stats.gap_sd_ns / 1e6,
        PERIOD.as_secs_f64() * 1e3
    );

    // The load has to have happened, or this is R14's original measurement with
    // a longer test name (`docs/RISKS.md` R15).
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

    // The mechanism.
    assert_eq!(
        stats.samples as u64, measured.emitted,
        "the client recorded {} of the {} device events it was given: under load \
         it is losing or coalescing samples",
        stats.samples, measured.emitted
    );
    assert_eq!(
        measured.distinct_stamps,
        stats.samples,
        "{} of {} samples share a timestamp with another, so something is stamping \
         more than one event at a time — which is what reading the clock once per \
         frame does",
        stats.samples - measured.distinct_stamps,
        stats.samples
    );
    assert_eq!(
        stats.coincident, 0,
        "the platform delivered an event more than once"
    );

    // The machine, bounded where a bound means something.
    let ceiling = PERIOD.as_nanos() as u64 * 4;
    assert!(
        stats.gaps_ns.p99 < ceiling,
        "the 99th percentile of the recorded inter-arrival is {:.3} ms against an \
         emission period of {:.3} ms: one sample in a hundred is arriving more than \
         four periods late, which is a stall in the capture loop and is the shape \
         of a hesitation a detector at M8 would read",
        ms(stats.gaps_ns.p99),
        PERIOD.as_secs_f64() * 1e3
    );
}
