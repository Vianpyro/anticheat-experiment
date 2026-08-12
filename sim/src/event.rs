//! What happened during the tick that produced a state.
//!
//! # Why the state carries this at all
//!
//! `docs/MILESTONES.md` M2 asks that no entity outside a player's vision appear
//! anywhere in [`crate::view::view_for`]'s output, "including in derived events
//! (damage, casts, sounds)". A derived signal is the second half of the maphack
//! problem and the half that is easy to lose: culling the entity list while
//! still announcing that *something* was cast nearby hands the attacker the
//! information the entity list was hiding. So the events have to exist before
//! they can be culled — an empty event list would make M2's exit criterion true
//! by vacuity, which is not the same as true.
//!
//! They live in [`crate::State`] rather than being returned beside it for two
//! reasons. `docs/SCOPE.md` freezes `step(&State, &[Input]) -> State` and
//! `view_for(&State, PlayerId) -> PlayerView`, and neither signature has a
//! place to put a second value. More importantly, being in the state means
//! being under [`crate::State::digest`]: two servers that disagree about what
//! their clients were *told* now fail the determinism suite, which is exactly
//! the class of disagreement this project cannot afford to discover from a
//! player report.
//!
//! # Every event carries the place it happened
//!
//! [`Event::at`] is not decoration, it is the culling key. An event is shown to
//! a player when the point it happened at is inside that player's vision, and
//! that single rule is what keeps the projection free of exceptions: a champion
//! killed this tick is no longer on the map and has no current position to test,
//! but the place it died at is a fact about the tick and does not move.
//!
//! # There is no sound system, and that is not a gap
//!
//! M2's criterion names sounds. This game has no audio, and a "sound cue" in a
//! MOBA is exactly the derived signal an event already is: the noise a cast
//! makes is the cast. [`EventKind::Cast`] is that cue, and it is culled on the
//! same rule as everything else. Inventing a `Sound` variant with no renderer
//! behind it would be a variant nothing produces — vacuity again, wearing the
//! costume of thoroughness.

use crate::fx::Fx;
use crate::state::EntityId;
use crate::vec2::FxVec2;

/// Events one tick can record before it starts dropping them.
///
/// Derived rather than guessed. Per tick the rules can emit at most: two casts
/// per seat (the cooldown blocks the second of each kind), so 12; one targeted
/// spell's damage per seat, 6; one hit per projectile in flight, and the
/// skillshot's 240-tick cooldown against its 45-tick lifetime caps the arena's
/// real occupancy at 6; one shot per tower, 4; one basic attack per seat, 6;
/// one death per seat, 6. Forty. Forty-eight is that with headroom.
///
/// Beyond it events are dropped, in the same spirit as a full projectile arena:
/// dropping is total, identical on every platform, and part of the rules, where
/// growing a buffer would put an allocator inside `step`.
pub const MAX_EVENTS: usize = 48;

/// Which of the two abilities was cast.
///
/// Basic attacks and tower shots are absent on purpose: they produce
/// [`EventKind::Damage`] and nothing else. A cast is worth announcing
/// separately because a skillshot leaves the caster before it does anything,
/// and the gap between the two is information a client has to render.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ability {
    /// The projectile ability.
    Skillshot,
    /// The instant, targeted ability.
    Targeted,
}

/// What kind of thing happened.
///
/// # What these deliberately do not name
///
/// [`EventKind::Damage`] does not name its source. It is the one field a first
/// draft always includes and it is a leak: an attacker within basic-attack
/// range of a point you can see is not necessarily at a point you can see, and
/// "seat 4 hit your ally" tells a client the identity and rough position of a
/// champion the fog was hiding. Dropping it costs damage attribution in a UI
/// that does not exist yet, and it buys the property that every `EntityId` in
/// an event is the entity the event happened *to*, at [`Event::at`] — one rule,
/// no exceptions, checkable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// An ability left a champion.
    Cast {
        /// Who cast it.
        caster: EntityId,
        /// Which ability.
        ability: Ability,
    },
    /// Something took damage.
    Damage {
        /// What was hit.
        target: EntityId,
        /// How much was applied, after the clamp at zero.
        amount: Fx,
    },
    /// A champion was reduced to zero hit points.
    ///
    /// Towers have no event of their own: a destroyed tower stays on the map as
    /// rubble at a position every client can compute from the rules, so its
    /// destruction is already legible from the `hp` of zero in the view.
    Death {
        /// Which champion.
        entity: EntityId,
    },
}

/// One thing that happened, and where.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    /// What happened.
    pub kind: EventKind,
    /// Where it happened, in world units. The key the visibility projection
    /// culls on; see the module documentation.
    pub at: FxVec2,
}

/// The events of one tick, in the order the rules produced them.
///
/// A fixed array rather than a `Vec`, for the reason
/// [`crate::Projectiles`] is one: no allocator inside `step`, and a layout that
/// is a function of the tick's history rather than of anything ambient.
/// Order is the documented order of operations in [`crate::step`], which makes
/// it a rule rather than an artefact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Events {
    pub(crate) slots: [Option<Event>; MAX_EVENTS],
}

impl Default for Events {
    fn default() -> Self {
        Self::new()
    }
}

impl Events {
    /// No events.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [None; MAX_EVENTS],
        }
    }

    /// The events recorded, in order.
    pub fn iter(&self) -> impl Iterator<Item = &Event> {
        self.slots.iter().flatten()
    }

    /// How many were recorded.
    #[must_use]
    pub fn count(&self) -> usize {
        self.iter().count()
    }

    /// Records an event, dropping it if the tick is already full.
    pub(crate) fn push(&mut self, event: Event) {
        for slot in &mut self.slots {
            if slot.is_none() {
                *slot = Some(event);
                return;
            }
        }
    }

    /// Forgets everything. Called once at the top of every tick: these describe
    /// one transition, not a history.
    pub(crate) fn clear(&mut self) {
        self.slots = [None; MAX_EVENTS];
    }
}
