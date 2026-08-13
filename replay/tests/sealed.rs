//! A replay sealed on one platform, read and resimulated on another.
//!
//! # The case this is, and why it had never been run
//!
//! Every digest in this repository was recorded on x86-64 Linux and the
//! `determinism` workflow checks the three targets against it, so "the same seed
//! and the same log reach the same state everywhere" has been evidence since M1.
//! What has never been checked is the layer M5 adds on top of that: **a file**.
//! A replay is a manifest, a signature and a log, and between `State::digest`
//! and the bytes on a disk there are three new places a platform can differ —
//! the manifest's encoding, the log's encoding, and the signature over them.
//!
//! The case that matters is a log recorded on one machine and verified on
//! another, because that is what a replay is *for*: somebody hands you a file.
//! It is also the case nothing in this project had ever exercised, and the
//! failure it would have would be silent in the worst direction — a verifier on
//! Windows reporting a digest mismatch on a perfectly honest replay from Linux,
//! in the one milestone whose subject is telling tampering apart from honest
//! disagreement.
//!
//! # How it is checked, and why a committed blob rather than a job comparison
//!
//! The bytes below were sealed on x86-64 Linux and are committed. Each target
//! rebuilds the same replay from the same script and the same key and requires
//! **byte equality with the constant** — which is exactly the argument
//! `sim/tests/determinism.rs` makes for golden digests over cross-job
//! comparison, one level up. It catches disagreement between platforms, because
//! all three compare against one constant; and it catches drift over time on a
//! single platform, which comparing three jobs to each other cannot see at all,
//! since three jobs that have all drifted the same way still agree.
//!
//! # The fixture is built for the format, not for the game
//!
//! Sixty inputs rather than a match's thousands, and every one of them chosen:
//! all five `Action` variants, several seats, coordinates at the ends of the
//! type, timestamps that use the width of a `u64`, and two participants in the
//! manifest. What is under test is the *encoding*, and a fixture of ten thousand
//! `Move` inputs would exercise one variant ten thousand times and four variants
//! never. [`the_fixture_exercises_the_format_it_is_a_fixture_for`] is the floor
//! that keeps that true (`docs/RISKS.md` R15).
//!
//! # Two fields are pinned, and the second pinning is a feature
//!
//! `sim_commit` is `Unknown`, because a committed fixture is not a build
//! artefact and a commit hash in it would be a hash of whichever commit somebody
//! regenerated it on.
//!
//! `sim_version` is `[0, 0, 0]`, which no build will ever have. That keeps the
//! blob stable across the `sim` version bumps `docs/RISKS.md` R13 requires of
//! every change to `sim/` — regenerating a committed file on every patch bump is
//! friction that ends with somebody weakening the check — and it turns the
//! pinning into a second assertion: verifying this fixture as *this* build must
//! fail with `SimVersion`, which is R13's whole mechanism demonstrated against a
//! real file rather than a constructed one. See
//! [`this_build_refuses_the_fixture_as_being_from_another_build`].
//!
//! # Regenerating
//!
//! ```sh
//! cargo test -p replay --test sealed -- --ignored --nocapture regenerate
//! ```
//!
//! and paste. Doing that is a deliberate act: the blob changes when the manifest
//! layout, the log layout or the rules change, and each of those makes every
//! replay recorded under the old bytes unverifiable (`docs/RISKS.md` R2).

#![deny(unsafe_code)]

use replay::keys::{KeyRegistry, KeyStatus, SigningKey};
use replay::manifest::{Build, Manifest, MatchId, Pseudonym, SimCommit};
use replay::{Recording, Replay, TimedInput, VerifyError};
use sim::{
    Action, EntityId, Fx, FxVec2, Input, PLAYER_COUNT, Seat, Tick, champion_entity_id,
    input_log_digest, new_state, rules_hash, step, tower_entity_id,
};

/// The key this fixture is sealed with. A written-down constant and not a
/// secret; a generated one would produce a different signature on every run and
/// there would be nothing to commit.
const FIXTURE_SEED: [u8; 32] = *b"moba cross-platform fixture key\0";

/// The version field pinned into the fixture. No build has it; see the header.
const FIXTURE_SIM_VERSION: [u16; 3] = [0, 0, 0];

/// Ticks the fixture spans.
const TICKS: u32 = 200;

/// The digest resimulating the fixture's log reaches.
const EXPECTED_DIGEST: &str = "1d8609661e2e7d4e04ec42d9e14c49fd8b8db043b0cf5e6a2a04f2586e801d15";

/// The sealed replay, as bytes, recorded on x86-64 Linux.
const SEALED: &str = concat!(
    "4d4f424152504c59000163726f73732d706c6174666f726d210091eec4e020cff2276991",
    "afbba8a770781ead0f934359ed329b7d7cf0f76dc7115ea1ed005ea1ed009b67d7fde443",
    "3a55334dd1702b8145d7885811ebb79604d5367274e1b3e9f16600000000000000000000",
    "00000000000000000000000000000000000000019fd5e5440008616c697a6172696e0000",
    "000000000000000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000662697374726500000000000000000000000000000000000000",
    "000000000000000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000000000000000000000000000000000000000000000000000000",
    "00c8000000000000003c307c7fd51be6ce7ff9a4f204fc94e978ceec3a7d4bbfddf59e16",
    "bf425b21bc580000000000001d8609661e2e7d4e04ec42d9e14c49fd8b8db043b0cf5e6a",
    "2a04f2586e801d15c78e7535e0eb22183132a2e9cf1a7f2acbcc0ada8d7a8416a55b8dc1",
    "4d5fe8ce8f5c3aad096205ac8037d0a41405c06bace883c21c737af7482a3d18598f2d08",
    "000000000000003c0000000000000000000000019fd5e544000000019fd5e54407000000",
    "0000000000000000000000000000040000019fd5e544000000019fd5e5440701ffd80000",
    "000000000000000100000000080000019fd5e544210000019fd5e54428027fffffff8000",
    "00000000000100000000020000019fd5e544210000019fd5e54428030006000000000000",
    "0000000100000000030000019fd5e544210000019fd5e5442804000b0000000000000000",
    "001000000001000000019fd5e546100000019fd5e5461701ffd800000000249200000010",
    "00000001040000019fd5e546100000019fd5e54617027fffffff80000000000000110000",
    "0001080000019fd5e546310000019fd5e546380300060000000000000000001100000001",
    "020000019fd5e546310000019fd5e5463804000a00000000000000000011000000010300",
    "00019fd5e546310000019fd5e54638000000000000000000000000200000000200000001",
    "9fd5e548200000019fd5e54827027fffffff800000000000002000000002040000019fd5",
    "e548200000019fd5e548270300060000000000000000002100000002080000019fd5e548",
    "410000019fd5e5484804000a0000000000000000002100000002020000019fd5e5484100",
    "00019fd5e548480000000000000000000000002100000002030000019fd5e54841000001",
    "9fd5e5484801ffd80000000049240000003000000003000000019fd5e54a300000019fd5",
    "e54a370300060000000000000000003000000003040000019fd5e54a300000019fd5e54a",
    "3704000a0000000000000000003100000003080000019fd5e54a510000019fd5e54a5800",
    "00000000000000000000003100000003020000019fd5e54a510000019fd5e54a5801ffd8",
    "000000006db60000003100000003030000019fd5e54a510000019fd5e54a58027fffffff",
    "800000000000004000000004000000019fd5e54c400000019fd5e54c4704000a00000000",
    "00000000004000000004040000019fd5e54c400000019fd5e54c47000000000000000000",
    "0000004100000004080000019fd5e54c610000019fd5e54c6801ffd80000000092490000",
    "004100000004020000019fd5e54c610000019fd5e54c68027fffffff8000000000000041",
    "00000004030000019fd5e54c610000019fd5e54c68030006000000000000000000500000",
    "0005000000019fd5e54e500000019fd5e54e570000000000000000000000005000000005",
    "040000019fd5e54e500000019fd5e54e5701ffd800000000b6db00000051000000050800",
    "00019fd5e54e710000019fd5e54e78027fffffff80000000000000510000000502000001",
    "9fd5e54e710000019fd5e54e780300060000000000000000005100000005030000019fd5",
    "e54e710000019fd5e54e7804000a0000000000000000006000000006000000019fd5e550",
    "600000019fd5e5506701ffd800000000db6d0000006000000006040000019fd5e5506000",
    "00019fd5e55067027fffffff800000000000006100000006080000019fd5e55081000001",
    "9fd5e550880300060000000000000000006100000006020000019fd5e550810000019fd5",
    "e5508804000b0000000000000000006100000006030000019fd5e550810000019fd5e550",
    "880000000000000000000000007000000007000000019fd5e552700000019fd5e5527702",
    "7fffffff800000000000007000000007040000019fd5e552700000019fd5e55277030006",
    "0000000000000000007100000007080000019fd5e552910000019fd5e5529804000a0000",
    "000000000000007100000007020000019fd5e552910000019fd5e5529800000000000000",
    "00000000007100000007030000019fd5e552910000019fd5e5529801ffd8000000010000",
    "0000008000000008000000019fd5e554800000019fd5e554870300060000000000000000",
    "008000000008040000019fd5e554800000019fd5e5548704000a00000000000000000081",
    "00000008080000019fd5e554a10000019fd5e554a8000000000000000000000000810000",
    "0008020000019fd5e554a10000019fd5e554a801ffd80000000124920000008100000008",
    "030000019fd5e554a10000019fd5e554a8027fffffff8000000000000090000000090000",
    "00019fd5e556900000019fd5e5569704000a000000000000000000900000000904000001",
    "9fd5e556900000019fd5e556970000000000000000000000009100000009080000019fd5",
    "e556b10000019fd5e556b801ffd80000000149240000009100000009020000019fd5e556",
    "b10000019fd5e556b8027fffffff800000000000009100000009030000019fd5e556b100",
    "00019fd5e556b8030006000000000000000000a00000000a000000019fd5e558a0000001",
    "9fd5e558a7000000000000000000000000a00000000a040000019fd5e558a00000019fd5",
    "e558a701ffd8000000016db6000000a10000000a080000019fd5e558c10000019fd5e558",
    "c8027fffffff80000000000000a10000000a020000019fd5e558c10000019fd5e558c803",
    "0006000000000000000000a10000000a030000019fd5e558c10000019fd5e558c804000a",
    "000000000000000000b00000000b000000019fd5e55ab00000019fd5e55ab701ffd80000",
    "00019249000000b00000000b040000019fd5e55ab00000019fd5e55ab7027fffffff8000",
    "0000000000b10000000b080000019fd5e55ad10000019fd5e55ad8030006000000000000",
    "000000b10000000b020000019fd5e55ad10000019fd5e55ad804000a0000000000000000",
    "00b10000000b030000019fd5e55ad10000019fd5e55ad8000000000000000000",
);

/// The log: sixty inputs chosen to exercise the encoding rather than the game.
///
/// Every `Action` variant, several seats, the ends of the coordinate type, and
/// timestamps wide enough that a `u64` is doing work. The ticks ascend so that
/// bucketing has something to bucket, and two inputs share a tick so that "more
/// than one input on a tick" is a shape the file has met.
fn log() -> Vec<TimedInput> {
    let mut inputs = Vec::new();
    let mut push = |tick: u32, seat: Seat, seq: u32, action: Action| {
        inputs.push(TimedInput {
            input: Input {
                tick: Tick(tick),
                seq,
                player: seat,
                action,
            },
            // Wide, and not equal to each other: the two clocks are separate
            // fields and a fixture that gave them the same value would not
            // notice an encoder that wrote one of them twice.
            claimed_at_ms: 1_786_000_000_000_u64.saturating_add(u64::from(tick) * 33),
            received_at_ms: 1_786_000_000_007_u64.saturating_add(u64::from(tick) * 33),
        });
    };

    let mut seq = [0u32; PLAYER_COUNT];
    for step_index in 0..12u32 {
        let tick = step_index.saturating_mul(16);
        for (offset, seat) in [
            Seat::Blue0,
            Seat::Red1,
            Seat::Green2,
            Seat::Blue2,
            Seat::Red0,
        ]
        .into_iter()
        .enumerate()
        {
            let index = seat.index();
            let action = match (step_index as usize + offset) % 5 {
                0 => Action::Idle,
                1 => Action::Move(FxVec2::new(
                    Fx::from_int(40).neg(),
                    Fx::from_ratio(i32::from(u8::try_from(step_index).unwrap_or(0)), 7),
                )),
                2 => Action::Skillshot(FxVec2::new(Fx::MAX, Fx::MIN)),
                3 => Action::Targeted(champion_entity_id(Seat::Green0)),
                _ => Action::Attack(tower_entity_id(usize::from(step_index % 6 == 0))),
            };
            // Two of the five share the tick they are issued on, so the file has
            // met a tick carrying more than one input.
            let at = if offset < 2 {
                tick
            } else {
                tick.saturating_add(1)
            };
            push(at, seat, seq[index], action);
            seq[index] = seq[index].saturating_add(1);
        }
    }
    inputs
}

/// The state that log reaches, and the recording that describes it.
fn recording() -> Recording {
    let inputs = log();
    let reached = replay::resimulate(SEED, TICKS, &inputs);
    Recording {
        seed: SEED,
        rules_hash: rules_hash(),
        ticks: TICKS,
        outcome: reached.outcome(),
        final_state_digest: reached.digest(),
        inputs,
    }
}

/// The fixture's seed. Arbitrary and frozen: it is part of the fixture.
const SEED: u64 = 0x5EA1_ED00_5EA1_ED00;

/// The fixture's manifest, built by hand so that the two pinned fields are
/// pinned rather than read from whichever build is running.
fn manifest() -> Manifest {
    let recording = recording();
    let key = SigningKey::from_seed(FIXTURE_SEED);
    let mut participants: [Option<Pseudonym>; PLAYER_COUNT] = [const { None }; PLAYER_COUNT];
    participants[0] = Pseudonym::parse("alizarin");
    participants[5] = Pseudonym::parse("bistre");

    let log: Vec<Input> = recording.inputs.iter().map(|timed| timed.input).collect();
    Manifest {
        match_id: MatchId(*b"cross-platform!\0"),
        server_identity: key.verifying(),
        seed: recording.seed,
        rules_hash: recording.rules_hash,
        sim_version: FIXTURE_SIM_VERSION,
        sim_commit: SimCommit::Unknown,
        started_at_unix_ms: 1_786_000_000_000,
        participants,
        ticks: recording.ticks,
        inputs: recording.inputs.len() as u64,
        input_log_digest: input_log_digest(&log),
        outcome: recording.outcome,
        final_state_digest: recording.final_state_digest,
    }
}

/// The fixture, sealed on whatever platform is running this.
fn sealed_here() -> Replay {
    let manifest = manifest();
    let key = SigningKey::from_seed(FIXTURE_SEED);
    Replay {
        signature: key.sign(&replay::signed_bytes(&manifest)),
        manifest,
        inputs: recording().inputs,
    }
}

/// The registry that accepts the fixture's key.
fn registry() -> KeyRegistry {
    let mut keys = KeyRegistry::new();
    keys.insert(
        SigningKey::from_seed(FIXTURE_SEED).verifying(),
        KeyStatus::Active,
        "cross-platform-fixture",
    );
    keys
}

/// The build the fixture was sealed for: this machine's constants, the
/// fixture's pinned version.
fn fixture_build() -> Build {
    Build {
        rules_hash: rules_hash(),
        sim_version: FIXTURE_SIM_VERSION,
    }
}

/// **The criterion.** The committed bytes are what this platform produces, and
/// they verify here.
#[test]
fn a_replay_sealed_on_one_platform_is_the_same_bytes_and_verifies_on_this_one() {
    let committed = unhex(SEALED).expect("the committed blob is not hex");
    let here = sealed_here().encode();

    assert_eq!(
        hex(&here),
        hex(&committed),
        "this platform seals the same match into different bytes than the \
         committed fixture. The manifest's encoding, the log's encoding or the \
         signature is a function of the platform, which means a replay recorded \
         on one machine cannot be verified on another"
    );

    // Read from the *committed* bytes rather than from what this platform just
    // built, because the claim is about a file somebody else produced.
    let replay = Replay::decode(&committed).expect("the committed replay did not decode");
    let verified = replay::verify(&replay, &registry(), &fixture_build())
        .expect("the committed replay did not verify here");

    assert_eq!(
        verified.final_state_digest.to_string(),
        EXPECTED_DIGEST,
        "resimulating a replay sealed elsewhere reached a different state here"
    );
    assert_eq!(verified.outcome, replay.manifest.outcome);
    assert_eq!(verified.ticks, TICKS);

    println!(
        "sealed: {} bytes, {} inputs, digest {}",
        committed.len(),
        replay.inputs.len(),
        verified.final_state_digest
    );
}

/// This build refuses the fixture as being from another build, which is
/// `docs/RISKS.md` R13's mechanism demonstrated against a real file.
///
/// The version pinned into the fixture is `0.0.0`, which no build has, so this
/// assertion is stable across every bump the `sim-version` job will ever demand
/// — and it is the reason the pinning is a feature rather than a workaround. A
/// verifier that read a replay from another build as a digest mismatch would be
/// reporting tampering where there was none, which is the confusion R13 exists
/// to remove.
#[test]
fn this_build_refuses_the_fixture_as_being_from_another_build() {
    let replay = Replay::decode(&unhex(SEALED).expect("hex")).expect("decode");
    let error = replay::verify(&replay, &registry(), &Build::current())
        .expect_err("this build accepted a replay sealed by sim 0.0.0");
    assert!(
        matches!(
            error,
            VerifyError::SimVersion {
                recorded: FIXTURE_SIM_VERSION,
                ..
            }
        ),
        "refused for the wrong reason: {error:?}"
    );
    println!("sealed: {error}");
}

/// The fixture exercises the format it is a fixture for.
///
/// `docs/RISKS.md` R15. Every claim above is about an encoding, and an encoding
/// is only checked on the shapes the fixture contains: a blob of a thousand
/// `Move` inputs would pin one variant a thousand times and four variants never,
/// and the first replay to carry a `Targeted` would be the first to find out
/// whether it survives a platform.
#[test]
fn the_fixture_exercises_the_format_it_is_a_fixture_for() {
    let replay = Replay::decode(&unhex(SEALED).expect("hex")).expect("decode");

    let mut idle = 0u32;
    let mut moves = 0u32;
    let mut skillshots = 0u32;
    let mut targeted = 0u32;
    let mut attacks = 0u32;
    let mut seats: Vec<usize> = Vec::new();
    let mut ticks_with_two = 0u32;
    let mut previous: Option<Tick> = None;

    for timed in &replay.inputs {
        match timed.input.action {
            Action::Idle => idle += 1,
            Action::Move(_) => moves += 1,
            Action::Skillshot(_) => skillshots += 1,
            Action::Targeted(EntityId(_)) => targeted += 1,
            Action::Attack(EntityId(_)) => attacks += 1,
        }
        if !seats.contains(&timed.input.player.index()) {
            seats.push(timed.input.player.index());
        }
        if previous == Some(timed.input.tick) {
            ticks_with_two += 1;
        }
        previous = Some(timed.input.tick);
        assert_ne!(
            timed.claimed_at_ms, timed.received_at_ms,
            "the two clocks hold the same value, so an encoder that wrote one of \
             them twice would pass"
        );
    }

    println!(
        "sealed fixture: {idle} idle, {moves} move, {skillshots} skillshot, \
         {targeted} targeted, {attacks} attack, {} seats, {ticks_with_two} ticks \
         carrying more than one input",
        seats.len()
    );

    for (what, count) in [
        ("Idle", idle),
        ("Move", moves),
        ("Skillshot", skillshots),
        ("Targeted", targeted),
        ("Attack", attacks),
    ] {
        assert!(count > 0, "no {what} input is in the fixture");
    }
    assert!(seats.len() >= 4, "only {} seats speak", seats.len());
    assert!(ticks_with_two > 0, "no tick carries more than one input");
    assert_eq!(
        replay.manifest.participants().len(),
        2,
        "the manifest names no participants, so the participant slots are \
         encoded as empty on every platform and nothing about them is checked"
    );
    assert!(
        matches!(replay.manifest.sim_commit, SimCommit::Unknown),
        "the fixture's commit is not pinned"
    );

    // …and the log reaches a state that is not the one it started in, or
    // "resimulating it reproduced the digest" is a claim about `new_state`.
    assert_ne!(
        replay.manifest.final_state_digest,
        new_state(SEED).digest(),
        "the fixture's log leaves the world where it found it"
    );
    let mut state = new_state(SEED);
    for _ in 0..TICKS {
        state = step(&state, &[]);
    }
    assert_ne!(
        replay.manifest.final_state_digest,
        state.digest(),
        "the fixture reaches the same state an empty log would, so its inputs are \
         doing nothing"
    );
}

/// Prints the constants at the top of this file. Ignored by default: a tool.
#[test]
#[ignore = "regeneration tool; see the module documentation"]
fn regenerate() {
    let replay = sealed_here();
    let bytes = replay.encode();
    println!(
        "const EXPECTED_DIGEST: &str = \"{}\";",
        replay.manifest.final_state_digest
    );
    println!("const SEALED: &str = concat!(");
    let text = hex(&bytes);
    for chunk in text.as_bytes().chunks(72) {
        println!("    \"{}\",", String::from_utf8_lossy(chunk));
    }
    println!(");");
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(text.get(at..at.checked_add(2)?)?, 16).ok())
        .collect()
}
