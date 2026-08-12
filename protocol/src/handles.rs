//! A projectile handle space per recipient.
//!
//! # The channel this closes
//!
//! Projectile handles come from a counter that is global to the match. A client
//! that is shown `1005` and later `1009` has learned that three skillshots were
//! cast in between, somewhere it could not see. That is a wider channel than
//! the one M2 closed by sorting the entity list — that one leaked the *order* of
//! two legitimate sightings, this one leaks a count of events the recipient was
//! never shown — and `docs/ARCHITECTURE.md` deferred it here because closing it
//! is a protocol decision with consequences for reconciliation.
//!
//! Champion and tower handles are not remapped and must not be. A champion's
//! handle *is* its seat, which is how a client knows whose champion it is
//! looking at, and a tower's position follows from the rules. Both are public
//! by construction; renaming them would cost the client the only identity it
//! has for them and buy nothing.
//!
//! # The shape that was rejected, and why it is the tempting one
//!
//! The obvious construction is a map from global handle to local handle,
//! allocated on first observation and **released when the projectile expires**,
//! with released handles going back into a free list. It looks strictly better:
//! bounded memory, small numbers, no counter to exhaust.
//!
//! It reopens the channel in a different place. A recycled handle is
//! observable: a client that has seen local handle 3 released and then sees a
//! new projectile arrive as handle 3 has learned something about the *timing* of
//! the allocator, and with a full free list it learns how many handles are
//! outstanding — which is a count of projectiles in flight, including the ones
//! it cannot see. Closing a counting channel with an allocator that recycles is
//! trading one leak for a subtler one.
//!
//! So: **a monotone local counter, never reused.** Recipient `R`'s first
//! projectile is `1000`, its second `1001`, and no local handle is ever issued
//! twice within a session. The only thing a recipient can read off its own
//! handles is how many distinct projectiles it has been shown, which it could
//! have counted anyway.
//!
//! # What it costs reconciliation
//!
//! Three consequences, all of them real and none of them expensive at this
//! milestone:
//!
//! - **Handles are session-local.** Two clients cannot compare projectile
//!   handles with each other, and a handle in a log means nothing outside the
//!   session that issued it. Anything that has to correlate across sessions —
//!   the replay container, the resimulator, the digest of an input log —
//!   correlates on the *state*, which is global, and never on what a client was
//!   told. That is already how the recorded log works: it holds `Input`s, and
//!   an `Input` names no projectile.
//! - **A reconnecting session gets a new space.** `docs/ARCHITECTURE.md`
//!   resynchronises a reconnecting client exactly like a joining one, by
//!   sending it a `PlayerView` and nothing else, so it has already thrown away
//!   the world it had; new handles for the projectiles in it are consistent
//!   with that rather than an extra cost. Reconnection lands with the playable
//!   client at M4, and this is the constraint it inherits.
//! - **Two sessions agree only if they saw the same history.** Three clients on
//!   one team receive the same visible set every tick — vision is a team
//!   property — and the entity list is ordered by handle, so they allocate in
//!   the same order and end up with identical local handles. That is what lets
//!   M3's exit criterion compare their reconciled worlds directly. It stops
//!   being true the moment one of them joins late, and a client that joined at
//!   tick 50 is *supposed* to disagree about the handles of projectiles it
//!   never saw.
//!
//! # The bound, and what happens at it
//!
//! The map is pruned each tick to the projectiles still in the arena, so it
//! holds at most [`sim::MAX_PROJECTILES`] entries however long the match runs.
//! The counter is not pruned — that is the whole point — so it is finite: at
//! most `u16::MAX - 1000` local handles per recipient. Six players, one
//! skillshot each on an eight-second cooldown, is three quarters of a cast per
//! second, so exhausting a recipient's space takes about a day of continuous
//! play. Past it, a projectile that has no handle is omitted from that
//! recipient's view. That is a degradation rather than a leak — it is a
//! function of how many projectiles this recipient has been shown, which is
//! this recipient's own history — and it is defined here rather than left to be
//! a panic in a server that has run for a day.

use std::collections::BTreeMap;

use sim::view::{EntityView, PlayerView, VisibleEvent};
use sim::{EntityId, PROJECTILE_ID_BASE, Projectiles};

/// One recipient's private naming of the projectiles it has been shown.
#[derive(Clone, Debug, Default)]
pub struct HandleSpace {
    /// Global handle to local handle, for the projectiles still in flight.
    named: BTreeMap<u16, u16>,
    /// The next local handle. Monotone, never reset, never reused. Held wider
    /// than the handles it issues so that exhaustion is a comparison rather
    /// than an overflow.
    next: u32,
}

impl HandleSpace {
    /// A recipient that has been shown nothing.
    #[must_use]
    pub fn new() -> Self {
        Self {
            named: BTreeMap::new(),
            next: PROJECTILE_ID_BASE as u32,
        }
    }

    /// How many distinct projectiles this recipient has been shown.
    ///
    /// Exposed for the tests that assert the counter is monotone and that two
    /// recipients who saw the same history agree; nothing in the protocol reads
    /// it.
    #[must_use]
    pub const fn issued(&self) -> u32 {
        self.next.saturating_sub(PROJECTILE_ID_BASE as u32)
    }

    /// Rewrites a view into this recipient's handle space.
    ///
    /// `in_flight` is the arena the view was projected from, and it is used for
    /// one thing: forgetting the projectiles that have left it, so the map stays
    /// bounded. It is not consulted about visibility — the view has already
    /// been culled, and a function that could see the arena and the view at once
    /// is a function that could put the two together.
    #[must_use]
    pub fn localize(&mut self, view: &PlayerView, in_flight: &Projectiles) -> PlayerView {
        self.forget_departed(in_flight);

        // `view.visible` is ordered by global handle, so allocation order is a
        // function of the content rather than of the arena — which is the
        // property M2's sort established and which this must not undo. Two
        // recipients shown the same projectiles in the same ticks therefore
        // issue the same local handles.
        let mut visible = Vec::with_capacity(view.visible.len());
        for entity in &view.visible {
            let localized = match *entity {
                EntityView::Champion { id, position, hp } => {
                    Some(EntityView::Champion { id, position, hp })
                }
                EntityView::Tower { id, position, hp } => {
                    Some(EntityView::Tower { id, position, hp })
                }
                EntityView::Projectile {
                    id,
                    position,
                    velocity,
                } => self.local(id).map(|id| EntityView::Projectile {
                    id,
                    position,
                    velocity,
                }),
            };
            if let Some(entity) = localized {
                visible.push(entity);
            }
        }

        // Events name champions and towers, never projectiles: a cast names its
        // caster, damage and death name what they happened to, and nothing in
        // the game can target a projectile. The mapping is applied anyway, and
        // it is the identity on every handle below the projectile base — so a
        // future event that did name one would be renamed rather than leaking,
        // instead of this becoming the exception somebody has to remember.
        let mut events = Vec::with_capacity(view.events.len());
        for event in &view.events {
            let localized = match *event {
                VisibleEvent::Cast {
                    caster,
                    ability,
                    at,
                } => self.local(caster).map(|caster| VisibleEvent::Cast {
                    caster,
                    ability,
                    at,
                }),
                VisibleEvent::Damage { target, amount, at } => self
                    .local(target)
                    .map(|target| VisibleEvent::Damage { target, amount, at }),
                VisibleEvent::Death { entity, at } => self
                    .local(entity)
                    .map(|entity| VisibleEvent::Death { entity, at }),
            };
            if let Some(event) = localized {
                events.push(event);
            }
        }

        PlayerView {
            tick: view.tick,
            outcome: view.outcome,
            own: view.own,
            visible,
            events,
        }
    }

    /// This recipient's name for a handle: the handle itself if it is public,
    /// its private name if it is a projectile, or `None` if the space is spent.
    fn local(&mut self, global: EntityId) -> Option<EntityId> {
        if global.0 < PROJECTILE_ID_BASE {
            return Some(global);
        }
        if let Some(local) = self.named.get(&global.0) {
            return Some(EntityId(*local));
        }
        let local = u16::try_from(self.next).ok()?;
        self.next = self.next.saturating_add(1);
        self.named.insert(global.0, local);
        Some(EntityId(local))
    }

    /// Drops the names of projectiles that have left the arena.
    ///
    /// Safe against aliasing because the global counter is monotone within a
    /// match: a handle that has left is never issued again, so a forgotten
    /// entry cannot be revived by a different projectile arriving under the
    /// same global name.
    fn forget_departed(&mut self, in_flight: &Projectiles) {
        if self.named.is_empty() {
            return;
        }
        self.named
            .retain(|global, _| in_flight.iter().any(|live| live.id.0 == *global));
    }
}
