//! The canonical byte encoding that [`State::digest`] hashes.
//!
//! # The invariant this module exists to hold
//!
//! **Every implementation below destructures its type exhaustively. `..` is
//! forbidden in a pattern here, and so is a `_` arm in a `match`.**
//!
//! `State` is deliberately not serializable (`docs/RISKS.md` R5), so there is
//! no derive to lean on and the encoding is written by hand. The danger is not
//! that it is wrong today — a wrong encoding fails the first time the fixture
//! runs. The danger is the field somebody adds in eight months and does not
//! hash. Nothing would break: the determinism suite would stay green while
//! quietly no longer covering part of the state, and the first thing to notice
//! would be two servers disagreeing about a match that both of them digested
//! identically. A silent gap in the digest is a silent gap in every claim built
//! on top of it, up to and including "this replay is the match that was
//! played".
//!
//! Exhaustive destructuring converts that silence into a compile error. Add a
//! field to `State` and `let State { .. } = self` stops compiling. Add a
//! variant to `Order` and its `match` stops compiling. Bind a field and forget
//! to hash it and `unused_variables`, denied at the crate root, stops the
//! build. The rule is mechanical, which is the only kind of rule that survives
//! a project revisited every few months.
//!
//! The one type not destructured here is [`Fx`], whose single field is private
//! to its module, so [`Fx::to_raw`] is its documented canonical encoding
//! instead. That leaves the exhaustiveness argument resting on a convention —
//! "nobody will add a second field" — which is precisely the kind of claim this
//! module refuses to make anywhere else, so `sim/src/fx.rs` carries a
//! compile-time assertion on the size of the type: a second field of any
//! non-zero size stops the build. The residual case, a zero-sized field, is
//! documented beside that assertion.
//!
//! # The encoding
//!
//! Fixed-width big-endian integers, in declaration order, with a one-byte tag
//! before the payload of every enum and every `Option`. Nothing is
//! length-prefixed because nothing is variable-length: the arena is a fixed
//! array of `Option<Projectile>` and an empty slot encodes as its own tag byte.
//! The encoding is never parsed, only hashed, so it needs to be unambiguous
//! rather than self-describing — and it is, because every value's width is a
//! function of its type alone.

use crate::event::{Ability, Event, EventKind, Events};
use crate::fx::Fx;
use crate::input::{Action, Input};
use crate::rng::Rng;
use crate::rules::Rules;
use crate::sha256::{Digest, Hasher};
use crate::state::{
    Champion, Cooldowns, EntityId, Liveness, Order, Outcome, PlayerId, Projectile, Projectiles,
    State, Team, Tick, Tower,
};
use crate::vec2::FxVec2;

/// Domain separator, so that a state digest and a rules digest can never
/// collide even if their encodings happened to coincide.
const DOMAIN_STATE: &[u8] = b"moba/sim/state/v1";
const DOMAIN_RULES: &[u8] = b"moba/sim/rules/v1";
const DOMAIN_INPUTS: &[u8] = b"moba/sim/inputs/v1";

/// Writes a value's canonical bytes into a hasher.
pub(crate) trait Canonical {
    /// Appends the encoding of `self`. See the module invariant.
    fn hash_into(&self, hasher: &mut Hasher);
}

/// The digest of a whole state.
pub(crate) fn digest_of(state: &State) -> Digest {
    let mut hasher = Hasher::new();
    hasher.update(DOMAIN_STATE);
    state.hash_into(&mut hasher);
    hasher.finish()
}

/// The digest of the rules a build plays by.
pub(crate) fn digest_of_rules(rules: &Rules) -> Digest {
    let mut hasher = Hasher::new();
    hasher.update(DOMAIN_RULES);
    rules.hash_into(&mut hasher);
    hasher.finish()
}

/// The digest of an input log, in the order given.
///
/// Order is part of the value: `step` neither sorts nor deduplicates
/// (`docs/ARCHITECTURE.md`), so two logs that differ only in order are two
/// different logs and must digest differently.
pub(crate) fn digest_of_inputs(inputs: &[Input]) -> Digest {
    let mut hasher = Hasher::new();
    hasher.update(DOMAIN_INPUTS);
    (inputs.len() as u64).hash_into(&mut hasher);
    for input in inputs {
        input.hash_into(&mut hasher);
    }
    hasher.finish()
}

impl Canonical for u8 {
    fn hash_into(&self, hasher: &mut Hasher) {
        hasher.update(&[*self]);
    }
}

impl Canonical for u16 {
    fn hash_into(&self, hasher: &mut Hasher) {
        hasher.update(&self.to_be_bytes());
    }
}

impl Canonical for u32 {
    fn hash_into(&self, hasher: &mut Hasher) {
        hasher.update(&self.to_be_bytes());
    }
}

impl Canonical for u64 {
    fn hash_into(&self, hasher: &mut Hasher) {
        hasher.update(&self.to_be_bytes());
    }
}

impl Canonical for i32 {
    fn hash_into(&self, hasher: &mut Hasher) {
        hasher.update(&self.to_be_bytes());
    }
}

impl<T: Canonical> Canonical for Option<T> {
    fn hash_into(&self, hasher: &mut Hasher) {
        match self {
            Self::None => 0u8.hash_into(hasher),
            Self::Some(value) => {
                1u8.hash_into(hasher);
                value.hash_into(hasher);
            }
        }
    }
}

impl<T: Canonical, const N: usize> Canonical for [T; N] {
    fn hash_into(&self, hasher: &mut Hasher) {
        for value in self {
            value.hash_into(hasher);
        }
    }
}

impl Canonical for Fx {
    fn hash_into(&self, hasher: &mut Hasher) {
        self.to_raw().hash_into(hasher);
    }
}

impl Canonical for FxVec2 {
    fn hash_into(&self, hasher: &mut Hasher) {
        let FxVec2 { x, y } = self;
        x.hash_into(hasher);
        y.hash_into(hasher);
    }
}

impl Canonical for Tick {
    fn hash_into(&self, hasher: &mut Hasher) {
        let Tick(value) = self;
        value.hash_into(hasher);
    }
}

impl Canonical for PlayerId {
    fn hash_into(&self, hasher: &mut Hasher) {
        let PlayerId(value) = self;
        value.hash_into(hasher);
    }
}

impl Canonical for EntityId {
    fn hash_into(&self, hasher: &mut Hasher) {
        let EntityId(value) = self;
        value.hash_into(hasher);
    }
}

impl Canonical for Team {
    fn hash_into(&self, hasher: &mut Hasher) {
        match self {
            Self::Blue => 0u8.hash_into(hasher),
            Self::Red => 1u8.hash_into(hasher),
        }
    }
}

impl Canonical for Rng {
    fn hash_into(&self, hasher: &mut Hasher) {
        let Rng { state } = self;
        state.hash_into(hasher);
    }
}

impl Canonical for Liveness {
    fn hash_into(&self, hasher: &mut Hasher) {
        match self {
            Self::Alive { hp } => {
                0u8.hash_into(hasher);
                hp.hash_into(hasher);
            }
            Self::Dead { respawn_at } => {
                1u8.hash_into(hasher);
                respawn_at.hash_into(hasher);
            }
        }
    }
}

impl Canonical for Order {
    fn hash_into(&self, hasher: &mut Hasher) {
        match self {
            Self::Idle => 0u8.hash_into(hasher),
            Self::MoveTo(destination) => {
                1u8.hash_into(hasher);
                destination.hash_into(hasher);
            }
            Self::Attack(target) => {
                2u8.hash_into(hasher);
                target.hash_into(hasher);
            }
        }
    }
}

impl Canonical for Cooldowns {
    fn hash_into(&self, hasher: &mut Hasher) {
        let Cooldowns {
            basic_attack,
            skillshot,
            targeted,
        } = self;
        basic_attack.hash_into(hasher);
        skillshot.hash_into(hasher);
        targeted.hash_into(hasher);
    }
}

impl Canonical for Champion {
    fn hash_into(&self, hasher: &mut Hasher) {
        let Champion {
            position,
            liveness,
            order,
            cooldowns,
        } = self;
        position.hash_into(hasher);
        liveness.hash_into(hasher);
        order.hash_into(hasher);
        cooldowns.hash_into(hasher);
    }
}

impl Canonical for Tower {
    fn hash_into(&self, hasher: &mut Hasher) {
        let Tower { hp, cooldown } = self;
        hp.hash_into(hasher);
        cooldown.hash_into(hasher);
    }
}

impl Canonical for Projectile {
    fn hash_into(&self, hasher: &mut Hasher) {
        let Projectile {
            id,
            owner,
            position,
            velocity,
            remaining,
        } = self;
        id.hash_into(hasher);
        owner.hash_into(hasher);
        position.hash_into(hasher);
        velocity.hash_into(hasher);
        remaining.hash_into(hasher);
    }
}

impl Canonical for Projectiles {
    fn hash_into(&self, hasher: &mut Hasher) {
        let Projectiles { slots } = self;
        slots.hash_into(hasher);
    }
}

impl Canonical for Ability {
    fn hash_into(&self, hasher: &mut Hasher) {
        match self {
            Self::Skillshot => 0u8.hash_into(hasher),
            Self::Targeted => 1u8.hash_into(hasher),
        }
    }
}

impl Canonical for EventKind {
    fn hash_into(&self, hasher: &mut Hasher) {
        match self {
            Self::Cast { caster, ability } => {
                0u8.hash_into(hasher);
                caster.hash_into(hasher);
                ability.hash_into(hasher);
            }
            Self::Damage { target, amount } => {
                1u8.hash_into(hasher);
                target.hash_into(hasher);
                amount.hash_into(hasher);
            }
            Self::Death { entity } => {
                2u8.hash_into(hasher);
                entity.hash_into(hasher);
            }
        }
    }
}

impl Canonical for Event {
    fn hash_into(&self, hasher: &mut Hasher) {
        let Event { kind, at } = self;
        kind.hash_into(hasher);
        at.hash_into(hasher);
    }
}

impl Canonical for Events {
    fn hash_into(&self, hasher: &mut Hasher) {
        let Events { slots } = self;
        slots.hash_into(hasher);
    }
}

impl Canonical for Outcome {
    fn hash_into(&self, hasher: &mut Hasher) {
        match self {
            Self::InProgress => 0u8.hash_into(hasher),
            Self::Decided { winner, at } => {
                1u8.hash_into(hasher);
                winner.hash_into(hasher);
                at.hash_into(hasher);
            }
        }
    }
}

impl Canonical for State {
    fn hash_into(&self, hasher: &mut Hasher) {
        let State {
            tick,
            rng,
            next_projectile_id,
            champions,
            towers,
            projectiles,
            events,
            outcome,
        } = self;
        tick.hash_into(hasher);
        rng.hash_into(hasher);
        next_projectile_id.hash_into(hasher);
        champions.hash_into(hasher);
        towers.hash_into(hasher);
        projectiles.hash_into(hasher);
        events.hash_into(hasher);
        outcome.hash_into(hasher);
    }
}

impl Canonical for Action {
    fn hash_into(&self, hasher: &mut Hasher) {
        match self {
            Self::Idle => 0u8.hash_into(hasher),
            Self::Move(destination) => {
                1u8.hash_into(hasher);
                destination.hash_into(hasher);
            }
            Self::Skillshot(direction) => {
                2u8.hash_into(hasher);
                direction.hash_into(hasher);
            }
            Self::Targeted(target) => {
                3u8.hash_into(hasher);
                target.hash_into(hasher);
            }
            Self::Attack(target) => {
                4u8.hash_into(hasher);
                target.hash_into(hasher);
            }
        }
    }
}

impl Canonical for Input {
    fn hash_into(&self, hasher: &mut Hasher) {
        let Input {
            tick,
            seq,
            player,
            action,
        } = self;
        tick.hash_into(hasher);
        seq.hash_into(hasher);
        player.hash_into(hasher);
        action.hash_into(hasher);
    }
}

impl Canonical for Rules {
    fn hash_into(&self, hasher: &mut Hasher) {
        let Rules {
            frac_bits,
            ticks_per_second,
            map_half_extent,
            spawn_x,
            spawn_spacing,
            tower_outer_x,
            tower_inner_x,
            champion_max_hp,
            champion_speed,
            champion_radius,
            respawn_ticks,
            champion_vision_radius,
            attack_range,
            attack_damage,
            attack_cooldown_ticks,
            skillshot_speed,
            skillshot_radius,
            skillshot_damage,
            skillshot_lifetime_ticks,
            skillshot_cooldown_ticks,
            targeted_range,
            targeted_damage,
            targeted_cooldown_ticks,
            tower_max_hp,
            tower_radius,
            tower_range,
            tower_damage,
            tower_cooldown_ticks,
            tower_vision_radius,
            min_direction_length,
        } = self;
        frac_bits.hash_into(hasher);
        ticks_per_second.hash_into(hasher);
        map_half_extent.hash_into(hasher);
        spawn_x.hash_into(hasher);
        spawn_spacing.hash_into(hasher);
        tower_outer_x.hash_into(hasher);
        tower_inner_x.hash_into(hasher);
        champion_max_hp.hash_into(hasher);
        champion_speed.hash_into(hasher);
        champion_radius.hash_into(hasher);
        respawn_ticks.hash_into(hasher);
        champion_vision_radius.hash_into(hasher);
        attack_range.hash_into(hasher);
        attack_damage.hash_into(hasher);
        attack_cooldown_ticks.hash_into(hasher);
        skillshot_speed.hash_into(hasher);
        skillshot_radius.hash_into(hasher);
        skillshot_damage.hash_into(hasher);
        skillshot_lifetime_ticks.hash_into(hasher);
        skillshot_cooldown_ticks.hash_into(hasher);
        targeted_range.hash_into(hasher);
        targeted_damage.hash_into(hasher);
        targeted_cooldown_ticks.hash_into(hasher);
        tower_max_hp.hash_into(hasher);
        tower_radius.hash_into(hasher);
        tower_range.hash_into(hasher);
        tower_damage.hash_into(hasher);
        tower_cooldown_ticks.hash_into(hasher);
        tower_vision_radius.hash_into(hasher);
        min_direction_length.hash_into(hasher);
    }
}

#[cfg(test)]
mod tests {
    use super::{Canonical, digest_of};
    use crate::fx::Fx;
    use crate::state::{Liveness, Order, Tick, new_state};
    use crate::vec2::FxVec2;

    /// Every field must reach the digest. These are not "does the hash work"
    /// tests — they are the runtime half of the invariant at the top of this
    /// module, checking that a *changed* field changes the digest, where the
    /// compile-time half only guarantees a *new* field is noticed.
    #[test]
    fn every_state_field_changes_the_digest() {
        let base = new_state(1);
        let baseline = digest_of(&base);

        let mut altered = base.clone();
        altered.tick = Tick(1);
        assert_ne!(digest_of(&altered), baseline, "tick");

        let mut altered = base.clone();
        altered.rng.state = 2;
        assert_ne!(digest_of(&altered), baseline, "rng");

        let mut altered = base.clone();
        altered.next_projectile_id = 1234;
        assert_ne!(digest_of(&altered), baseline, "next_projectile_id");

        let mut altered = base.clone();
        altered.champions[0].position = FxVec2::new(Fx::ONE, Fx::ONE);
        assert_ne!(digest_of(&altered), baseline, "champion position");

        let mut altered = base.clone();
        altered.champions[0].liveness = Liveness::Dead {
            respawn_at: Tick(7),
        };
        assert_ne!(digest_of(&altered), baseline, "champion liveness");

        let mut altered = base.clone();
        altered.champions[0].order = Order::MoveTo(FxVec2::ZERO);
        assert_ne!(digest_of(&altered), baseline, "champion order");

        let mut altered = base.clone();
        altered.champions[0].cooldowns.skillshot = 3;
        assert_ne!(digest_of(&altered), baseline, "champion cooldowns");

        let mut altered = base.clone();
        altered.towers[0].hp = Fx::ONE;
        assert_ne!(digest_of(&altered), baseline, "tower hp");

        let mut altered = base.clone();
        altered.towers[0].cooldown = 5;
        assert_ne!(digest_of(&altered), baseline, "tower cooldown");

        let mut altered = base.clone();
        altered.outcome = crate::state::Outcome::Decided {
            winner: crate::state::Team::Red,
            at: Tick(0),
        };
        assert_ne!(digest_of(&altered), baseline, "outcome");
    }

    /// A champion in the sixth seat must not hash like the same champion in the
    /// first: the array is positional and position carries identity.
    #[test]
    fn position_in_an_array_is_part_of_the_encoding() {
        let base = new_state(1);
        let mut swapped = base.clone();
        swapped.champions.swap(0, 5);
        assert_ne!(digest_of(&swapped), digest_of(&base));
    }

    /// The enum tag has to disambiguate variants whose payloads coincide.
    #[test]
    fn enum_variants_with_equal_payloads_encode_differently() {
        let mut alive = crate::sha256::Hasher::new();
        Liveness::Alive { hp: Fx::ZERO }.hash_into(&mut alive);
        let mut dead = crate::sha256::Hasher::new();
        Liveness::Dead {
            respawn_at: Tick(0),
        }
        .hash_into(&mut dead);
        assert_ne!(alive.finish(), dead.finish());
    }
}
