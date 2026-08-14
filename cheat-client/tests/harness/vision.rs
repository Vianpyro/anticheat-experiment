//! The two projections the maphack is judged between, and the ground truth its
//! conclusions are checked against.
//!
//! Included only by `tests/maphack.rs`, and every function here is used by it —
//! which is why this is a sibling of `authority.rs` rather than pooled with it
//! (see that file for the convention). The maphack's whole claim is a comparison
//! between what a leaking projection discloses and what the real one does, and
//! between what the attacker concluded and what was true; those are the two
//! projections and the ground truth, and nothing else.

use crate::entitlement::team_can_see;
use sim::view::view_for;
use sim::view::{EntityView, OwnView, PlayerView, VisibleEvent};
use sim::{Liveness, Seat, State, TOWER_COUNT};
use sim::{champion_entity_id, tower_entity_id, tower_position};

/// The weakened projection: a view that culls nothing.
///
/// This is the server `docs/MILESTONES.md` M7 asks the maphack to be run against
/// first — "a build with culling disabled" — expressed as a projection rather
/// than a Cargo feature, for the reason `cheat-client/src/lib.rs` gives: a
/// feature on `sim` is a switch any crate can throw for the server binary, and
/// `docs/ARCHITECTURE.md` refuses that shape.
///
/// It names every living champion and every tower at its true position,
/// regardless of whether the requesting player could see it, and the maphack
/// folds it exactly as it folds a real one — the attacker does not know which
/// projection produced the bytes, which is the point.
#[must_use]
pub fn omniscient(state: &State, player: Seat) -> PlayerView {
    let mut visible = Vec::new();
    for seat in Seat::ALL {
        if seat == player {
            continue;
        }
        if let Liveness::Alive { hp } = state.champion(seat).liveness {
            visible.push(EntityView::Champion {
                id: champion_entity_id(seat),
                position: state.champion(seat).position,
                hp,
            });
        }
    }
    for index in 0..TOWER_COUNT {
        if let Some(tower) = state.towers().get(index) {
            visible.push(EntityView::Tower {
                id: tower_entity_id(index),
                position: tower_position(index, &sim::RULES),
                hp: tower.hp,
            });
        }
    }
    for projectile in state.projectiles().iter() {
        visible.push(EntityView::Projectile {
            id: projectile.id,
            position: projectile.position,
            velocity: projectile.velocity,
        });
    }

    let mut events = Vec::new();
    for event in state.events().iter() {
        events.push(visible_event(event));
    }

    PlayerView {
        tick: state.tick(),
        outcome: state.outcome(),
        own: own_view(state, player),
        visible,
        events,
    }
}

/// The real, culling projection this project ships, so a test names one function
/// for "what the server would actually send".
#[must_use]
pub fn culled(state: &State, player: Seat) -> PlayerView {
    view_for(state, player)
}

/// Where an enemy champion of `player` really is, if it is alive.
///
/// The truth the exploit's conclusion is checked against: a claim to have located
/// a champion is worth nothing unless the location was right, and worth nothing
/// *against a defence* unless the champion was one the fog was hiding — both of
/// which only the state answers.
#[must_use]
pub fn true_enemy_positions(state: &State, player: Seat) -> Vec<(Seat, sim::FxVec2)> {
    Seat::ALL
        .into_iter()
        .filter(|seat| seat.team() != player.team())
        .filter_map(|seat| match state.champion(seat).liveness {
            Liveness::Alive { .. } => Some((seat, state.champion(seat).position)),
            Liveness::Dead { .. } => None,
        })
        .collect()
}

/// Which enemy champions `player`'s team is entitled to see.
///
/// The set the maphack's "surplus" is measured against — built on
/// `crate::entitlement::team_can_see`, which re-derives the rule, and **not** read
/// off `culled`'s own output. It was read off `culled` in the first draft and the
/// mutation exercise found it: with culling removed on purpose the exploit went
/// red at its own R15 antecedent instead of at the exploit, because the broken
/// projection had redefined what "hidden" meant. See `harness/entitlement.rs`.
#[must_use]
pub fn legitimately_visible_enemies(state: &State, player: Seat) -> Vec<Seat> {
    Seat::ALL
        .into_iter()
        .filter(|seat| seat.team() != player.team())
        .filter(|seat| matches!(state.champion(*seat).liveness, Liveness::Alive { .. }))
        .filter(|seat| team_can_see(state, player.team(), state.champion(*seat).position))
        .collect()
}

/// A seat's champion handle, as an attacker reads it off the wire.
#[must_use]
pub fn champion_handle(seat: Seat) -> sim::EntityId {
    champion_entity_id(seat)
}

fn own_view(state: &State, player: Seat) -> OwnView {
    let champion = state.champion(player);
    OwnView {
        id: champion_entity_id(player),
        position: champion.position,
        liveness: champion.liveness,
        cooldowns: champion.cooldowns,
    }
}

fn visible_event(event: &sim::Event) -> VisibleEvent {
    match event.kind {
        sim::EventKind::Cast { caster, ability } => VisibleEvent::Cast {
            caster,
            ability,
            at: event.at,
        },
        sim::EventKind::Damage { target, amount } => VisibleEvent::Damage {
            target,
            amount,
            at: event.at,
        },
        sim::EventKind::Death { entity } => VisibleEvent::Death {
            entity,
            at: event.at,
        },
    }
}
