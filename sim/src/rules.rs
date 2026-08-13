//! Every constant the rules read, in one struct.
//!
//! # Why a struct and not thirty `const` items
//!
//! Because of [`rules_hash`]. `docs/RISKS.md` R2 asks the replay container to
//! carry a hash covering the fixed-point parameters, the tick rate and the
//! balance constants, so that a replay recorded under different numbers is
//! rejected loudly instead of resimulating into garbage. A hash over loose
//! constants is a hash somebody forgets to extend the day they add the
//! thirty-first; a hash over a struct is one that stops compiling. Same
//! argument as [`crate::canonical`], same mechanism.
//!
//! # Balance
//!
//! The numbers are not tuned for play and are not meant to be. The game is a
//! fixture (`docs/SCOPE.md`); what matters is that the rules exercise movement,
//! a projectile, a targeted effect, a range check, death, respawn and a win
//! condition, and that changing any of them is a visible diff with a new hash.
//!
//! **No number here was chosen to make a test reach a code path.** That is
//! worth stating because it was briefly done and then undone: the hit points
//! and the respawn delay were once lowered so that death and respawn would
//! happen inside a thirty-second fixture. It works, and in six months it reads
//! as a design decision about how lethal the game is — a test requirement
//! wearing the costume of a rule. The fix is the one this module was already
//! built for: a fixture that needs different constants carries its own
//! [`Rules`] value and runs through [`crate::step_with_rules`], where
//! [`Rules::hash`] tells the two apart. Test coverage buys its own constants
//! instead of spending the game's.

use crate::fx::Fx;
use crate::sha256::Digest;
use crate::state::TEAM_COUNT;
use crate::vec2::FxVec2;

/// Simulation ticks per second.
///
/// Frozen by `docs/RISKS.md` R2 alongside the fractional width: every recorded
/// human match and every replay is expressed in these ticks, so changing it
/// retunes every trajectory in the corpus. `sim` itself never divides by it —
/// durations are counted in ticks — and it appears here only so that speeds
/// stay readable as "units per second" at the point of definition and so that
/// the hash covers it.
pub const TICKS_PER_SECOND: i32 = 30;

/// Every constant the rules read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rules {
    /// Fractional bits in [`Fx`], mirrored here so the hash covers it.
    pub frac_bits: u32,
    /// Ticks per second, mirrored here so the hash covers it.
    pub ticks_per_second: i32,

    /// Half the width and half the height of the square map, in world units.
    /// Positions are clamped into `[-extent, extent]` on both axes.
    ///
    /// The map is square and the *game* is a triangle inscribed in it: the
    /// legal coordinate domain `docs/ARCHITECTURE.md` states is a property of
    /// the fixed-point type and does not change shape when the layout does.
    pub map_half_extent: Fx,
    /// The three bases, in [`crate::Team`] order: the vertices of the triangle.
    ///
    /// A field rather than three, so that one destructuring in
    /// [`crate::canonical`] covers all of them and a fourth base could not be
    /// added without the hash noticing.
    pub bases: [FxVec2; TEAM_COUNT],
    /// Spacing between the three seats of a team at spawn, across the mouth of
    /// their own base.
    pub spawn_spacing: Fx,
    /// How far along its own lane a team's tower stands, as a fraction of the
    /// distance to the base at the other end.
    pub tower_lane_fraction: Fx,

    /// Hit points a champion starts and respawns with.
    pub champion_max_hp: Fx,
    /// Distance a champion covers in one tick.
    pub champion_speed: Fx,
    /// Radius used for projectile collision.
    pub champion_radius: Fx,
    /// Ticks between death and respawn.
    pub respawn_ticks: u16,
    /// How far a living champion lets its team see.
    pub champion_vision_radius: Fx,

    /// Maximum distance at which a basic attack lands.
    pub attack_range: Fx,
    /// Damage of one basic attack.
    pub attack_damage: Fx,
    /// Ticks between two basic attacks.
    pub attack_cooldown_ticks: u16,

    /// Distance the skillshot projectile covers in one tick.
    pub skillshot_speed: Fx,
    /// Collision radius of the projectile itself.
    pub skillshot_radius: Fx,
    /// Damage on hit. The projectile is consumed by the first thing it hits.
    pub skillshot_damage: Fx,
    /// Ticks before the projectile expires.
    pub skillshot_lifetime_ticks: u16,
    /// Ticks between two casts.
    pub skillshot_cooldown_ticks: u16,

    /// Maximum distance at which the targeted spell can be cast.
    pub targeted_range: Fx,
    /// Damage applied instantly on cast.
    pub targeted_damage: Fx,
    /// Ticks between two casts.
    pub targeted_cooldown_ticks: u16,

    /// Hit points a tower starts with.
    pub tower_max_hp: Fx,
    /// Radius used for projectile collision.
    pub tower_radius: Fx,
    /// Maximum distance at which a tower fires.
    pub tower_range: Fx,
    /// Damage of one tower shot.
    pub tower_damage: Fx,
    /// Ticks between two tower shots.
    pub tower_cooldown_ticks: u16,
    /// How far a standing tower lets its team see.
    pub tower_vision_radius: Fx,

    /// Shortest direction vector the rules will normalise.
    ///
    /// Below this length the integer square root has too few significant bits
    /// for the result to be near unit length — see
    /// [`crate::FxVec2::normalize_or_zero`]. A skillshot aimed with a shorter
    /// direction is discarded rather than fired somewhere arbitrary, and the
    /// cooldown is not consumed.
    pub min_direction_length: Fx,
}

/// The rules this build plays by.
pub const RULES: Rules = Rules {
    frac_bits: crate::fx::FRAC_BITS,
    ticks_per_second: TICKS_PER_SECOND,

    map_half_extent: Fx::from_int(128),
    // A triangle of circumradius 100, apex up: Blue at the top, Red at the
    // lower right, Green at the lower left. The two lower vertices are exact
    // mirrors of each other; the apex is not related to either by an exact
    // rotation, because a rotation by 120 degrees is not a fixed-point
    // operation. Sides come out at 173.2 units against a vision radius of 12,
    // so a lane is something you walk down in the dark.
    bases: [
        FxVec2::new(Fx::ZERO, Fx::from_int(100)),
        FxVec2::new(Fx::from_ratio(866, 10), Fx::from_int(-50)),
        FxVec2::new(Fx::from_ratio(-866, 10), Fx::from_int(-50)),
    ],
    spawn_spacing: Fx::from_int(3),
    // A quarter of the way down its own lane: far enough out that the two teams
    // contesting a lane meet between their towers rather than under one of
    // them, and close enough in that a tower still covers the ground its team
    // has to cross to leave.
    tower_lane_fraction: Fx::from_ratio(1, 4),

    champion_max_hp: Fx::from_int(600),
    // 6 units per second.
    champion_speed: Fx::from_ratio(6, TICKS_PER_SECOND),
    champion_radius: Fx::from_ratio(1, 2),
    // 15 seconds.
    respawn_ticks: 450,
    // Comfortably beyond every range a champion can act at — the targeted
    // spell reaches 6 and a tower reaches 7 — so that fog is a thing you walk
    // into rather than a thing that hides what is already hitting you. Well
    // under the 200 units between the two spawns, so that most of the map is
    // dark most of the time: a vision radius that covers the lane would make
    // `docs/MILESTONES.md` M2's exit criterion true without culling anything.
    champion_vision_radius: Fx::from_int(12),

    attack_range: Fx::from_ratio(5, 2),
    attack_damage: Fx::from_int(12),
    // 0.8 seconds.
    attack_cooldown_ticks: 24,

    // 18 units per second.
    skillshot_speed: Fx::from_ratio(18, TICKS_PER_SECOND),
    skillshot_radius: Fx::from_ratio(1, 2),
    skillshot_damage: Fx::from_int(80),
    // 1.5 seconds of flight.
    skillshot_lifetime_ticks: 45,
    // 8 seconds.
    skillshot_cooldown_ticks: 240,

    targeted_range: Fx::from_int(6),
    targeted_damage: Fx::from_int(60),
    // 12 seconds.
    targeted_cooldown_ticks: 360,

    tower_max_hp: Fx::from_int(2000),
    tower_radius: Fx::from_int(1),
    tower_range: Fx::from_int(7),
    tower_damage: Fx::from_int(45),
    // 1 second.
    tower_cooldown_ticks: 30,
    // Wider than the tower's own reach of 7, so that a tower sees what is about
    // to be in range of it rather than only what already is.
    tower_vision_radius: Fx::from_int(10),

    min_direction_length: Fx::from_ratio(1, 16),
};

impl Rules {
    /// The fingerprint of *these* constants.
    ///
    /// Separate from [`crate::State::digest`] on purpose, and that separation is
    /// what lets a fixture run under constants of its own without either
    /// contaminating the game's balance or weakening the determinism claim: two
    /// fixtures with different rules produce two different hashes, so a digest
    /// is never compared against one recorded under other numbers.
    ///
    /// It is stable across platforms for the same reason `State::digest` is:
    /// one canonical big-endian encoding, hashed with a hand-written SHA-256.
    #[must_use]
    pub fn hash(&self) -> Digest {
        crate::canonical::digest_of_rules(self)
    }
}

/// The fingerprint of [`RULES`], the constants this build plays by.
///
/// This is what the replay container will stamp into its manifest at M5 so that
/// a replay recorded under other numbers is rejected with a distinct error
/// rather than resimulated into a different match (`docs/RISKS.md` R2, R4).
#[must_use]
pub fn rules_hash() -> Digest {
    RULES.hash()
}
