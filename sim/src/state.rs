//! The world, and everything the digest has to cover.
//!
//! # What is deliberately *not* stored
//!
//! Several things a first draft would put in a struct are computed instead: a
//! champion's `EntityId` and team follow from its index, a tower's position and
//! team follow from its index and [`crate::RULES`]. The reason is not economy,
//! it is that redundant state can disagree with itself, and two states that
//! disagree in a field nothing reads still produce two different digests. Every
//! field below is one the rules actually write.
//!
//! Sizes are fixed: six champions, four towers, a projectile arena of a fixed
//! capacity, and no `Vec` anywhere. `docs/RISKS.md` R10 asks for this so that
//! the reusable-buffer `step_into` the reinforcement-learning sub-project will
//! eventually want is an addition beside `step` rather than a redesign of the
//! state.

use crate::event::Events;
use crate::fx::Fx;
use crate::rng::Rng;
use crate::rules::{RULES, Rules};
use crate::sha256::Digest;
use crate::vec2::FxVec2;

/// Players in a match, three per team.
pub const PLAYER_COUNT: usize = 6;

/// Players on one team.
pub const TEAM_SIZE: usize = 3;

/// Towers on the map, two per team.
pub const TOWER_COUNT: usize = 4;

/// Projectiles that can be in flight at once.
///
/// A cast that finds no free slot is dropped rather than queued. With one
/// skillshot on a long cooldown and six players this is unreachable in play;
/// the bound exists so that the arena is a fixed-size array, and dropping is
/// the behaviour that keeps the rules total.
pub const MAX_PROJECTILES: usize = 32;

/// First `EntityId` handed to a tower. Champions occupy `0..6`.
const TOWER_ID_BASE: u16 = 10;

/// First `EntityId` handed to a projectile.
const PROJECTILE_ID_BASE: u16 = 1000;

/// A simulation tick. The only notion of time `sim` has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Tick(pub u32);

/// A seat in the match, `0..6`. Seats `0..3` are [`Team::Blue`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PlayerId(pub u8);

/// A stable handle to something that can be targeted or hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EntityId(pub u16);

/// One of the two sides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Team {
    /// Spawns at negative `x` and defends the towers there.
    Blue,
    /// Spawns at positive `x`.
    Red,
}

impl Team {
    /// The other side.
    #[must_use]
    pub const fn opponent(self) -> Self {
        match self {
            Self::Blue => Self::Red,
            Self::Red => Self::Blue,
        }
    }

    /// The team a seat belongs to. Seats outside `0..6` are reported as
    /// [`Team::Blue`]; the callers that matter reject the seat before asking.
    #[must_use]
    pub const fn of_player(player: PlayerId) -> Self {
        if (player.0 as usize) < TEAM_SIZE {
            Self::Blue
        } else {
            Self::Red
        }
    }
}

/// Whether a champion is on the map, and what it is waiting for if not.
///
/// An enum rather than an `hp` field plus a `respawn_at` field, because exactly
/// one of the two is meaningful at any moment and a struct holding both invites
/// a state where a champion is alive with a pending respawn. Towers get the
/// simpler treatment — a destroyed tower carries no second field, so it is just
/// `hp == 0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Liveness {
    /// On the map, with the remaining hit points.
    Alive {
        /// Remaining hit points. Never negative: damage clamps at zero so that
        /// two different killing blows leave the same digest.
        hp: Fx,
    },
    /// Awaiting respawn on the tick given.
    Dead {
        /// The tick on which the champion returns.
        respawn_at: Tick,
    },
}

/// A champion's standing order, which survives across ticks until replaced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    /// Hold position.
    Idle,
    /// Walk to a point, then hold.
    MoveTo(FxVec2),
    /// Walk into range of an entity and attack it until it dies.
    Attack(EntityId),
}

/// Remaining cooldown for each ability, counted in ticks.
///
/// Ticks rather than a duration: a duration would need a tick rate to be
/// interpreted, and the tick rate is precisely the thing `docs/RISKS.md` R2
/// says must not be able to change quietly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Cooldowns {
    /// Ticks until the basic attack is available.
    pub basic_attack: u16,
    /// Ticks until the skillshot is available.
    pub skillshot: u16,
    /// Ticks until the targeted spell is available.
    pub targeted: u16,
}

/// One of the six champions. Its identity is its index in
/// [`State::champions`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Champion {
    /// Where it stands. Always inside the map.
    pub position: FxVec2,
    /// Alive with hit points, or dead with a respawn tick.
    pub liveness: Liveness,
    /// What it is currently doing.
    pub order: Order,
    /// Remaining ability cooldowns.
    pub cooldowns: Cooldowns,
}

/// One of the four towers. Its position and team follow from its index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tower {
    /// Remaining hit points; zero means destroyed, permanently.
    pub hp: Fx,
    /// Ticks until it can fire again.
    pub cooldown: u16,
}

impl Tower {
    /// Whether the tower still stands.
    #[must_use]
    pub const fn is_standing(self) -> bool {
        self.hp.to_raw() > 0
    }
}

/// A skillshot in flight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Projectile {
    /// Allocated at cast time and never reused within a match, so that a
    /// visible-event stream can refer to it.
    pub id: EntityId,
    /// Who cast it. Determines what it can hit.
    pub owner: PlayerId,
    /// Current position.
    pub position: FxVec2,
    /// Displacement applied each tick. Constant for the projectile's life.
    pub velocity: FxVec2,
    /// Ticks before it expires.
    pub remaining: u16,
}

/// The bounded arena of projectiles.
///
/// A fixed array of slots rather than a `Vec`, and allocation always takes the
/// lowest free index. Both halves matter for determinism: the capacity means no
/// allocator is involved, and the lowest-index rule means the arena's layout is
/// a function of the history of casts rather than of anything ambient.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Projectiles {
    pub(crate) slots: [Option<Projectile>; MAX_PROJECTILES],
}

impl Default for Projectiles {
    fn default() -> Self {
        Self::new()
    }
}

impl Projectiles {
    /// An empty arena.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [None; MAX_PROJECTILES],
        }
    }

    /// Every projectile in flight, in slot order.
    pub fn iter(&self) -> impl Iterator<Item = &Projectile> {
        self.slots.iter().flatten()
    }

    /// How many are in flight.
    #[must_use]
    pub fn count(&self) -> usize {
        self.iter().count()
    }

    /// Whether another projectile would fit.
    #[must_use]
    pub fn has_free_slot(&self) -> bool {
        self.slots.iter().any(Option::is_none)
    }

    /// Places a projectile in the lowest free slot. Returns `false`, and drops
    /// it, when the arena is full.
    pub(crate) fn insert(&mut self, projectile: Projectile) -> bool {
        for slot in &mut self.slots {
            if slot.is_none() {
                *slot = Some(projectile);
                return true;
            }
        }
        false
    }
}

/// How the match ended, if it has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Still being played.
    InProgress,
    /// One team has lost both towers.
    Decided {
        /// The team that still has a tower standing.
        winner: Team,
        /// The tick on which the last tower fell.
        at: Tick,
    },
}

/// The whole world at one instant.
///
/// **This type is not serializable and must never become so.** That is the
/// mechanism behind the maphack defence: a server cannot accidentally send the
/// world to a client if the world has no encoding. `docs/RISKS.md` R5 states
/// the rule and `docs/ARCHITECTURE.md` answers the two pressures that will push
/// against it — mid-match reconnection and test fixtures — in advance. The
/// enforcement today is that `sim` has no serialization dependency at all,
/// which is stronger than a lint, plus a check in CI for when the view types
/// bring `serde` in at M2.
///
/// [`State::digest`] is the only way to compare two of these.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub(crate) tick: Tick,
    pub(crate) rng: Rng,
    pub(crate) next_projectile_id: u16,
    pub(crate) champions: [Champion; PLAYER_COUNT],
    pub(crate) towers: [Tower; TOWER_COUNT],
    pub(crate) projectiles: Projectiles,
    /// What happened during the tick that produced this state, cleared at the
    /// top of the next one. Under the digest deliberately: see
    /// `crate::event`.
    pub(crate) events: Events,
    pub(crate) outcome: Outcome,
}

/// The initial world for a match, under [`crate::RULES`].
///
/// The seed is the only input. Everything else — spawn positions, hit points,
/// tower placement — comes from the rules, so two servers that agree on the
/// seed and the rules hash agree on the whole match.
#[must_use]
pub fn new_state(seed: u64) -> State {
    new_state_with_rules(seed, &RULES)
}

/// The initial world for a match played under constants other than
/// [`crate::RULES`].
///
/// The reason this exists is stated in [`crate::step_with_rules`]: a test that
/// needs a shorter respawn or a frailer champion says so in its own [`Rules`]
/// value, instead of moving the number the game is played by.
#[must_use]
pub fn new_state_with_rules(seed: u64, rules: &Rules) -> State {
    let mut champions = [Champion {
        position: FxVec2::ZERO,
        liveness: Liveness::Alive {
            hp: rules.champion_max_hp,
        },
        order: Order::Idle,
        cooldowns: Cooldowns::default(),
    }; PLAYER_COUNT];
    for (index, champion) in champions.iter_mut().enumerate() {
        champion.position = spawn_position(index, rules);
    }

    State {
        tick: Tick(0),
        rng: Rng::from_seed(seed),
        next_projectile_id: PROJECTILE_ID_BASE,
        champions,
        towers: [Tower {
            hp: rules.tower_max_hp,
            cooldown: 0,
        }; TOWER_COUNT],
        projectiles: Projectiles::new(),
        events: Events::new(),
        outcome: Outcome::InProgress,
    }
}

/// Where the champion in `index` starts, and returns to on respawn.
///
/// The three seats of a team are spread along `y` so that they do not begin
/// stacked on one point, which would make the first tick's collision geometry
/// degenerate.
pub(crate) fn spawn_position(index: usize, rules: &Rules) -> FxVec2 {
    let team = Team::of_player(PlayerId(index as u8));
    let seat = index.checked_rem(TEAM_SIZE).unwrap_or(0);
    let offset = Fx::from_int(seat as i32)
        .sub(Fx::ONE)
        .mul(rules.spawn_spacing);
    let x = match team {
        Team::Blue => rules.spawn_x,
        Team::Red => rules.spawn_x.neg(),
    };
    FxVec2::new(x, offset)
}

/// Where the tower in `index` stands. Towers `0..2` are Blue's, outer first.
///
/// Takes its rules explicitly rather than reading [`crate::RULES`]: tower
/// placement is derived from the constants, so a caller simulating under other
/// constants must be unable to ask this question without saying which ones.
#[must_use]
pub fn tower_position(index: usize, rules: &Rules) -> FxVec2 {
    let x = match index {
        0 => rules.tower_outer_x,
        1 => rules.tower_inner_x,
        2 => rules.tower_outer_x.neg(),
        _ => rules.tower_inner_x.neg(),
    };
    FxVec2::new(x, Fx::ZERO)
}

/// Where an [`EntityId`] points, once it has been checked.
///
/// Not an `Entity` trait and not a sum type holding the entity itself: the
/// rules only ever need to know *which array and which index*, and everything
/// else is a lookup on the state they already hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityRef {
    /// The champion in this seat.
    Champion(usize),
    /// The tower at this index.
    Tower(usize),
}

/// Which team an entity belongs to.
#[must_use]
pub const fn entity_team(entity: EntityRef) -> Team {
    match entity {
        EntityRef::Champion(seat) => Team::of_player(PlayerId(seat as u8)),
        EntityRef::Tower(index) => tower_team(index),
    }
}

/// Which team owns the tower in `index`.
#[must_use]
pub const fn tower_team(index: usize) -> Team {
    if index < 2 { Team::Blue } else { Team::Red }
}

/// The handle of the champion in `index`.
#[must_use]
pub const fn champion_entity_id(index: usize) -> EntityId {
    EntityId(index as u16)
}

/// The handle of the tower in `index`.
#[must_use]
pub const fn tower_entity_id(index: usize) -> EntityId {
    EntityId(TOWER_ID_BASE.saturating_add(index as u16))
}

/// The handle of a resolved entity.
#[must_use]
pub const fn entity_id(entity: EntityRef) -> EntityId {
    match entity {
        EntityRef::Champion(seat) => champion_entity_id(seat),
        EntityRef::Tower(index) => tower_entity_id(index),
    }
}

impl State {
    /// The tick this state describes. Advanced by exactly one per
    /// [`crate::step`].
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// Whether the match has been decided, and by whom.
    #[must_use]
    pub const fn outcome(&self) -> Outcome {
        self.outcome
    }

    /// The champions, in seat order.
    #[must_use]
    pub const fn champions(&self) -> &[Champion; PLAYER_COUNT] {
        &self.champions
    }

    /// The towers, in the order given by [`tower_position`].
    #[must_use]
    pub const fn towers(&self) -> &[Tower; TOWER_COUNT] {
        &self.towers
    }

    /// The projectiles in flight.
    #[must_use]
    pub const fn projectiles(&self) -> &Projectiles {
        &self.projectiles
    }

    /// What happened during the tick that produced this state.
    ///
    /// This is the full, uncensored record. Nothing outside `sim` may send it
    /// to a client: [`crate::view::view_for`] is what decides which of these a
    /// given player is allowed to learn about.
    #[must_use]
    pub const fn events(&self) -> &Events {
        &self.events
    }

    /// The 32-byte fingerprint of the entire state.
    ///
    /// This is the only sanctioned way to compare two states, and the value the
    /// determinism suite compares across three CPU targets. The encoding, and
    /// the invariant that keeps it covering the whole state as the state grows,
    /// are documented in `sim/src/canonical.rs` and in `docs/ARCHITECTURE.md`.
    #[must_use]
    pub fn digest(&self) -> Digest {
        crate::canonical::digest_of(self)
    }

    /// Resolves a handle to something that can be hit **right now**.
    ///
    /// A dead champion, a destroyed tower and a handle that never named
    /// anything are all `None`, deliberately indistinguishable: the rules
    /// treat "cannot be targeted" as one case, and the caller that wants to
    /// tell them apart does not exist. Projectiles are not resolvable at all —
    /// nothing in the game can target one.
    pub(crate) fn resolve(&self, id: EntityId) -> Option<EntityRef> {
        let handle = id.0 as usize;
        if handle < PLAYER_COUNT {
            let champion = self.champions.get(handle)?;
            return match champion.liveness {
                Liveness::Alive { .. } => Some(EntityRef::Champion(handle)),
                Liveness::Dead { .. } => None,
            };
        }
        let tower = handle.checked_sub(TOWER_ID_BASE as usize)?;
        if tower >= TOWER_COUNT {
            return None;
        }
        if self.towers.get(tower)?.is_standing() {
            Some(EntityRef::Tower(tower))
        } else {
            None
        }
    }

    /// Where a resolved entity stands.
    pub(crate) fn entity_position(&self, entity: EntityRef, rules: &Rules) -> FxVec2 {
        match entity {
            EntityRef::Champion(seat) => self
                .champions
                .get(seat)
                .map_or(FxVec2::ZERO, |champion| champion.position),
            EntityRef::Tower(index) => tower_position(index, rules),
        }
    }

    /// Allocates the next projectile handle.
    ///
    /// Wraps back to the base rather than overflowing. At one projectile per
    /// cast and a cooldown measured in seconds, a match cannot reach the wrap;
    /// it is defined anyway because "cannot happen" is not a thing this project
    /// gets to assume about a counter an attacker can drive.
    pub(crate) fn allocate_projectile_id(&mut self) -> EntityId {
        let id = EntityId(self.next_projectile_id);
        self.next_projectile_id = if self.next_projectile_id == u16::MAX {
            PROJECTILE_ID_BASE
        } else {
            self.next_projectile_id.saturating_add(1)
        };
        id
    }
}

/// Direct constructors for unit tests, and nothing else.
///
/// This is the one narrow door `docs/ARCHITECTURE.md` leaves open: a
/// `#[cfg(test)]` block is compiled only while building `sim`'s own test
/// target, so it is unreachable from every other crate and from `sim`'s own
/// integration tests. It builds states; it still cannot encode one.
#[cfg(test)]
impl State {
    /// Places a champion, alive, at a chosen point with chosen hit points.
    pub(crate) fn place_champion(&mut self, index: usize, position: FxVec2, hp: Fx) {
        if let Some(champion) = self.champions.get_mut(index) {
            champion.position = position;
            champion.liveness = Liveness::Alive { hp };
        }
    }

    /// Sets a tower's remaining hit points.
    pub(crate) fn set_tower_hp(&mut self, index: usize, hp: Fx) {
        if let Some(tower) = self.towers.get_mut(index) {
            tower.hp = hp;
        }
    }
}
