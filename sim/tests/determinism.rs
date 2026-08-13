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

mod fixture;

use fixture::{DUEL_RULES, DUEL_SEED, DUEL_TICKS, SEED, TICKS, duel_script, script};
use sim::{
    Action, Digest, EntityId, Fx, FxVec2, Input, Liveness, Outcome, PLAYER_COUNT, RULES, Rules,
    Seat, State, TOWER_COUNT, TOWER_ID_BASE, Tick, champion_entity_id, input_log_digest,
    new_state_with_rules, rules_hash, step_with_rules, tower_entity_id,
};

/// Ticks between two recorded checkpoints.
const CHECKPOINT_EVERY: u32 = 100;

/// The digest of the input log the script produces.
///
/// Pinned separately from the state digests so that a change to the *script*
/// fails with an unambiguous message, instead of presenting as a mysterious
/// simulation divergence.
const EXPECTED_INPUT_LOG: &str = "b6bc3bc325f6308a60a07a0d43c92a7c6c7fcc18e7858f2a724d1f69f93a4c0e";

/// The digest of [`RULES`]. A balance change lands here first.
const EXPECTED_RULES: &str = "9b67d7fde4433a55334dd1702b8145d7885811ebb79604d5367274e1b3e9f166";

/// `State::digest()` at tick 100, 200, … 1000.
const EXPECTED_CHECKPOINTS: [&str; 10] = [
    // tick 100
    "e2e4548fdabf0e9a59352915bcd4fda6fecbbaa56a12b19c6789f674c2fe1e59",
    // tick 200
    "c1b8cd2c47a9dcb7f7df8b4a8dbc8879fe4fe6a73596dceed5cb38ea1fa054c3",
    // tick 300
    "4c64d4bdbb38b27bcd20d9b7223c30228aba490366fcf91679a83253891f4198",
    // tick 400
    "b397b1b1078b237ba226f3bc4bc9b11e19870d052396a92bb6346d4c64e80fb9",
    // tick 500
    "6773b29ff24a62afd7b4b7cfbe0482f9f7ab0f3818019fb2821a4b11bf5ffad8",
    // tick 600
    "08836d7f31919203c0ca41defdb755d3f7b6cb776c059b9e1cd04922295441d5",
    // tick 700
    "b9b9f2fde68266758cf5a713b13ed536c6befc235e2f14f193f656a3fcab0f11",
    // tick 800
    "0db93d3272703ea2a438229e0254d7bbfe11cc32822bbedafb8bfb05728d32be",
    // tick 900
    "460d6d6d22f50fd99e2f360c585c997349c7e49c5f8f8936648f2ad11f3567dd",
    // tick 1000
    "f8ef206f75e90ff8bf35082287c71b73094409426bdb769178f9a13a1327406b",
];

/// The hash of [`DUEL_RULES`]. Distinct from `EXPECTED_RULES` by construction,
/// which is the point: it is what stops a digest recorded under the fixture's
/// constants from ever being read as one recorded under the game's.
const EXPECTED_DUEL_RULES: &str =
    "3fc32c37c1559d02a3d2b2262117bfc222e212e1ad04050f15ab8cfbbb747e7b";

/// `State::digest()` at the end of the duel fixture.
const EXPECTED_DUEL: &str = "33df0ab2df8c014ce577db75f38edd8014678160a8636549860dfe42bfa42277";

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

/// Whether a handle names an entity a rule can act on.
///
/// Champions occupy `0..PLAYER_COUNT` and towers `TOWER_ID_BASE..+TOWER_COUNT`;
/// everything else — including a projectile's handle, which no order may name —
/// is a byte an attacker chose. Written out from the two ranges rather than
/// asked of `sim`, because `State::resolve` is private and a test that called
/// it would be asking the rules whether the rules think a handle is valid.
fn names_something(handle: u16) -> bool {
    let champion = (handle as usize) < PLAYER_COUNT;
    let tower = handle >= TOWER_ID_BASE && ((handle - TOWER_ID_BASE) as usize) < TOWER_COUNT;
    champion || tower
}

/// The fixture exercises the rules its own documentation says it exercises.
///
/// `docs/RISKS.md` R15, and this one is a belt beside a brace rather than a
/// repair: the golden digests above already fail if the scripted match stops
/// reaching something, because reaching less produces different bytes. What
/// they cannot do is *say* what stopped being reached — a changed constant
/// presents as ten changed hex strings and a decision about whether to
/// regenerate them. This is the test that answers "regenerate what?", and it is
/// also the one that fails when the answer is "the fixture no longer fights".
///
/// The counts are printed and the floors are set below what the run reaches, in
/// the shape `sim/tests/visibility.rs` already uses: a floor at the observed
/// value is a floor that goes red on an innocent change, and a floor of zero is
/// not a floor.
#[test]
fn the_fixture_reaches_the_rules_its_documentation_claims() {
    let mut state = new_state_with_rules(SEED, &RULES);
    let mut casts = [0u32; 2];
    let mut damage = 0u32;
    let mut hostile_actions = 0u32;
    let mut clamped_destinations = 0u32;

    for inputs in &script() {
        for input in inputs {
            match input.action {
                // The hostile tail: a handle naming nothing, a zero direction,
                // a destination outside the world. Every one is a no-op or a
                // clamp, and every one is a path nothing else in the fixture
                // takes.
                Action::Move(destination)
                    if destination.x.abs() > RULES.map_half_extent
                        || destination.y.abs() > RULES.map_half_extent =>
                {
                    clamped_destinations += 1;
                    hostile_actions += 1;
                }
                Action::Skillshot(direction) if direction == FxVec2::ZERO => hostile_actions += 1,
                Action::Attack(EntityId(handle)) | Action::Targeted(EntityId(handle))
                    if !names_something(handle) =>
                {
                    hostile_actions += 1;
                }
                _ => {}
            }
        }
        state = step_with_rules(&state, inputs, &RULES);
        for event in state.events().iter() {
            match event.kind {
                sim::EventKind::Cast { ability, .. } => {
                    casts[usize::from(ability == sim::Ability::Targeted)] += 1;
                }
                sim::EventKind::Damage { .. } => damage += 1,
                sim::EventKind::Death { .. } => {}
            }
        }
    }

    let hurt_champions = state
        .champions()
        .iter()
        .filter(|champion| {
            matches!(champion.liveness, Liveness::Alive { hp } if hp < RULES.champion_max_hp)
        })
        .count();
    let hurt_towers = state
        .towers()
        .iter()
        .filter(|tower| tower.hp < RULES.tower_max_hp)
        .count();

    println!(
        "fixture reach: {} skillshots, {} targeted spells, {damage} damage events, \
         {hurt_champions} champions and {hurt_towers} towers below full health, \
         {hostile_actions} hostile inputs ({clamped_destinations} of them clamped)",
        casts[0], casts[1]
    );

    assert!(casts[0] > 0, "the fixture never casts a skillshot");
    // And **not** a floor on the targeted spell, because writing this test found
    // that there is nothing to put one under: the scripted match issues the
    // order about twenty-seven times and lands it **zero** times, because the
    // quarry it names is a lane away and the spell has a range. Its own
    // documentation said the fixture "exercises movement, both abilities, …";
    // that sentence was false for three milestones and is corrected where it was
    // written. The duel is what covers the targeted spell, and
    // `the_duel_reaches_the_abilities_the_scripted_match_cannot` is the floor.
    assert_eq!(
        casts[1], 0,
        "the scripted match has started landing targeted spells, which is a better \
         fixture than it was — move the floor here and update `tests/fixture`"
    );
    assert!(damage >= 20, "only {damage} damage events");
    assert!(
        hurt_champions >= 2,
        "only {hurt_champions} champions finished below full health, so nothing \
         reached a champion"
    );
    assert!(
        hurt_towers >= 2,
        "only {hurt_towers} towers finished below full health, so nothing ever \
         attacked a structure"
    );
    assert!(
        hostile_actions >= 5,
        "only {hostile_actions} of the fixture's inputs were the hostile tail its \
         documentation describes"
    );
    assert!(
        clamped_destinations > 0,
        "no destination outside the map was ever sent, so the clamp is untested here"
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

/// The duel lands both abilities, which is the half of the rules the scripted
/// match cannot reach.
///
/// The two fixtures are a division of labour and it was an accidental one until
/// this test named it. The scripted match sends its quarry a targeted spell from
/// a hundred and seventy-three units away and the spell has a range, so it lands
/// none at all; the duel's three defenders stand on top of their victim and land
/// them. Without a floor here, the targeted spell is a rule that no fixture on
/// any of the three platforms ever executes — which is `docs/RISKS.md` R15 in
/// its purest form, since the digests would go on agreeing perfectly about a
/// match that never casts it.
#[test]
fn the_duel_reaches_the_abilities_the_scripted_match_cannot() {
    let mut state = new_state_with_rules(DUEL_SEED, &DUEL_RULES);
    let mut casts = [0u32; 2];
    let mut damage = 0u32;
    for inputs in &duel_script() {
        state = step_with_rules(&state, inputs, &DUEL_RULES);
        for event in state.events().iter() {
            match event.kind {
                sim::EventKind::Cast { ability, .. } => {
                    casts[usize::from(ability == sim::Ability::Targeted)] += 1;
                }
                sim::EventKind::Damage { .. } => damage += 1,
                sim::EventKind::Death { .. } => {}
            }
        }
    }
    println!(
        "duel reach: {} skillshots, {} targeted spells, {damage} damage events",
        casts[0], casts[1]
    );
    assert!(casts[0] > 0, "the duel never casts a skillshot");
    assert!(
        casts[1] > 0,
        "the duel never lands a targeted spell, so no fixture in this repository \
         executes that rule on any platform"
    );
    assert!(damage >= 20, "only {damage} damage events");
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
        player: Seat::Blue0,
        action: Action::Move(FxVec2::new(Fx::from_int(40), Fx::from_int(7))),
    });
    assert_ne!(run(&tampered).0.digest(), baseline);
}

/// Entity handles are laid out as the rest of the workspace will assume.
#[test]
fn entity_handles_are_where_the_fixture_expects_them() {
    for seat in Seat::ALL {
        assert_eq!(champion_entity_id(seat), EntityId(seat.index() as u16));
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
