//! The visibility projection: what one player is allowed to learn this tick.
//!
//! # The rule, in one sentence
//!
//! **An entity outside a player's vision is absent from that player's view, not
//! present with a flag.** `docs/SCOPE.md` makes this a structural invariant and
//! `docs/MILESTONES.md` M2 makes it an exit criterion, because it is the whole
//! of the maphack defence: a client that receives an invisible entity and is
//! trusted to hide it is a client that shows it, and this project starts from
//! the axiom that the client is compromised and lying.
//!
//! # What is culled on, and why it is a position rather than an entity
//!
//! A player sees a point when some source on their team covers it: a living
//! champion within [`Rules::champion_vision_radius`], or a standing tower
//! within [`Rules::tower_vision_radius`]. Everything in the output — entities
//! and events alike — is included exactly when the position it is associated
//! with is a point the player sees.
//!
//! Deriving it from positions rather than from entity identity is what keeps
//! the rule free of exceptions. A champion killed this tick is off the map and
//! has no current position to test, so an identity-based rule would need a
//! special case for deaths, and a special case in this function is where a
//! maphack would live. Events carry the place they happened
//! (`crate::event::Event::at`), so a death is shown to whoever could see the
//! ground it happened on, and nothing needs an exception.
//!
//! # This is team vision, not per-player vision
//!
//! Two players on the same team get the same visible set. That is the MOBA
//! model and it is also the conservative one to implement: allies share what
//! they see, so an ally is always in view (it stands inside its own radius) and
//! there is no ally-only side channel to get wrong.
//!
//! # Serialization
//!
//! [`PlayerView::encode`] is a hand-written canonical encoding, and `sim` still
//! has an empty `[dependencies]` table. `docs/ARCHITECTURE.md` allows `serde`
//! here; it is deliberately not taken yet, and the reason is in that document
//! under this module's heading — the transport that would choose a codec does
//! not exist until M3, and the traffic-shape invariant that governs message
//! size wants an encoding whose byte layout is decided here rather than by a
//! crate.
//!
//! What the encoding is *not*, yet: a constant size. `docs/ARCHITECTURE.md`
//! requires every `View` message to encode to the same number of bytes and to
//! be sent at a constant cadence, because message length and message count leak
//! the number of visible entities as surely as the entities themselves would.
//! That is a property of the transport, the transport is M3, and padding a
//! bound into a bucket here would be building half of it in the wrong crate.
//! [`PlayerView::MAX_ENCODED_BYTES`] is the bound that padding will eventually
//! round up to.

use crate::event::{Ability, Event, EventKind};
use crate::fx::Fx;
use crate::rules::{RULES, Rules};
use crate::state::{
    Cooldowns, EntityId, Liveness, Outcome, PLAYER_COUNT, PlayerId, State, TOWER_COUNT, Team, Tick,
    champion_entity_id, tower_entity_id, tower_position,
};
use crate::vec2::FxVec2;

/// Everything one player may know about one tick.
///
/// # Why each field is here
///
/// This type is the serialization frontier of the project: what enters it is
/// what a client can learn, and therefore what an attacker can learn. So the
/// justification is per field rather than only for the entities that were left
/// out.
///
/// - `tick` — the client has to order and reconcile against something, and the
///   tick is public: the server emits one view per player per tick regardless
///   of content, so the number is already implied by the message's existence.
/// - `outcome` — the match ending, and who won, is a global fact the moment it
///   happens. Withholding it would hide the end of the game from the loser.
/// - `own` — the player's own champion, in full. Nothing here is secret *from
///   this player*.
/// - `visible` — the culled entity list. This is the field the milestone is
///   about.
/// - `events` — the culled derived signals. Culling entities while announcing
///   that something was cast nearby would hand back exactly what the entity
///   list withheld.
///
/// And what is deliberately absent, because absence is a decision too:
///
/// - **No standing order.** An `Order::Attack` names an `EntityId` the player
///   may no longer be able to see, and echoing it would put an out-of-vision
///   handle in the message. The client originated the order and can track it;
///   reconciling a server-side order change is M3's problem and must not be
///   solved by shipping handles.
/// - **No enemy cooldowns.** A cooldown tracker is a classic cheat, and a
///   protocol that ships enemy cooldowns has implemented it in the server.
/// - **No projectile owner.** A skillshot can outlive its caster's visibility;
///   naming the owner would identify a champion the fog is hiding, from the
///   projectile alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerView {
    /// The tick this view describes.
    pub tick: Tick,
    /// Whether the match is over, and who won.
    pub outcome: Outcome,
    /// The player's own champion.
    pub own: OwnView,
    /// Everything else the player can see, in a fixed order: champions by seat,
    /// then towers by index, then projectiles in arena order. The order is part
    /// of the encoding rather than an artefact of iteration, so that two
    /// servers producing the same view produce the same bytes.
    pub visible: Vec<EntityView>,
    /// What happened this tick, where the player could see it happen.
    pub events: Vec<VisibleEvent>,
}

/// The requesting player's own champion, in full detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnView {
    /// Which handle is the player's own, so that events naming it can be
    /// recognised.
    pub id: EntityId,
    /// Where it stands, or its spawn point while it is dead.
    pub position: FxVec2,
    /// Alive with hit points, or dead with the tick it returns on. The player
    /// is entitled to its own respawn timer.
    pub liveness: Liveness,
    /// The player's own remaining cooldowns.
    pub cooldowns: Cooldowns,
}

/// Something other than the player's own champion, seen at a point.
///
/// Positions are carried rather than implied even where they are derivable —
/// a tower's position follows from the rules — because the culling proof is
/// "every handle in this message is accompanied by the visible point it was
/// seen at", and a handle without a position cannot take part in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityView {
    /// A living champion. The team follows from the handle, which is the seat.
    Champion {
        /// Its handle.
        id: EntityId,
        /// Where it stands.
        position: FxVec2,
        /// Its remaining hit points. The maximum is a public constant.
        hp: Fx,
    },
    /// A tower, standing or rubble.
    Tower {
        /// Its handle.
        id: EntityId,
        /// Where it stands. Derivable from the rules; sent anyway, see above.
        position: FxVec2,
        /// Remaining hit points. Zero is a destroyed tower.
        hp: Fx,
    },
    /// A projectile in flight.
    Projectile {
        /// Its handle.
        id: EntityId,
        /// Where it is now.
        position: FxVec2,
        /// Its per-tick displacement, so that a client can interpolate a fast
        /// object between two ticks. It leaks nothing a client could not
        /// recover from two consecutive positions of the same projectile.
        velocity: FxVec2,
    },
}

/// A derived signal the player was in a position to perceive.
///
/// The shape mirrors `crate::event::EventKind` today and is nonetheless a
/// separate type, because they answer different questions. The state's event is
/// what happened; this is what a client is told. Making them one type would
/// mean every field added to the former ships to clients by default, which is
/// the exact opposite of what this module exists to guarantee.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisibleEvent {
    /// An ability was cast at a point the player can see.
    Cast {
        /// Who cast it.
        caster: EntityId,
        /// Which ability.
        ability: Ability,
        /// Where the cast happened.
        at: FxVec2,
    },
    /// Something took damage at a point the player can see.
    Damage {
        /// What was hit.
        target: EntityId,
        /// How much was applied.
        amount: Fx,
        /// Where it landed.
        at: FxVec2,
    },
    /// A champion died at a point the player can see.
    Death {
        /// Which champion.
        entity: EntityId,
        /// Where it fell.
        at: FxVec2,
    },
}

/// What one player may know about one tick, under [`RULES`].
#[must_use]
pub fn view_for(state: &State, player: PlayerId) -> PlayerView {
    view_for_with_rules(state, player, &RULES)
}

/// The same projection under the constants given.
///
/// Exists for the same reason [`crate::step_with_rules`] does: a fixture
/// recorded under its own [`Rules`] must be projected under those rules too,
/// and a caller that has to name the constants cannot silently mix two sets.
#[must_use]
pub fn view_for_with_rules(state: &State, player: PlayerId, rules: &Rules) -> PlayerView {
    let team = Team::of_player(player);
    let own_seat = player.0 as usize;

    let mut visible = Vec::new();
    for seat in 0..PLAYER_COUNT {
        if seat == own_seat {
            continue;
        }
        let Some(champion) = state.champions().get(seat) else {
            continue;
        };
        // A dead champion is not on the map. It is therefore not in anybody's
        // view, including its own team's — the fact that a teammate is waiting
        // to respawn is information about a player rather than about the world,
        // and a second visibility rule to carry it is a second place for a leak
        // to hide. The cost is a missing respawn timer in an ally panel that
        // does not exist yet.
        let Liveness::Alive { hp } = champion.liveness else {
            continue;
        };
        if !can_see(state, team, champion.position, rules) {
            continue;
        }
        visible.push(EntityView::Champion {
            id: champion_entity_id(seat),
            position: champion.position,
            hp,
        });
    }

    for index in 0..TOWER_COUNT {
        let Some(tower) = state.towers().get(index) else {
            continue;
        };
        let position = tower_position(index, rules);
        if !can_see(state, team, position, rules) {
            continue;
        }
        visible.push(EntityView::Tower {
            id: tower_entity_id(index),
            position,
            hp: tower.hp,
        });
    }

    for projectile in state.projectiles().iter() {
        // Including the caster's own projectile only while it is in team vision
        // is the strict reading, and it is the one taken: one rule, applied to
        // everything, is worth more than a courtesy that would need its own
        // sentence in this module and its own branch in the test.
        if !can_see(state, team, projectile.position, rules) {
            continue;
        }
        visible.push(EntityView::Projectile {
            id: projectile.id,
            position: projectile.position,
            velocity: projectile.velocity,
        });
    }

    let mut events = Vec::new();
    for event in state.events().iter() {
        if !can_see(state, team, event.at, rules) {
            continue;
        }
        events.push(visible_event(event));
    }

    PlayerView {
        tick: state.tick(),
        outcome: state.outcome(),
        own: own_view(state, own_seat, rules),
        visible,
        events,
    }
}

/// Whether `team` can see the point given.
///
/// Vision sources are living champions and standing towers, per
/// `docs/MILESTONES.md` M2. A dead champion and a destroyed tower see nothing,
/// which is what makes losing a tower cost map control rather than only hit
/// points.
fn can_see(state: &State, team: Team, point: FxVec2, rules: &Rules) -> bool {
    for seat in 0..PLAYER_COUNT {
        if Team::of_player(PlayerId(seat as u8)) != team {
            continue;
        }
        let Some(champion) = state.champions().get(seat) else {
            continue;
        };
        if !matches!(champion.liveness, Liveness::Alive { .. }) {
            continue;
        }
        if champion
            .position
            .within_range(point, rules.champion_vision_radius)
        {
            return true;
        }
    }

    for index in 0..TOWER_COUNT {
        if crate::state::tower_team(index) != team {
            continue;
        }
        let Some(tower) = state.towers().get(index) else {
            continue;
        };
        if !tower.is_standing() {
            continue;
        }
        if tower_position(index, rules).within_range(point, rules.tower_vision_radius) {
            return true;
        }
    }

    false
}

/// The player's own champion. Always present, alive or dead: a player is never
/// hidden from itself, so this needs no visibility test and is not an exception
/// to the culling rule.
fn own_view(state: &State, seat: usize, rules: &Rules) -> OwnView {
    let id = champion_entity_id(seat);
    match state.champions().get(seat) {
        Some(champion) => OwnView {
            id,
            position: champion.position,
            liveness: champion.liveness,
            cooldowns: champion.cooldowns,
        },
        // A seat outside the match. `view_for` is total for the same reason
        // `step` is: the caller is the server, and a server that panics on a
        // bad seat is a match everybody loses.
        None => OwnView {
            id,
            position: crate::state::spawn_position(seat, rules),
            liveness: Liveness::Alive {
                hp: rules.champion_max_hp,
            },
            cooldowns: Cooldowns::default(),
        },
    }
}

/// Translates a state event into its wire form. Exhaustive by construction: a
/// new `EventKind` variant stops this compiling, which is the same discipline
/// `crate::canonical` uses and for the same reason — a variant that silently
/// fails to reach the client is a signal nobody notices is missing.
fn visible_event(event: &Event) -> VisibleEvent {
    let Event { kind, at } = *event;
    match kind {
        EventKind::Cast { caster, ability } => VisibleEvent::Cast {
            caster,
            ability,
            at,
        },
        EventKind::Damage { target, amount } => VisibleEvent::Damage { target, amount, at },
        EventKind::Death { entity } => VisibleEvent::Death { entity, at },
    }
}

/// Appends a big-endian value to the buffer.
trait Encode {
    fn encode_into(&self, out: &mut Vec<u8>);
}

impl Encode for u8 {
    fn encode_into(&self, out: &mut Vec<u8>) {
        out.push(*self);
    }
}

impl Encode for u16 {
    fn encode_into(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.to_be_bytes());
    }
}

impl Encode for u32 {
    fn encode_into(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.to_be_bytes());
    }
}

impl Encode for i32 {
    fn encode_into(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.to_be_bytes());
    }
}

impl Encode for Fx {
    fn encode_into(&self, out: &mut Vec<u8>) {
        self.to_raw().encode_into(out);
    }
}

impl Encode for FxVec2 {
    fn encode_into(&self, out: &mut Vec<u8>) {
        let FxVec2 { x, y } = self;
        x.encode_into(out);
        y.encode_into(out);
    }
}

impl Encode for EntityId {
    fn encode_into(&self, out: &mut Vec<u8>) {
        let EntityId(value) = self;
        value.encode_into(out);
    }
}

impl Encode for Tick {
    fn encode_into(&self, out: &mut Vec<u8>) {
        let Tick(value) = self;
        value.encode_into(out);
    }
}

impl Encode for Team {
    fn encode_into(&self, out: &mut Vec<u8>) {
        match self {
            Self::Blue => 0u8.encode_into(out),
            Self::Red => 1u8.encode_into(out),
        }
    }
}

impl Encode for Ability {
    fn encode_into(&self, out: &mut Vec<u8>) {
        match self {
            Self::Skillshot => 0u8.encode_into(out),
            Self::Targeted => 1u8.encode_into(out),
        }
    }
}

impl Encode for Outcome {
    fn encode_into(&self, out: &mut Vec<u8>) {
        match self {
            Self::InProgress => {
                0u8.encode_into(out);
                // Padded to the width of the other variant. The encoding is
                // fixed-width per variant everywhere else; here it also keeps
                // "the match ended" from being readable off the message length
                // by an observer who cannot decrypt it.
                Team::Blue.encode_into(out);
                Tick(0).encode_into(out);
            }
            Self::Decided { winner, at } => {
                1u8.encode_into(out);
                winner.encode_into(out);
                at.encode_into(out);
            }
        }
    }
}

impl Encode for Liveness {
    fn encode_into(&self, out: &mut Vec<u8>) {
        match self {
            Self::Alive { hp } => {
                0u8.encode_into(out);
                hp.encode_into(out);
            }
            Self::Dead { respawn_at } => {
                1u8.encode_into(out);
                respawn_at.encode_into(out);
            }
        }
    }
}

impl Encode for Cooldowns {
    fn encode_into(&self, out: &mut Vec<u8>) {
        let Cooldowns {
            basic_attack,
            skillshot,
            targeted,
        } = self;
        basic_attack.encode_into(out);
        skillshot.encode_into(out);
        targeted.encode_into(out);
    }
}

impl Encode for OwnView {
    fn encode_into(&self, out: &mut Vec<u8>) {
        let OwnView {
            id,
            position,
            liveness,
            cooldowns,
        } = self;
        id.encode_into(out);
        position.encode_into(out);
        liveness.encode_into(out);
        cooldowns.encode_into(out);
    }
}

impl Encode for EntityView {
    fn encode_into(&self, out: &mut Vec<u8>) {
        match self {
            Self::Champion { id, position, hp } => {
                0u8.encode_into(out);
                id.encode_into(out);
                position.encode_into(out);
                hp.encode_into(out);
            }
            Self::Tower { id, position, hp } => {
                1u8.encode_into(out);
                id.encode_into(out);
                position.encode_into(out);
                hp.encode_into(out);
            }
            Self::Projectile {
                id,
                position,
                velocity,
            } => {
                2u8.encode_into(out);
                id.encode_into(out);
                position.encode_into(out);
                velocity.encode_into(out);
            }
        }
    }
}

impl Encode for VisibleEvent {
    fn encode_into(&self, out: &mut Vec<u8>) {
        match self {
            Self::Cast {
                caster,
                ability,
                at,
            } => {
                0u8.encode_into(out);
                caster.encode_into(out);
                ability.encode_into(out);
                at.encode_into(out);
            }
            Self::Damage { target, amount, at } => {
                1u8.encode_into(out);
                target.encode_into(out);
                amount.encode_into(out);
                at.encode_into(out);
            }
            Self::Death { entity, at } => {
                2u8.encode_into(out);
                entity.encode_into(out);
                at.encode_into(out);
            }
        }
    }
}

impl PlayerView {
    /// The largest number of bytes [`PlayerView::encode`] can produce.
    ///
    /// Derived from the encoding rather than measured, and the derivation is
    /// the point: a bound obtained by running the fixture and rounding up would
    /// grow silently the day the view gained a field, which is the failure this
    /// number exists to catch. `docs/MILESTONES.md` M2 asks for a size
    /// assertion so that an accidental full-state leak fails a test instead of
    /// merely inflating a packet.
    ///
    /// | Part | Bytes | Count |
    /// | --- | --- | --- |
    /// | `tick` | 4 | 1 |
    /// | `outcome` | 1 + 1 + 4 = 6 | 1 |
    /// | `own` | 2 + 8 + (1 + 4) + 6 = 21 | 1 |
    /// | `visible` length | 2 | 1 |
    /// | champion or tower entry | 1 + 2 + 8 + 4 = 15 | 5 + 4 |
    /// | projectile entry | 1 + 2 + 8 + 8 = 19 | [`crate::MAX_PROJECTILES`] |
    /// | `events` length | 2 | 1 |
    /// | widest event (damage) | 1 + 2 + 4 + 8 = 15 | [`crate::MAX_EVENTS`] |
    ///
    /// Five champions rather than six: the sixth is the player's own, which is
    /// in `own` and never in `visible`.
    pub const MAX_ENCODED_BYTES: usize = 1498;

    /// The canonical wire encoding of this view.
    ///
    /// Fixed-width big-endian, one tag byte before every enum, a `u16` length
    /// before each of the two lists. Written by hand and by exhaustive
    /// destructuring, exactly as `crate::canonical` is, so that a field added to
    /// a view type and not encoded stops the build rather than quietly never
    /// reaching a client.
    ///
    /// The size is variable, and deliberately so at this milestone; see the
    /// module documentation on where the constant-size requirement lives.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let PlayerView {
            tick,
            outcome,
            own,
            visible,
            events,
        } = self;
        let mut out = Vec::with_capacity(Self::MAX_ENCODED_BYTES);
        tick.encode_into(&mut out);
        outcome.encode_into(&mut out);
        own.encode_into(&mut out);

        // Lengths are `u16` and the counts are bounded by the state's fixed
        // arrays, so the `as` cannot truncate; it is written with a saturating
        // conversion anyway, because "cannot happen" is not something this
        // project gets to assume about a length an attacker's inputs help
        // determine.
        u16::try_from(visible.len())
            .unwrap_or(u16::MAX)
            .encode_into(&mut out);
        for entity in visible {
            entity.encode_into(&mut out);
        }
        u16::try_from(events.len())
            .unwrap_or(u16::MAX)
            .encode_into(&mut out);
        for event in events {
            event.encode_into(&mut out);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{EntityView, PlayerView, VisibleEvent, view_for, view_for_with_rules};
    use crate::event::{Ability, MAX_EVENTS};
    use crate::fx::Fx;
    use crate::input::{Action, Input};
    use crate::rules::{RULES, Rules};
    use crate::state::{
        Cooldowns, EntityId, Liveness, MAX_PROJECTILES, Outcome, PlayerId, Tick,
        champion_entity_id, new_state, tower_entity_id, tower_position,
    };
    use crate::step::step;
    use crate::vec2::FxVec2;
    use crate::view::OwnView;

    fn ids(view: &PlayerView) -> Vec<EntityId> {
        view.visible
            .iter()
            .map(|entity| match entity {
                EntityView::Champion { id, .. }
                | EntityView::Tower { id, .. }
                | EntityView::Projectile { id, .. } => *id,
            })
            .collect()
    }

    #[test]
    fn an_enemy_across_the_map_is_absent_rather_than_flagged() {
        let state = new_state(3);
        let view = view_for(&state, PlayerId(0));
        for seat in 3..6 {
            assert!(
                !ids(&view).contains(&champion_entity_id(seat)),
                "seat {seat} is two hundred units away and still in the view"
            );
        }
        // …and the culling is not "return nothing": the allies at spawn are
        // there, which is what makes the assertion above mean something.
        assert!(ids(&view).contains(&champion_entity_id(1)));
        assert!(ids(&view).contains(&champion_entity_id(2)));
    }

    #[test]
    fn an_enemy_that_walks_into_range_appears() {
        let mut state = new_state(3);
        let meeting_point = FxVec2::new(Fx::ZERO, Fx::ZERO);
        state.place_champion(0, meeting_point, RULES.champion_max_hp);
        state.place_champion(
            3,
            FxVec2::new(RULES.champion_vision_radius.add(Fx::ONE), Fx::ZERO),
            RULES.champion_max_hp,
        );
        assert!(!ids(&view_for(&state, PlayerId(0))).contains(&champion_entity_id(3)));

        state.place_champion(
            3,
            FxVec2::new(RULES.champion_vision_radius.sub(Fx::ONE), Fx::ZERO),
            RULES.champion_max_hp,
        );
        assert!(ids(&view_for(&state, PlayerId(0))).contains(&champion_entity_id(3)));
    }

    #[test]
    fn a_players_own_champion_is_in_the_view_while_it_is_dead() {
        let mut state = new_state(3);
        state.place_champion(0, FxVec2::ZERO, Fx::EPSILON);
        state.place_champion(3, FxVec2::new(Fx::ONE, Fx::ZERO), RULES.champion_max_hp);
        let state = step(&state, &[]);
        let state = step(
            &state,
            &[Input {
                tick: state.tick(),
                seq: 0,
                player: PlayerId(3),
                action: Action::Attack(EntityId(0)),
            }],
        );
        let state = step(&state, &[]);
        assert!(matches!(
            state.champions()[0].liveness,
            Liveness::Dead { .. }
        ));

        let view = view_for(&state, PlayerId(0));
        assert!(matches!(view.own.liveness, Liveness::Dead { .. }));
        // And it is not in `visible`, which is the list of things on the map.
        assert!(!ids(&view).contains(&champion_entity_id(0)));
    }

    #[test]
    fn a_cast_out_of_vision_is_not_announced() {
        let mut state = new_state(3);
        // Blue seat 0 at the origin; Red seat 3 well beyond its vision.
        state.place_champion(0, FxVec2::ZERO, RULES.champion_max_hp);
        state.place_champion(
            3,
            FxVec2::new(Fx::from_int(60), Fx::ZERO),
            RULES.champion_max_hp,
        );
        let cast = [Input {
            tick: Tick(0),
            seq: 0,
            player: PlayerId(3),
            action: Action::Skillshot(FxVec2::new(Fx::NEG_ONE, Fx::ZERO)),
        }];
        let state = step(&state, &cast);

        assert_eq!(state.events().count(), 1, "the cast happened");
        assert!(
            view_for(&state, PlayerId(0)).events.is_empty(),
            "and seat 0 was told about it"
        );
        assert_eq!(
            view_for(&state, PlayerId(3)).events.len(),
            1,
            "while the caster's own team saw it"
        );
    }

    #[test]
    fn a_death_is_announced_to_whoever_could_see_the_ground_it_happened_on() {
        let mut state = new_state(3);
        // Seat 3 dies at the origin, where seat 0 is standing. Seat 4 and 5 are
        // left at their spawn, far away.
        state.place_champion(0, FxVec2::ZERO, RULES.champion_max_hp);
        state.place_champion(3, FxVec2::new(Fx::ONE, Fx::ZERO), Fx::EPSILON);
        let state = step(
            &state,
            &[Input {
                tick: Tick(0),
                seq: 0,
                player: PlayerId(0),
                action: Action::Attack(champion_entity_id(3)),
            }],
        );
        assert!(matches!(
            state.champions()[3].liveness,
            Liveness::Dead { .. }
        ));

        let killer = view_for(&state, PlayerId(0));
        assert!(
            killer
                .events
                .iter()
                .any(|event| matches!(event, VisibleEvent::Death { entity, .. }
                    if *entity == champion_entity_id(3))),
            "the killer saw the kill"
        );
        // The victim's own team-mates were at their spawn and saw nothing, but
        // seat 3's own view still says it is dead, through `own`.
        let distant_ally = view_for(&state, PlayerId(4));
        assert!(distant_ally.events.is_empty());
    }

    #[test]
    fn a_destroyed_tower_stops_giving_vision() {
        let mut state = new_state(3);
        // Nobody is near Blue's outer tower, so it is the only thing that can
        // see the point beside it.
        let beside_the_tower = tower_position(0, &RULES).add(FxVec2::new(Fx::ONE, Fx::ZERO));
        state.place_champion(3, beside_the_tower, RULES.champion_max_hp);
        assert!(ids(&view_for(&state, PlayerId(0))).contains(&champion_entity_id(3)));

        state.set_tower_hp(0, Fx::ZERO);
        assert!(
            !ids(&view_for(&state, PlayerId(0))).contains(&champion_entity_id(3)),
            "rubble sees nothing"
        );
    }

    #[test]
    fn the_two_seats_of_a_team_see_the_same_world() {
        let mut state = new_state(3);
        state.place_champion(0, FxVec2::ZERO, RULES.champion_max_hp);
        state.place_champion(
            3,
            FxVec2::new(Fx::from_int(5), Fx::ZERO),
            RULES.champion_max_hp,
        );
        let first = view_for(&state, PlayerId(1));
        let second = view_for(&state, PlayerId(2));
        assert_eq!(
            first
                .visible
                .iter()
                .filter(|entity| !matches!(entity, EntityView::Champion { id, .. }
                    if *id == champion_entity_id(1) || *id == champion_entity_id(2)))
                .count(),
            second
                .visible
                .iter()
                .filter(|entity| !matches!(entity, EntityView::Champion { id, .. }
                    if *id == champion_entity_id(1) || *id == champion_entity_id(2)))
                .count()
        );
        assert_eq!(first.events, second.events);
    }

    #[test]
    fn a_seat_outside_the_match_gets_a_view_rather_than_a_panic() {
        let state = new_state(3);
        let view = view_for(&state, PlayerId(200));
        assert_eq!(view.tick, Tick(0));
    }

    /// The projection reads the constants it is handed, so a fixture recorded
    /// under its own rules is projected under them too.
    #[test]
    fn the_vision_radius_comes_from_the_rules_it_is_given() {
        let mut state = new_state(3);
        state.place_champion(0, FxVec2::ZERO, RULES.champion_max_hp);
        state.place_champion(
            3,
            FxVec2::new(Fx::from_int(40), Fx::ZERO),
            RULES.champion_max_hp,
        );
        assert!(!ids(&view_for(&state, PlayerId(0))).contains(&champion_entity_id(3)));

        let far_sighted = Rules {
            champion_vision_radius: Fx::from_int(64),
            ..RULES
        };
        assert!(
            ids(&view_for_with_rules(&state, PlayerId(0), &far_sighted))
                .contains(&champion_entity_id(3))
        );
    }

    /// The bound is the encoding's, not the fixture's. Built by hand at the
    /// worst case rather than observed, so that it fails when the *type* grows
    /// and not only when a match happens to get busy.
    #[test]
    fn the_worst_case_view_encodes_to_exactly_the_bound() {
        let mut visible = Vec::new();
        for seat in 1..6 {
            visible.push(EntityView::Champion {
                id: champion_entity_id(seat),
                position: FxVec2::ZERO,
                hp: Fx::ONE,
            });
        }
        for index in 0..4 {
            visible.push(EntityView::Tower {
                id: tower_entity_id(index),
                position: FxVec2::ZERO,
                hp: Fx::ONE,
            });
        }
        for slot in 0..MAX_PROJECTILES {
            visible.push(EntityView::Projectile {
                id: EntityId(1000 + slot as u16),
                position: FxVec2::ZERO,
                velocity: FxVec2::ZERO,
            });
        }
        let events = vec![
            VisibleEvent::Damage {
                target: EntityId(0),
                amount: Fx::ONE,
                at: FxVec2::ZERO,
            };
            MAX_EVENTS
        ];
        let view = PlayerView {
            tick: Tick(1),
            outcome: Outcome::Decided {
                winner: crate::state::Team::Blue,
                at: Tick(1),
            },
            own: OwnView {
                id: champion_entity_id(0),
                position: FxVec2::ZERO,
                liveness: Liveness::Alive { hp: Fx::ONE },
                cooldowns: Cooldowns::default(),
            },
            visible,
            events,
        };
        assert_eq!(view.encode().len(), PlayerView::MAX_ENCODED_BYTES);
    }

    /// Two views that differ anywhere encode differently. Without this the size
    /// assertions would be checking an encoding that could be constant.
    #[test]
    fn the_encoding_follows_the_view() {
        let state = new_state(3);
        let baseline = view_for(&state, PlayerId(0)).encode();
        let stepped = view_for(&step(&state, &[]), PlayerId(0)).encode();
        assert_ne!(baseline, stepped, "the tick alone must move the bytes");

        let other_team = view_for(&state, PlayerId(3)).encode();
        assert_ne!(baseline, other_team);
    }

    /// A cast the caster's own team can see, announced with the ability that
    /// produced it — the cue M2 asks to be culled has to exist to be culled.
    #[test]
    fn a_cast_reaches_the_caster_as_a_cue() {
        let state = new_state(3);
        let state = step(
            &state,
            &[Input {
                tick: Tick(0),
                seq: 0,
                player: PlayerId(0),
                action: Action::Skillshot(FxVec2::new(Fx::ONE, Fx::ZERO)),
            }],
        );
        let view = view_for(&state, PlayerId(0));
        assert!(view.events.iter().any(|event| matches!(
            event,
            VisibleEvent::Cast {
                ability: Ability::Skillshot,
                ..
            }
        )));
    }
}
