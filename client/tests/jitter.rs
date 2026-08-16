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

/// A quarter of a device count per event, which over 1200 events is 45 world
/// units — well inside `map_half_extent`, so the aim never reaches its clamp.
///
/// `docs/RISKS.md` R15's fourth instance was a capture fixture that saturated
/// this clamp and made an equality true of anything. Nothing here compares two
/// aims, but the arithmetic is written down so that a later edit to `EVENTS`
/// has to look at it — and the guard below caught the edit that came from the
/// other end, `client::input::WORLD_UNITS_PER_COUNT` tripling under a fixture
/// whose counts had not moved. The number is therefore chosen in **world
/// units**: it is the travel that has to stay inside the map, and the count is
/// whatever produces it.
const COUNTS_PER_EVENT: f64 = 0.25;

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
    /// **The residual, isolated.** For every event, the nanoseconds between the
    /// producer sending it and the capture loop stamping it, sorted.
    ///
    /// This field is the answer to what the first Windows CI run of this file
    /// found. The recorded *inter-arrival* distribution is the sum of two
    /// things — how regularly the events were produced, and how promptly they
    /// were stamped — and on a host whose sleep granularity is coarse the first
    /// term dominates completely. Windows' default timer resolution is about
    /// 15.6 ms, so a producer asking for 8 ms overshoots and then catches up,
    /// and the inter-arrival distribution it generates says nothing whatever
    /// about the client. `docs/RISKS.md` R14 needs the second term alone.
    ///
    /// Differencing against a timestamp the producer read from the same clock
    /// removes the first term exactly. What is left is the delay the capture
    /// loop adds, which is what the residual R14 leaves open *is*.
    latency_ns: Vec<u64>,
    /// The longest single pass of the capture loop.
    ///
    /// The bound the latency is a claim against: an event waits at most one pass
    /// before it is stamped, so a latency above this is the loop falling behind
    /// the device rather than a slow frame.
    slowest_pass_ns: u64,
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

    // The device stream. Each event carries the producer's own reading of the
    // shared clock at the moment it was sent, and that field is what makes this
    // a measurement of the client rather than of the producer — see
    // [`Measured::latency_ns`].
    let epoch = Instant::now();
    let (device, motions) = mpsc::channel::<(u64, f64)>();
    let producer = std::thread::spawn(move || {
        let start = Instant::now();
        for index in 0..EVENTS {
            let due = start + PERIOD.saturating_mul(u32::try_from(index).unwrap_or(u32::MAX));
            let now = Instant::now();
            if due > now {
                std::thread::sleep(due - now);
            }
            // A delta that changes every event. A constant one would make
            // consecutive samples bit-identical, and `TraceStats::coincident` —
            // which exists to catch a *platform* delivering one event twice —
            // would report the producer's own bursts instead. That is what it
            // did on the first Windows CI run of this file: the default timer
            // resolution there is about 15.6 ms, so a `sleep(8 ms)` overshoots
            // and the loop above sends the overdue events back to back,
            // eighteen of which arrived under 100 microseconds apart carrying
            // the same number.
            let delta = COUNTS_PER_EVENT + (index % 7) as f64 * 0.125;
            let at_ns = u64::try_from(epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
            if device.send((at_ns, delta)).is_err() {
                return index;
            }
        }
        EVENTS
    });

    let mut play = Play::new();
    play.resized(WINDOW.0, WINDOW.1);
    let mut pixels = vec![0u32; (WINDOW.0 as usize).saturating_mul(WINDOW.1 as usize)];
    let mut applied = 0u32;
    let mut frames = 0u64;
    let mut next_redraw = Instant::now();
    let mut latency_ns: Vec<u64> = Vec::with_capacity(EVENTS as usize);
    let mut slowest_pass_ns = 0u64;
    let standing = Action::Move(base_position(seat.team(), &RULES));

    loop {
        let pass_began = Instant::now();

        // 1. The device queue, stamped per event in the callback's position.
        //    This is `Session::device_event`: read the clock first, then decide
        //    what the event means.
        match motions.recv_timeout(Duration::from_millis(2)) {
            Ok((emitted_at_ns, delta)) => {
                let at_ns = u64::try_from(epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
                play.moved(at_ns, delta, 0.0);
                latency_ns.push(at_ns.saturating_sub(emitted_at_ns));
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

        slowest_pass_ns =
            slowest_pass_ns.max(u64::try_from(pass_began.elapsed().as_nanos()).unwrap_or(u64::MAX));
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
    latency_ns.sort_unstable();

    Measured {
        stats,
        emitted,
        distinct_stamps: stamps.len(),
        views: applied,
        frames,
        latency_ns,
        slowest_pass_ns,
    }
}

/// Mean and population standard deviation of an already-sorted slice, in
/// nanoseconds.
#[expect(
    clippy::cast_precision_loss,
    reason = "a summary of a distribution, never a recorded value"
)]
fn spread(sorted: &[u64]) -> (f64, f64) {
    if sorted.is_empty() {
        return (0.0, 0.0);
    }
    let count = sorted.len() as f64;
    let mean = sorted.iter().map(|value| *value as f64).sum::<f64>() / count;
    let variance = sorted
        .iter()
        .map(|value| {
            let deviation = *value as f64 - mean;
            deviation * deviation
        })
        .sum::<f64>()
        / count;
    (mean, variance.sqrt())
}

/// The measurement, and the claims it is allowed to make.
///
/// The numbers are printed on every run, because a distribution that drifts is
/// something a reader should be able to see in a log rather than in a bisect.
/// What is *asserted* is narrower than what is printed, and the split is between
/// claims about the mechanism and claims about the host:
///
/// - **Mechanism, asserted.** Every emitted event produces exactly one sample;
///   no two samples share a timestamp; none is a coincident duplicate. A client
///   that stamped once per frame, or that folded a frame's worth of motion into
///   one sample, fails all of these on a busy machine and none of them on an
///   idle one — which is exactly why they are the assertions rather than a
///   threshold.
/// - **The residual, asserted against the loop rather than against a stopwatch.**
///   An event waits at most one pass of the capture loop before it is stamped.
///   That is machine-independent and it is the claim that matters: a latency
///   above one pass is the loop falling behind the device, which is the only way
///   this design can bake a delay into a corpus.
/// - **The host, printed.** The spread and the tail of the added latency are
///   what `docs/RISKS.md` R14 is closed against, and their values are a property
///   of the machine and of the profile. They are reported here and recorded there
///   against the run that produced them, rather than asserted against every
///   runner this will ever meet.
///
/// # Why the added latency and not the recorded inter-arrival
///
/// Because the first Windows run of this file measured the producer. A recorded
/// inter-arrival is the sum of how regularly the events were produced and how
/// promptly they were stamped, and Windows' default timer resolution is about
/// 15.6 ms — so a producer asking for 8 ms overshoots and catches up, and the
/// distribution it generates is its own. Differencing against a timestamp the
/// producer read from the same clock removes that term exactly and leaves the
/// delay the capture loop adds, which is what R14's residual is. The
/// inter-arrival is still printed, because it is what a corpus would contain; it
/// is no longer what anything is asserted about.
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
        "jitter: inter-arrival mean {:.3} ms, standard deviation {:.3} ms, against \
         an emission period of {:.3} ms",
        stats.gap_mean_ns / 1e6,
        stats.gap_sd_ns / 1e6,
        PERIOD.as_secs_f64() * 1e3
    );

    let latency = client::input::Percentiles::of(&measured.latency_ns);
    let (latency_mean, latency_sd) = spread(&measured.latency_ns);
    println!(
        "jitter: added latency ms  min {:.3}  p50 {:.3}  p95 {:.3}  p99 {:.3}  \
         max {:.3}",
        ms(latency.min),
        ms(latency.p50),
        ms(latency.p95),
        ms(latency.p99),
        ms(latency.max)
    );
    println!(
        "jitter: added latency mean {:.3} ms, standard deviation {:.3} ms; the \
         slowest single pass of the capture loop was {:.3} ms",
        latency_mean / 1e6,
        latency_sd / 1e6,
        ms(measured.slowest_pass_ns)
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

    // The residual, bounded by the loop rather than by a stopwatch. An event
    // waits at most one pass before it is stamped; anything longer is the
    // capture loop falling behind the device, which is the one way this design
    // could write a delay into a corpus. Machine-independent, and red under
    // per-frame stamping, which holds every event in a frame until the frame
    // ends and therefore beyond the pass that drained it.
    assert!(
        latency.max <= measured.slowest_pass_ns,
        "an event waited {:.3} ms to be stamped while the slowest single pass of \
         the capture loop was {:.3} ms: the loop is falling behind the device, and \
         a corpus recorded like this carries the backlog as if it were the hand",
        ms(latency.max),
        ms(measured.slowest_pass_ns)
    );
}
