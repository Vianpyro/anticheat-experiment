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
//!   for it. It is now unrepresentable: [`Seat`] has nine values, the byte that
//!   would have carried a tenth is refused by `protocol`'s decoder, and this
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
    Rules, Seat, State, TEAM_COUNT, TOWER_COUNT, Team, Tick, champion_entity_id,
    new_state_with_rules, step_with_rules, tower_entity_id, tower_position, tower_team,
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

/// A brawl: the game, on a triangle whose bases are sixteen units apart instead
/// of a hundred and seventy-three, with a champion that dies to one hit of
/// anything and comes back a fifth of a second later.
///
/// This is the family that reaches the configurations the fixtures took nine
/// hundred ticks to reach, and some they never did: champions dead, champions
/// returning, several projectiles in flight at once, three teams within reach of
/// one another, and — because the bases are four units outside vision — a fog
/// line that is crossed and recrossed rather than sat behind. Under the game's
/// own constants a property test would spend its entire budget walking.
const BRAWL_RULES: Rules = Rules {
    // The three bases pulled in to a circumradius of eight, so the no man's
    // land between them is sixteen units rather than a hundred and seventy.
    bases: [
        FxVec2::new(Fx::ZERO, Fx::from_int(8)),
        FxVec2::new(Fx::from_ratio(693, 100), Fx::from_int(-4)),
        FxVec2::new(Fx::from_ratio(-693, 100), Fx::from_int(-4)),
    ],
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
///
/// It got harder when the third team arrived, and that is the point of keeping
/// it: with two teams a decided match was four hit points away from any state,
/// and with three it takes *two* teams knocked out, so a recipe that flattens
/// one of them reaches nothing. The floors at the end of this file caught
/// exactly that.
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
    /// The game's own rules, a hundred and seventy-three units of lane between
    /// any two bases: the regime where almost everything is culled almost always.
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
/// tenth, which used to be drawn here and is now a value nobody can build.
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
/// arrangement of nine champions that a straight walk can produce, including the
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

/// Everybody walks into the towers of the two teams that are not the drawn
/// survivor, and knocks all four of them down.
///
/// Four rather than two, because a three-team match is decided when one team is
/// *left*, not when one team is out. The assignment rotates by seat so that all
/// four towers are attacked rather than all nine seats piling onto one, and it
/// steps past any tower the attacking seat's own team owns — an order to attack
/// an ally is discarded by the rules, which would leave that tower standing and
/// the match undecided.
fn endgame() -> impl Strategy<Value = Recipe> {
    (proptest::num::u64::ANY, 0usize..TEAM_COUNT).prop_map(|(seed, survivor)| {
        let targets: Vec<usize> = Team::ALL
            .into_iter()
            .filter(|team| team.index() != survivor)
            .flat_map(|team| [team.index() * 2, team.index() * 2 + 1])
            .collect();
        let orders = Seat::ALL
            .into_iter()
            .map(|player| {
                let mut choice = player.index() % targets.len();
                while tower_team(targets[choice]) == player.team() {
                    choice = (choice + 1) % targets.len();
                }
                Command {
                    when: When::Now,
                    seq: 0,
                    player,
                    action: Action::Attack(tower_entity_id(targets[choice])),
                }
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

/// The two teams an observer is not on.
///
/// A three-team map has no "the enemy", and this is where that stops being a
/// wording problem: every rule and every property that used to say *the other
/// team* now has to say which of two, or say neither. The properties below say
/// neither, on purpose.
fn enemies_of(team: Team) -> [Team; 2] {
    let mut out = [Team::Blue; 2];
    let mut at = 0usize;
    for other in Team::ALL {
        if other != team {
            out[at] = other;
            at += 1;
        }
    }
    out
}

/// The same commands, issued by another team's seats.
///
/// Position within the team is preserved, so the exchanged pair of batches is
/// the same play performed by two different teams rather than two different
/// plays.
fn issued_by(batch: &[Command], team: Team) -> Vec<Command> {
    batch
        .iter()
        .map(|command| Command {
            player: Seat::ALL[team.index() * sim::TEAM_SIZE + command.player.within_team()],
            ..*command
        })
        .collect()
}

/// Two states that differ by which of the observer's two enemy teams did which
/// of two things.
fn exchange(
    state: &State,
    observer: Seat,
    first: &[Command],
    second: &[Command],
    rules: &Rules,
) -> (State, State) {
    let now = state.tick();
    let [x, y] = enemies_of(observer.team());
    let fork = |to_first: Team, to_second: Team| {
        let mut batch = issued_by(first, to_first);
        batch.extend(issued_by(second, to_second));
        let inputs: Vec<Input> = batch.iter().map(|command| command.at(now)).collect();
        step_with_rules(state, &inputs, rules)
    };
    (fork(x, y), fork(y, x))
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


    /// **The two enemy teams are told apart only by what is visible.**
    ///
    /// The generalisation of property 3 to three teams, and the property that
    /// only a third team makes possible to state. With two sides, "which enemy
    /// is this about" has one answer and nothing in a view can encode it; with
    /// three, a field naming the nearest enemy team, a counter kept per enemy
    /// team, or an order correlated with team membership are all leaks that a
    /// two-team format has no room for.
    ///
    /// The construction is an **exchange** rather than an arbitrary fork: the
    /// same two batches of commands are performed by the observer's two enemy
    /// teams, once each way round. Everything that differs between the two
    /// states is therefore *which enemy team* did something, and nothing else —
    /// so when the observer's entitlement is the same in both, a view that
    /// differs is a view carrying the identity of a team the observer cannot
    /// see.
    ///
    /// The consequent is byte equality rather than value equality, because the
    /// thing being ruled out includes an ordering: two views holding the same
    /// entities in a different order are equal in neither, but only the
    /// encoding makes it obvious which claim is being made.
    #[test]
    fn a_view_tells_the_two_enemy_teams_apart_only_by_what_it_shows(
        (recipe, first, second) in (reachable(),
                                    prop::collection::vec(command(), 0..=3),
                                    prop::collection::vec(command(), 0..=3)),
    ) {
        let rules = recipe.rules();
        let state = recipe.state();

        for observer in Seat::ALL {
            let (left, right) = exchange(&state, observer, &first, &second, &rules);
            let left_entitled: Entitled = entitled(&left, observer, &rules);
            let right_entitled: Entitled = entitled(&right, observer, &rules);
            if left_entitled != right_entitled {
                continue;
            }
            let [x, y] = enemies_of(observer.team());
            prop_assert_eq!(
                view_for_with_rules(&left, observer, &rules).encode(),
                view_for_with_rules(&right, observer, &rules).encode(),
                "{:?} can tell {:?} having acted from {:?} having acted, and is \
                 entitled to neither",
                observer,
                x,
                y
            );
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
    /// Two champions are walked to points exactly `offset` apart, near the
    /// origin and out of every other source's reach, and the projection has to
    /// flip exactly at the radius: a champion at exactly
    /// `champion_vision_radius` is seen, and one a single raw unit — a
    /// sixty-five-thousandth of a world unit — further away is not.
    ///
    /// # The offset is a vector, and that is the whole of this property
    ///
    /// It used to be a scalar separation along `x`, and that made the property
    /// a test of the one case where the question is easy. A squared distance
    /// that is a perfect square has an *exact* integer square root, so on that
    /// axis the truncating comparison and the exact one agree at every
    /// separation and the property could not tell them apart — it went green
    /// against a rule with the truncation put back in, at every case budget.
    /// `docs/ARCHITECTURE.md` recorded that as a known blind spot and
    /// `sim/tests/spec/mod.rs` named the counter-example, `(12.0, 0.00001)`
    /// against a radius of `12.0`, which is off the axis by construction.
    ///
    /// So the offset is drawn as a direction rather than a distance, and
    /// [`offsets`] aims most of its draws at the shell one raw unit thick just
    /// outside the circle — the band where `isqrt` truncates a distance down
    /// onto the radius. Both the rule and the specification are asserted
    /// against the exact criterion, so either one adopting a truncating
    /// comparison fails here, at proptest's development budget rather than at
    /// CI's.
    #[test]
    fn vision_flips_exactly_at_the_radius(offset in offsets()) {
        let rules = SPRINT_RULES;
        let state = face_off(offset);
        let watcher = Seat::Blue0;
        let watched = Seat::Red0;

        // The construction is only worth anything if the pair is alone: any
        // other source in range would decide the question instead of the
        // offset.
        let (near, far) = (
            state.champion(Seat::Blue0).position,
            state.champion(Seat::Red0).position,
        );
        prop_assert_eq!(far.sub(near), offset,
                        "the two champions did not end up exactly {:?} apart", offset);
        for other in [Seat::Blue1, Seat::Blue2, Seat::Red1, Seat::Red2] {
            let source = state.champion(other).position;
            let target = if other.team() == Team::Blue { far } else { near };
            prop_assert!(
                !source.within_range(target, rules.champion_vision_radius),
                "{:?} is close enough to decide the question by itself", other
            );
        }
        // And neither is a tower: they are the other vision source, they are
        // fixed, and a run that drifted the pair into one of their discs would
        // be answering a different question with this property's name on it.
        for index in 0..TOWER_COUNT {
            let target = if sim::tower_team(index) == Team::Blue { far } else { near };
            prop_assert!(
                !tower_position(index, &rules).within_range(target, rules.tower_vision_radius),
                "tower {index} is close enough to decide the question by itself"
            );
        }

        // The exact criterion, over `i64`: the circle itself counts as inside,
        // and no integer square root takes part in the comparison.
        let radius = i64::from(rules.champion_vision_radius.to_raw());
        let inside = offset.length_squared_wide() <= radius * radius;

        let seen = reported_ids(&view_for_with_rules(&state, watcher, &rules))
            .contains(&champion_entity_id(Seat::Red0).0);
        prop_assert_eq!(
            seen,
            inside,
            "offset {:?} against a radius of {:?}",
            offset,
            rules.champion_vision_radius
        );
        // Vision is a team property and both sides are symmetric, so the same
        // answer has to come back the other way round.
        let mirrored = reported_ids(&view_for_with_rules(&state, watched, &rules))
            .contains(&champion_entity_id(Seat::Blue0).0);
        prop_assert_eq!(mirrored, inside, "the two sides disagree about one distance");

        // The specification is held to the same criterion, and this is the half
        // that was missing. `sim/tests/spec/mod.rs` re-derives the vision rule
        // on purpose, so that no culling assertion is `view_for` agreeing with
        // itself; the cost is that the two can drift, and until now the only
        // thing that noticed a drift was `everything_inside_vision_is_named` at
        // CI's raised budget. Here the drift is the whole target.
        prop_assert_eq!(
            team_sees(&state, Team::Blue, far, &rules),
            inside,
            "the specification and the exact criterion disagree at {:?}",
            offset
        );
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

/// Offsets to test the boundary at, drawn as a direction rather than as a
/// distance along an axis.
///
/// Most of the weight goes on the construction that matters: a horizontal
/// component anywhere across the circle, and a vertical component placed within
/// a few raw units of the point that would put the offset exactly on it. That
/// sweeps the band `radius² < dx² + dy² < (radius + 1)²` — the shell in which a
/// truncating comparison says "inside" and an exact one says "outside", one
/// part in 786 432 of the radius wide and invisible to any sampling that is not
/// aimed at it.
///
/// Constructed, not filtered: `dy` is *computed* from `dx` so that every draw
/// lands near the circle. A `prop_assume!` that kept only the draws that
/// happened to fall in a shell this thin would reject every case and abort the
/// run, which is the failure `sim/tests/properties.rs` records.
fn offsets() -> impl Strategy<Value = FxVec2> {
    let radius = i64::from(RULES.champion_vision_radius.to_raw());
    prop_oneof![
        6 => (
            -RULES.champion_vision_radius.to_raw()..=RULES.champion_vision_radius.to_raw(),
            -4i32..=4,
            proptest::bool::ANY,
        )
            .prop_map(move |(dx, delta, below)| {
                // The vertical component that would put the offset exactly on
                // the circle, truncated: `dy0² + dx² <= radius²` and one more
                // raw unit crosses it.
                let squared = radius * radius - i64::from(dx) * i64::from(dx);
                let dy0 = i32::try_from(squared.max(0).isqrt()).unwrap_or(i32::MAX);
                let dy = dy0.saturating_add(delta);
                FxVec2::new(Fx::from_raw(dx), Fx::from_raw(if below { -dy } else { dy }))
            }),
        // …and a sweep that is not about the boundary at all, so the property
        // still says something about offsets nowhere near it.
        1 => (-20 * 65536i32..=20 * 65536, -20 * 65536i32..=20 * 65536)
            .prop_map(|(x, y)| FxVec2::new(Fx::from_raw(x), Fx::from_raw(y))),
    ]
}

/// Two champions, exactly `offset` apart near the origin, with the other four
/// walked into the corners of the map.
///
/// Reached rather than placed: the nine champions are given move orders and
/// walked there under [`SPRINT_RULES`], and `step_toward` lands exactly on a
/// destination it can reach, which is what makes an *exact* offset something a
/// simulation can produce.
fn face_off(offset: FxVec2) -> State {
    let rules = SPRINT_RULES;
    let left = FxVec2::new(Fx::from_int(-6), Fx::ZERO);
    let right = left.add(offset);
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
    exchanges_hidden_from_the_observer: u64,
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

        // And the antecedent of the three-team property: an exchange between
        // the observer's two enemy teams that reaches two different worlds the
        // observer cannot tell apart.
        for observer in Seat::ALL {
            let (left, right) = exchange(&state, observer, &batches[0], &batches[1], &rules);
            if left.digest() != right.digest()
                && entitled(&left, observer, &rules) == entitled(&right, observer, &rules)
            {
                reach.exchanges_hidden_from_the_observer += 1;
            }
        }
    }

    println!("reach: {reach:?}");

    let floors: [(&str, u64, u64); 10] = [
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
        (
            "enemy-team exchanges the observer cannot tell apart",
            reach.exchanges_hidden_from_the_observer,
            200,
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
