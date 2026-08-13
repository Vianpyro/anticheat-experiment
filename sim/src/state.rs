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
//! Sizes are fixed: nine champions, six towers, a projectile arena of a fixed
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
pub const PLAYER_COUNT: usize = 9;

/// Players on one team.
pub const TEAM_SIZE: usize = 3;

/// Teams in a match.
///
/// Three, on a triangular map: a base at each vertex, a lane along each edge,
/// and every lane contested by exactly the two teams whose bases it joins. The
/// third team is not a variation on the two-team game — it is a different game,
/// and `docs/SCOPE.md` records the exploit class it introduces that a two-team
/// format does not have.
pub const TEAM_COUNT: usize = 3;

/// Towers on the map, two per team: one on each of the two lanes that leave a
/// team's own base.
pub const TOWER_COUNT: usize = 6;

/// Projectiles that can be in flight at once.
///
/// A cast that finds no free slot is dropped rather than queued. The skillshot's
/// lifetime is shorter than its cooldown, so a seat has at most one projectile
/// in flight and the arena cannot hold more than [`PLAYER_COUNT`] of them under
/// the game's constants; the capacity is far above that. The bound exists so
/// that the arena is a fixed-size array, and dropping is the behaviour that
/// keeps the rules total under a fixture's own constants, where the ratio can
/// go the other way.
///
/// The sentence above used to be the whole guarantee, which is the failure mode
/// [`MAX_PROJECTILES_IN_FLIGHT`] exists to remove: it is the same derivation,
/// executable, and the assertion beside it is what a change to the roster or to
/// the two constants it reads runs into.
pub const MAX_PROJECTILES: usize = 32;

/// Projectiles the rules can actually have in flight at once, derived.
///
/// A seat casts at most once every `cooldown_ticks` and each cast lives
/// `lifetime_ticks`, so a seat holds at most `ceil(lifetime / cooldown)` of them
/// at any moment. Under [`RULES`] that is `ceil(45 / 240)` — one — and nine
/// seats make nine.
///
/// Unclamped by the arena on purpose: clamping would make the assertion below
/// tautological, and the question it exists to ask is whether the arena is still
/// wide enough for what the rules can produce.
pub const MAX_PROJECTILES_IN_FLIGHT: usize = projectiles_in_flight(
    PLAYER_COUNT,
    RULES.skillshot_lifetime_ticks,
    RULES.skillshot_cooldown_ticks,
);

/// How many projectiles `seats` seats can hold in flight at once, given the
/// lifetime and the cooldown of the cast that produces them.
///
/// A `const fn` rather than a sentence in a comment, so that the derivation is
/// the thing the compiler checks rather than the thing a reader is asked to
/// re-do. A cooldown of zero means a seat may cast every tick, which no
/// constants in this repository use and which the arena is the only bound on;
/// it is answered rather than divided by.
#[must_use]
pub const fn projectiles_in_flight(
    seats: usize,
    lifetime_ticks: u16,
    cooldown_ticks: u16,
) -> usize {
    if cooldown_ticks == 0 {
        return usize::MAX;
    }
    let per_seat = (lifetime_ticks as usize).div_ceil(cooldown_ticks as usize);
    seats.saturating_mul(per_seat)
}

/// The arena has to be at least as wide as the rules can fill it, or a cast the
/// game's own cooldowns permit is dropped for want of a slot.
///
/// This is the sentence in [`MAX_PROJECTILES`]'s documentation, made executable.
/// It was true and unchecked through three milestones, which is exactly how long
/// a comment stays right by accident.
const _: () = assert!(
    MAX_PROJECTILES >= MAX_PROJECTILES_IN_FLIGHT,
    "the projectile arena is narrower than the rules can fill it: raise MAX_PROJECTILES, \
     or lower the skillshot's lifetime against its cooldown"
);

/// The roster is three teams of three, and three places say so. A fourth seat
/// per team that reached only one of them would leave the other two describing a
/// match nobody plays.
const _: () = assert!(PLAYER_COUNT == TEAM_COUNT.saturating_mul(TEAM_SIZE));

/// Two towers per team, one on each lane leaving its own base.
const _: () = assert!(TOWER_COUNT == TEAM_COUNT.saturating_mul(2));

/// First `EntityId` handed to a tower. Champions occupy `0..9`.
///
/// Public because the handle layout is not an implementation detail: a
/// champion's handle *is* its seat and a tower's handle is derivable from the
/// rules, which is why `docs/ARCHITECTURE.md` records both as information a
/// client may have. `protocol` reads these two constants to tell the handles a
/// recipient may keep from the ones it must be given a private name for.
pub const TOWER_ID_BASE: u16 = 10;

/// First `EntityId` handed to a projectile.
///
/// The one handle range that is *not* public information. It comes from a
/// match-global counter, so a gap between two handles a player was shown counts
/// the casts that happened out of their sight — which is why `protocol` gives
/// every recipient a handle space of its own above this base.
pub const PROJECTILE_ID_BASE: u16 = 1000;

/// The three handle ranges do not overlap, and the margins are thinner than
/// they look: `TOWER_ID_BASE` is *one* above the last champion handle. A fourth
/// seat per team would put a champion on a tower's handle, and every consumer of
/// the layout — `protocol`'s handle space, the client's local world, the
/// culling proof — would agree about a world with two entities wearing one name.
/// Nothing would fail; the wrong thing would simply be true everywhere at once.
const _: () = assert!(TOWER_ID_BASE as usize >= PLAYER_COUNT);
const _: () = assert!(PROJECTILE_ID_BASE as usize >= (TOWER_ID_BASE as usize).saturating_add(TOWER_COUNT));

/// A simulation tick. The only notion of time `sim` has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Tick(pub u32);

/// A seat in the match. There are exactly nine, and there is no tenth.
///
/// # Why this is an enum and not an integer
///
/// It was `PlayerId(pub u8)` until M3, with a comment saying `0..6` and nothing
/// enforcing it. `Team::of_player(PlayerId(200))` answered `Red`, and
/// [`crate::view::view_for`] handed that seat a view — a total function over a
/// domain that included seats nobody is sitting in. While the only caller was a
/// test that constructed the number itself, that was inert. From M3 the seat
/// arrives from a session, the session arrives from the network, and the
/// function that would have distributed a team's vision to an unvalidated
/// handle is the most sensitive one in the project.
///
/// So the impossibility is carried by the type rather than by a check inside
/// the projection. There is no `Seat` value outside the match, which means
/// there is no branch in `view_for` deciding what to do about one — and a
/// branch in the culling function is where a maphack lives. The validation that
/// remains is [`Seat::from_index`], and it belongs at the frontier where an
/// untrusted byte becomes a seat: `protocol`'s decoder, and nowhere else.
///
/// Seats `Blue0..Blue2` are [`Team::Blue`] and so on round the triangle; the
/// discriminants are the indices into every per-seat array in this crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Seat {
    /// Blue's first seat.
    Blue0 = 0,
    /// Blue's second seat.
    Blue1 = 1,
    /// Blue's third seat.
    Blue2 = 2,
    /// Red's first seat.
    Red0 = 3,
    /// Red's second seat.
    Red1 = 4,
    /// Red's third seat.
    Red2 = 5,
    /// Green's first seat.
    Green0 = 6,
    /// Green's second seat.
    Green1 = 7,
    /// Green's third seat.
    Green2 = 8,
}

impl Seat {
    /// Every seat, in index order.
    pub const ALL: [Self; PLAYER_COUNT] = [
        Self::Blue0,
        Self::Blue1,
        Self::Blue2,
        Self::Red0,
        Self::Red1,
        Self::Red2,
        Self::Green0,
        Self::Green1,
        Self::Green2,
    ];

    /// The index this seat occupies in every per-seat array.
    ///
    /// Total and always in bounds: the assertion below ties the number of
    /// variants to [`PLAYER_COUNT`], so indexing a `[T; PLAYER_COUNT]` with
    /// this value cannot panic and no accessor needs an `unwrap` or a fallback
    /// that would have to invent a champion.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Which side this seat plays for.
    ///
    /// Exhaustive rather than arithmetic on the index: a tenth variant would
    /// stop this compiling instead of quietly joining Green.
    #[must_use]
    pub const fn team(self) -> Team {
        match self {
            Self::Blue0 | Self::Blue1 | Self::Blue2 => Team::Blue,
            Self::Red0 | Self::Red1 | Self::Red2 => Team::Red,
            Self::Green0 | Self::Green1 | Self::Green2 => Team::Green,
        }
    }

    /// The seat at `index`, or `None`.
    ///
    /// **This is the only door into the type, and it exists for exactly one
    /// caller: the code that turns a byte off the wire into a seat.** Anything
    /// else already has a `Seat` or has [`Seat::ALL`] to iterate. A `None` here
    /// is a protocol violation and must be answered as one — never by
    /// substituting a seat.
    #[must_use]
    pub const fn from_index(index: u8) -> Option<Self> {
        match index {
            0 => Some(Self::Blue0),
            1 => Some(Self::Blue1),
            2 => Some(Self::Blue2),
            3 => Some(Self::Red0),
            4 => Some(Self::Red1),
            5 => Some(Self::Red2),
            6 => Some(Self::Green0),
            7 => Some(Self::Green1),
            8 => Some(Self::Green2),
            _ => None,
        }
    }

    /// This seat's position within its own team, `0..3`.
    #[must_use]
    pub const fn within_team(self) -> usize {
        match self {
            Self::Blue0 | Self::Red0 | Self::Green0 => 0,
            Self::Blue1 | Self::Red1 | Self::Green1 => 1,
            Self::Blue2 | Self::Red2 | Self::Green2 => 2,
        }
    }
}

/// What makes [`Seat::index`] a total index rather than a hopeful one. A
/// variant added without growing the arrays stops the build here.
const _: () = assert!(Seat::ALL.len() == PLAYER_COUNT);

/// A stable handle to something that can be targeted or hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EntityId(pub u16);

/// One of the three sides.
///
/// Three rather than two, and there is deliberately no `opponent()`: a team has
/// two of them, and a function that had to pick one would be picking it for a
/// rule that should be asking "not mine" instead. Every rule that used it — the
/// towers' target selection, the projectile's collision filter — now compares
/// team membership directly, which is the form that stayed correct when the
/// third team arrived.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Team {
    /// Base at the apex of the triangle.
    Blue = 0,
    /// Base at the lower-right vertex.
    Red = 1,
    /// Base at the lower-left vertex.
    Green = 2,
}

impl Team {
    /// Every team, in index order.
    pub const ALL: [Self; TEAM_COUNT] = [Self::Blue, Self::Red, Self::Green];

    /// The index this team occupies in every per-team array.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// What makes [`Team::index`] a total index. A variant added without growing
/// the arrays stops the build here.
const _: () = assert!(Team::ALL.len() == TEAM_COUNT);

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

/// One of the nine champions. Its identity is its index in
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

/// One of the six towers. Its position and team follow from its index.
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
    pub owner: Seat,
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
    for seat in Seat::ALL {
        champions[seat.index()].position = spawn_position(seat, rules);
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

/// Where a team's base stands.
///
/// The three bases are the vertices of the triangle, and they are constants
/// rather than a rotation of one another: a rotation by 120 degrees needs
/// `sqrt(3)/2`, which is not a fixed-point number, so a computed layout would
/// be three approximations with a trigonometric function in the middle of the
/// rules. Written down, they are exact values the hash covers.
///
/// The map is symmetric about `x = 0` and not under rotation, which is a real
/// asymmetry between Blue and the other two and is stated rather than papered
/// over. It is worth about a raw unit of position, on a map two hundred units
/// across, in a game `docs/SCOPE.md` calls a fixture.
#[must_use]
pub fn base_position(team: Team, rules: &Rules) -> FxVec2 {
    rules.bases[team.index()]
}

/// Where the champion in `seat` starts, and returns to on respawn.
///
/// The three seats of a team are spread across the mouth of their own base —
/// perpendicular to the line from the origin — so that they do not begin
/// stacked on one point, which would make the first tick's collision geometry
/// degenerate, and so that the spread is the same shape for all three teams
/// rather than along an axis that means something different to each of them.
pub(crate) fn spawn_position(seat: Seat, rules: &Rules) -> FxVec2 {
    let base = base_position(seat.team(), rules);
    let across = FxVec2::new(base.y.neg(), base.x).normalize_or_zero();
    let offset = Fx::from_int(seat.within_team() as i32)
        .sub(Fx::ONE)
        .mul(rules.spawn_spacing);
    base.add(across.scale(offset))
}

/// The lane a tower stands on: whose tower it is, and which base the lane runs
/// to.
///
/// A table rather than arithmetic on the index. Three lanes — Blue–Red,
/// Red–Green, Green–Blue — each contested by exactly the two teams whose bases
/// it joins, and each carrying one tower per contestant, at that contestant's
/// own end. Two towers per team, two lanes per team, and every entry visible in
/// one place.
#[must_use]
pub const fn tower_lane(index: usize) -> (Team, Team) {
    match index {
        0 => (Team::Blue, Team::Red),
        1 => (Team::Blue, Team::Green),
        2 => (Team::Red, Team::Green),
        3 => (Team::Red, Team::Blue),
        4 => (Team::Green, Team::Blue),
        _ => (Team::Green, Team::Red),
    }
}

/// Where the tower in `index` stands: along its own lane, a fixed fraction of
/// the way from its own base toward the base at the other end.
///
/// Takes its rules explicitly rather than reading [`crate::RULES`]: tower
/// placement is derived from the constants, so a caller simulating under other
/// constants must be unable to ask this question without saying which ones.
#[must_use]
pub fn tower_position(index: usize, rules: &Rules) -> FxVec2 {
    let (own, toward) = tower_lane(index);
    let from = base_position(own, rules);
    let to = base_position(toward, rules);
    from.add(to.sub(from).scale(rules.tower_lane_fraction))
}

/// Where an [`EntityId`] points, once it has been checked.
///
/// Not an `Entity` trait and not a sum type holding the entity itself: the
/// rules only ever need to know *which array and which index*, and everything
/// else is a lookup on the state they already hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityRef {
    /// The champion in this seat.
    Champion(Seat),
    /// The tower at this index.
    Tower(usize),
}

/// Which team an entity belongs to.
#[must_use]
pub const fn entity_team(entity: EntityRef) -> Team {
    match entity {
        EntityRef::Champion(seat) => seat.team(),
        EntityRef::Tower(index) => tower_team(index),
    }
}

/// Which team owns the tower in `index`.
#[must_use]
pub const fn tower_team(index: usize) -> Team {
    tower_lane(index).0
}

/// The handle of the champion in `seat`.
///
/// The handle *is* the seat, which is why `docs/ARCHITECTURE.md` records a
/// champion's team as public information: it follows from the number.
#[must_use]
pub const fn champion_entity_id(seat: Seat) -> EntityId {
    EntityId(seat as u16)
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

    /// The champion in `seat`.
    ///
    /// Total, and not because of a fallback: [`Seat`] has exactly
    /// [`PLAYER_COUNT`] values and the array has exactly that many slots, which
    /// is asserted at compile time beside [`Seat::index`]. Every caller that
    /// used to write `champions().get(seat)` and invent something for `None`
    /// calls this instead — the invented champion was the shape of bug the
    /// seat type exists to remove.
    #[must_use]
    pub const fn champion(&self, seat: Seat) -> &Champion {
        &self.champions[seat.index()]
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

    /// The champion in `seat`, for the rules to write to. Total for the reason
    /// [`State::champion`] is.
    pub(crate) const fn champion_mut(&mut self, seat: Seat) -> &mut Champion {
        &mut self.champions[seat.index()]
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
            let seat = Seat::from_index(u8::try_from(handle).ok()?)?;
            return match self.champion(seat).liveness {
                Liveness::Alive { .. } => Some(EntityRef::Champion(seat)),
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
            EntityRef::Champion(seat) => self.champion(seat).position,
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
    pub(crate) fn place_champion(&mut self, seat: Seat, position: FxVec2, hp: Fx) {
        let champion = &mut self.champions[seat.index()];
        champion.position = position;
        champion.liveness = Liveness::Alive { hp };
    }

    /// Sets a tower's remaining hit points.
    pub(crate) fn set_tower_hp(&mut self, index: usize, hp: Fx) {
        if let Some(tower) = self.towers.get_mut(index) {
            tower.hp = hp;
        }
    }
}
