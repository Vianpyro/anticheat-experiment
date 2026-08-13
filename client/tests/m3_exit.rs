//! `docs/MILESTONES.md` M3's exit criterion, run end to end.
//!
//! > Three headless clients join one match, run 1000 ticks of scripted input,
//! > and (a) all three report identical digests of their reconciled local view
//! > at every checkpoint tick, (b) the server's authoritative digest matches an
//! > offline resimulation of the recorded input log, run as a separate process.
//!
//! Both halves are here, over the real transport: a `quinn` endpoint, three QUIC
//! sessions, fixed-size frames on a stream, and at the end a `resim` process
//! booted from the recording alone.
//!
//! # What each half actually proves, and what it does not
//!
//! **(a) is a statement about the projection and the handle space, not about
//! the network.** The three clients occupy one team, and vision is a team
//! property, so they are *entitled* to the same world; their local worlds
//! agreeing is evidence that the per-recipient handle spaces stayed in step and
//! that nothing per-player leaked into what they were told. It would not catch a
//! leak the whole team receives — for that, `sim/tests/view_properties.rs` and
//! `server/tests/traffic.rs` are the coverage, and M7's exploit is the proof.
//!
//! **(b) proves the server did not corrupt itself.** `docs/SCOPE.md` is explicit
//! that resimulating the server's own inputs against a fully authoritative
//! server catches a broken server and not a cheating client; the surfaces where
//! resimulation is evidence are the ones where a *client-supplied artifact*
//! asserts an outcome, and those are M5's. What it does establish here is that
//! the recording is complete and correctly ordered — an input dropped from the
//! log, or logged under the wrong tick, is a digest mismatch — which is the
//! precondition everything at M5 rests on.
//!
//! # The tick period is compressed and the invariant is not
//!
//! The match runs at a millisecond a tick rather than at 30 Hz, so the test
//! costs a second instead of thirty-three. The traffic-shape invariant is "a
//! constant number of bytes at a constant interval", and scaling the interval
//! uniformly leaves both halves of that exactly where they were. What the
//! compression does change is scheduling jitter relative to the period, which is
//! why the match is not required to be identical across runs: which tick an
//! input lands on depends on when it arrived, and the criterion is about the
//! three clients agreeing with each other and with the log, not about two runs
//! agreeing.

#![deny(unsafe_code)]

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use client::{Headless, net::Wire};
use server::{MatchConfig, net::Listener};
use sim::{Action, Digest, Fx, FxVec2, Seat, Tick};

/// M1's fixture length, and M3's criterion says the same number.
const TICKS: u32 = 1000;

/// Every hundredth tick is compared. Frequent enough that a divergence is
/// located rather than merely detected, and not so frequent that the assertion
/// is "the whole stream was identical", which would be a different and weaker
/// claim — it would pass on three clients that all received nothing.
const CHECKPOINT: u32 = 100;

/// One tick of the scripted input, for a seat.
///
/// Deliberately not the same for all three: a script that made the three
/// clients do the same thing would leave them in the same place, and three
/// champions standing on one point see the same world for a reason that has
/// nothing to do with the projection being right.
fn scripted(seat: Seat, tick: u32) -> Action {
    let lane = match seat {
        Seat::Blue0 => -8,
        Seat::Blue1 => 0,
        _ => 8,
    };
    if tick % 240 == 60 {
        // On cooldown, so projectiles exist and the per-recipient handle spaces
        // have something to name.
        Action::Skillshot(FxVec2::new(Fx::ONE, Fx::from_int(lane / 8)))
    } else if tick.is_multiple_of(120) {
        // Toward the enemy base. A thousand ticks at six units a second covers
        // the two hundred units between the spawn lines, so the three of them
        // walk out of their own half, meet the Red team standing at its spawn,
        // and take tower fire on the way — which is what puts entities into and
        // out of the fog rather than leaving every view empty.
        Action::Move(FxVec2::new(Fx::from_int(120), Fx::from_int(lane)))
    } else {
        Action::Idle
    }
}

/// One headless client, driven to the end of the match.
///
/// Returns the digest of its reconciled local world at every checkpoint tick.
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
    // Until the server finishes the stream, rather than until a tick count.
    // A client that stopped reading the moment it had what it came for would
    // leave the last frames unread, and the server — which waits for its peers
    // before closing, so that a clean shutdown is not a truncated one — would
    // spend its drain timeout on every run.
    loop {
        // One frame in, one intention out. The server ticks on its own clock
        // whatever this client does, so this cannot deadlock; what it does mean
        // is that a client which stalls simply misses ticks, which is the
        // behaviour a real one has.
        let Ok(frame) = wire.recv_state().await else {
            break;
        };
        headless
            .receive(&frame)
            .map_err(|error| error.to_string())?;

        let Tick(tick) = headless.world().tick();
        if tick.is_multiple_of(CHECKPOINT) {
            checkpoints.insert(tick, headless.world().digest());
        }

        let action = scripted(seat, tick);
        if wire.send(&headless.intend(action, 0)).await.is_err() {
            break;
        }
    }

    // Not an error any more, and the change is the transport's. Views arrive as
    // datagrams, which QUIC neither retransmits nor orders, so a view older
    // than the one already applied is a reordering rather than a bug upstream —
    // and a frame that never completes is a tick this client did not see. Both
    // are reported and neither is fatal; what the criterion asserts is below.
    let (incomplete, late_shards) = wire.losses();
    Ok(Report {
        seat,
        checkpoints,
        stale_views: headless.stale(),
        incomplete_frames: incomplete,
        late_shards,
    })
}

/// What one client came back with.
#[derive(Debug)]
struct Report {
    seat: Seat,
    checkpoints: BTreeMap<u32, Digest>,
    /// Views discarded for not being newer than what the client already held.
    stale_views: u32,
    /// Frames abandoned because one of their shards never arrived.
    incomplete_frames: u32,
    /// Shards that arrived after a newer frame had started.
    late_shards: u32,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn three_headless_clients_agree_and_the_log_resimulates() {
    let listener = Listener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).expect("bind");
    let address = listener.local_addr().expect("local address");
    let certificate = listener.certificate().to_vec();

    let hosting = tokio::spawn(listener.host(
        MatchConfig {
            seed: 0x00C0_FFEE_0D15_EA5E,
            players: 3,
        },
        Duration::from_millis(1),
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

    // The three clients are one team, which is what makes them comparable.
    reports.sort_by_key(|report| report.seat.index());
    let seats: Vec<Seat> = reports.iter().map(|report| report.seat).collect();
    assert_eq!(seats, vec![Seat::Blue0, Seat::Blue1, Seat::Blue2]);
    for report in &reports {
        println!(
            "{:?}: {} checkpoints, {} frames lost, {} shards late, {} views stale",
            report.seat,
            report.checkpoints.len(),
            report.incomplete_frames,
            report.late_shards,
            report.stale_views
        );
    }

    // ---------------------------------------------------------------
    // (a) identical digests at every checkpoint both clients received
    // ---------------------------------------------------------------
    //
    // "Every checkpoint tick" is what M3's criterion said, and it said it about
    // a transport that retransmitted. State travels in datagrams now, so a
    // client can legitimately miss a tick, and requiring all three to hold every
    // checkpoint would be requiring the loss rate to be zero — a property of the
    // loopback rather than of this project. What is asserted instead is the
    // claim the criterion was making: **no two clients ever disagree**, and the
    // channel delivered essentially everything. `docs/MILESTONES.md` records the
    // weakening and what it costs.
    let expected = (TICKS / CHECKPOINT) as usize;
    for report in &reports {
        assert!(
            report.checkpoints.len() * 10 >= expected * 9,
            "{:?} reached {} of {} checkpoints; the state channel is losing more \
             than a tenth of its frames, which is a transport failure rather than \
             the occasional dropped datagram this tolerates",
            report.seat,
            report.checkpoints.len(),
            expected
        );
    }

    let first = &reports[0];
    // Distinct digests, or "all three agreed" would be a statement about a
    // world that never changed.
    let distinct: std::collections::BTreeSet<&Digest> = first.checkpoints.values().collect();
    assert!(
        distinct.len() > 1,
        "every checkpoint of the run produced the same digest, so agreement \
         between the clients is not evidence of anything"
    );

    // Pairwise, on the checkpoints both members of the pair hold. Counted, so
    // that a run in which the three clients happened to share no checkpoint at
    // all cannot pass by comparing nothing.
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
            compared += 1;
        }
    }
    assert!(
        compared as usize >= expected * 3 / 2,
        "only {compared} checkpoint digests were compared across the three \
         clients, which is too few for their agreement to mean anything"
    );

    // ---------------------------------------------------------------
    // (b) the log resimulates, in another process
    // ---------------------------------------------------------------

    assert!(
        recording.ticks >= TICKS,
        "the server ran {} ticks, not {TICKS}",
        recording.ticks
    );
    assert!(
        !recording.inputs.is_empty(),
        "the recording holds no inputs, so resimulating it proves nothing"
    );

    let path = std::env::temp_dir().join(format!("moba-m3-{}.replay", std::process::id()));
    std::fs::write(&path, recording.encode()).expect("write the recording");

    let output = std::process::Command::new(resim_binary())
        .arg(&path)
        .output()
        .expect("run resim");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let _ = std::fs::remove_file(&path);

    assert!(
        output.status.success(),
        "resim refused the recording.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains(&hex(&recording.final_state_digest)),
        "resim did not report the server's own digest.\nstdout:\n{stdout}"
    );
}

/// The `resim` binary, built if it is not there yet.
///
/// Found beside the test binary rather than through `CARGO_BIN_EXE_`, which
/// Cargo only sets for a package's *own* binaries and `resim` belongs to
/// `replay`.
///
/// It is built on demand because the obvious assumption is wrong, and CI is
/// where that showed: `cargo test --workspace` does **not** build every binary
/// in the workspace. It builds what each package's test targets need, and no
/// test target needs `resim` — locally it existed only because a
/// `cargo build --all-targets` had happened to run first, which is the shape of
/// green that turns red on a clean checkout. Building it here makes the test
/// self-contained on any machine, and the nested invocation is safe because the
/// outer cargo is not holding the build lock while its tests run.
fn resim_binary() -> PathBuf {
    let mut path = std::env::current_exe().expect("the test binary has a path");
    path.pop(); // deps/
    path.pop(); // debug/ or release/
    path.push(if cfg!(windows) { "resim.exe" } else { "resim" });
    if path.exists() {
        return path;
    }

    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the client crate has a parent directory")
        .join("Cargo.toml");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let built = std::process::Command::new(cargo)
        .args(["build", "--locked", "-p", "replay", "--bin", "resim"])
        .arg("--manifest-path")
        .arg(&workspace)
        .status()
        .expect("run cargo to build resim");
    assert!(built.success(), "cargo could not build resim");
    assert!(
        path.exists(),
        "resim was built but is not at {}",
        path.display()
    );
    path
}

fn hex(digest: &Digest) -> String {
    digest
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
