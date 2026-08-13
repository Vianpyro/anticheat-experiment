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
//! [`PlayerView::encode`] and [`PlayerView::decode`] are a hand-written
//! canonical encoding and its inverse, and `sim` still has an empty
//! `[dependencies]` table. `docs/ARCHITECTURE.md` allows `serde` here; the
//! permission was not taken at M3 either, and the reason is now stronger than
//! it was: the transport pads every `View` message to
//! [`PlayerView::MAX_ENCODED_BYTES`], and that bound is derived from this byte
//! layout rather than measured from a run. A codec that chose its own widths
//! would make the padding budget a number nobody could derive.
//!
//! The two halves live in one file deliberately. A decoder in `protocol` would
//! be a second copy of the layout, and a drifted decoder does not error — it
//! produces a plausible view. Here, a field added to a view type has to be
//! handled twice in the same diff, and the round-trip test below covers both.
//!
//! What the encoding is *not* is a constant size, and it is not supposed to be:
//! the constant size is the frame's, one bucket wide enough for the worst case,
//! and `protocol::ServerFrame` is where it is imposed. The reason it is imposed
//! there rather than here is that the other half of the traffic-shape invariant
//! — one message per player per tick, whatever happened — is a statement about
//! a server loop and cannot be made by a projection at all.

use crate::event::{Ability, Event, EventKind};
use crate::fx::Fx;
use crate::rules::{RULES, Rules};
use crate::state::{
    Cooldowns, EntityId, Liveness, MAX_PROJECTILES, Outcome, PLAYER_COUNT, Seat, State,
    TOWER_COUNT, Team, Tick, champion_entity_id, tower_entity_id, tower_position,
};
use crate::vec2::FxVec2;

/// Events one view may carry, and therefore the ones a frame can.
///
/// # Why the frame has a smaller event budget than the tick does
///
/// [`crate::MAX_EVENTS`] is what one *tick* can record; this is what one
/// *message* can carry, and they are different numbers for a reason that lives
/// outside this crate. The transport pads every frame to
/// [`PlayerView::MAX_ENCODED_BYTES`] and cuts it into a constant number of
/// datagrams that must each fit the path MTU, so the event budget is what is
/// left of a datagram after the entity list — and the entity list cannot be
/// capped, because which entities survive a cap is a function of what is
/// visible and trading a length channel for a content channel is not a saving
/// (`docs/ARCHITECTURE.md`, the padding budget). Sixteen is what the geometry
/// leaves: two datagrams under 600 bytes each.
///
/// # What happens to the seventeenth event
///
/// It is **deferred, not dropped**: `protocol::EventBacklog` holds a
/// per-recipient queue and delivers the overflow on the next frame, in the
/// order the rules produced it. No event is lost until the queue itself
/// overflows, which takes a sustained rate no reachable state produces.
///
/// That is not a side channel, and the argument is the one the per-recipient
/// handle space already makes: the queue is a function of the events this
/// recipient was *entitled* to see, and of nothing else. Two states a player
/// cannot tell apart produce the same entitled events, hence the same queue,
/// hence the same bytes.
///
/// The cap is enforced twice, and the two halves do different jobs.
/// [`PlayerView::encode`] writes at most this many events, which is what makes
/// the bound above a property of the *encoding* rather than a promise every
/// caller has to keep — there is no view that encodes to more than
/// [`PlayerView::MAX_ENCODED_BYTES`], so no framing code has a failure path.
/// The backlog is what makes the truncation unreachable, so that the game does
/// not silently lose a cue.
pub const MAX_EVENTS_PER_VIEW: usize = 16;

/// The budget is smaller than a tick's capacity, which is the whole reason the
/// backlog exists. If they ever coincide, the deferral is dead code and the
/// comment above it is a lie, so this stops the build instead.
const _: () = assert!(MAX_EVENTS_PER_VIEW < crate::event::MAX_EVENTS);

/// Bytes in the parts of the encoding that are always present: `tick`,
/// `outcome`, `own`, and the two list lengths.
const FIXED_BYTES: usize = 4 + 6 + 21 + 2 + 2;
/// Bytes in a champion or tower entry: tag, handle, position, hit points.
const ENTITY_ENTRY_BYTES: usize = 1 + 2 + 8 + 4;
/// Bytes in a projectile entry: tag, handle, position, velocity.
const PROJECTILE_ENTRY_BYTES: usize = 1 + 2 + 8 + 8;
/// Bytes in the widest event, which is damage: tag, handle, amount, place.
const EVENT_ENTRY_BYTES: usize = 1 + 2 + 4 + 8;

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
    /// Everything else the player can see, in ascending [`EntityId`] order:
    /// champions, then towers, then projectiles, because the handle spaces are
    /// laid out in that order.
    ///
    /// The order is part of the encoding rather than an artefact of iteration,
    /// and it is the handle rather than the iteration order for a reason that
    /// is about leaks and not about tidiness — see [`view_for_with_rules`],
    /// where the projectiles are sorted.
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
///
/// The seat is a [`Seat`], which has no value outside the match. That is the
/// whole of the argument that this function cannot be asked for the vision of a
/// player who is not playing: there is no such request to make. The byte that
/// arrived over the network became a seat at the protocol boundary or it was
/// refused there — see [`Seat::from_index`].
#[must_use]
pub fn view_for(state: &State, player: Seat) -> PlayerView {
    view_for_with_rules(state, player, &RULES)
}

/// The same projection under the constants given.
///
/// Exists for the same reason [`crate::step_with_rules`] does: a fixture
/// recorded under its own [`Rules`] must be projected under those rules too,
/// and a caller that has to name the constants cannot silently mix two sets.
#[must_use]
pub fn view_for_with_rules(state: &State, player: Seat, rules: &Rules) -> PlayerView {
    let team = player.team();

    let mut visible = Vec::new();
    for seat in Seat::ALL {
        if seat == player {
            continue;
        }
        let champion = state.champion(seat);
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

    // Including the caster's own projectile only while it is in team vision is
    // the strict reading, and it is the one taken: one rule, applied to
    // everything, is worth more than a courtesy that would need its own
    // sentence in this module and its own branch in the test.
    //
    // Sorted by handle rather than emitted in arena order, and that is a
    // culling rule rather than a formatting choice. The arena allocates the
    // lowest free slot, so which slot a projectile occupies is a function of
    // every cast that came before it — including the ones this player was never
    // shown. Two views listing the same two projectiles in different orders
    // therefore differ in a way that reports on the fog, which is the shape of
    // thing `docs/SCOPE.md` counts as an information leak. Ordering by handle
    // makes the order a function of the content, and `sim/tests/view_properties.rs`
    // asserts it over reachable states; it is what found this.
    //
    // The sort is stable, so two projectiles that shared a handle — reachable
    // only by wrapping the allocator, see `State::allocate_projectile_id` —
    // would still come out in arena order rather than in an order that depends
    // on the sort's internals.
    let mut in_flight: Vec<&crate::state::Projectile> = state
        .projectiles()
        .iter()
        .filter(|projectile| can_see(state, team, projectile.position, rules))
        .collect();
    in_flight.sort_by_key(|projectile| projectile.id.0);
    for projectile in in_flight {
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
        own: own_view(state, player),
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
    for seat in Seat::ALL {
        if seat.team() != team {
            continue;
        }
        let champion = state.champion(seat);
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
///
/// It had a second arm until M3 — the seat that named nobody, answered with an
/// invented champion standing on a spawn point. That arm was the reason a
/// server passing an unvalidated handle would have got a plausible view back
/// instead of a refusal. [`Seat`] removed the case rather than the branch's
/// contents, which is the difference the debt was about.
fn own_view(state: &State, seat: Seat) -> OwnView {
    let champion = state.champion(seat);
    OwnView {
        id: champion_entity_id(seat),
        position: champion.position,
        liveness: champion.liveness,
        cooldowns: champion.cooldowns,
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
            Self::Green => 2u8.encode_into(out),
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
    /// | champion or tower entry | 1 + 2 + 8 + 4 = 15 | [`PLAYER_COUNT`] − 1 + [`TOWER_COUNT`] |
    /// | projectile entry | 1 + 2 + 8 + 8 = 19 | [`MAX_PROJECTILES`] |
    /// | `events` length | 2 | 1 |
    /// | widest event (damage) | 1 + 2 + 4 + 8 = 15 | [`MAX_EVENTS_PER_VIEW`] |
    ///
    /// One champion short of the roster: a player's own is in `own` and never
    /// in `visible`.
    ///
    /// It is an expression rather than a literal, so that a change to the
    /// roster or to the arenas moves it without anyone re-doing the sum by
    /// hand. The widths are still written out, and they are still a claim about
    /// the encoding below rather than a fact the compiler checks — which is
    /// what `the_worst_case_view_encodes_to_exactly_the_bound` is for.
    pub const MAX_ENCODED_BYTES: usize = FIXED_BYTES
        + (PLAYER_COUNT - 1 + TOWER_COUNT) * ENTITY_ENTRY_BYTES
        + MAX_PROJECTILES * PROJECTILE_ENTRY_BYTES
        + MAX_EVENTS_PER_VIEW * EVENT_ENTRY_BYTES;

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
        // At most `MAX_EVENTS_PER_VIEW`, so that the bound above is a property
        // of this function rather than an obligation on its caller: a framing
        // layer that pads to `MAX_ENCODED_BYTES` must have no input that
        // exceeds it. The transport never reaches the truncation — its
        // per-recipient backlog defers the overflow instead — and that
        // separation is the point: this half keeps the encoding total, the
        // other half keeps the game lossless.
        let shown = events.len().min(MAX_EVENTS_PER_VIEW);
        u16::try_from(shown)
            .unwrap_or(u16::MAX)
            .encode_into(&mut out);
        for event in events.iter().take(shown) {
            event.encode_into(&mut out);
        }
        out
    }

    /// Reads a view back, returning it and how many bytes it occupied.
    ///
    /// The byte count is the second half of the return value rather than an
    /// implied "all of them", because the transport pads: a `View` frame is a
    /// constant number of bytes of which the view is a prefix and the rest is
    /// zero, and the caller has to know where the view stopped in order to
    /// check that the remainder is padding rather than a message somebody
    /// smuggled in behind one.
    ///
    /// `None` for any byte string that is not an encoding of a view. Total on
    /// every input, including truncated and adversarial ones.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<(Self, usize)> {
        let mut cursor = Cursor::new(bytes);
        let tick = Tick::decode_from(&mut cursor)?;
        let outcome = Outcome::decode_from(&mut cursor)?;
        let own = OwnView::decode_from(&mut cursor)?;

        // The two lengths are the only variable-length part of the encoding,
        // and they are the one place a hostile frame could ask for work
        // proportional to a number it chose. Bounded against what the *state*
        // can hold rather than against the buffer: `visible` cannot exceed the
        // arenas that produce it, and a frame claiming more is malformed, not
        // large.
        let visible_len = usize::from(u16::decode_from(&mut cursor)?);
        if visible_len
            > PLAYER_COUNT
                .saturating_add(TOWER_COUNT)
                .saturating_add(MAX_PROJECTILES)
        {
            return None;
        }
        let mut visible = Vec::with_capacity(visible_len);
        for _ in 0..visible_len {
            visible.push(EntityView::decode_from(&mut cursor)?);
        }

        let events_len = usize::from(u16::decode_from(&mut cursor)?);
        // Against what a *frame* can carry rather than against what a tick can
        // record: the encoder writes no more than this, so anything above it is
        // a frame nothing in this workspace produced.
        if events_len > MAX_EVENTS_PER_VIEW {
            return None;
        }
        let mut events = Vec::with_capacity(events_len);
        for _ in 0..events_len {
            events.push(VisibleEvent::decode_from(&mut cursor)?);
        }

        Some((
            Self {
                tick,
                outcome,
                own,
                visible,
                events,
            },
            cursor.at,
        ))
    }
}

/// Reads a big-endian value out of a cursor, or gives up.
///
/// The mirror of [`Encode`], and it lives beside it rather than in `protocol`
/// on purpose. The encoding is one object with two halves, and a decoder in
/// another crate is a second copy of the byte layout that can drift from this
/// one — silently, because a drifted decoder produces a *plausible* view rather
/// than an error. Keeping the pair in one file means the round-trip test below
/// covers both, and a field added to a view type has to be handled twice in the
/// same diff.
///
/// Every implementation is total: it returns `None` rather than panicking on
/// any byte string, because the bytes come off a socket. The one that consumes
/// them today is the headless client, reading from the server it trusts; that
/// is not a reason to write a parser that can be made to panic.
trait Decode: Sized {
    fn decode_from(cursor: &mut Cursor<'_>) -> Option<Self>;
}

/// A position in a byte string, which never reads past the end.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(count)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }
}

impl Decode for u8 {
    fn decode_from(cursor: &mut Cursor<'_>) -> Option<Self> {
        cursor.take(1)?.first().copied()
    }
}

impl Decode for u16 {
    fn decode_from(cursor: &mut Cursor<'_>) -> Option<Self> {
        Some(Self::from_be_bytes(cursor.take(2)?.try_into().ok()?))
    }
}

impl Decode for u32 {
    fn decode_from(cursor: &mut Cursor<'_>) -> Option<Self> {
        Some(Self::from_be_bytes(cursor.take(4)?.try_into().ok()?))
    }
}

impl Decode for i32 {
    fn decode_from(cursor: &mut Cursor<'_>) -> Option<Self> {
        Some(Self::from_be_bytes(cursor.take(4)?.try_into().ok()?))
    }
}

impl Decode for Fx {
    fn decode_from(cursor: &mut Cursor<'_>) -> Option<Self> {
        Some(Self::from_raw(i32::decode_from(cursor)?))
    }
}

impl Decode for FxVec2 {
    fn decode_from(cursor: &mut Cursor<'_>) -> Option<Self> {
        Some(Self::new(
            Fx::decode_from(cursor)?,
            Fx::decode_from(cursor)?,
        ))
    }
}

impl Decode for EntityId {
    fn decode_from(cursor: &mut Cursor<'_>) -> Option<Self> {
        Some(Self(u16::decode_from(cursor)?))
    }
}

impl Decode for Tick {
    fn decode_from(cursor: &mut Cursor<'_>) -> Option<Self> {
        Some(Self(u32::decode_from(cursor)?))
    }
}

impl Decode for Team {
    fn decode_from(cursor: &mut Cursor<'_>) -> Option<Self> {
        match u8::decode_from(cursor)? {
            0 => Some(Self::Blue),
            1 => Some(Self::Red),
            2 => Some(Self::Green),
            _ => None,
        }
    }
}

impl Decode for Ability {
    fn decode_from(cursor: &mut Cursor<'_>) -> Option<Self> {
        match u8::decode_from(cursor)? {
            0 => Some(Self::Skillshot),
            1 => Some(Self::Targeted),
            _ => None,
        }
    }
}

impl Decode for Outcome {
    fn decode_from(cursor: &mut Cursor<'_>) -> Option<Self> {
        let tag = u8::decode_from(cursor)?;
        let winner = Team::decode_from(cursor)?;
        let at = Tick::decode_from(cursor)?;
        match tag {
            // The padded variant: both fields are present on the wire and both
            // are meaningless, which is what keeps "the match ended" out of the
            // message length.
            0 => Some(Self::InProgress),
            1 => Some(Self::Decided { winner, at }),
            _ => None,
        }
    }
}

impl Decode for Liveness {
    fn decode_from(cursor: &mut Cursor<'_>) -> Option<Self> {
        match u8::decode_from(cursor)? {
            0 => Some(Self::Alive {
                hp: Fx::decode_from(cursor)?,
            }),
            1 => Some(Self::Dead {
                respawn_at: Tick::decode_from(cursor)?,
            }),
            _ => None,
        }
    }
}

impl Decode for Cooldowns {
    fn decode_from(cursor: &mut Cursor<'_>) -> Option<Self> {
        Some(Self {
            basic_attack: u16::decode_from(cursor)?,
            skillshot: u16::decode_from(cursor)?,
            targeted: u16::decode_from(cursor)?,
        })
    }
}

impl Decode for OwnView {
    fn decode_from(cursor: &mut Cursor<'_>) -> Option<Self> {
        Some(Self {
            id: EntityId::decode_from(cursor)?,
            position: FxVec2::decode_from(cursor)?,
            liveness: Liveness::decode_from(cursor)?,
            cooldowns: Cooldowns::decode_from(cursor)?,
        })
    }
}

impl Decode for EntityView {
    fn decode_from(cursor: &mut Cursor<'_>) -> Option<Self> {
        match u8::decode_from(cursor)? {
            0 => Some(Self::Champion {
                id: EntityId::decode_from(cursor)?,
                position: FxVec2::decode_from(cursor)?,
                hp: Fx::decode_from(cursor)?,
            }),
            1 => Some(Self::Tower {
                id: EntityId::decode_from(cursor)?,
                position: FxVec2::decode_from(cursor)?,
                hp: Fx::decode_from(cursor)?,
            }),
            2 => Some(Self::Projectile {
                id: EntityId::decode_from(cursor)?,
                position: FxVec2::decode_from(cursor)?,
                velocity: FxVec2::decode_from(cursor)?,
            }),
            _ => None,
        }
    }
}

impl Decode for VisibleEvent {
    fn decode_from(cursor: &mut Cursor<'_>) -> Option<Self> {
        match u8::decode_from(cursor)? {
            0 => Some(Self::Cast {
                caster: EntityId::decode_from(cursor)?,
                ability: Ability::decode_from(cursor)?,
                at: FxVec2::decode_from(cursor)?,
            }),
            1 => Some(Self::Damage {
                target: EntityId::decode_from(cursor)?,
                amount: Fx::decode_from(cursor)?,
                at: FxVec2::decode_from(cursor)?,
            }),
            2 => Some(Self::Death {
                entity: EntityId::decode_from(cursor)?,
                at: FxVec2::decode_from(cursor)?,
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EntityView, MAX_EVENTS_PER_VIEW, PlayerView, VisibleEvent, view_for, view_for_with_rules,
    };
    use crate::event::{Ability, MAX_EVENTS};
    use crate::fx::Fx;
    use crate::input::{Action, Input};
    use crate::rules::{RULES, Rules};
    use crate::state::{
        Cooldowns, EntityId, Liveness, MAX_PROJECTILES, Outcome, Seat, Tick, champion_entity_id,
        new_state, tower_entity_id, tower_position,
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
        let view = view_for(&state, Seat::Blue0);
        for seat in [Seat::Red0, Seat::Red1, Seat::Red2] {
            assert!(
                !ids(&view).contains(&champion_entity_id(seat)),
                "{seat:?} is a lane away and still in the view"
            );
        }
        // …and the culling is not "return nothing": the allies at spawn are
        // there, which is what makes the assertion above mean something.
        assert!(ids(&view).contains(&champion_entity_id(Seat::Blue1)));
        assert!(ids(&view).contains(&champion_entity_id(Seat::Blue2)));
    }

    #[test]
    fn an_enemy_that_walks_into_range_appears() {
        let mut state = new_state(3);
        let meeting_point = FxVec2::new(Fx::ZERO, Fx::ZERO);
        state.place_champion(Seat::Blue0, meeting_point, RULES.champion_max_hp);
        state.place_champion(
            Seat::Red0,
            FxVec2::new(RULES.champion_vision_radius.add(Fx::ONE), Fx::ZERO),
            RULES.champion_max_hp,
        );
        assert!(!ids(&view_for(&state, Seat::Blue0)).contains(&champion_entity_id(Seat::Red0)));

        state.place_champion(
            Seat::Red0,
            FxVec2::new(RULES.champion_vision_radius.sub(Fx::ONE), Fx::ZERO),
            RULES.champion_max_hp,
        );
        assert!(ids(&view_for(&state, Seat::Blue0)).contains(&champion_entity_id(Seat::Red0)));
    }

    #[test]
    fn a_players_own_champion_is_in_the_view_while_it_is_dead() {
        let mut state = new_state(3);
        state.place_champion(Seat::Blue0, FxVec2::ZERO, Fx::EPSILON);
        state.place_champion(
            Seat::Red0,
            FxVec2::new(Fx::ONE, Fx::ZERO),
            RULES.champion_max_hp,
        );
        let state = step(&state, &[]);
        let state = step(
            &state,
            &[Input {
                tick: state.tick(),
                seq: 0,
                player: Seat::Red0,
                action: Action::Attack(EntityId(0)),
            }],
        );
        let state = step(&state, &[]);
        assert!(matches!(
            state.champions()[0].liveness,
            Liveness::Dead { .. }
        ));

        let view = view_for(&state, Seat::Blue0);
        assert!(matches!(view.own.liveness, Liveness::Dead { .. }));
        // And it is not in `visible`, which is the list of things on the map.
        assert!(!ids(&view).contains(&champion_entity_id(Seat::Blue0)));
    }

    #[test]
    fn a_cast_out_of_vision_is_not_announced() {
        let mut state = new_state(3);
        // Blue seat 0 at the origin; Red seat 3 well beyond its vision.
        state.place_champion(Seat::Blue0, FxVec2::ZERO, RULES.champion_max_hp);
        state.place_champion(
            Seat::Red0,
            FxVec2::new(Fx::from_int(60), Fx::ZERO),
            RULES.champion_max_hp,
        );
        let cast = [Input {
            tick: Tick(0),
            seq: 0,
            player: Seat::Red0,
            action: Action::Skillshot(FxVec2::new(Fx::NEG_ONE, Fx::ZERO)),
        }];
        let state = step(&state, &cast);

        assert_eq!(state.events().count(), 1, "the cast happened");
        assert!(
            view_for(&state, Seat::Blue0).events.is_empty(),
            "and seat 0 was told about it"
        );
        assert_eq!(
            view_for(&state, Seat::Red0).events.len(),
            1,
            "while the caster's own team saw it"
        );
    }

    #[test]
    fn a_death_is_announced_to_whoever_could_see_the_ground_it_happened_on() {
        let mut state = new_state(3);
        // Seat 3 dies at the origin, where seat 0 is standing. Seat 4 and 5 are
        // left at their spawn, far away.
        state.place_champion(Seat::Blue0, FxVec2::ZERO, RULES.champion_max_hp);
        state.place_champion(Seat::Red0, FxVec2::new(Fx::ONE, Fx::ZERO), Fx::EPSILON);
        let state = step(
            &state,
            &[Input {
                tick: Tick(0),
                seq: 0,
                player: Seat::Blue0,
                action: Action::Attack(champion_entity_id(Seat::Red0)),
            }],
        );
        assert!(matches!(
            state.champions()[3].liveness,
            Liveness::Dead { .. }
        ));

        let killer = view_for(&state, Seat::Blue0);
        assert!(
            killer
                .events
                .iter()
                .any(|event| matches!(event, VisibleEvent::Death { entity, .. }
                    if *entity == champion_entity_id(Seat::Red0))),
            "the killer saw the kill"
        );
        // The victim's own team-mates were at their spawn and saw nothing, but
        // seat 3's own view still says it is dead, through `own`.
        let distant_ally = view_for(&state, Seat::Red1);
        assert!(distant_ally.events.is_empty());
    }

    #[test]
    fn a_destroyed_tower_stops_giving_vision() {
        let mut state = new_state(3);
        // Nobody is near Blue's outer tower, so it is the only thing that can
        // see the point beside it.
        let beside_the_tower = tower_position(0, &RULES).add(FxVec2::new(Fx::ONE, Fx::ZERO));
        state.place_champion(Seat::Red0, beside_the_tower, RULES.champion_max_hp);
        assert!(ids(&view_for(&state, Seat::Blue0)).contains(&champion_entity_id(Seat::Red0)));

        state.set_tower_hp(0, Fx::ZERO);
        assert!(
            !ids(&view_for(&state, Seat::Blue0)).contains(&champion_entity_id(Seat::Red0)),
            "rubble sees nothing"
        );
    }

    #[test]
    fn the_two_seats_of_a_team_see_the_same_world() {
        let mut state = new_state(3);
        state.place_champion(Seat::Blue0, FxVec2::ZERO, RULES.champion_max_hp);
        state.place_champion(
            Seat::Red0,
            FxVec2::new(Fx::from_int(5), Fx::ZERO),
            RULES.champion_max_hp,
        );
        let first = view_for(&state, Seat::Blue1);
        let second = view_for(&state, Seat::Blue2);
        assert_eq!(
            first
                .visible
                .iter()
                .filter(|entity| !matches!(entity, EntityView::Champion { id, .. }
                    if *id == champion_entity_id(Seat::Blue1) || *id == champion_entity_id(Seat::Blue2)))
                .count(),
            second
                .visible
                .iter()
                .filter(|entity| !matches!(entity, EntityView::Champion { id, .. }
                    if *id == champion_entity_id(Seat::Blue1) || *id == champion_entity_id(Seat::Blue2)))
                .count()
        );
        assert_eq!(first.events, second.events);
    }

    /// There is no seat outside the match to test, and that is the point.
    ///
    /// This used to assert that `view_for(&state, PlayerId(200))` returned a
    /// view rather than panicking, which was the best a `u8` allowed. The
    /// replacement is not another assertion: it is [`Seat`], which has nine
    /// values, so the call this test used to make no longer type-checks. What
    /// remains to be tested is the frontier that turns a byte into a seat, and
    /// that lives in `protocol` with the rest of the decoder.
    #[test]
    fn every_seat_that_exists_gets_a_view() {
        let state = new_state(3);
        for seat in Seat::ALL {
            assert_eq!(view_for(&state, seat).tick, Tick(0));
            assert_eq!(view_for(&state, seat).own.id, champion_entity_id(seat));
        }
    }

    /// The projection reads the constants it is handed, so a fixture recorded
    /// under its own rules is projected under them too.
    #[test]
    fn the_vision_radius_comes_from_the_rules_it_is_given() {
        let mut state = new_state(3);
        state.place_champion(Seat::Blue0, FxVec2::ZERO, RULES.champion_max_hp);
        state.place_champion(
            Seat::Red0,
            FxVec2::new(Fx::from_int(40), Fx::ZERO),
            RULES.champion_max_hp,
        );
        assert!(!ids(&view_for(&state, Seat::Blue0)).contains(&champion_entity_id(Seat::Red0)));

        let far_sighted = Rules {
            champion_vision_radius: Fx::from_int(64),
            ..RULES
        };
        assert!(
            ids(&view_for_with_rules(&state, Seat::Blue0, &far_sighted))
                .contains(&champion_entity_id(Seat::Red0))
        );
    }

    /// The bound is the encoding's, not the fixture's. Built by hand at the
    /// worst case rather than observed, so that it fails when the *type* grows
    /// and not only when a match happens to get busy.
    #[test]
    fn the_worst_case_view_encodes_to_exactly_the_bound() {
        let mut visible = Vec::new();
        for seat in Seat::ALL.into_iter().filter(|seat| *seat != Seat::Blue0) {
            visible.push(EntityView::Champion {
                id: champion_entity_id(seat),
                position: FxVec2::ZERO,
                hp: Fx::ONE,
            });
        }
        for index in 0..crate::state::TOWER_COUNT {
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
            MAX_EVENTS_PER_VIEW
        ];
        let view = PlayerView {
            tick: Tick(1),
            outcome: Outcome::Decided {
                winner: crate::state::Team::Blue,
                at: Tick(1),
            },
            own: OwnView {
                id: champion_entity_id(Seat::Blue0),
                position: FxVec2::ZERO,
                liveness: Liveness::Alive { hp: Fx::ONE },
                cooldowns: Cooldowns::default(),
            },
            visible,
            events,
        };
        assert_eq!(view.encode().len(), PlayerView::MAX_ENCODED_BYTES);
    }

    /// The event budget is a property of the encoding, not a promise the caller
    /// makes.
    ///
    /// A tick can record more events than one frame can carry
    /// (`MAX_EVENTS_PER_VIEW` against [`MAX_EVENTS`]), and the transport's
    /// per-recipient backlog is what keeps the overflow from being lost. This
    /// is the other half: whatever the caller hands over, the bytes stay inside
    /// the bound, so no framing layer has a payload it cannot pad. Truncating
    /// here is unreachable from the server, and it is what makes that true
    /// rather than hoped.
    #[test]
    fn the_encoding_carries_no_more_events_than_a_frame_has_room_for() {
        let overfull = vec![
            VisibleEvent::Death {
                entity: EntityId(0),
                at: FxVec2::ZERO,
            };
            MAX_EVENTS
        ];
        let view = PlayerView {
            tick: Tick(1),
            outcome: Outcome::InProgress,
            own: OwnView {
                id: champion_entity_id(Seat::Blue0),
                position: FxVec2::ZERO,
                liveness: Liveness::Alive { hp: Fx::ONE },
                cooldowns: Cooldowns::default(),
            },
            visible: Vec::new(),
            events: overfull,
        };
        let encoded = view.encode();
        assert!(encoded.len() <= PlayerView::MAX_ENCODED_BYTES);
        let (decoded, read) = PlayerView::decode(&encoded).expect("the bound was not encodable");
        assert_eq!(read, encoded.len());
        assert_eq!(decoded.events.len(), MAX_EVENTS_PER_VIEW);
    }

    /// …and a frame claiming more events than that is refused rather than
    /// half-read.
    #[test]
    fn a_view_claiming_more_events_than_a_frame_holds_is_refused() {
        let view = view_for(&new_state(3), Seat::Blue0);
        let mut bytes = view.encode();
        let events_len_at = bytes.len() - 2;
        let too_many = u16::try_from(MAX_EVENTS_PER_VIEW + 1).expect("fits");
        bytes[events_len_at..].copy_from_slice(&too_many.to_be_bytes());
        assert!(PlayerView::decode(&bytes).is_none());
    }

    /// Two views that differ anywhere encode differently. Without this the size
    /// assertions would be checking an encoding that could be constant.
    #[test]
    fn the_encoding_follows_the_view() {
        let state = new_state(3);
        let baseline = view_for(&state, Seat::Blue0).encode();
        let stepped = view_for(&step(&state, &[]), Seat::Blue0).encode();
        assert_ne!(baseline, stepped, "the tick alone must move the bytes");

        let other_team = view_for(&state, Seat::Red0).encode();
        assert_ne!(baseline, other_team);
    }

    /// The encoding and its inverse agree, on views reached by simulation and
    /// on the worst case the bound is derived from.
    ///
    /// Not a "does the codec work" test: it is the runtime half of keeping the
    /// two functions in one file. A field added to `encode` and forgotten in
    /// `decode` reads the next field's bytes as its own, so the round trip
    /// comes back wrong rather than short, which is precisely the failure a
    /// length check would miss.
    #[test]
    fn a_view_survives_a_round_trip_through_its_own_encoding() {
        let mut state = new_state(5);
        state.place_champion(Seat::Blue0, FxVec2::ZERO, RULES.champion_max_hp);
        state.place_champion(
            Seat::Red0,
            FxVec2::new(Fx::from_int(2), Fx::ONE),
            RULES.champion_max_hp,
        );
        let mut state = step(
            &state,
            &[Input {
                tick: Tick(0),
                seq: 0,
                player: Seat::Blue0,
                action: Action::Skillshot(FxVec2::new(Fx::ONE, Fx::ZERO)),
            }],
        );
        for _ in 0..3 {
            state = step(&state, &[]);
        }

        for seat in Seat::ALL {
            let view = view_for(&state, seat);
            let encoded = view.encode();
            let (decoded, read) =
                PlayerView::decode(&encoded).expect("a view this crate encoded did not decode");
            assert_eq!(decoded, view, "{seat:?}");
            assert_eq!(read, encoded.len(), "{seat:?}: bytes consumed");
        }
    }

    /// Decoding is total: no byte string panics it, and a truncated or
    /// nonsensical one is refused rather than half-read.
    #[test]
    fn decoding_refuses_what_it_cannot_read() {
        let view = view_for(&new_state(5), Seat::Blue0);
        let encoded = view.encode();

        for cut in 0..encoded.len() {
            assert!(
                PlayerView::decode(&encoded[..cut]).is_none(),
                "a view truncated to {cut} bytes decoded anyway"
            );
        }
        // Trailing bytes are not an error here — the frame is padded and the
        // caller checks the remainder. What comes back is the view and the
        // length it occupied, which is how the caller can check it at all.
        let mut padded = encoded.clone();
        padded.extend_from_slice(&[0u8; 64]);
        let (decoded, read) = PlayerView::decode(&padded).expect("padding refused a valid view");
        assert_eq!(decoded, view);
        assert_eq!(read, encoded.len());

        // A length prefix claiming more entities than the arenas can hold is
        // refused without trying to allocate for it.
        let mut lying = encoded.clone();
        let visible_len_at = 4 + 6 + 21;
        lying[visible_len_at] = 0xFF;
        lying[visible_len_at + 1] = 0xFF;
        assert!(PlayerView::decode(&lying).is_none());
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
                player: Seat::Blue0,
                action: Action::Skillshot(FxVec2::new(Fx::ONE, Fx::ZERO)),
            }],
        );
        let view = view_for(&state, Seat::Blue0);
        assert!(view.events.iter().any(|event| matches!(
            event,
            VisibleEvent::Cast {
                ability: Ability::Skillshot,
                ..
            }
        )));
    }
}
