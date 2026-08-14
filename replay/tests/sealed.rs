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

use replay::SessionFacts;
use replay::keys::{KeyRegistry, KeyStatus, SigningKey};
use replay::manifest::{Build, Commitment, Manifest, MatchId, Pseudonym, SimCommit};
use replay::session::{Clock, Platform};
use replay::telemetry::{
    Control, Event as TelemetryEvent, Sample, SeatStream, Telemetry, TelemetryLog,
};
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

/// The sealed telemetry companion, as bytes, recorded on x86-64 Linux.
///
/// The replay below commits to this file's digest, so the two constants move
/// together and the regeneration tool prints the companion first.
const SEALED_TELEMETRY: &str = concat!(
    "4d4f4241544c4d59000163726f73732d706c6174666f726d210091eec4e020cff2276991",
    "afbba8a770781ead0f934359ed329b7d7cf0f76dc7110000019fd5e54400010100000000",
    "000000c35000000000000000090000000000000006000000000000000600000000000000",
    "000000000000000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000000000000000000000000000000000000000001010000000000",
    "0000c3500000000000000009000000000000000600000000000000060000000000000007",
    "000000000000000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000000000000000000000000000000000000000000000000000000",
    "00000000000000000000000000000000000000000093359cbfd595a1dee87dcd16e3bbcd",
    "4be14fcc2578247022788b612fb30e1211c2f7a40b5fef9513f5869ce429b5ad808c447b",
    "995ef1bc4fd2614a928750303397531628be112a17411ad3b997ae5f32b85398fcf434ba",
    "11a8dda59127880b000000000000008954403ff0000000000000bff00000000000000100",
    "000000009c2ac700010000000000000000000000000000020000000000baaf4700000000",
    "00000000000000000000000000000000000134c1473fe0000000000000bfd00000000000",
    "000200000000015345c700000010000000010000000000000000000000000001cd57c7c0",
    "080000000000000000000000000000010000000001e02e4e020000000000000000000000",
    "00000000020000000001feb2ce0000002000000002000000000000000000000000000278",
    "c4ce0010000000000000801000000000000002000000000297494e000000300000000300",
    "00000000000000000000000003115b4e7e37e43c8800759c81a56e1fc2f8f35901000000",
    "00032431d50401000000000000000000000000000002000000000342b655000000400000",
    "00040000000000000000000000000003bcc8550000000000000000000000000000000002",
    "0000000003db4cd500000050000000050000000000000000000000000000d59f803ff000",
    "0000000000bff0000000000000010000000000e876070001000000000000000000000000",
    "000002000000000106fa8700000000000000000000000000000000000000000001810c87",
    "3fe0000000000000bfd00000000000000200000000019f91070000001000000001000000",
    "000000000000000000000219a307c0080000000000000000000000000000010000000002",
    "2c798e020000000000000000000000000000000200000000024afe0e0000002000000002",
    "0000000000000000000000000002c5100e00100000000000008010000000000000020000",
    "000002e3948e000000300000000300000000000000000000000000035da68e7e37e43c88",
    "00759c81a56e1fc2f8f359010000000003707d1504010000000000000000000000000000",
    "0200000000038f0195000000400000000400000000000000000000000000040913950000",
    "000000000000000000000000000002000000000427981500000050000000050000000000",
    "000000",
);

/// The sealed replay, as bytes, recorded on x86-64 Linux.
///
/// Its manifest commits to the digest of `SEALED_TELEMETRY` above, which is why
/// the two constants are regenerated together and in that order.
const SEALED: &str = concat!(
    "4d4f424152504c59000263726f73732d706c6174666f726d210091eec4e020cff2276991",
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
    "2a04f2586e801d1501403070531fa15624388f6660980ab2bcd69893c528d052b809010f",
    "3346036e482bd6efc1f36bccbe179ba39a703f614e26e41ebdd804bce5641767b50b5dbb",
    "68f5d38b4a1c480146b3f073f7b6ff3cb36274740f1e1ad48d4c906faa320d6405000000",
    "000000003c0000000000000000000000019fd5e544000000019fd5e54407000000000000",
    "0000000000000000000000040000019fd5e544000000019fd5e5440701ffd80000000000",
    "000000000100000000080000019fd5e544210000019fd5e54428027fffffff8000000000",
    "00000100000000020000019fd5e544210000019fd5e54428030006000000000000000000",
    "0100000000030000019fd5e544210000019fd5e5442804000b0000000000000000001000",
    "000001000000019fd5e546100000019fd5e5461701ffd800000000249200000010000000",
    "01040000019fd5e546100000019fd5e54617027fffffff80000000000000110000000108",
    "0000019fd5e546310000019fd5e546380300060000000000000000001100000001020000",
    "019fd5e546310000019fd5e5463804000a0000000000000000001100000001030000019f",
    "d5e546310000019fd5e546380000000000000000000000002000000002000000019fd5e5",
    "48200000019fd5e54827027fffffff800000000000002000000002040000019fd5e54820",
    "0000019fd5e548270300060000000000000000002100000002080000019fd5e548410000",
    "019fd5e5484804000a0000000000000000002100000002020000019fd5e548410000019f",
    "d5e548480000000000000000000000002100000002030000019fd5e548410000019fd5e5",
    "484801ffd80000000049240000003000000003000000019fd5e54a300000019fd5e54a37",
    "0300060000000000000000003000000003040000019fd5e54a300000019fd5e54a370400",
    "0a0000000000000000003100000003080000019fd5e54a510000019fd5e54a5800000000",
    "00000000000000003100000003020000019fd5e54a510000019fd5e54a5801ffd8000000",
    "006db60000003100000003030000019fd5e54a510000019fd5e54a58027fffffff800000",
    "000000004000000004000000019fd5e54c400000019fd5e54c4704000a00000000000000",
    "00004000000004040000019fd5e54c400000019fd5e54c47000000000000000000000000",
    "4100000004080000019fd5e54c610000019fd5e54c6801ffd80000000092490000004100",
    "000004020000019fd5e54c610000019fd5e54c68027fffffff8000000000000041000000",
    "04030000019fd5e54c610000019fd5e54c68030006000000000000000000500000000500",
    "0000019fd5e54e500000019fd5e54e570000000000000000000000005000000005040000",
    "019fd5e54e500000019fd5e54e5701ffd800000000b6db0000005100000005080000019f",
    "d5e54e710000019fd5e54e78027fffffff800000000000005100000005020000019fd5e5",
    "4e710000019fd5e54e780300060000000000000000005100000005030000019fd5e54e71",
    "0000019fd5e54e7804000a0000000000000000006000000006000000019fd5e550600000",
    "019fd5e5506701ffd800000000db6d0000006000000006040000019fd5e550600000019f",
    "d5e55067027fffffff800000000000006100000006080000019fd5e550810000019fd5e5",
    "50880300060000000000000000006100000006020000019fd5e550810000019fd5e55088",
    "04000b0000000000000000006100000006030000019fd5e550810000019fd5e550880000",
    "000000000000000000007000000007000000019fd5e552700000019fd5e55277027fffff",
    "ff800000000000007000000007040000019fd5e552700000019fd5e55277030006000000",
    "0000000000007100000007080000019fd5e552910000019fd5e5529804000a0000000000",
    "000000007100000007020000019fd5e552910000019fd5e5529800000000000000000000",
    "00007100000007030000019fd5e552910000019fd5e5529801ffd8000000010000000000",
    "8000000008000000019fd5e554800000019fd5e554870300060000000000000000008000",
    "000008040000019fd5e554800000019fd5e5548704000a00000000000000000081000000",
    "08080000019fd5e554a10000019fd5e554a8000000000000000000000000810000000802",
    "0000019fd5e554a10000019fd5e554a801ffd80000000124920000008100000008030000",
    "019fd5e554a10000019fd5e554a8027fffffff800000000000009000000009000000019f",
    "d5e556900000019fd5e5569704000a0000000000000000009000000009040000019fd5e5",
    "56900000019fd5e556970000000000000000000000009100000009080000019fd5e556b1",
    "0000019fd5e556b801ffd80000000149240000009100000009020000019fd5e556b10000",
    "019fd5e556b8027fffffff800000000000009100000009030000019fd5e556b10000019f",
    "d5e556b8030006000000000000000000a00000000a000000019fd5e558a00000019fd5e5",
    "58a7000000000000000000000000a00000000a040000019fd5e558a00000019fd5e558a7",
    "01ffd8000000016db6000000a10000000a080000019fd5e558c10000019fd5e558c8027f",
    "ffffff80000000000000a10000000a020000019fd5e558c10000019fd5e558c803000600",
    "0000000000000000a10000000a030000019fd5e558c10000019fd5e558c804000a000000",
    "000000000000b00000000b000000019fd5e55ab00000019fd5e55ab701ffd80000000192",
    "49000000b00000000b040000019fd5e55ab00000019fd5e55ab7027fffffff8000000000",
    "0000b10000000b080000019fd5e55ad10000019fd5e55ad8030006000000000000000000",
    "b10000000b020000019fd5e55ad10000019fd5e55ad804000a000000000000000000b100",
    "00000b030000019fd5e55ad10000019fd5e55ad8000000000000000000",
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
        telemetry: Commitment::Sealed(telemetry_here().digest()),
    }
}

/// The companion fixture's streams: two seats, all three record kinds, and
/// motions chosen for the `f64` encoding rather than for the game.
///
/// The seats are 0 and 5 because those are the two the manifest names
/// participants in, and `replay::telemetry::verify` refuses a companion whose
/// coverage disagrees with them — so this fixture exercises that agreement as
/// well as the bytes.
///
/// The deltas are the point. `1.0` and `-1.0` are what a mouse mostly reports;
/// `0.5` and `-0.25` are what a fractional unaccelerated backend reports;
/// `f64::MIN_POSITIVE` and `1.0e300` are the ends of the type, and they are here
/// because the encoding is `to_bits` and a fixture of whole numbers would pin the
/// easy half of it on every platform and the hard half on none.
fn telemetry_log() -> TelemetryLog {
    let mut log = TelemetryLog::new();
    let stream = |seat: usize| -> SeatStream {
        let mut samples = Vec::new();
        let mut at_ns = 1_000_000u64.saturating_mul(seat as u64 + 1);
        let deltas = [
            (1.0f64, -1.0f64),
            (0.5, -0.25),
            (-3.0, 0.0),
            (f64::MIN_POSITIVE, -f64::MIN_POSITIVE),
            (1.0e300, -1.0e-300),
            (0.0, 0.0),
        ];
        for (index, (dx, dy)) in deltas.into_iter().enumerate() {
            at_ns = at_ns.saturating_add(8_000_000);
            samples.push(Sample {
                at_ns,
                event: TelemetryEvent::Moved { dx, dy },
            });
            if index % 2 == 0 {
                at_ns = at_ns.saturating_add(1_234_567);
                samples.push(Sample {
                    at_ns,
                    event: TelemetryEvent::Pressed {
                        control: [
                            Control::Move,
                            Control::Attack,
                            Control::Skillshot,
                            Control::Targeted,
                            Control::Stop,
                        ][index % 5],
                        down: index % 4 == 0,
                    },
                });
            }
            at_ns = at_ns.saturating_add(2_000_000);
            samples.push(Sample {
                at_ns,
                event: TelemetryEvent::Viewed {
                    tick: Tick(index as u32 * 16),
                    seq: index as u32,
                },
            });
        }
        SeatStream {
            clock: Clock::Dequeue,
            platform: Platform::Linux,
            world_units_per_count_e6: 50_000,
            // Non-zero on one seat, so the field is exercised rather than
            // encoded as zero on every platform (`docs/RISKS.md` R15).
            dropped: if seat == 5 { 7 } else { 0 },
            samples,
        }
    };
    log.seats[0] = Some(stream(0));
    log.seats[5] = Some(stream(5));
    log
}

/// The companion, sealed on whatever platform is running this.
///
/// `Platform::Linux` is written into it as a *constant of the fixture* rather
/// than read from `cfg!(target_os)`: the claim under test is that three targets
/// produce the same bytes from the same values, and a field that changed with the
/// target would make the fixture disagree with itself for a reason that is not
/// the encoding.
fn telemetry_here() -> Telemetry {
    replay::telemetry::seal(
        &telemetry_log(),
        &session_facts(),
        &SigningKey::from_seed(FIXTURE_SEED),
    )
}

/// The session facts both artefacts are sealed against.
fn session_facts() -> SessionFacts {
    SessionFacts {
        match_id: MatchId(*b"cross-platform!\0"),
        started_at_unix_ms: 1_786_000_000_000,
        participants: [const { None }; PLAYER_COUNT],
        sim_commit: SimCommit::Unknown,
        telemetry: Commitment::Absent,
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

/// **The companion's half of the criterion.** The committed bytes are what this
/// platform produces, and they verify here against the replay that names them.
///
/// A companion is a file, and the three places a platform can differ are the
/// same three the replay's fixture exists for: the manifest's encoding, the
/// stream's encoding and the signature over them. The one that is new is the
/// stream, and it is the one worth having a fixture for — an `f64` pair per
/// record, written by `to_bits`, is exactly specified by IEEE-754 and therefore
/// *ought* to be identical everywhere, which is the shape of claim
/// `docs/RISKS.md` R1's negative control exists to distrust.
#[test]
fn a_companion_sealed_on_one_platform_is_the_same_bytes_and_verifies_on_this_one() {
    let committed = unhex(SEALED_TELEMETRY).expect("the committed companion is not hex");
    let here = telemetry_here().encode();

    assert_eq!(
        hex(&here),
        hex(&committed),
        "this platform seals the same device stream into different bytes than the \
         committed fixture. A corpus recorded on one machine could not then be read \
         on another, which is the whole of what a sealed file is for"
    );

    let telemetry = Telemetry::decode(&committed).expect("the committed companion did not decode");
    let replay = Replay::decode(&unhex(SEALED).expect("hex")).expect("decode");
    let verified = replay::telemetry::verify(&replay, &telemetry, &registry())
        .expect("the committed companion did not verify here");

    assert_eq!(verified.match_id, replay.manifest.match_id);
    assert_eq!(
        replay.manifest.telemetry,
        Commitment::Sealed(telemetry.digest()),
        "the replay does not commit to the companion beside it"
    );

    println!(
        "sealed: telemetry {} bytes, {} device event(s), {} motion(s), {} seat(s), \
         digest {}",
        committed.len(),
        verified.samples,
        verified.motions,
        telemetry.manifest.occupied().len(),
        telemetry.digest()
    );
}

/// The companion fixture exercises the format it is a fixture for.
///
/// `docs/RISKS.md` R15, pointed at the one encoding in this repository whose
/// hard cases are `f64` bit patterns. A stream of `(1.0, -1.0)` motions would pin
/// the easy half on three platforms and the ends of the type on none, and every
/// claim above would be about a file that had never met a subnormal.
#[test]
fn the_companion_fixture_exercises_the_format_it_is_a_fixture_for() {
    let telemetry = Telemetry::decode(&unhex(SEALED_TELEMETRY).expect("hex")).expect("decode");

    let mut moves = 0u32;
    let mut presses = 0u32;
    let mut views = 0u32;
    let mut controls: Vec<u8> = Vec::new();
    let mut extreme = 0u32;
    let mut fractional = 0u32;
    let mut negative = 0u32;

    for stream in telemetry.log.seats.iter().flatten() {
        for sample in &stream.samples {
            match sample.event {
                TelemetryEvent::Moved { dx, dy } => {
                    moves += 1;
                    if dx.abs() >= 1.0e300 || (dx != 0.0 && dx.abs() <= f64::MIN_POSITIVE) {
                        extreme += 1;
                    }
                    if dx.fract() != 0.0 || dy.fract() != 0.0 {
                        fractional += 1;
                    }
                    if dx < 0.0 || dy < 0.0 {
                        negative += 1;
                    }
                }
                TelemetryEvent::Pressed { control, .. } => {
                    presses += 1;
                    if !controls.contains(&control.tag()) {
                        controls.push(control.tag());
                    }
                }
                TelemetryEvent::Viewed { .. } => views += 1,
            }
        }
    }

    println!(
        "sealed companion: {moves} motion(s) ({extreme} at the ends of the type, \
         {fractional} fractional, {negative} negative), {presses} press(es) over \
         {} control(s), {views} view anchor(s), {} seat(s)",
        controls.len(),
        telemetry.manifest.occupied().len()
    );

    assert!(moves > 0, "the fixture holds no motion");
    assert!(presses > 0, "the fixture holds no control transition");
    assert!(
        views > 0,
        "the fixture holds no view anchor, so the one record kind that is not a \
         device event is encoded on no platform"
    );
    assert!(
        extreme > 0,
        "no motion in the fixture is near the ends of the f64 domain, so the \
         encoding is pinned only where every platform agrees anyway"
    );
    assert!(
        fractional > 0,
        "every delta in the fixture is a whole number"
    );
    assert!(negative > 0, "no delta in the fixture is negative");
    assert!(
        telemetry
            .manifest
            .seats
            .iter()
            .flatten()
            .any(|seat| seat.dropped > 0),
        "no seat in the fixture dropped anything, so the field is zero on every \
         platform and nothing about it is checked"
    );
    assert_eq!(
        telemetry.manifest.occupied(),
        vec![0, 5],
        "the companion covers seats the replay does not name participants in"
    );
}

/// A companion is refused for the replay it is not the companion of.
///
/// The substitution `replay::manifest::Commitment` exists to refuse, executed
/// against the committed bytes rather than against a constructed pair. The
/// attacker here holds the fixture's own key — they are the strongest attacker
/// this format admits — and what refuses them is that the replay named
/// thirty-two other bytes before they arrived.
#[test]
fn a_companion_is_refused_for_the_replay_that_did_not_name_it() {
    let replay = Replay::decode(&unhex(SEALED).expect("hex")).expect("decode");
    let mut log = telemetry_log();
    // One motion, smoothed. Everything else about the file is honest, it is
    // sealed by a key the registry accepts, it names this match and these seats.
    if let Some(stream) = log.seats[0].as_mut()
        && let Some(sample) = stream
            .samples
            .iter_mut()
            .find(|sample| matches!(sample.event, TelemetryEvent::Moved { .. }))
    {
        sample.event = TelemetryEvent::Moved {
            dx: 1.0,
            dy: -1.0001,
        };
    }
    let smoothed =
        replay::telemetry::seal(&log, &session_facts(), &SigningKey::from_seed(FIXTURE_SEED));

    let error = replay::telemetry::verify(&replay, &smoothed, &registry())
        .expect_err("a companion the replay never named was accepted");
    assert!(
        matches!(error, replay::TelemetryError::Substituted { .. }),
        "refused for the wrong reason: {error:?}"
    );
    println!("sealed: {error}");
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
    let print = |name: &str, bytes: &[u8]| {
        println!("const {name}: &str = concat!(");
        let text = hex(bytes);
        for chunk in text.as_bytes().chunks(72) {
            println!("    \"{}\",", String::from_utf8_lossy(chunk));
        }
        println!(");");
    };
    // The companion first, because the replay's manifest commits to its digest:
    // regenerating them in the other order would print a replay that names a
    // companion nobody has yet.
    print("SEALED_TELEMETRY", &telemetry_here().encode());
    print("SEALED", &bytes);
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
