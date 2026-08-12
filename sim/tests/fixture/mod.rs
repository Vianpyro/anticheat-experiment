//! The two fixtures, shared by the test binaries that run them.
//!
//! `sim/tests/determinism.rs` compares their digests against constants
//! committed in the repository; `sim/tests/visibility.rs` runs the same matches
//! and projects every tick of them through the visibility rules. They have to
//! be the *same* matches — `docs/MILESTONES.md` M2 says "across the M1
//! fixture", and a culling test over a script of its own would be a culling
//! test over a world nobody has ever checked the determinism of.

// Each test binary compiles this module and uses part of it. `dead_code` fires
// on the rest, which is an artefact of how Cargo builds integration tests
// rather than anything about the code.
#![allow(dead_code, reason = "each test binary uses a subset of the fixtures")]

use sim::{
    Action, EntityId, Fx, FxVec2, Input, PLAYER_COUNT, PlayerId, RULES, Rng, Rules, Tick,
    champion_entity_id, tower_entity_id,
};

/// The match seed. Arbitrary, and frozen: it is part of the fixture.
pub const SEED: u64 = 0x00C0_FFEE_0D15_EA5E;

/// The seed of the *script*, which is a separate generator from the match's.
/// Keeping them apart means a rule that starts drawing from the match RNG does
/// not silently rewrite the input log as well as the outcome.
const SCRIPT_SEED: u64 = 0x5EED_1234_5EED_4321;

/// Ticks in the fixture, per M1's exit criterion.
pub const TICKS: u32 = 1000;

/// A scripted match: six players issuing plausible commands, plus the
/// occasional piece of nonsense a real client would never send and a
/// compromised one certainly would.
///
/// The number of draws per tick depends only on the tick and the seat, never on
/// the state, so the log is a function of `SCRIPT_SEED` alone. If it depended
/// on the state, a divergence in the simulation would rewrite the inputs too
/// and the fixture would stop being able to tell you which one broke.
pub fn script() -> Vec<Vec<Input>> {
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
pub const DUEL_SEED: u64 = 0x0DEA_D0DE_0DEA_D0DE;
pub const DUEL_TICKS: u32 = 900;

/// The constants the duel is played under: [`RULES`], made lethal enough to
/// reach death and respawn inside thirty seconds.
pub const DUEL_RULES: Rules = Rules {
    champion_max_hp: Fx::from_int(350),
    // 5 seconds, against the game's 15.
    respawn_ticks: 150,
    ..RULES
};

pub fn duel_script() -> Vec<Vec<Input>> {
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
