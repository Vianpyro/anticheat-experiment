//! Vision, re-derived from `docs/MILESTONES.md` M2 rather than from `sim`.
//!
//! Every test that checks the visibility projection needs a second opinion
//! about what a player is entitled to see, because asserting that `view_for`
//! agrees with `sim::view::can_see` would be asserting that a function agrees
//! with itself. This module is that second opinion, and it lives here rather
//! than inside one test binary so that there is exactly one of it: two
//! re-derivations that had drifted apart would be worse than none, since each
//! test would still be green against its own private idea of the rule.
//!
//! It is deliberately written as the naive double loop it is. This is the
//! specification, and it is supposed to be boring enough to read as one.
//!
//! # Two things about it that are specification and not implementation detail
//!
//! **The boundary is closed, and the comparison is exact.** A point at exactly
//! `radius` is seen. The comparison is made on squared distances over `i64`, so
//! the answer never inherits the truncation of an integer square root. Written
//! the other way — `distance(point) <= radius`, with the truncating
//! [`sim::FxVec2::distance`] — the predicate disagrees with the rule in a shell
//! one raw unit thick just outside the circle: a champion at `(12.0, 0.00001)`
//! from a viewer with a radius of `12.0` has a true distance above the radius
//! and a *truncated* distance equal to it. That is not a rounding preference,
//! it is a different predicate, and the difference is only ever visible where
//! a test aims at the boundary on purpose — which
//! `sim/tests/view_properties.rs` does, and which the fixtures never did.
//!
//! **Vision is a team property.** Sources are the player's own team: champions
//! that are alive, towers that still stand. Two players on the same team are
//! entitled to exactly the same world, minus each other's own handle.

// Each test binary compiles this module and uses part of it, in the same way
// and for the same reason as `tests/fixture/mod.rs`.
#![allow(
    dead_code,
    reason = "each test binary uses a subset of the specification"
)]

use std::collections::BTreeSet;

use sim::view::{EntityView, PlayerView, VisibleEvent};
use sim::{
    Cooldowns, EntityId, Event, EventKind, Fx, FxVec2, Liveness, Outcome, Rules, Seat, State,
    TOWER_COUNT, Team, Tick, champion_entity_id, tower_entity_id, tower_position, tower_team,
};

/// Whether `team` sees `point`, under `rules`.
///
/// The exact comparison, for the reason in the module documentation: squared
/// distances over `i64`, and the circle itself counts as inside.
#[must_use]
pub fn team_sees(state: &State, team: Team, point: FxVec2, rules: &Rules) -> bool {
    let covered = |source: FxVec2, radius: Fx| -> bool {
        if radius.to_raw() < 0 {
            return false;
        }
        let reach = i64::from(radius.to_raw());
        source.sub(point).length_squared_wide() <= reach.saturating_mul(reach)
    };

    for seat in Seat::ALL {
        if seat.team() != team {
            continue;
        }
        let champion = *state.champion(seat);
        if !matches!(champion.liveness, Liveness::Alive { .. }) {
            continue;
        }
        if covered(champion.position, rules.champion_vision_radius) {
            return true;
        }
    }
    for index in 0..TOWER_COUNT {
        if tower_team(index) != team {
            continue;
        }
        if !state.towers()[index].is_standing() {
            continue;
        }
        if covered(tower_position(index, rules), rules.tower_vision_radius) {
            return true;
        }
    }
    false
}

/// One thing a player is entitled to see, and what they are entitled to know
/// about it.
///
/// A separate type from [`sim::view::EntityView`] on purpose: this one is
/// derived from the state by the specification, and comparing the two is the
/// whole point. Sharing a type would make the comparison structural rather than
/// semantic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    /// A living champion, at a point in vision.
    Champion {
        /// Where it stands.
        position: FxVec2,
        /// Its remaining hit points.
        hp: Fx,
    },
    /// A tower, standing or rubble.
    Tower {
        /// Where it stands.
        position: FxVec2,
        /// Its remaining hit points.
        hp: Fx,
    },
    /// A projectile in flight.
    Projectile {
        /// Where it is.
        position: FxVec2,
        /// Its per-tick displacement.
        velocity: FxVec2,
    },
}

impl Seen {
    /// The point the sighting is culled on.
    #[must_use]
    pub const fn position(self) -> FxVec2 {
        match self {
            Self::Champion { position, .. }
            | Self::Tower { position, .. }
            | Self::Projectile { position, .. } => position,
        }
    }
}

/// The player's own champion, which is never culled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Own {
    /// Its handle.
    pub id: EntityId,
    /// Where it stands, or its spawn point while it is dead.
    pub position: FxVec2,
    /// Alive with hit points, or dead with a respawn tick.
    pub liveness: Liveness,
    /// Its remaining cooldowns.
    pub cooldowns: Cooldowns,
}

/// Everything one player is entitled to learn from one state.
///
/// This is the specification of `view_for`'s *content*, derived from the state
/// and from nothing else. Two states that produce the same `Entitled` for a
/// player differ only in things that player may not know about, which is what
/// makes it the antecedent of the side-channel property in
/// `sim/tests/view_properties.rs`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entitled {
    /// The tick, which is public: one message per player per tick.
    pub tick: Tick,
    /// The outcome, which is global the moment it happens.
    pub outcome: Outcome,
    /// The player's own champion. Not an `Option` since M3: a [`Seat`] names a
    /// champion that exists, so the case the specification used to decline to
    /// describe is one nobody can construct.
    pub own: Own,
    /// Everything else in vision, keyed by handle and ordered by it. Ordered by
    /// handle rather than by anything about the state, because the order a
    /// player is told things in must be a function of what they were told.
    pub visible: Vec<(EntityId, Seen)>,
    /// The events that happened somewhere in vision, in the order the rules
    /// produced them.
    pub events: Vec<Event>,
}

/// What `player` may learn from `state`, under `rules`.
#[must_use]
pub fn entitled(state: &State, player: Seat, rules: &Rules) -> Entitled {
    let team = player.team();

    let mut visible = Vec::new();
    for seat in Seat::ALL {
        if seat == player {
            continue;
        }
        let champion = *state.champion(seat);
        // A dead champion is not on the map, for its own team either.
        let Liveness::Alive { hp } = champion.liveness else {
            continue;
        };
        if team_sees(state, team, champion.position, rules) {
            visible.push((
                champion_entity_id(seat),
                Seen::Champion {
                    position: champion.position,
                    hp,
                },
            ));
        }
    }
    for index in 0..TOWER_COUNT {
        let position = tower_position(index, rules);
        if team_sees(state, team, position, rules) {
            visible.push((
                tower_entity_id(index),
                Seen::Tower {
                    position,
                    hp: state.towers()[index].hp,
                },
            ));
        }
    }
    for projectile in state.projectiles().iter() {
        if team_sees(state, team, projectile.position, rules) {
            visible.push((
                projectile.id,
                Seen::Projectile {
                    position: projectile.position,
                    velocity: projectile.velocity,
                },
            ));
        }
    }
    visible.sort_by_key(|(id, _)| id.0);

    let own = Own {
        id: champion_entity_id(player),
        position: state.champion(player).position,
        liveness: state.champion(player).liveness,
        cooldowns: state.champion(player).cooldowns,
    };

    Entitled {
        tick: state.tick(),
        outcome: state.outcome(),
        own,
        visible,
        events: expected_events(state, player, rules),
    }
}

/// The handles `player` is entitled to see.
#[must_use]
pub fn expected_ids(state: &State, player: Seat, rules: &Rules) -> BTreeSet<u16> {
    entitled(state, player, rules)
        .visible
        .iter()
        .map(|(id, _)| id.0)
        .collect()
}

/// The events of this tick that happened somewhere `player` can see.
#[must_use]
pub fn expected_events(state: &State, player: Seat, rules: &Rules) -> Vec<Event> {
    let team = player.team();
    state
        .events()
        .iter()
        .filter(|event| team_sees(state, team, event.at, rules))
        .copied()
        .collect()
}

/// Every handle a view names, paired with the position it was named at.
///
/// This pairing is the whole argument: the criterion is that no entity outside
/// vision is named, and an entity is outside vision exactly when the point it
/// was named at is. A handle with no position attached could not take part in
/// it, which is why the view type does not have one.
#[must_use]
pub fn handles_with_positions(view: &PlayerView) -> Vec<(u16, FxVec2)> {
    let mut out = Vec::new();
    for entity in &view.visible {
        let (id, position) = match entity {
            EntityView::Champion { id, position, .. }
            | EntityView::Tower { id, position, .. }
            | EntityView::Projectile { id, position, .. } => (*id, *position),
        };
        out.push((id.0, position));
    }
    for event in &view.events {
        let (id, at) = match event {
            VisibleEvent::Cast { caster: id, at, .. }
            | VisibleEvent::Damage { target: id, at, .. }
            | VisibleEvent::Death { entity: id, at } => (*id, *at),
        };
        out.push((id.0, at));
    }
    out
}

/// The handles a view names in its entity list, in the order it names them.
#[must_use]
pub fn reported_ids(view: &PlayerView) -> Vec<u16> {
    view.visible
        .iter()
        .map(|entity| match entity {
            EntityView::Champion { id, .. }
            | EntityView::Tower { id, .. }
            | EntityView::Projectile { id, .. } => id.0,
        })
        .collect()
}

/// Whether a state event and a view event describe the same thing.
#[must_use]
pub fn same_event(state: &Event, view: &VisibleEvent) -> bool {
    match (state.kind, view) {
        (
            EventKind::Cast { caster, ability },
            VisibleEvent::Cast {
                caster: seen_caster,
                ability: seen_ability,
                at,
            },
        ) => caster == *seen_caster && ability == *seen_ability && state.at == *at,
        (
            EventKind::Damage { target, amount },
            VisibleEvent::Damage {
                target: seen_target,
                amount: seen_amount,
                at,
            },
        ) => target == *seen_target && amount == *seen_amount && state.at == *at,
        (
            EventKind::Death { entity },
            VisibleEvent::Death {
                entity: seen_entity,
                at,
            },
        ) => entity == *seen_entity && state.at == *at,
        _ => false,
    }
}
