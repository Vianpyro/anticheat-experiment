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
//! Where a number was chosen rather than picked, it was chosen so that the
//! thirty-three seconds of the determinism fixture reach the rule it governs:
//! hit points low enough and the respawn delay short enough that death and
//! respawn happen inside the fixture rather than only inside a unit test. A
//! path exercised on one platform by a unit test and on three by nothing is a
//! path the cross-platform claim does not cover.

use crate::fx::Fx;
use crate::sha256::Digest;

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
    pub map_half_extent: Fx,
    /// Distance from the origin to a team's spawn line, along `x`.
    pub spawn_x: Fx,
    /// Spacing along `y` between the three seats of a team at spawn.
    pub spawn_spacing: Fx,
    /// `x` of Blue's outer tower. Red's is the negation.
    pub tower_outer_x: Fx,
    /// `x` of Blue's inner tower. Red's is the negation.
    pub tower_inner_x: Fx,

    /// Hit points a champion starts and respawns with.
    pub champion_max_hp: Fx,
    /// Distance a champion covers in one tick.
    pub champion_speed: Fx,
    /// Radius used for projectile collision.
    pub champion_radius: Fx,
    /// Ticks between death and respawn.
    pub respawn_ticks: u16,

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
    spawn_x: Fx::from_int(-100),
    spawn_spacing: Fx::from_int(3),
    tower_outer_x: Fx::from_int(-40),
    tower_inner_x: Fx::from_int(-90),

    champion_max_hp: Fx::from_int(350),
    // 6 units per second.
    champion_speed: Fx::from_ratio(6, TICKS_PER_SECOND),
    champion_radius: Fx::from_ratio(1, 2),
    // 5 seconds.
    respawn_ticks: 150,

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

    min_direction_length: Fx::from_ratio(1, 16),
};

/// The fingerprint of [`RULES`].
///
/// This is what the replay container will stamp into its manifest at M5 so that
/// a replay recorded under other numbers is rejected with a distinct error
/// rather than resimulated into a different match (`docs/RISKS.md` R2, R4). It
/// is stable across platforms for the same reason [`crate::State::digest`] is:
/// one canonical big-endian encoding, hashed with a hand-written SHA-256.
#[must_use]
pub fn rules_hash() -> Digest {
    crate::canonical::digest_of_rules(&RULES)
}
