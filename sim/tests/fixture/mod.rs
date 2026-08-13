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
    Action, EntityId, Fx, FxVec2, Input, PLAYER_COUNT, RULES, Rng, Rules, Seat, TEAM_SIZE, Team,
    Tick, base_position, champion_entity_id, tower_entity_id,
};

/// Which team each team walks at: Blue at Red, Red at Green, Green at Blue.
///
/// Arbitrary and fixed. What it buys is that all three lanes are used and no
/// team is left unattacked, which is what makes the fixture exercise the
/// triangle rather than one edge of it. A three-team map has no notion of "the
/// enemy", so a fixture has to pick, and picking a rotation is the pick that
/// treats the three teams alike.
const fn quarry(team: Team) -> Team {
    match team {
        Team::Blue => Team::Red,
        Team::Red => Team::Green,
        Team::Green => Team::Blue,
    }
}

/// The tower the quarry keeps on the lane the two of them share.
///
/// Read off `sim::tower_lane`: team `t`'s towers are indices `2t` (toward the
/// next base) and `2t + 1` (toward the previous one), so the tower Blue meets
/// walking at Red is Red's own on the Red–Blue lane, which is index 3.
const fn quarry_tower(team: Team) -> usize {
    match team {
        Team::Blue => 3,
        Team::Red => 5,
        Team::Green => 1,
    }
}

/// The seat of the quarry team sitting in the same place in its own line-up.
fn quarry_seat(seat: Seat) -> Seat {
    Seat::ALL[quarry(seat.team()).index() * TEAM_SIZE + seat.within_team()]
}

/// A point `percent` of the way from a seat's own base toward its quarry's,
/// nudged sideways by `offset` units.
fn along_the_lane(seat: Seat, percent: i32, offset: i32, rules: &Rules) -> FxVec2 {
    let from = base_position(seat.team(), rules);
    let to = base_position(quarry(seat.team()), rules);
    let along = from.add(to.sub(from).scale(Fx::from_ratio(percent, 100)));
    // Sideways is perpendicular to the lane, so the jitter means the same thing
    // on all three of them — on the old two-team map it was `y`, which was
    // perpendicular to the one lane there was.
    let direction = to.sub(from);
    let across = FxVec2::new(direction.y.neg(), direction.x).normalize_or_zero();
    along
        .add(across.scale(Fx::from_int(offset)))
        .clamp_components(rules.map_half_extent)
}

/// The match seed. Arbitrary, and frozen: it is part of the fixture.
pub const SEED: u64 = 0x00C0_FFEE_0D15_EA5E;

/// The seed of the *script*, which is a separate generator from the match's.
/// Keeping them apart means a rule that starts drawing from the match RNG does
/// not silently rewrite the input log as well as the outcome.
const SCRIPT_SEED: u64 = 0x5EED_1234_5EED_4321;

/// Ticks in the fixture, per M1's exit criterion.
pub const TICKS: u32 = 1000;

/// A scripted match: nine players issuing plausible commands, plus the
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
        for seat in Seat::ALL {
            let index = seat.index();
            let roll = rng.below(1000);
            // Two draws are consumed unconditionally so that the generator's
            // position does not depend on which branch was taken.
            let a = rng.below(1000) as i32;
            let b = rng.below(1000) as i32;

            let enemy_seat = quarry_seat(seat);
            let enemy_tower = quarry_tower(seat.team());

            let action = if roll < 950 {
                // Most ticks carry no new command. Both halves of that matter:
                // a player issuing sixty orders a second is not play, and a
                // champion re-ordered every few ticks never travels far enough
                // to reach a tower — which would leave tower fire, death and
                // respawn exercised by unit tests on one platform and by this
                // fixture on none.
                continue;
            } else if roll < 964 {
                // Always down its own lane, never back toward its own base. A
                // destination drawn symmetrically averages out to standing
                // still, which is the same problem in a different costume.
                Action::Move(along_the_lane(seat, a % 90 + 20, b % 41 - 20, &RULES))
            } else if roll < 990 {
                if a % 5 < 1 {
                    Action::Attack(champion_entity_id(enemy_seat))
                } else {
                    Action::Attack(tower_entity_id(enemy_tower))
                }
            } else if roll < 995 {
                // Aimed down the lane, with a few degrees of spread.
                let from = base_position(seat.team(), &RULES);
                let to = base_position(quarry(seat.team()), &RULES);
                let direction = to.sub(from);
                let across = FxVec2::new(direction.y.neg(), direction.x).normalize_or_zero();
                Action::Skillshot(
                    direction
                        .normalize_or_zero()
                        .scale(Fx::from_int(10))
                        .add(across.scale(Fx::from_ratio(b % 21 - 10, 10))),
                )
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
                seq: seq[index],
                player: seat,
                action,
            });
            seq[index] += 1;
        }
        log.push(inputs);
    }
    log
}

/// A second fixture, whose only job is to kill somebody.
///
/// The scripted match above exercises movement, both abilities, basic attacks,
/// tower fire, projectile collision, clamping and the hostile tail — but over
/// thirty-three seconds of nine-way skirmishing it never quite finishes anyone
/// off, and death and respawn would then be verified by unit tests on one
/// platform and by nothing on three.
///
/// Seat 0 walks alone down the Blue–Red lane; Red's three defenders focus it and
/// the towers on that lane join in. Green sits at its own base and takes no
/// part, which is a configuration only a three-team map has and one the
/// projection has to answer for: a player watching a fight it is not in.
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
/// Forty seconds, against the scripted match's thirty-three.
///
/// Longer than it was, and the reason is the map rather than the rules: a lane
/// of the triangle is 173 units where the old map's single lane was 200 between
/// spawns but had its towers at 40 and 90 from the middle, so a champion walking
/// out of its base meets the first thing that can kill it later than it used to.
/// The fixture has to contain a death *and* the respawn after it *and* enough
/// ticks for the champion to move again afterwards, which is what
/// `the_duel_kills_and_respawns_its_victim` checks.
pub const DUEL_TICKS: u32 = 1200;

/// The constants the duel is played under: [`RULES`], made lethal enough to
/// reach death and respawn inside forty seconds.
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
        let mut issue = |seat: Seat, action: Action, inputs: &mut Vec<Input>| {
            inputs.push(Input {
                tick: Tick(tick),
                seq: seq[seat.index()],
                player: seat,
                action,
            });
            seq[seat.index()] += 1;
        };

        // Seat 0 keeps walking at Red's base, including after it respawns —
        // which is what makes the second death, and therefore the respawn
        // timer, part of what this fixture checks.
        if tick.is_multiple_of(60) {
            issue(
                Seat::Blue0,
                Action::Move(along_the_lane(Seat::Blue0, 95, 0, &DUEL_RULES)),
                &mut inputs,
            );
        }
        // The three defenders hold their ground and attack it on sight.
        if tick == 0 {
            for seat in [Seat::Red0, Seat::Red1, Seat::Red2] {
                issue(
                    seat,
                    Action::Attack(champion_entity_id(Seat::Blue0)),
                    &mut inputs,
                );
            }
        }
        // Everything else they have, on cooldown. Aimed back down their own
        // lane, which is where seat 0 is coming from.
        if tick % 240 == 30 {
            let toward = along_the_lane(Seat::Red0, -100, 0, &DUEL_RULES)
                .sub(base_position(Team::Red, &DUEL_RULES));
            for seat in [Seat::Red0, Seat::Red1, Seat::Red2] {
                issue(seat, Action::Skillshot(toward), &mut inputs);
            }
        }
        if tick % 360 == 60 {
            for seat in [Seat::Red0, Seat::Red1, Seat::Red2] {
                issue(
                    seat,
                    Action::Targeted(champion_entity_id(Seat::Blue0)),
                    &mut inputs,
                );
            }
        }

        log.push(inputs);
    }
    log
}
