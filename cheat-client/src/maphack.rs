//! Exploit class 1: reading information the fog was supposed to withhold.
//!
//! The attacker here is the simplest one there is and that is deliberate: it
//! decodes the frames the server sends it and writes down everything in them.
//! No memory reading, no injection, no second connection. If a server sends a
//! client more than that client is entitled to, this is all it takes.
//!
//! # What "the attacker learns X" means, precisely
//!
//! Not "the attacker holds a stale belief". An attacker that remembers where it
//! last saw somebody has learned nothing it was not told, and a maphack that
//! reported yesterday's positions would be a maphack nobody would run. So
//! [`Maphack::locates`] reports only what **this tick's** frame put in its hands:
//! the enemy champions it can place *now*, at the position they are *now* at.
//!
//! That makes the accounting exact and it makes the exploit's failure mode
//! legible. Against a projection that culls, the set of enemies the attacker can
//! place is the set the server chose to name, and the surplus is zero — not
//! because the attacker is weak but because the information is not in the bytes.
//! Against a projection that does not cull, the same attacker, unchanged, places
//! every living enemy on the map. `tests/maphack.rs` runs both, over one world,
//! and is red if either half comes out wrong.
//!
//! # Champion handles are public, and that is what makes this attacker cheap
//!
//! A champion's handle *is* its seat (`docs/ARCHITECTURE.md`), so an attacker
//! reading a handle knows which team it belongs to without being told. That is a
//! deliberate disclosure — a view already shows the champion, and hiding which
//! side it is on would be hiding it from the player who can see it — and it is
//! why [`Maphack`] needs no learning phase and no correlation step.
//!
//! # And one inference the culling does not reach
//!
//! [`Maphack::candidate_origins`] is an exploit that **works against the shipping
//! build**, and it is here because `docs/MILESTONES.md` M7 is worth less if it
//! only carries the attacks that fail. A projectile is shown with its position
//! and its velocity, and the velocity is constant for its life, so an attacker
//! who sees one can run it *backwards*: the point it was cast from is on the ray
//! behind it, and that point may be one the fog was hiding. The view names no
//! caster and no owner — `docs/ARCHITECTURE.md` removed both, for exactly this
//! family of reasons — and the ray survives anyway, because it is derived from
//! two numbers the recipient is entitled to.
//!
//! What that is worth is stated where the test asserts it: a **line**, not a
//! position, and a line that is only interesting while the projectile is young.
//! What would close it is removing projectiles from views, which is not a game,
//! or capping the entity list, which `docs/ARCHITECTURE.md` refuses because it
//! trades a length channel for a content channel. It is recorded as a limit
//! rather than defended, in the register `docs/SCOPE.md` uses for the ceiling of
//! behavioural detection.

use std::collections::BTreeMap;

use protocol::{
    DecodeError, EntityId, EntityView, Fx, FxVec2, PLAYER_COUNT, PlayerView, Seat, ServerFrame,
    ServerMessage, Team, Tick,
};

/// One sighting of a projectile: where it was, how fast, and when.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sighting {
    tick: Tick,
    position: FxVec2,
    velocity: FxVec2,
}

/// An attacker that writes down everything it is sent.
#[derive(Clone, Debug)]
pub struct Maphack {
    own: Seat,
    /// The tick of the newest view folded in.
    tick: Tick,
    /// Champion handles this tick's view named, and where it put them.
    placed: BTreeMap<u16, FxVec2>,
    /// The first sighting of each projectile handle, which is the one the ray
    /// below is run from: the earliest sighting is the one closest to the cast.
    first_seen: BTreeMap<u16, Sighting>,
    /// Views folded in, and frames refused. `docs/RISKS.md` R15: an attacker
    /// that learned nothing because it decoded nothing is not a defence.
    views: u32,
    refused: u32,
}

impl Maphack {
    /// An attacker seated in `own`, having been told nothing.
    #[must_use]
    pub fn new(own: Seat) -> Self {
        Self {
            own,
            tick: Tick(0),
            placed: BTreeMap::new(),
            first_seen: BTreeMap::new(),
            views: 0,
            refused: 0,
        }
    }

    /// Folds in one server frame.
    ///
    /// # Errors
    ///
    /// [`DecodeError`] for bytes that are not one frame of this protocol. The
    /// attacker counts them rather than ignoring them, because an exploit that
    /// silently failed to parse would report the same "learned nothing" a
    /// working defence does.
    pub fn observe(&mut self, bytes: &[u8]) -> Result<(), DecodeError> {
        match ServerFrame::decode(bytes) {
            Ok(ServerMessage::View { view, .. }) => {
                self.fold(&view);
                Ok(())
            }
            Ok(_) => Ok(()),
            Err(error) => {
                self.refused = self.refused.saturating_add(1);
                Err(error)
            }
        }
    }

    /// Folds in a view the attacker already holds.
    ///
    /// The same entry point as [`Maphack::observe`] without the framing, for the
    /// exploit that is about what a *projection* discloses rather than about what
    /// a transport does.
    pub fn fold(&mut self, view: &PlayerView) {
        self.views = self.views.saturating_add(1);
        self.tick = view.tick;
        self.placed.clear();
        for entity in &view.visible {
            match *entity {
                EntityView::Champion { id, position, .. } => {
                    self.placed.insert(id.0, position);
                }
                EntityView::Projectile {
                    id,
                    position,
                    velocity,
                } => {
                    self.first_seen.entry(id.0).or_insert(Sighting {
                        tick: view.tick,
                        position,
                        velocity,
                    });
                }
                EntityView::Tower { .. } => {}
            }
        }
    }

    /// The enemy champions this attacker can place *right now*, and where.
    ///
    /// Enemy is decided from the handle alone, because a champion's handle is
    /// its seat and the seat's team follows from it.
    #[must_use]
    pub fn locates(&self) -> Vec<(EntityId, FxVec2)> {
        self.placed
            .iter()
            .filter(|(handle, _)| {
                Self::team_of(**handle).is_some_and(|team| team != self.own.team())
            })
            .map(|(handle, position)| (EntityId(*handle), *position))
            .collect()
    }

    /// The points a projectile could have been cast from, newest first.
    ///
    /// The projectile's velocity is its per-tick displacement and it is constant
    /// for the projectile's life, so the cast point is `position - velocity * k`
    /// for a `k` the attacker does not know. `k` is bounded by the flight time,
    /// which is a published constant, so the attacker enumerates instead of
    /// guessing — and the true origin is one of the points returned whenever the
    /// projectile was cast within `max_ticks` of the sighting.
    ///
    /// This is an exploit that lands. See the module header for what it is worth
    /// and why nothing here stops it.
    #[must_use]
    pub fn candidate_origins(&self, projectile: EntityId, max_ticks: u16) -> Vec<FxVec2> {
        let Some(seen) = self.first_seen.get(&projectile.0) else {
            return Vec::new();
        };
        (0..=i32::from(max_ticks))
            .map(|back| seen.position.sub(seen.velocity.scale(Fx::from_int(back))))
            .collect()
    }

    /// Projectile handles this attacker has ever been shown.
    #[must_use]
    pub fn projectiles(&self) -> Vec<EntityId> {
        self.first_seen
            .keys()
            .map(|handle| EntityId(*handle))
            .collect()
    }

    /// The tick on which a projectile was first seen.
    #[must_use]
    pub fn first_seen_at(&self, projectile: EntityId) -> Option<Tick> {
        self.first_seen.get(&projectile.0).map(|seen| seen.tick)
    }

    /// The newest tick folded in.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// Views folded in, and frames that did not decode.
    #[must_use]
    pub const fn counts(&self) -> (u32, u32) {
        (self.views, self.refused)
    }

    /// The team a champion handle belongs to, or `None` if it is not a champion.
    fn team_of(handle: u16) -> Option<Team> {
        if usize::from(handle) >= PLAYER_COUNT {
            return None;
        }
        Seat::from_index(u8::try_from(handle).ok()?).map(Seat::team)
    }
}
