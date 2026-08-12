//! What a player asks for, which is not the same as what happens.
//!
//! Everything in this module arrives from a client, and `docs/SCOPE.md` starts
//! from the axiom that the client is compromised and lying. So none of these
//! values is validated at construction: an [`Action`] can name an `EntityId`
//! that does not exist, aim a skillshot with a zero-length direction, or ask to
//! walk a thousand units off the map. Rejecting those at the type level would
//! only move the problem to whoever decodes the wire format.
//!
//! Instead the rules absorb them: out-of-range coordinates are clamped,
//! unresolvable targets make the order a no-op, and a direction too short to
//! normalise discards the cast without consuming its cooldown. There is no
//! input `step` can be handed that produces an error, a panic, or a state
//! outside the legal domain. That is a property the property tests assert
//! directly, because it is the one an attacker will test first.

use crate::state::{EntityId, Seat, Tick};
use crate::vec2::FxVec2;

/// One command from one player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    /// The tick this input applies to.
    ///
    /// [`crate::step`] ignores an input whose tick is not the state's own, so a
    /// mis-bucketed or replayed input is a no-op rather than an effect applied
    /// at the wrong moment. This makes the field authoritative rather than
    /// advisory, and lets an offline resimulation feed a whole log without
    /// having to bucket it correctly first.
    pub tick: Tick,
    /// Per-player monotonic sequence number. `sim` does not read it; it is the
    /// protocol's identity for the input and it is carried here so that the
    /// replay log and the simulation speak about the same object.
    pub seq: u32,
    /// Which seat issued it.
    ///
    /// A [`Seat`] rather than a number, so an input attributed to a player who
    /// is not in the match is not a case `step` has to absorb — it is a value
    /// that cannot be built. The server writes this field from the session the
    /// message arrived on and never from the message itself, which is what
    /// makes "a client drove somebody else's champion" unreachable rather than
    /// merely rejected.
    pub player: Seat,
    /// What they asked for.
    pub action: Action,
}

/// The five things a player can ask for.
///
/// One champion means a concrete set of actions, not a trait and not a generic
/// ability system — `docs/ARCHITECTURE.md` is explicit that one implementation
/// does not earn an abstraction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Cancel the standing order and hold position.
    Idle,
    /// Walk to a point, clamped into the map.
    Move(FxVec2),
    /// Fire the skillshot along a direction. The direction is normalised; if
    /// it is shorter than [`crate::RULES`]'s `min_direction_length` the cast is
    /// discarded and the cooldown is not consumed.
    Skillshot(FxVec2),
    /// Cast the targeted spell at an entity. Requires the target to be a living
    /// enemy champion in range at the moment of the cast.
    Targeted(EntityId),
    /// Walk into basic-attack range of an entity and attack it. Unlike the
    /// other two abilities this is a standing order, so it persists until
    /// replaced.
    Attack(EntityId),
}
