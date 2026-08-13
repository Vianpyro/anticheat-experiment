//! Client-side prediction of the one champion a client is entitled to predict,
//! and reconciliation against the server's acknowledgement.
//!
//! # What this predicts, and what it deliberately does not
//!
//! **Its own champion's position, and nothing else.** Not enemies, not allies,
//! not projectiles, not hit points. The reason is not economy of effort: a
//! client that extrapolated other entities would be inventing world state, and
//! the moment it renders an invented enemy it has built the first half of a
//! cheat for free — the second half is showing the invention where the fog would
//! have hidden the real thing. What a player needs prediction *for* is the
//! latency between clicking and seeing their own champion start walking, and
//! that is exactly the one thing the client already knows the input to.
//!
//! # The rule this re-applies is `sim`'s, not a second copy of it
//!
//! [`FxVec2::step_toward`] and [`FxVec2::clamp_components`] are the functions
//! `sim::step`'s `move_champions` calls, under the same [`RULES`] constants.
//! What lives here is the *loop* around them — fold the standing order forward
//! through the unacknowledged intentions, one tick of movement each — which is
//! three lines and cannot be shared, because `step` moves nine champions from a
//! `State` and a client has neither. A divergence between the two therefore
//! shows up as the prediction disagreeing with the authoritative position, which
//! is what `client/tests/prediction.rs` measures rather than assumes.
//!
//! # One intention per tick, and why that is a precondition rather than a hope
//!
//! The fold applies one tick of movement per outstanding intention. That is
//! exact when the client sends at most one intention per tick, which is the
//! protocol's shape — `docs/ARCHITECTURE.md`'s "one message per player per tick"
//! — and what the playable client does. A client that sent four intentions in
//! one tick would have them applied in one tick by the server and predicted as
//! four ticks of walking here, so it would over-predict and be corrected. That
//! is a degradation of prediction quality, not of correctness: the server is
//! still the authority, and the next view replaces whatever the client believed.
//!
//! # What happens when the client cannot predict
//!
//! An `Attack` order walks toward a target whose position the client may not be
//! entitled to see. Rather than guess it, the prediction stops advancing and
//! reports the last authoritative position: standing still until the server says
//! otherwise is visibly a lag, where walking toward a guessed point is visibly
//! wrong.
//!
//! # What it hides, and the tick it does not
//!
//! The predicted position is **the position after the server has applied every
//! intention this client has sent and not had acknowledged**. That definition is
//! deliberate and it is narrower than the one a shipping game would use.
//!
//! What it hides is the round trip: click, and the champion starts walking in
//! the frame it was clicked in rather than one round trip later. That is the
//! latency a player feels.
//!
//! What it does not hide is one tick. With nothing outstanding, the prediction
//! *is* the last view, and the server has since run the tick that view was
//! about — so a client that stopped sending would render a world 33 ms old, plus
//! however long the frame took to arrive. Removing that would mean extrapolating
//! the standing order forward by an estimate of how many ticks are in flight,
//! which needs a local clock, an RTT estimate, and rubber-banding when the
//! estimate is wrong. It is left out at M4, and the reason is not that it is
//! hard: prediction that is exact is a claim a test can check to the last bit,
//! and prediction that extrapolates is a claim about a tolerance.
//!
//! The playable client sends one intention every tick — `Idle` when the player
//! is doing nothing — which is the protocol's own shape and which keeps exactly
//! one intention outstanding at all times. Under that traffic the prediction is
//! exact on every tick, which is what `client/tests/prediction.rs` asserts with
//! no tolerance at all.

use std::collections::VecDeque;

use sim::view::{OwnView, PlayerView};
use sim::{Action, FxVec2, Liveness, RULES, Tick};

/// The standing order the client believes the server is holding for it.
///
/// A client's shadow of `sim::Order`, and it is a separate type because it has a
/// case `sim` does not: an order whose destination this client is not entitled
/// to know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Predicted {
    /// Standing still.
    Idle,
    /// Walking to a point.
    MoveTo(FxVec2),
    /// An order the client cannot follow forward — an attack order, whose
    /// destination is an entity's position and therefore something the fog may
    /// be hiding.
    Unknown,
}

impl Predicted {
    /// Where this order is walking to, if the client knows.
    const fn destination(self) -> Option<FxVec2> {
        match self {
            Self::MoveTo(point) => Some(point),
            Self::Idle | Self::Unknown => None,
        }
    }

    /// The order in force after this action.
    ///
    /// Mirrors `apply_inputs` in `sim::step` for the orders a client can
    /// follow: a move replaces the standing order, an idle clears it, and
    /// casting an ability leaves it alone — a skillshot does not stop a
    /// champion walking.
    fn after(self, action: Action) -> Self {
        match action {
            Action::Idle => Self::Idle,
            Action::Move(destination) => {
                Self::MoveTo(destination.clamp_components(RULES.map_half_extent))
            }
            Action::Skillshot(_) | Action::Targeted(_) => self,
            Action::Attack(_) => Self::Unknown,
        }
    }
}

/// One intention the client has sent and the server has not acknowledged.
#[derive(Clone, Copy, Debug)]
struct Outstanding {
    seq: u32,
    action: Action,
}

/// The client's belief about where its own champion is, ahead of the server.
#[derive(Clone, Debug)]
pub struct Prediction {
    /// The last authoritative statement about this champion, and the tick it
    /// described.
    anchor: OwnView,
    anchor_tick: Tick,
    /// The standing order at the anchor: every intention the server has
    /// acknowledged, folded forward.
    anchor_order: Predicted,
    /// Sent and not yet acknowledged, oldest first.
    outstanding: VecDeque<Outstanding>,
    /// How far the last authoritative position was from where this client had
    /// predicted it would be, in raw fixed-point units.
    ///
    /// Not a diagnostic afterthought: prediction that is quietly wrong looks
    /// exactly like prediction that is right, and the number is what
    /// `client/tests/prediction.rs` asserts on and what the playable client
    /// shows in its corner.
    last_correction: i32,
    worst_correction: i32,
}

impl Prediction {
    /// A prediction anchored on the first view a client received.
    ///
    /// The only constructor, and it takes a view rather than nothing because
    /// there is no honest state for a prediction that has not been told where
    /// its champion is. A client builds this when the first `View` frame
    /// arrives; before that it has nothing to render and no input worth
    /// predicting.
    #[must_use]
    pub fn anchored(view: &PlayerView) -> Self {
        Self {
            anchor: view.own,
            anchor_tick: view.tick,
            anchor_order: Predicted::Idle,
            outstanding: VecDeque::new(),
            last_correction: 0,
            worst_correction: 0,
        }
    }

    /// Records an intention this client has just sent.
    pub fn sent(&mut self, seq: u32, action: Action) {
        self.outstanding.push_back(Outstanding { seq, action });
    }

    /// Folds in a view and the acknowledgement that came with it.
    ///
    /// The two arrive together and are consumed together, because using one
    /// without the other is the bug this whole module exists to avoid: dropping
    /// outstanding intentions without re-anchoring loses them, and re-anchoring
    /// without dropping them re-applies inputs the world has already seen.
    pub fn observe(&mut self, view: &PlayerView, applied_through: Option<u32>) {
        // The correction is measured before the anchor moves, and against what
        // this client predicted for *this* tick — which is the position it
        // reported when it had exactly the intentions the server has now
        // acknowledged. Measuring it after would be measuring nothing.
        let acknowledged = self.acknowledged_by(applied_through);
        let predicted = self.position_after(acknowledged);
        let correction = predicted.sub(view.own.position).length().to_raw();
        self.last_correction = correction;
        self.worst_correction = self.worst_correction.max(correction);

        for _ in 0..acknowledged {
            let Some(intention) = self.outstanding.pop_front() else {
                break;
            };
            self.anchor_order = self.anchor_order.after(intention.action);
        }

        self.anchor = view.own;
        self.anchor_tick = view.tick;
    }

    /// How many of the outstanding intentions this acknowledgement covers.
    fn acknowledged_by(&self, applied_through: Option<u32>) -> usize {
        let Some(through) = applied_through else {
            return 0;
        };
        self.outstanding
            .iter()
            .take_while(|intention| intention.seq <= through)
            .count()
    }

    /// Where this client believes its champion will be once the server has
    /// applied everything it has sent.
    ///
    /// This is the position to render. It is the authoritative position when
    /// nothing is outstanding, which is the state a client on a fast enough
    /// connection spends most of its time in.
    #[must_use]
    pub fn position(&self) -> FxVec2 {
        self.position_after(self.outstanding.len())
    }

    /// The authoritative position, with no prediction applied. What the server
    /// last said, for a client that wants to draw the correction.
    #[must_use]
    pub const fn authoritative(&self) -> FxVec2 {
        self.anchor.position
    }

    /// The last authoritative statement about this champion.
    #[must_use]
    pub const fn own(&self) -> OwnView {
        self.anchor
    }

    /// The tick the anchor describes.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.anchor_tick
    }

    /// Intentions sent and not yet acknowledged.
    #[must_use]
    pub fn outstanding(&self) -> usize {
        self.outstanding.len()
    }

    /// How far the last view moved this client's champion from where it had been
    /// predicted, in raw fixed-point units, and the worst such correction of the
    /// session.
    #[must_use]
    pub const fn corrections(&self) -> (i32, i32) {
        (self.last_correction, self.worst_correction)
    }

    /// The predicted position after the first `count` outstanding intentions.
    ///
    /// A dead champion is not on the map and its position is its spawn point, so
    /// there is nothing to predict; the authoritative answer is returned rather
    /// than a walk from a body that is not there.
    fn position_after(&self, count: usize) -> FxVec2 {
        if !matches!(self.anchor.liveness, Liveness::Alive { .. }) {
            return self.anchor.position;
        }

        let mut order = self.anchor_order;
        let mut position = self.anchor.position;
        for intention in self.outstanding.iter().take(count) {
            order = order.after(intention.action);
            let Some(destination) = order.destination() else {
                continue;
            };
            position = position
                .step_toward(destination, RULES.champion_speed)
                .clamp_components(RULES.map_half_extent);
            // `move_champions` clears the order on arrival, and so does this: an
            // order kept past its destination is an order that keeps calling
            // `step_toward` with a zero delta, which is harmless here and would
            // diverge the moment the rule stopped being idempotent.
            if position == destination {
                order = Predicted::Idle;
            }
        }
        position
    }
}
