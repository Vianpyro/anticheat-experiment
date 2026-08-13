//! The per-recipient event queue, and why the frame's event budget is not a
//! loss of information.
//!
//! # What this exists for
//!
//! A frame is padded to one bucket and cut into a constant number of datagrams
//! that must each fit a path MTU, so the view it carries has a byte budget and
//! the events have to live inside it: `sim::view::MAX_EVENTS_PER_VIEW` is what
//! is left after the entity list. A tick can produce more visible events than
//! that — `sim::MAX_EVENTS` is the tick's own capacity and it is larger.
//!
//! **The overflow is deferred, not dropped.** The events a recipient is
//! entitled to are queued here in the order the rules produced them, and each
//! frame carries the oldest of them. A recipient in a fight that produces
//! twenty visible events on one tick is told about sixteen of them now and four
//! of them next tick, a thirtieth of a second later, in the right order.
//!
//! # Why deferral rather than a smaller bucket
//!
//! The alternative to a budget is a bucket sized for the whole tick's capacity,
//! which is `docs/ARCHITECTURE.md`'s old 1501-byte frame: it does not fit a
//! datagram, so the frames go on a reliable stream, so a lost packet blocks
//! every frame behind it. The alternative to *deferral* is dropping the
//! overflow, which is cheaper here and loses a cue the player was entitled to.
//! Deferring costs one bounded queue per session and loses nothing.
//!
//! # Why this is not a side channel
//!
//! The queue is a function of the events this recipient was **entitled** to
//! see, and of nothing else. It is fed from an already-culled `PlayerView`; it
//! never sees the state, so there is nothing hidden for it to be a function of.
//! Two states a player cannot tell apart produce the same entitled events,
//! hence the same queue, hence the same bytes — which is the same argument
//! `HandleSpace` makes about its counter, and it is checked by the same
//! property in `server/tests/traffic.rs`.
//!
//! What an observer could in principle read off a deferral is *timing*: an
//! event delivered one tick late says that the previous tick was busy. It says
//! that to the recipient who was already told about sixteen events on that
//! tick, so it is not information they did not have. It says nothing to a
//! third party, because the frame is padded and the cadence is constant, which
//! is the whole point of the bucket.
//!
//! # The bound, and what happens at it
//!
//! The queue holds at most `sim::MAX_EVENTS` events — one tick's worth of
//! capacity. Past that the oldest are dropped, exactly as `sim`'s own event
//! buffer drops past its capacity, and for the same reason: dropping is total,
//! it is identical on every platform, and a queue that grows is an allocator an
//! attacker can drive. Reaching it needs a sustained rate above sixteen visible
//! events per tick for several consecutive ticks, which no reachable state
//! under the game's constants produces.

use std::collections::VecDeque;

use sim::MAX_EVENTS;
use sim::view::{MAX_EVENTS_PER_VIEW, PlayerView, VisibleEvent};

/// One recipient's queue of events not yet delivered.
#[derive(Clone, Debug, Default)]
pub struct EventBacklog {
    pending: VecDeque<VisibleEvent>,
    /// Events dropped because the queue was full. Never read by the protocol;
    /// it is the number that tells an operator a session is in a fight the
    /// frame budget cannot keep up with.
    dropped: u32,
}

impl EventBacklog {
    /// A recipient that is owed nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pending: VecDeque::new(),
            dropped: 0,
        }
    }

    /// How many events are waiting for a later frame.
    #[must_use]
    pub fn deferred(&self) -> usize {
        self.pending.len()
    }

    /// How many events this recipient will never be told about.
    #[must_use]
    pub const fn dropped(&self) -> u32 {
        self.dropped
    }

    /// Returns the view with at most [`MAX_EVENTS_PER_VIEW`] events, keeping
    /// the rest for the next frame.
    ///
    /// Oldest first, always: the queue is drained before this tick's events are
    /// considered, so a client is never shown a later event before an earlier
    /// one. Everything else in the view passes through untouched — this is a
    /// budget on one list, not a second projection.
    #[must_use]
    pub fn shape(&mut self, view: &PlayerView) -> PlayerView {
        for event in &view.events {
            if self.pending.len() >= MAX_EVENTS {
                self.pending.pop_front();
                self.dropped = self.dropped.saturating_add(1);
            }
            self.pending.push_back(*event);
        }

        let mut events = Vec::with_capacity(MAX_EVENTS_PER_VIEW);
        while events.len() < MAX_EVENTS_PER_VIEW {
            let Some(event) = self.pending.pop_front() else {
                break;
            };
            events.push(event);
        }

        PlayerView {
            tick: view.tick,
            outcome: view.outcome,
            own: view.own,
            visible: view.visible.clone(),
            events,
        }
    }
}
