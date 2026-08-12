//! Property coverage for the visibility projection, over states reached by
//! simulation.
//!
//! # Why this exists beside `sim/tests/visibility.rs`
//!
//! Culling is the project's principal defence and until now it was checked on
//! two scripted fixtures. A fixture proves the projection is right about the
//! world it walks through; it says nothing about the world it does not. The
//! states that worry a maphack defence are the ones nobody scripted — a
//! champion dead this tick, a projectile in flight over a fog line, an entity
//! one raw unit outside a vision radius — and a property test is how a suite
//! reaches those without somebody having thought of them first.
//!
//! What this is **not** is a delivered defence. `docs/SCOPE.md` is explicit
//! that a defence counts once the matching exploit exists in the repository and
//! fails against it in CI, which is M7. These properties cover the state space
//! a fixture never will; they do not replace the exploit.
//!
//! # States are reached, never built
//!
//! Every state below comes from `new_state_with_rules` followed by
//! `step_with_rules`, from a drawn seed and a drawn script. That is not
//! ceremony: a state assembled field by field can be unreachable — a champion
//! alive at negative hit points, a projectile owned by a seat that is dead —
//! and a property that fails on an unreachable state proves nothing about the
//! game. It also happens to be the only door open, since `State`'s direct
//! constructors are `#[cfg(test)]`-gated inside `sim` and deliberately out of
//! reach from an integration test (`docs/ARCHITECTURE.md`, "The `State` escape
//! hatch").
//!
//! The cost is that a configuration must be *driven* to rather than declared,
//! and the four families below are the answer. Three of them carry [`Rules`] of
//! their own, exactly as `tests/fixture/mod.rs` does and for the reason
//! `docs/ARCHITECTURE.md` gives: a fixture that needs a frailer champion or a
//! faster one says so in its own constants instead of editing the game's
//! balance until a test fits. No digest is compared here, so the only thing
//! these constants have to be is reachable.
//!
//! # Hostile inputs, and what `Input` cannot express
//!
//! Half the domain is what a compromised client sends, because
//! `docs/SCOPE.md`'s starting axiom is that the client is lying, and the legal
//! domain of M1 bounds what the simulation *accepts*, not what arrives. Every
//! hostile value at this layer is expressible and is drawn: coordinates across
//! the whole `i32` range, handles that name nothing, seats outside the match,
//! ticks that are not the current one, repeated sequence numbers, several
//! commands from one seat in one tick.
//!
//! Three kinds of hostility are **not** expressible against `view_for`, and
//! saying so is more useful than simulating them:
//!
//! - *A malformed input.* `Input` is a struct of integers with no invalid
//!   bit patterns, so there is no "corrupt input" below the type. Malformed
//!   *frames* are the protocol's problem, and `protocol/tests/wire.rs` is where
//!   they are refused.
//! - *An input attributed to a seat the sender does not own.* Expressible here
//!   — `player` is whatever the caller writes — but it is a claim the server
//!   overwrites from the session, so at this layer it is only a test that a
//!   seat drives its own champion and nobody else's, which
//!   `sim/tests/properties.rs` already makes.
//! - *A player asking for another player's view, or for a seat outside the
//!   match.* `view_for` takes the seat from its caller, and the caller is the
//!   server. Until M3 the adjacent case — a server passing an unvalidated
//!   handle — was reachable and was drawn here, and the projection was total
//!   for it. It is now unrepresentable: [`Seat`] has six values, the byte that
//!   would have carried a seventh is refused by `protocol`'s decoder, and this
//!   file stopped drawing a case the type no longer admits.

// The crate under test denies these at its root; repeated here because
// crate-level attributes do not reach an integration test.
#![deny(clippy::float_arithmetic, unsafe_code)]

mod spec;

use std::collections::BTreeSet;

use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::{FileFailurePersistence, TestRunner};

use sim::view::{EntityView, PlayerView, view_for, view_for_with_rules};
use sim::{
    Action, EntityId, Fx, FxVec2, Input, Liveness, MAX_PROJECTILES, Outcome, PLAYER_COUNT, RULES,
    Rules, Seat, State, TOWER_COUNT, Team, Tick, champion_entity_id, new_state_with_rules,
    step_with_rules, tower_entity_id, tower_position,
};
use spec::{
    Entitled, entitled, expected_events, expected_ids, handles_with_positions, reported_ids,
    same_event, team_sees,
};

/// Where a counter-example is written down.
///
/// A file of its own rather than `properties.txt`, because proptest keys
/// persisted cases by source location and mixing two test binaries into one
/// file makes the seeds of each unreadable to the other. `Direct` for the
/// reason spelled out in `sim/tests/properties.rs`: the default walks up from
/// the test source looking for a `lib.rs` it will never find in `tests/`, and
/// writes the seeds somewhere nobody commits.
const REGRESSIONS: &str = "proptest-regressions/view_properties.txt";

/// The configuration every property below runs under.
///
/// The same shape as `sim/tests/properties.rs`, and for the same two reasons.
/// `..ProptestConfig::default()` carries `PROPTEST_CASES` through, which is how
/// the `properties` job raises the budget above the development default; and
/// `max_global_rejects` is scaled to the case count rather than left at
/// proptest's fixed 1024, which is the trap that once turned a raised budget
/// into three aborted tests — a test that stops running, not a property that
/// stops holding. Nothing below rejects: every strategy here is constructed so
/// that every draw is a case, and the budget is scaled anyway so that adding a
/// `prop_assume!` later cannot silently reintroduce the failure.
fn config() -> ProptestConfig {
    let default = ProptestConfig::default();
    ProptestConfig {
        max_global_rejects: default.cases.saturating_mul(4),
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(REGRESSIONS))),
        ..default
    }
}

/// Half-extent of the legal coordinate domain, in raw Q15.16 units.
const DOMAIN_RAW: i32 = 128 * 65536;

// ---------------------------------------------------------------------------
// The constants each family is played under
// ---------------------------------------------------------------------------

/// A brawl: the game, on a map whose two spawn lines are sixteen units apart
/// instead of two hundred, with a champion that dies to one hit of anything and
/// comes back a fifth of a second later.
///
/// This is the family that reaches the configurations the fixtures took nine
/// hundred ticks to reach, and some they never did: champions dead, champions
/// returning, several projectiles in flight at once, and — because the spawn
/// lines are four units outside vision — a fog line that is crossed and
/// recrossed rather than sat behind. Under the game's own constants a property
/// test would spend its entire budget walking.
const BRAWL_RULES: Rules = Rules {
    spawn_x: Fx::from_int(-8),
    // 45 units per second: sixteen units of no man's land is crossed in eleven
    // ticks rather than in eighty, which is the difference between a script
    // that reaches a death and one that spends its whole length walking.
    champion_speed: Fx::from_ratio(3, 2),
    // One basic attack, one skillshot or one targeted spell is lethal.
    champion_max_hp: Fx::from_int(12),
    respawn_ticks: 6,
    skillshot_cooldown_ticks: 3,
    skillshot_lifetime_ticks: 8,
    targeted_cooldown_ticks: 6,
    ..RULES
};

/// A sprint: the game, with a champion that crosses the map in twenty-two
/// ticks.
///
/// Twelve units per tick is inside the sixteen `docs/ARCHITECTURE.md` allows a
/// per-tick displacement to be, so the arithmetic stays in the domain the M1
/// properties certify. It exists so that "walk to this point" is a way of
/// *reaching* a position — a corner of the map, a spot exactly one raw unit
/// outside somebody's vision — rather than a way of spending a budget on
/// walking.
const SPRINT_RULES: Rules = Rules {
    champion_speed: Fx::from_int(12),
    ..RULES
};

/// An endgame: the sprint, with towers a single basic attack destroys.
///
/// The only way to reach a decided match inside a property test, and therefore
/// the only way to project one. A decided match is a state `step` freezes and
/// keeps ticking, and `view_for` still has to answer for it.
const ENDGAME_RULES: Rules = Rules {
    tower_max_hp: Fx::from_int(12),
    ..SPRINT_RULES
};

// ---------------------------------------------------------------------------
// Recipes: a state as the seed and the script that reach it
// ---------------------------------------------------------------------------

/// Which family of states a recipe belongs to, which is also which constants it
/// is played under.
#[derive(Clone, Copy, Debug)]
enum Family {
    /// The game's own rules, two hundred units between the teams: the regime
    /// where almost everything is culled almost always.
    Wander,
    /// Close quarters, frail champions, short respawns.
    Brawl,
    /// Champions fast enough to be placed anywhere on the map by walking there.
    Sprint,
    /// Towers frail enough for the match to end.
    Endgame,
}

impl Family {
    const fn rules(self) -> Rules {
        match self {
            Self::Wander => RULES,
            Self::Brawl => BRAWL_RULES,
            Self::Sprint => SPRINT_RULES,
            Self::Endgame => ENDGAME_RULES,
        }
    }
}

/// Which tick a command claims to be for.
///
/// `step` ignores an input whose tick is not the state's own, so this is the
/// difference between a command and a no-op — and a compromised client sends
/// both.
#[derive(Clone, Copy, Debug)]
enum When {
    /// The tick it is handed to.
    Now,
    /// A tick of the sender's choosing.
    Claimed(u32),
}

/// An input minus its tick, so that a script can be replayed wherever it lands.
#[derive(Clone, Copy, Debug)]
struct Command {
    when: When,
    seq: u32,
    player: Seat,
    action: Action,
}

impl Command {
    fn at(self, now: Tick) -> Input {
        Input {
            tick: match self.when {
                When::Now => now,
                When::Claimed(tick) => Tick(tick),
            },
            seq: self.seq,
            player: self.player,
            action: self.action,
        }
    }
}

/// A state, expressed as the way to reach it.
///
/// This is what the strategies produce and what proptest shrinks, rather than
/// the `State` itself: a shrunken seed and script is a case that can be
/// replayed, where a shrunken state would be a state nothing can produce.
#[derive(Clone, Debug)]
struct Recipe {
    family: Family,
    seed: u64,
    /// One batch per tick, in tick order.
    script: Vec<Vec<Command>>,
    /// Ticks of nothing appended afterwards, so that orders issued at the top
    /// have time to be carried out.
    settle: u32,
}

impl Recipe {
    const fn rules(&self) -> Rules {
        self.family.rules()
    }

    /// The state the recipe reaches.
    fn state(&self) -> State {
        let rules = self.rules();
        let mut state = new_state_with_rules(self.seed, &rules);
        for batch in &self.script {
            let inputs: Vec<Input> = batch
                .iter()
                .map(|command| command.at(state.tick()))
                .collect();
            state = step_with_rules(&state, &inputs, &rules);
        }
        for _ in 0..self.settle {
            state = step_with_rules(&state, &[], &rules);
        }
        state
    }

    /// Every state the recipe passes through, starting with the untouched
    /// initial one.
    ///
    /// The first tick and the last are both in here, which is the cheap way to
    /// cover them: a property that only looked at the state a recipe ends on
    /// would never see tick zero, where nothing has happened yet and every
    /// champion stands on its spawn point.
    fn every_tick(&self) -> Vec<State> {
        let rules = self.rules();
        let mut state = new_state_with_rules(self.seed, &rules);
        let mut out = vec![state.clone()];
        for batch in &self.script {
            let inputs: Vec<Input> = batch
                .iter()
                .map(|command| command.at(state.tick()))
                .collect();
            state = step_with_rules(&state, &inputs, &rules);
            out.push(state.clone());
        }
        for _ in 0..self.settle {
            state = step_with_rules(&state, &[], &rules);
            out.push(state.clone());
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// A coordinate.
///
/// Weighted toward the legal domain, with its two bounds over-sampled because
/// the edge of the map is where the clamp lives, and with a tail across the
/// whole type because that is what a client that is not a client sends.
fn coordinate() -> impl Strategy<Value = Fx> {
    prop_oneof![
        6 => (-DOMAIN_RAW..=DOMAIN_RAW).prop_map(Fx::from_raw),
        2 => prop_oneof![
            Just(Fx::from_int(128)),
            Just(Fx::from_int(-128)),
            Just(Fx::ZERO),
            Just(Fx::EPSILON),
        ],
        1 => proptest::num::i32::ANY.prop_map(Fx::from_raw),
    ]
}

fn point() -> impl Strategy<Value = FxVec2> {
    (coordinate(), coordinate()).prop_map(|(x, y)| FxVec2::new(x, y))
}

/// A handle: mostly one that names something, sometimes one that names nothing.
fn handle() -> impl Strategy<Value = EntityId> {
    prop_oneof![
        4 => (0u16..PLAYER_COUNT as u16).prop_map(EntityId),
        3 => (10u16..10 + TOWER_COUNT as u16).prop_map(EntityId),
        2 => (1000u16..1000 + MAX_PROJECTILES as u16).prop_map(EntityId),
        2 => proptest::num::u16::ANY.prop_map(EntityId),
    ]
}

/// A seat. Every one of them exists; see the module documentation on the
/// seventh, which used to be drawn here and is now a value nobody can build.
fn seat() -> impl Strategy<Value = Seat> {
    (0usize..PLAYER_COUNT).prop_map(|index| Seat::ALL[index])
}

fn action() -> impl Strategy<Value = Action> {
    prop_oneof![
        2 => Just(Action::Idle),
        6 => point().prop_map(Action::Move),
        5 => point().prop_map(Action::Skillshot),
        4 => handle().prop_map(Action::Attack),
        3 => handle().prop_map(Action::Targeted),
    ]
}

fn command() -> impl Strategy<Value = Command> {
    let when = prop_oneof![
        9 => Just(When::Now),
        1 => proptest::num::u32::ANY.prop_map(When::Claimed),
    ];
    (when, proptest::num::u32::ANY, seat(), action()).prop_map(|(when, seq, player, action)| {
        Command {
            when,
            seq,
            player,
            action,
        }
    })
}

/// A script of up to `ticks` batches, each of up to three commands — enough for
/// one seat to send several in a tick, which is itself a thing only a
/// compromised client does.
fn script(ticks: usize) -> impl Strategy<Value = Vec<Vec<Command>>> {
    prop::collection::vec(prop::collection::vec(command(), 0..=3), 0..=ticks)
}

fn wander() -> impl Strategy<Value = Recipe> {
    (proptest::num::u64::ANY, script(24)).prop_map(|(seed, script)| Recipe {
        family: Family::Wander,
        seed,
        script,
        settle: 0,
    })
}

fn brawl() -> impl Strategy<Value = Recipe> {
    (proptest::num::u64::ANY, script(40)).prop_map(|(seed, script)| Recipe {
        family: Family::Brawl,
        seed,
        script,
        settle: 0,
    })
}

/// Six drawn destinations, walked to at twelve units a tick and then held.
///
/// This is the family that reaches the corners of the legal domain and any
/// arrangement of six champions that a straight walk can produce, including the
/// ones where an entity ends up a hair inside or outside a vision radius.
fn sprint() -> impl Strategy<Value = Recipe> {
    (
        proptest::num::u64::ANY,
        prop::collection::vec(point(), PLAYER_COUNT),
    )
        .prop_map(|(seed, destinations)| {
            let orders = destinations
                .into_iter()
                .enumerate()
                .map(|(index, destination)| Command {
                    when: When::Now,
                    seq: 0,
                    player: Seat::ALL[index],
                    action: Action::Move(destination),
                })
                .collect();
            Recipe {
                family: Family::Sprint,
                seed,
                script: vec![orders],
                settle: 26,
            }
        })
}

/// One team walks into the other's towers and knocks both down.
fn endgame() -> impl Strategy<Value = Recipe> {
    (proptest::num::u64::ANY, proptest::bool::ANY).prop_map(|(seed, blue_attacks)| {
        let (first_seat, first_tower) = if blue_attacks {
            (0usize, 2usize)
        } else {
            (3, 0)
        };
        let orders = (0..3usize)
            .map(|offset| Command {
                when: When::Now,
                seq: 0,
                player: Seat::ALL[first_seat + offset],
                action: Action::Attack(tower_entity_id(first_tower + offset.min(1))),
            })
            .collect();
        Recipe {
            family: Family::Endgame,
            seed,
            script: vec![orders],
            settle: 60,
        }
    })
}

/// A state reached by simulation, from one of the four families.
///
/// The weights are the cost: a wander or a brawl is a few dozen ticks, an
/// endgame is sixty-one, and every property below draws from all four so that
/// none of them is checked only on the cheap half of the space.
fn reachable() -> impl Strategy<Value = Recipe> {
    prop_oneof![
        4 => wander(),
        5 => brawl(),
        3 => sprint(),
        1 => endgame(),
    ]
}

// ---------------------------------------------------------------------------
// The two halves of the criterion, as assertions
// ---------------------------------------------------------------------------

/// **Soundness.** Nothing in the view was seen at a point outside the vision of
/// this player's team. Its violation is a maphack.
fn assert_sound(state: &State, player: Seat, rules: &Rules) {
    let team = player.team();
    let view = view_for_with_rules(state, player, rules);
    for (id, position) in handles_with_positions(&view) {
        assert!(
            team_sees(state, team, position, rules),
            "tick {:?}, {player:?}: entity {id} named at {position:?}, which is outside \
             this team's vision",
            state.tick()
        );
    }
}

/// **Completeness.** Everything in vision is in the view, with the attributes
/// the state actually holds.
///
/// Not a security property: an omission is a bug in the game, not a leak. It is
/// here because without it the degenerate projection that returns nothing
/// satisfies soundness, and "true by vacuity" is a failure mode this project
/// has already caught itself in once.
fn assert_complete(state: &State, player: Seat, rules: &Rules) {
    let view = view_for_with_rules(state, player, rules);

    let reported: BTreeSet<u16> = reported_ids(&view).into_iter().collect();
    assert_eq!(
        reported,
        expected_ids(state, player, rules),
        "tick {:?}, {player:?}: the visible set is not the entitled set",
        state.tick()
    );

    for entity in &view.visible {
        match entity {
            EntityView::Champion { id, position, hp } => {
                let champion = state.champions()[id.0 as usize];
                assert_eq!(*position, champion.position);
                assert_eq!(Liveness::Alive { hp: *hp }, champion.liveness);
            }
            EntityView::Tower { id, position, hp } => {
                let index = usize::from(id.0 - 10);
                assert_eq!(*position, tower_position(index, rules));
                assert_eq!(*hp, state.towers()[index].hp);
            }
            EntityView::Projectile {
                id,
                position,
                velocity,
            } => {
                let found = state
                    .projectiles()
                    .iter()
                    .find(|projectile| projectile.id == *id)
                    .expect("a projectile that is not in the arena");
                assert_eq!(*position, found.position);
                assert_eq!(*velocity, found.velocity);
            }
        }
    }

    let entitled = expected_events(state, player, rules);
    assert_eq!(
        view.events.len(),
        entitled.len(),
        "tick {:?}, {player:?}: event count",
        state.tick()
    );
    for (expected, seen) in entitled.iter().zip(&view.events) {
        assert!(
            same_event(expected, seen),
            "tick {:?}, {player:?}: {seen:?} is not {expected:?}",
            state.tick()
        );
    }
}

/// Every seat a server could ask about, which since M3 is every seat there is.
///
/// It used to append two seats a server should not have been able to ask about
/// — `PlayerId(6)` and `PlayerId(200)` — because the type admitted them. They
/// are gone from the domain rather than from the coverage: `Seat::ALL` is the
/// whole of it now.
fn every_player() -> impl Iterator<Item = Seat> {
    Seat::ALL.into_iter()
}

/// Vision wide enough to cover the map's diagonal from anywhere: the
/// accidental full-state leak, built on purpose so that a real view can be
/// compared against one.
fn omniscient(rules: &Rules) -> Rules {
    Rules {
        champion_vision_radius: Fx::from_int(512),
        tower_vision_radius: Fx::from_int(512),
        ..*rules
    }
}

proptest! {
    #![proptest_config(config())]

    /// **Property 1, soundness.** For every state a match can reach and every
    /// player, nothing in the view was seen from outside that player's team's
    /// vision — in the entity list and in the derived events alike.
    ///
    /// Checked on every tick of the run rather than on the state it ends at:
    /// the same simulation then covers the first tick, the last, and everything
    /// between, and the culling of a champion that died *this* tick is a
    /// question that only exists on the tick it died.
    #[test]
    fn nothing_outside_vision_is_ever_named(recipe in reachable()) {
        let rules = recipe.rules();
        for state in recipe.every_tick() {
            for player in every_player() {
                assert_sound(&state, player, &rules);
            }
        }
    }

    /// **Property 2, completeness.** Everything inside vision is named, with
    /// the attributes the state holds, and the events are exactly the entitled
    /// ones in the order the rules produced them.
    #[test]
    fn everything_inside_vision_is_named(recipe in reachable()) {
        let rules = recipe.rules();
        for state in recipe.every_tick() {
            for player in every_player() {
                assert_complete(&state, player, &rules);
            }
        }
    }

    /// **Property 3, no side channel in the view itself.** Two states that a
    /// player is entitled to know exactly the same things about produce the
    /// same view for that player.
    ///
    /// The two states are reached by forking one run: the same state, stepped
    /// twice with different commands. When the entitlement the specification
    /// derives is identical for a player, everything that differs between the
    /// two states is something that player may not know about — so a view that
    /// differs is a view that carries it. A counter, a length, a recycled
    /// handle or a collection order that follows hidden state all fail here,
    /// and none of them would fail properties 1 and 2.
    ///
    /// What it does not do is prove the absence of a side channel today. With
    /// `PlayerView`'s current fields the antecedent nearly determines the
    /// consequent, so this mostly re-derives properties 1 and 2 — it earns its
    /// place as the guard that fires on the day a field is added to the view
    /// whose value is a function of the whole state.
    #[test]
    fn a_view_is_a_function_of_what_its_player_is_entitled_to(
        (recipe, first, second) in (reachable(),
                                    prop::collection::vec(command(), 0..=3),
                                    prop::collection::vec(command(), 0..=3)),
    ) {
        let rules = recipe.rules();
        let state = recipe.state();
        let now = state.tick();
        let fork = |batch: &[Command]| {
            let inputs: Vec<Input> = batch.iter().map(|command| command.at(now)).collect();
            step_with_rules(&state, &inputs, &rules)
        };
        let left = fork(&first);
        let right = fork(&second);

        for player in Seat::ALL {
            let left_entitled: Entitled = entitled(&left, player, &rules);
            let right_entitled: Entitled = entitled(&right, player, &rules);
            if left_entitled == right_entitled {
                prop_assert_eq!(
                    view_for_with_rules(&left, player, &rules),
                    view_for_with_rules(&right, player, &rules),
                    "{:?} was told two different things about two states it \
                     cannot tell apart",
                    player
                );
            }
        }
    }

    /// **Property 3, second half: the order is a function of the content.**
    ///
    /// A view lists its entities in ascending handle order. The order a player
    /// is told things in has to be a function of what they were told, and the
    /// arena the projectiles live in is not: its slots are allocated
    /// lowest-free-first, so which slot a visible projectile occupies depends
    /// on the casts that came before it — including the ones the player never
    /// saw. That is thin, and it is exactly the shape of channel this project
    /// counts as a leak (`docs/SCOPE.md`, adversary model: "Observe packet
    /// sizes and arrival times").
    #[test]
    fn the_entity_list_is_ordered_by_handle(recipe in reachable()) {
        let rules = recipe.rules();
        for state in recipe.every_tick() {
            for player in every_player() {
                let ids = reported_ids(&view_for_with_rules(&state, player, &rules));
                let mut ascending = ids.clone();
                ascending.sort_unstable();
                prop_assert_eq!(
                    &ids,
                    &ascending,
                    "tick {:?}, {:?}: the entity list is ordered by something \
                     other than the handle",
                    state.tick(),
                    player
                );
            }
        }
    }

    /// **Property 4, purity.** Two calls on one state return one view, and the
    /// state is untouched by having been projected.
    ///
    /// `view_for` takes `&State`, so the second half is a claim the borrow
    /// checker already makes — except through interior mutability, which is
    /// what this would catch, and except in the reading that matters at M3: the
    /// transport will hash and cache views, and both assume the projection is a
    /// function of its arguments.
    #[test]
    fn view_for_is_pure_and_leaves_the_state_alone(recipe in reachable()) {
        let rules = recipe.rules();
        let state = recipe.state();
        let before = state.digest();
        for player in every_player() {
            let first = view_for_with_rules(&state, player, &rules);
            let second = view_for_with_rules(&state, player, &rules);
            prop_assert_eq!(&first, &second);
            prop_assert_eq!(first.encode(), second.encode());
        }
        prop_assert_eq!(state.digest(), before);
    }

    /// The boundary itself, which no fixture ever aimed at.
    ///
    /// Two champions are walked to points exactly `separation` apart, near the
    /// origin and out of every other source's reach, and the projection has to
    /// flip exactly at the radius: a champion at exactly
    /// `champion_vision_radius` is seen, and one a single raw unit — a
    /// sixty-five-thousandth of a world unit — further away is not.
    #[test]
    fn vision_flips_exactly_at_the_radius(separation in separations(), y in -10i32..=10) {
        let rules = SPRINT_RULES;
        let state = face_off(separation, Fx::from_int(y));
        let watcher = Seat::Blue0;
        let watched = Seat::Red0;

        // The construction is only worth anything if the pair is alone: any
        // other source in range would decide the question instead of the
        // separation.
        let (near, far) = (
            state.champion(Seat::Blue0).position,
            state.champion(Seat::Red0).position,
        );
        prop_assert_eq!(near.sub(far).length_squared_wide(),
                        i64::from(separation.to_raw()) * i64::from(separation.to_raw()),
                        "the two champions did not end up exactly {:?} apart", separation);
        for other in [Seat::Blue1, Seat::Blue2, Seat::Red1, Seat::Red2] {
            let source = state.champion(other).position;
            let target = if other.team() == Team::Blue { far } else { near };
            prop_assert!(
                !source.within_range(target, rules.champion_vision_radius),
                "{:?} is close enough to decide the question by itself", other
            );
        }

        let inside = separation.to_raw() <= rules.champion_vision_radius.to_raw();
        let seen = reported_ids(&view_for_with_rules(&state, watcher, &rules))
            .contains(&champion_entity_id(Seat::Red0).0);
        prop_assert_eq!(
            seen,
            inside,
            "separation {:?} against a radius of {:?}",
            separation,
            rules.champion_vision_radius
        );
        // Vision is a team property and both sides are symmetric, so the same
        // answer has to come back the other way round.
        let mirrored = reported_ids(&view_for_with_rules(&state, watched, &rules))
            .contains(&champion_entity_id(Seat::Blue0).0);
        prop_assert_eq!(mirrored, inside, "the two sides disagree about one distance");
    }

    /// Culling is monotone in the radius: widening vision can only add.
    ///
    /// A sign slipped in a comparison, a radius read from the wrong constant or
    /// a boundary that excludes what it should include all break this, and they
    /// break it in the direction the fixtures are least likely to notice.
    #[test]
    fn a_wider_radius_can_only_add(
        recipe in reachable(),
        champion_radius in 0i32..=40 * 65536,
        tower_radius in 0i32..=40 * 65536,
        widening in 0i32..=40 * 65536,
    ) {
        let base = Rules {
            champion_vision_radius: Fx::from_raw(champion_radius),
            tower_vision_radius: Fx::from_raw(tower_radius),
            ..recipe.rules()
        };
        let wider = Rules {
            champion_vision_radius: Fx::from_raw(champion_radius.saturating_add(widening)),
            tower_vision_radius: Fx::from_raw(tower_radius.saturating_add(widening)),
            ..base
        };
        let state = recipe.state();
        for player in every_player() {
            let narrow = view_for_with_rules(&state, player, &base);
            let broad = view_for_with_rules(&state, player, &wider);
            let broad_ids: BTreeSet<u16> = reported_ids(&broad).into_iter().collect();
            for id in reported_ids(&narrow) {
                prop_assert!(
                    broad_ids.contains(&id),
                    "entity {id} is visible at radius {:?} and not at {:?}",
                    base.champion_vision_radius,
                    wider.champion_vision_radius
                );
            }
            for event in &narrow.events {
                prop_assert!(
                    broad.events.contains(event),
                    "an event visible at the narrower radius is absent at the wider one"
                );
            }
            prop_assert_eq!(narrow.own, broad.own, "vision changed the player's own champion");
        }
    }

    /// Two seats of one team are told the same world.
    ///
    /// Team vision is the model (`docs/ARCHITECTURE.md`), and it is also the
    /// one with no ally-only channel to get wrong. Stated over reachable states
    /// rather than over one scripted position, and as set equality rather than
    /// as the count the unit test compares.
    #[test]
    fn the_seats_of_a_team_are_told_the_same_world(recipe in reachable()) {
        let rules = recipe.rules();
        let state = recipe.state();
        for team in [Team::Blue, Team::Red] {
            for first in Seat::ALL.into_iter().filter(|seat| seat.team() == team) {
                for second in Seat::ALL.into_iter().filter(|seat| seat.team() == team) {
                    if first == second {
                        continue;
                    }
                    let left = view_for_with_rules(&state, first, &rules);
                    let right = view_for_with_rules(&state, second, &rules);
                    let ours = [champion_entity_id(first), champion_entity_id(second)];
                    let strip = |view: &PlayerView| -> Vec<EntityView> {
                        view.visible
                            .iter()
                            .filter(|entity| !matches!(entity,
                                EntityView::Champion { id, .. } if ours.contains(id)))
                            .copied()
                            .collect()
                    };
                    prop_assert_eq!(strip(&left), strip(&right),
                        "{:?} and {:?} of one team see different worlds", first, second);
                    prop_assert_eq!(&left.events, &right.events);
                    prop_assert_eq!(left.outcome, right.outcome);
                    prop_assert_eq!(left.tick, right.tick);
                }
            }
        }
    }

    /// An encoded view stays inside the bound the transport will pad to, and
    /// never exceeds what an omniscient projection would have produced.
    ///
    /// # This one has no teeth, and that is worth writing down
    ///
    /// Every other property here was checked by breaking `view_for` on purpose
    /// and watching it go red. This one could not be made to fail, and the two
    /// reasons are structural rather than a matter of trying harder:
    ///
    /// - [`PlayerView::MAX_ENCODED_BYTES`] is derived from the *type* — a full
    ///   projectile arena and a full event buffer — and a reachable view is a
    ///   couple of hundred bytes against a bound of 1498. A projection emitting
    ///   every visible entity four times over stays inside it.
    /// - The comparison against an omniscient projection re-runs the *same*
    ///   function under wider radii, so any transformation applied to the
    ///   output is applied to both sides and cancels. It detects a projection
    ///   that ignores the radius, which is what `sim/tests/visibility.rs` uses
    ///   it for; it does not detect one that emits too much.
    ///
    /// So this property is the bound M3's padding will round up to, asserted
    /// over reachable states, and nothing more. The claim that a leak fails a
    /// test is carried by soundness and completeness above, which both go red
    /// on the mutation that stops culling.
    #[test]
    fn an_encoded_view_stays_within_its_bound(recipe in reachable()) {
        let rules = recipe.rules();
        let leaky = omniscient(&rules);
        let state = recipe.state();
        for player in every_player() {
            let encoded = view_for_with_rules(&state, player, &rules).encode();
            prop_assert!(
                encoded.len() <= PlayerView::MAX_ENCODED_BYTES,
                "{} bytes exceeds the bound of {}",
                encoded.len(),
                PlayerView::MAX_ENCODED_BYTES
            );
            prop_assert!(
                encoded.len() <= view_for_with_rules(&state, player, &leaky).encode().len(),
                "a culled view cannot be larger than an unculled one"
            );
        }
    }

    /// `view_for` is `view_for_with_rules` under [`RULES`], which is what lets
    /// every property above be stated on the second and hold for the first.
    #[test]
    fn the_two_entry_points_agree_under_the_games_rules(recipe in wander()) {
        let state = recipe.state();
        for player in every_player() {
            prop_assert_eq!(
                view_for(&state, player),
                view_for_with_rules(&state, player, &RULES)
            );
        }
    }
}

/// Separations to test the boundary at: a raw unit either side of the radius,
/// and a sweep across it.
fn separations() -> impl Strategy<Value = Fx> {
    let radius = RULES.champion_vision_radius.to_raw();
    prop_oneof![
        3 => (radius - 4..=radius + 4).prop_map(Fx::from_raw),
        1 => (0..=20 * 65536i32).prop_map(Fx::from_raw),
    ]
}

/// Two champions, exactly `separation` apart at height `y` near the origin,
/// with the other four walked into the corners of the map.
///
/// Reached rather than placed: the six champions are given move orders and
/// walked there under [`SPRINT_RULES`], and `step_toward` lands exactly on a
/// destination it can reach, which is what makes an *exact* separation
/// something a simulation can produce.
fn face_off(separation: Fx, y: Fx) -> State {
    let rules = SPRINT_RULES;
    let left = FxVec2::new(Fx::from_int(-6), y);
    let right = FxVec2::new(Fx::from_int(-6).add(separation), y);
    let destinations = [
        left,
        FxVec2::new(Fx::from_int(-128), Fx::from_int(-128)),
        FxVec2::new(Fx::from_int(-128), Fx::from_int(128)),
        right,
        FxVec2::new(Fx::from_int(128), Fx::from_int(-128)),
        FxVec2::new(Fx::from_int(128), Fx::from_int(128)),
    ];

    let mut state = new_state_with_rules(0x0FAC_E0FF, &rules);
    let orders: Vec<Input> = destinations
        .iter()
        .enumerate()
        .map(|(seat, destination)| Input {
            tick: Tick(0),
            seq: 0,
            player: Seat::ALL[seat],
            action: Action::Move(*destination),
        })
        .collect();
    state = step_with_rules(&state, &orders, &rules);
    // Twenty-six ticks at twelve units each covers the map's diagonal, so every
    // champion has arrived and stopped.
    for _ in 0..26 {
        state = step_with_rules(&state, &[], &rules);
    }
    state
}

// ---------------------------------------------------------------------------
// The generators are the argument, so they are checked too
// ---------------------------------------------------------------------------

/// What one sweep of the generators actually reached.
#[derive(Default, Debug)]
struct Reach {
    recipes: u64,
    states: u64,
    dead_champions: u64,
    respawns: u64,
    projectiles_in_flight: u64,
    at_the_map_bound: u64,
    decided: u64,
    views_with_something_withheld: u64,
    views_with_an_event: u64,
    views_with_an_event_withheld: u64,
    forks_hidden_from_somebody: u64,
}

/// The properties above are all conditional on the states these strategies
/// produce, and a strategy that produced only empty matches would satisfy every
/// one of them. So the strategies are sampled deterministically and the run is
/// required to have reached each configuration the properties are supposed to
/// be about — the same argument, and the same shape of assertion, as the
/// coverage counters at the end of `sim/tests/visibility.rs`.
///
/// It is a plain test rather than a property: it is a statement about the
/// generators, not about `view_for`, and it has to sample them itself to make
/// it.
#[test]
fn the_generators_reach_the_states_these_properties_are_about() {
    const SAMPLES: usize = 256;

    let mut runner = TestRunner::deterministic();
    let strategy = reachable();
    let fork_commands = prop::collection::vec(command(), 0..=3);
    let mut reach = Reach::default();

    for _ in 0..SAMPLES {
        let recipe = strategy
            .new_tree(&mut runner)
            .expect("the strategy produced no value")
            .current();
        let rules = recipe.rules();
        let leaky = omniscient(&rules);
        reach.recipes += 1;

        let states = recipe.every_tick();
        let mut previously_dead = [false; PLAYER_COUNT];
        for state in &states {
            reach.states += 1;
            if state.projectiles().count() > 0 {
                reach.projectiles_in_flight += 1;
            }
            if matches!(state.outcome(), Outcome::Decided { .. }) {
                reach.decided += 1;
            }
            for (seat, champion) in state.champions().iter().enumerate() {
                let dead = matches!(champion.liveness, Liveness::Dead { .. });
                if dead {
                    reach.dead_champions += 1;
                } else if previously_dead[seat] {
                    reach.respawns += 1;
                }
                previously_dead[seat] = dead;
                if champion.position.x.abs() == rules.map_half_extent
                    || champion.position.y.abs() == rules.map_half_extent
                {
                    reach.at_the_map_bound += 1;
                }
            }

            for player in Seat::ALL {
                let view = view_for_with_rules(state, player, &rules);
                let all = view_for_with_rules(state, player, &leaky);
                if view.visible.len() < all.visible.len() {
                    reach.views_with_something_withheld += 1;
                }
                if !view.events.is_empty() {
                    reach.views_with_an_event += 1;
                }
                if view.events.len() < all.events.len() {
                    reach.views_with_an_event_withheld += 1;
                }
            }
        }

        // And the antecedent of the side-channel property: two forks of one
        // state that at least one player cannot tell apart while the states
        // themselves differ.
        let state = recipe.state();
        let now = state.tick();
        let batches: Vec<Vec<Command>> = (0..2)
            .map(|_| {
                fork_commands
                    .new_tree(&mut runner)
                    .expect("the strategy produced no value")
                    .current()
            })
            .collect();
        let step_of = |batch: &Vec<Command>| {
            let inputs: Vec<Input> = batch.iter().map(|command| command.at(now)).collect();
            step_with_rules(&state, &inputs, &rules)
        };
        let left = step_of(&batches[0]);
        let right = step_of(&batches[1]);
        if left.digest() != right.digest() {
            for player in Seat::ALL {
                if entitled(&left, player, &rules) == entitled(&right, player, &rules) {
                    reach.forks_hidden_from_somebody += 1;
                    break;
                }
            }
        }
    }

    println!("reach: {reach:?}");

    let floors: [(&str, u64, u64); 9] = [
        ("dead champions", reach.dead_champions, 100),
        ("respawns", reach.respawns, 10),
        ("projectiles in flight", reach.projectiles_in_flight, 100),
        ("champions at the map bound", reach.at_the_map_bound, 100),
        ("decided matches", reach.decided, 10),
        (
            "views with something withheld",
            reach.views_with_something_withheld,
            1_000,
        ),
        ("views carrying an event", reach.views_with_an_event, 100),
        (
            "views with an event withheld",
            reach.views_with_an_event_withheld,
            50,
        ),
        (
            "forks a player cannot tell apart",
            reach.forks_hidden_from_somebody,
            50,
        ),
    ];
    for (what, reached, floor) in floors {
        assert!(
            reached >= floor,
            "{SAMPLES} sampled recipes reached {what} {reached} times, under the floor \
             of {floor}: the properties are drifting toward vacuity"
        );
    }
}
