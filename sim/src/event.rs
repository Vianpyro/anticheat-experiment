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
/// Seventy-two is [`derived_max_events`] with headroom, and the assertion below
/// is what keeps that sentence true.
///
/// # Why the derivation is a function and not a comment
///
/// It was a comment, and the comment went stale in exactly the way a comment
/// does. `MAX_EVENTS` was 48, derived for a match of six seats; the roster went
/// to nine at M3 and the same derivation now gives 60, so the buffer had
/// silently stopped being a bound and a busy tick would have dropped events that
/// the rules produced. Nothing failed, because nothing was checking: the
/// derivation existed only in prose, and prose does not get recompiled.
///
/// Raising the number to 72 fixes that instance. Making the derivation
/// executable fixes the mechanism — a change to the roster, to the tower count,
/// or to the skillshot's lifetime against its cooldown now stops the build here
/// rather than quietly widening the gap between what a tick can produce and what
/// it can record.
///
/// # Why it is not simply set equal to the derivation
///
/// The headroom is deliberate. `MAX_EVENTS` is an array length inside [`State`]
/// and therefore under [`State::digest`], so moving it invalidates every digest
/// committed in this repository — the two fixtures, and from M4 every recorded
/// match. A bound with slack in it absorbs a rule that emits one more event
/// without costing a re-recording; a bound sitting exactly on its derivation
/// would turn every such change into a corpus migration.
///
/// [`State`]: crate::State
/// [`State::digest`]: crate::State::digest
///
/// # What is outside the derivation, stated rather than hidden
///
/// It is a bound under [`crate::RULES`]. A fixture running through
/// [`crate::step_with_rules`] with a shorter skillshot cooldown can put more
/// projectiles in flight than [`crate::MAX_PROJECTILES_IN_FLIGHT`] counts, and
/// therefore produce more hits in one tick than this bound allows. That is not
/// unsound — beyond the bound events are dropped, which is total, identical on
/// every platform and part of the rules, in the same spirit as a full projectile
/// arena — but it means a fixture with unusual constants can lose a cue, and no
/// assertion here can see that coming. `MAX_EVENTS` is an array length and
/// cannot be a function of a runtime `Rules`.
///
/// # This is a tick's capacity, not a frame's
///
/// What one *message* can carry is [`crate::view::MAX_EVENTS_PER_VIEW`], which
/// is smaller and lives outside these rules: it is a frame budget, the overflow
/// waits for the next frame rather than being lost, and the two numbers are
/// deliberately not the same one.
pub const MAX_EVENTS: usize = 72;

/// The most events the rules can emit in one tick, derived from the roster and
/// the arena rather than remembered.
///
/// Every term is a place in [`crate::step`] that calls `emit`, and the order is
/// that function's order of operations:
///
/// | Term | Where | Count |
/// | --- | --- | --- |
/// | Casts | step 3, and a cooldown blocks a second cast of each ability | `2 × seats` |
/// | The targeted spell's damage | step 3, dealt in the tick it is cast | `seats` |
/// | Projectile hits | step 5, one per projectile in flight | `in_flight` |
/// | Tower shots | step 6, one per standing tower | `towers` |
/// | Basic attacks | step 7, one per seat | `seats` |
/// | Deaths | step 8, resolved once per seat | `seats` |
///
/// Under [`crate::RULES`] that is `2×9 + 9 + 9 + 6 + 9 + 9 = 60`.
#[must_use]
pub const fn derived_max_events(seats: usize, towers: usize, in_flight: usize) -> usize {
    let casts = seats.saturating_mul(2);
    let targeted_damage = seats;
    let projectile_hits = in_flight;
    let tower_shots = towers;
    let basic_attacks = seats;
    let deaths = seats;

    casts
        .saturating_add(targeted_damage)
        .saturating_add(projectile_hits)
        .saturating_add(tower_shots)
        .saturating_add(basic_attacks)
        .saturating_add(deaths)
}

/// The guard the comment used to be.
///
/// A seat added to the roster, a tower added to the map, or a skillshot whose
/// lifetime catches up with its cooldown stops the build here instead of
/// silently turning a bound into a budget.
const _: () = assert!(
    MAX_EVENTS
        >= derived_max_events(
            crate::state::PLAYER_COUNT,
            crate::state::TOWER_COUNT,
            crate::state::MAX_PROJECTILES_IN_FLIGHT,
        ),
    "MAX_EVENTS is no longer a bound on what one tick can record: a tick can now produce more \
     events than the buffer holds, and the excess is dropped. Raise MAX_EVENTS to at least \
     derived_max_events(PLAYER_COUNT, TOWER_COUNT, MAX_PROJECTILES_IN_FLIGHT) — and note that \
     moving it changes every State::digest in the repository"
);

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
