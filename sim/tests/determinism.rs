//! The 1000-tick fixture, and the exit criterion of `docs/MILESTONES.md` M1.
//!
//! # What this test is for
//!
//! A determinism bug found late invalidates every recorded replay, every human
//! match in the corpus, and the detector calibration derived from them — not
//! just the code. So this exists from the first simulation commit rather than
//! from the end of the milestone, and the `determinism` workflow runs it on
//! x86-64 Linux, x86-64 Windows and aarch64 macOS. The second architecture is
//! the point: it is what catches the leaks an x86-only matrix hides
//! (`docs/RISKS.md` R1).
//!
//! # Why golden constants rather than comparing jobs to each other
//!
//! The three platforms could have uploaded their digests and had a fourth job
//! compare them. Committing the expected values instead is strictly stronger
//! and much simpler: it catches disagreement *between* platforms — all three
//! compare against the same constant — and also drift *over time* on a single
//! platform, which cross-job comparison cannot see at all. A compiler upgrade
//! that perturbs the simulation shows up here as a failing test with a diff,
//! which is exactly the reviewable event `docs/ENGINEERING.md` asks a pinned
//! toolchain to produce.
//!
//! The checkpoints exist so that a failure says *when* the runs diverged.
//! A single final digest tells you only that they did.
//!
//! # Regenerating
//!
//! When a rule legitimately changes, the constants below change with it. Run:
//!
//! ```sh
//! cargo test -p sim --test determinism -- --ignored --nocapture regenerate
//! ```
//!
//! and paste its output. Doing that is a deliberate, reviewable act: the diff
//! shows a changed rules hash next to changed digests, which is the signal that
//! every replay recorded under the old numbers is now unverifiable
//! (`docs/RISKS.md` R2).

#![deny(clippy::float_arithmetic, unsafe_code)]

use sim::{
    Action, Digest, EntityId, Fx, FxVec2, Input, Liveness, Outcome, PLAYER_COUNT, PlayerId, RULES,
    Rng, Rules, State, TOWER_COUNT, Tick, champion_entity_id, input_log_digest,
    new_state_with_rules, rules_hash, step_with_rules, tower_entity_id,
};

/// The match seed. Arbitrary, and frozen: it is part of the fixture.
const SEED: u64 = 0x00C0_FFEE_0D15_EA5E;

/// The seed of the *script*, which is a separate generator from the match's.
/// Keeping them apart means a rule that starts drawing from the match RNG does
/// not silently rewrite the input log as well as the outcome.
const SCRIPT_SEED: u64 = 0x5EED_1234_5EED_4321;

/// Ticks in the fixture, per M1's exit criterion.
const TICKS: u32 = 1000;

/// Ticks between two recorded checkpoints.
const CHECKPOINT_EVERY: u32 = 100;

/// The digest of the input log the script produces.
///
/// Pinned separately from the state digests so that a change to the *script*
/// fails with an unambiguous message, instead of presenting as a mysterious
/// simulation divergence.
const EXPECTED_INPUT_LOG: &str = "0430e59cfbc35b63ee2289b63bcfa9b054a1f9cfc7f3c427a4860a67daabd54f";

/// The digest of [`RULES`]. A balance change lands here first.
const EXPECTED_RULES: &str = "d05b4a56a94c63c52e497dbe772b22d8a3260443207c18a2e1a2f1536f68b297";

/// `State::digest()` at tick 100, 200, … 1000.
const EXPECTED_CHECKPOINTS: [&str; 10] = [
    // tick 100
    "27da063e193d2e0f471b4e51bb1dcb9066e2c235a3b3fac41fe5ab39306e647b",
    // tick 200
    "f8066b437649ab8530382351a3e62218c75837bec0c61d614ae467f4b13b2ca4",
    // tick 300
    "f81210c00fbc8f829a4fd8d79c86a6c15de8c17ecac26ee72057be149a9e0b40",
    // tick 400
    "454a2ae7e38e5caccda33d83b337599a46f42f74f54782f54279c6e7b74ebf7d",
    // tick 500
    "66657b824c0c855637642c3934cc978e9b8c88588fd7f153b2515e1f20a47a0a",
    // tick 600
    "384d39f0e0ee09299e2d3231efd907bcd965b758342ff05b722b29b4621c70c8",
    // tick 700
    "17ff8e53968c7dfad9e9abcd65788bd1f3c96ed9a5e79b0cd291e66f2cf0bbea",
    // tick 800
    "0ac9be58a75ec7a39330e07e4e063b8e7691a3179d603a9288e3e790be7e4d3f",
    // tick 900
    "371b4faa8cccb4abdc6a8172aaf09458c8941a7bc37d6287d99bc6ff69a22c0c",
    // tick 1000
    "1205ef8639bf5a816adb1cc51595728339738363932dd24bf12b0c126bd5c547",
];

/// A scripted match: six players issuing plausible commands, plus the
/// occasional piece of nonsense a real client would never send and a
/// compromised one certainly would.
///
/// The number of draws per tick depends only on the tick and the seat, never on
/// the state, so the log is a function of `SCRIPT_SEED` alone. If it depended
/// on the state, a divergence in the simulation would rewrite the inputs too
/// and the fixture would stop being able to tell you which one broke.
fn script() -> Vec<Vec<Input>> {
    let mut rng = Rng::from_seed(SCRIPT_SEED);
    let mut seq = [0u32; PLAYER_COUNT];
    let mut log = Vec::with_capacity(TICKS as usize);

    for tick in 0..TICKS {
        let mut inputs = Vec::new();
        for (seat, next_seq) in seq.iter_mut().enumerate() {
            let roll = rng.below(1000);
            // Two draws are consumed unconditionally so that the generator's
            // position does not depend on which branch was taken.
            let a = rng.below(1000) as i32;
            let b = rng.below(1000) as i32;

            let toward_enemy = if seat < PLAYER_COUNT / 2 { 1 } else { -1 };
            let enemy_seat = (seat + PLAYER_COUNT / 2) % PLAYER_COUNT;
            let enemy_tower = if seat < PLAYER_COUNT / 2 { 2 } else { 0 };

            let action = if roll < 950 {
                // Most ticks carry no new command. Both halves of that matter:
                // a player issuing sixty orders a second is not play, and a
                // champion re-ordered every few ticks never travels far enough
                // to reach a tower — which would leave tower fire, death and
                // respawn exercised by unit tests on one platform and by this
                // fixture on none.
                continue;
            } else if roll < 964 {
                // Always toward the enemy base, never back toward one's own.
                // A destination drawn symmetrically averages out to standing
                // still, which is the same problem in a different costume.
                let x = toward_enemy * (a % 160 + 40);
                let y = b % 41 - 20;
                Action::Move(FxVec2::new(Fx::from_int(x), Fx::from_int(y)))
            } else if roll < 990 {
                if a % 5 < 1 {
                    Action::Attack(champion_entity_id(enemy_seat))
                } else {
                    Action::Attack(tower_entity_id(enemy_tower + (b as usize % 2)))
                }
            } else if roll < 995 {
                let y = b % 21 - 10;
                Action::Skillshot(FxVec2::new(
                    Fx::from_int(toward_enemy * 10),
                    Fx::from_int(y),
                ))
            } else if roll < 998 {
                Action::Targeted(champion_entity_id(enemy_seat))
            } else {
                // The hostile tail. Every one of these is a no-op or a clamp,
                // and every one of them is a path the fixture would otherwise
                // never take.
                match a % 4 {
                    0 => Action::Attack(EntityId(b as u16)),
                    1 => Action::Skillshot(FxVec2::ZERO),
                    2 => Action::Move(FxVec2::new(Fx::MAX, Fx::MIN)),
                    _ => Action::Targeted(EntityId(u16::MAX)),
                }
            };

            inputs.push(Input {
                tick: Tick(tick),
                seq: *next_seq,
                player: PlayerId(seat as u8),
                action,
            });
            *next_seq += 1;
        }
        log.push(inputs);
    }
    log
}

/// A second fixture, whose only job is to kill somebody.
///
/// The scripted match above exercises movement, both abilities, basic attacks,
/// tower fire, projectile collision, clamping and the hostile tail — but over
/// thirty-three seconds of six-way skirmishing it never quite finishes anyone
/// off, and death and respawn would then be verified by unit tests on one
/// platform and by nothing on three.
///
/// Seat 0 walks alone into the enemy half; the three defenders focus it and
/// their towers join in.
///
/// # Why this fixture carries its own rules
///
/// Under [`RULES`] a champion has 600 hit points and stays dead for fifteen
/// seconds, so a fixture that has to contain a death *and* the respawn that
/// follows it would have to run for minutes. The two ways out are to change the
/// game's constants until the test fits them, or to give the test constants of
/// its own. The first was tried and reverted: a `champion_max_hp` of 350 chosen
/// to make a fixture terminate is indistinguishable, six months later, from a
/// decision about how lethal the game is, and the balance of the game is not a
/// place to store test requirements.
///
/// So the frailty and the short respawn live here, next to the fixture that
/// needs them, and [`Rules::hash`] keeps the two sets of constants apart: the
/// digest below is recorded under [`DUEL_RULES`] and `EXPECTED_DUEL_RULES`
/// pins which constants that was. A change to either fails loudly. Every other
/// number is inherited from [`RULES`] through the update syntax, so this
/// fixture keeps testing the real map, the real speeds and the real damage,
/// and a new constant added to `Rules` reaches it without anyone updating it
/// here.
const DUEL_SEED: u64 = 0x0DEA_D0DE_0DEA_D0DE;
const DUEL_TICKS: u32 = 900;

/// The constants the duel is played under: [`RULES`], made lethal enough to
/// reach death and respawn inside thirty seconds.
const DUEL_RULES: Rules = Rules {
    champion_max_hp: Fx::from_int(350),
    // 5 seconds, against the game's 15.
    respawn_ticks: 150,
    ..RULES
};

/// The hash of [`DUEL_RULES`]. Distinct from `EXPECTED_RULES` by construction,
/// which is the point: it is what stops a digest recorded under the fixture's
/// constants from ever being read as one recorded under the game's.
const EXPECTED_DUEL_RULES: &str =
    "46c4c57095fbded8fdf70c9c32871d0fe0ff33bbadebc63f3c1eac6e7a548274";

/// `State::digest()` at the end of the duel fixture.
const EXPECTED_DUEL: &str = "f2a84bfe2b4111c24f8ef0a2c5f8bc489081c5d3b1c06990cac2feb3547d162b";

fn duel_script() -> Vec<Vec<Input>> {
    let mut seq = [0u32; PLAYER_COUNT];
    let mut log = Vec::with_capacity(DUEL_TICKS as usize);

    for tick in 0..DUEL_TICKS {
        let mut inputs = Vec::new();
        let mut issue = |seat: usize, action: Action, inputs: &mut Vec<Input>| {
            inputs.push(Input {
                tick: Tick(tick),
                seq: seq[seat],
                player: PlayerId(seat as u8),
                action,
            });
            seq[seat] += 1;
        };

        // Seat 0 keeps walking into the enemy base, including after it
        // respawns — which is what makes the second death, and therefore the
        // respawn timer, part of what this fixture checks.
        if tick.is_multiple_of(60) {
            issue(
                0,
                Action::Move(FxVec2::new(Fx::from_int(88), Fx::ZERO)),
                &mut inputs,
            );
        }
        // The three defenders hold their ground and attack it on sight.
        if tick == 0 {
            for seat in PLAYER_COUNT / 2..PLAYER_COUNT {
                issue(seat, Action::Attack(champion_entity_id(0)), &mut inputs);
            }
        }
        // Everything else they have, on cooldown.
        if tick % 240 == 30 {
            for seat in PLAYER_COUNT / 2..PLAYER_COUNT {
                issue(
                    seat,
                    Action::Skillshot(FxVec2::new(Fx::from_int(-10), Fx::ZERO)),
                    &mut inputs,
                );
            }
        }
        if tick % 360 == 60 {
            for seat in PLAYER_COUNT / 2..PLAYER_COUNT {
                issue(seat, Action::Targeted(champion_entity_id(0)), &mut inputs);
            }
        }

        log.push(inputs);
    }
    log
}

/// Runs a fixture from a given seed under given rules, returning the final
/// state and one digest per checkpoint.
///
/// The rules are a parameter here rather than an ambient constant so that no
/// fixture can record a digest without having said which constants it recorded
/// it under.
fn run_from(seed: u64, log: &[Vec<Input>], rules: &Rules) -> (State, Vec<(u32, Digest)>) {
    let mut state = new_state_with_rules(seed, rules);
    let mut checkpoints = Vec::new();
    for (index, inputs) in log.iter().enumerate() {
        state = step_with_rules(&state, inputs, rules);
        let tick = index as u32 + 1;
        if tick.is_multiple_of(CHECKPOINT_EVERY) {
            checkpoints.push((tick, state.digest()));
        }
    }
    (state, checkpoints)
}

fn run(log: &[Vec<Input>]) -> (State, Vec<(u32, Digest)>) {
    run_from(SEED, log, &RULES)
}

fn flat(log: &[Vec<Input>]) -> Vec<Input> {
    log.iter().flatten().copied().collect()
}

/// The exit criterion. Identical on every platform, or this fails.
#[test]
fn the_fixture_reaches_its_recorded_digests() {
    let log = script();
    let (state, checkpoints) = run(&log);

    assert_eq!(
        rules_hash().to_string(),
        EXPECTED_RULES,
        "the rules changed; every replay recorded under the old ones is now \
         unverifiable (docs/RISKS.md R2). Regenerate deliberately."
    );
    assert_eq!(
        input_log_digest(&flat(&log)).to_string(),
        EXPECTED_INPUT_LOG,
        "the input script changed, so the digests below are testing a \
         different match than the one they were recorded from"
    );

    for (index, (tick, digest)) in checkpoints.iter().enumerate() {
        assert_eq!(
            digest.to_string(),
            EXPECTED_CHECKPOINTS[index],
            "divergence first visible at tick {tick}"
        );
    }

    // Printed so that the CI job can put it in the run summary, which is what
    // makes a three-platform disagreement legible at a glance rather than
    // something you reconstruct from three failing logs.
    println!(
        "determinism: seed={SEED:#018x} ticks={TICKS} digest={}",
        state.digest()
    );
}

/// The fixture must still be a live match at the end.
///
/// If a balance change ever decides the match at tick 300, the last seven
/// checkpoints become the same frozen state with a different tick, and the
/// fixture quietly stops exercising the rules while still passing.
#[test]
fn the_fixture_is_still_being_played_at_the_last_tick() {
    let (state, checkpoints) = run(&script());
    assert_eq!(state.tick(), Tick(TICKS));
    assert_eq!(state.outcome(), Outcome::InProgress);

    let distinct: std::collections::BTreeSet<_> = checkpoints
        .iter()
        .map(|(_, digest)| digest.to_string())
        .collect();
    assert_eq!(distinct.len(), checkpoints.len(), "checkpoints repeat");
}

/// The duel fixture: same cross-platform claim, over the death and respawn
/// rules the scripted match never reaches.
#[test]
fn the_duel_reaches_its_recorded_digest() {
    assert_eq!(
        DUEL_RULES.hash().to_string(),
        EXPECTED_DUEL_RULES,
        "the duel's own constants changed, so the digest below was recorded \
         under different rules than the ones being run"
    );
    let (state, _) = run_from(DUEL_SEED, &duel_script(), &DUEL_RULES);
    assert_eq!(state.digest().to_string(), EXPECTED_DUEL);
    println!(
        "determinism: seed={DUEL_SEED:#018x} ticks={DUEL_TICKS} digest={}",
        state.digest()
    );
}

/// The two fixtures are played under two different sets of constants, and the
/// hash says so.
///
/// This is the assertion that makes the arrangement safe. `State::digest()`
/// covers the state and nothing else, so a state reached under the duel's
/// frailer champion is indistinguishable, as bytes, from one reached under the
/// game's — if the constants ever coincided, the fixture would silently stop
/// being the thing it claims to be. `rules_hash()` is what keeps them apart,
/// and it is the same value the replay manifest carries at M5 for the same
/// reason (`docs/RISKS.md` R2).
#[test]
fn the_two_fixtures_do_not_share_a_rules_hash() {
    assert_ne!(rules_hash(), DUEL_RULES.hash());
}

/// …and it has to actually kill somebody, and put them back on the map.
///
/// Without this the duel could quietly stop being lethal — a balance change, a
/// range that no longer reaches — and go on passing as a determinism check over
/// a fixture that exercises nothing the other one does not. The third assertion
/// is the one that matters most: a champion that respawns and then cannot move
/// is a broken respawn that a digest comparison alone would happily certify as
/// deterministic.
#[test]
fn the_duel_kills_and_respawns_its_victim() {
    let mut state = new_state_with_rules(DUEL_SEED, &DUEL_RULES);
    let mut deaths = 0u32;
    let mut respawns = 0u32;
    let mut was_dead = false;
    let mut position_after_respawn = None;
    let mut moved_after_respawn = false;

    for inputs in &duel_script() {
        state = step_with_rules(&state, inputs, &DUEL_RULES);
        let victim = state.champions()[0];
        let dead = matches!(victim.liveness, Liveness::Dead { .. });
        if dead && !was_dead {
            deaths += 1;
        }
        if !dead && was_dead {
            respawns += 1;
            position_after_respawn = Some(victim.position);
        }
        if let Some(spawned_at) = position_after_respawn
            && victim.position != spawned_at
        {
            moved_after_respawn = true;
        }
        was_dead = dead;
    }

    assert!(
        deaths >= 1,
        "seat 0 never died; the duel is not lethal any more"
    );
    assert!(respawns >= 1, "seat 0 never came back");
    assert!(moved_after_respawn, "seat 0 came back but could not move");
}

/// Two runs in one process agree. Cheap, and it separates "the simulation is
/// nondeterministic" from "the platforms disagree" when both are failing.
#[test]
fn two_runs_in_one_process_agree() {
    let log = script();
    let (first, first_checkpoints) = run(&log);
    let (second, second_checkpoints) = run(&log);
    assert_eq!(first.digest(), second.digest());
    assert_eq!(first_checkpoints, second_checkpoints);
}

/// The log itself is reproducible, independently of the simulation.
#[test]
fn the_script_is_reproducible() {
    assert_eq!(
        input_log_digest(&flat(&script())),
        input_log_digest(&flat(&script()))
    );
}

/// A single altered input must change the final digest.
///
/// Without this, "the digests match" would be consistent with a `step` that
/// ignores its inputs entirely — which is a passing determinism suite that
/// proves nothing at all.
#[test]
fn the_fixture_actually_depends_on_its_inputs() {
    let baseline = run(&script()).0.digest();
    let mut tampered = script();
    tampered[0].push(Input {
        tick: Tick(0),
        seq: u32::MAX,
        player: PlayerId(0),
        action: Action::Move(FxVec2::new(Fx::from_int(40), Fx::from_int(7))),
    });
    assert_ne!(run(&tampered).0.digest(), baseline);
}

/// Entity handles are laid out as the rest of the workspace will assume.
#[test]
fn entity_handles_are_where_the_fixture_expects_them() {
    for seat in 0..PLAYER_COUNT {
        assert_eq!(champion_entity_id(seat), EntityId(seat as u16));
    }
    for index in 0..TOWER_COUNT {
        assert_eq!(tower_entity_id(index), EntityId(10 + index as u16));
    }
    assert!(RULES.map_half_extent > Fx::ZERO);
}

/// Prints the constants at the top of this file, for when a rule legitimately
/// changes. Ignored by default: it is a tool, not a test.
#[test]
#[ignore = "regeneration tool; see the module documentation"]
fn regenerate_golden_digests() {
    let log = script();
    let (_, checkpoints) = run(&log);
    println!(
        "const EXPECTED_INPUT_LOG: &str = \"{}\";",
        input_log_digest(&flat(&log))
    );
    println!("const EXPECTED_RULES: &str = \"{}\";", rules_hash());
    println!(
        "const EXPECTED_CHECKPOINTS: [&str; {}] = [",
        checkpoints.len()
    );
    for (tick, digest) in &checkpoints {
        println!("    // tick {tick}");
        println!("    \"{digest}\",");
    }
    println!("];");
    println!(
        "const EXPECTED_DUEL_RULES: &str = \"{}\";",
        DUEL_RULES.hash()
    );
    let (duel, _) = run_from(DUEL_SEED, &duel_script(), &DUEL_RULES);
    println!("const EXPECTED_DUEL: &str = \"{}\";", duel.digest());
}
