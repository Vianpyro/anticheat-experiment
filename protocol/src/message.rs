//! What the two ends can say to each other.
//!
//! Two enums rather than one, because the two directions are not symmetric and
//! never will be: `docs/ARCHITECTURE.md` puts "two message directions mean two
//! enums, not a codec framework" among the deliberate non-abstractions.

use sim::view::PlayerView;
use sim::{Action, Digest, Seat};

/// Everything a client may say.
///
/// **The client sends intentions and never state.** There is no field here for
/// a position, a hit-point total, a tick the client believes it is on, or a
/// seat: every one of those is something the server knows better, and a field a
/// client can write is a field an attacker writes. What is left is a request to
/// join, a declaration of readiness, one intention per tick, and a surrender.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientMessage {
    /// Asks for a seat. The server picks which one; a client that could name
    /// its seat could name somebody else's.
    Join,
    /// Says this client is ready for the match to start. Idempotent, so a
    /// duplicate is not a protocol violation — `docs/SCOPE.md` class 5 asks for
    /// idempotent session commands, and this is one of the two that has to be.
    Ready,
    /// One tick's intention.
    Input {
        /// Per-player, monotonically increasing. The protocol's identity for
        /// this input: the server accepts a sequence number strictly greater
        /// than the last it accepted from this seat, which is what makes a
        /// replayed or reordered input a no-op rather than a second command.
        seq: u32,
        /// When the client says it produced this input.
        ///
        /// Attacker-controlled by definition and never trusted. It is recorded
        /// beside the server's own arrival timestamp, and the divergence
        /// between the two is the signal for exploit class 4
        /// (`docs/SCOPE.md`). No rule reads it.
        claimed_at_ms: u64,
        /// What the player is asking for.
        action: Action,
    },
    /// Concedes the match.
    Surrender,
}

/// Everything the server may say.
///
/// # There is no `Outcome` message, and that is the traffic invariant
///
/// `docs/ARCHITECTURE.md` sketches a third variant, `Outcome(MatchRecord)`,
/// sent when the match ends. It is deliberately absent, and the reason is the
/// cadence half of the traffic-shape invariant rather than an omission: a
/// message whose *existence* depends on what happened is a message an observer
/// can count, and "the server sent a seventh frame this tick" is content
/// leaking through the packet stream no matter how well each frame is padded.
/// The outcome is already a field of every [`PlayerView`], so a client learns
/// the match ended from the frame it was going to receive anyway. The signed
/// match record is a different object with a different purpose — it is evidence,
/// not a notification — and it belongs to M5 with the rest of the replay
/// container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerMessage {
    /// The session has a seat, and the match it is a seat in.
    Accepted {
        /// Which seat. The client is told; it does not ask.
        seat: Seat,
        /// The match seed, so the client can name the match it is in.
        seed: u64,
        /// The constants the match is played under (`docs/RISKS.md` R2). A
        /// client that disagrees is a client running another build, and it can
        /// say so instead of playing a different game quietly.
        rules_hash: Digest,
    },
    /// The session has no seat, and why.
    Rejected(RejectReason),
    /// One tick, already culled, with this recipient's own handles.
    View {
        /// The world this recipient may know about, at this tick.
        view: PlayerView,
        /// The highest sequence number from **this** recipient that the server
        /// had folded into the world by the tick the view describes, or `None`
        /// if it has folded in none.
        ///
        /// # Why this field exists, and why it is here rather than in the view
        ///
        /// `docs/MILESTONES.md` M3 closes on a thing it could not build:
        /// client-side prediction needs the client to know *which of its inputs
        /// the server applied to which tick*, and nothing told it. The server
        /// buckets an intention into whichever tick it is about to run, and
        /// `PlayerView` deliberately carries no standing order — an order names
        /// an `EntityId` the player may no longer be able to see. M4 needed one
        /// more field, and this is it.
        ///
        /// It is **outside** [`PlayerView`] on purpose. A view is what
        /// `sim::view::view_for` computes from a `State`, and that function has
        /// no session to ask: an acknowledgement is a fact about a connection,
        /// not about the world. Putting it in the view would mean either a
        /// projection that takes a session argument or a view type with a field
        /// its own constructor cannot fill — and the second is how a type stops
        /// meaning what its name says.
        ///
        /// # Why it is not a leak
        ///
        /// It is a number this recipient wrote and sent. It says nothing about
        /// the world, nothing about any other seat, and nothing the sender did
        /// not already know. Its width is constant — a tag byte and four bytes
        /// present in both cases — so the frame's size does not follow it
        /// either.
        ///
        /// The traffic property in `server/tests/traffic.rs` is stated with this
        /// in mind: the frame is a function of what the recipient is entitled to
        /// know about the world **and of what the recipient itself said**, both
        /// of which are things the recipient already has.
        ///
        /// # What "folded into the world" means, exactly
        ///
        /// That the server drained the input into the slice it handed `step`,
        /// not that a rule acted on it. An input from a dead champion is applied
        /// and does nothing; an input the sequence rule or the rate limit
        /// refused is never applied and never acknowledged. The client needs the
        /// first meaning: it is asking which of its outstanding intentions it
        /// may stop re-applying.
        ///
        /// The distinction between *applied* and *accepted* is not observable
        /// today and is written down anyway, because the field's correctness
        /// depends on it the day it becomes observable: the server's loop drains
        /// its whole queue and emits its frames in one call, so every accepted
        /// input is applied by the very next tick and no frame is sent in
        /// between. `server/tests/traffic.rs` says which half of this it can and
        /// cannot check.
        applied_through: Option<u32>,
    },
}

/// Why a session was refused.
///
/// Distinguished cases rather than one opaque refusal, for the reason M5's
/// `VerifyError` distinguishes its own: a client that cannot tell "you are
/// running the wrong build" from "the match is full" retries the one it should
/// not. None of them tells an attacker anything it did not already know by
/// having sent the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectReason {
    /// The frame's protocol version is not this build's.
    UnsupportedVersion,
    /// Every seat is taken.
    MatchFull,
    /// The match has already started.
    AlreadyStarted,
    /// The session broke the protocol: a frame that did not decode, a message
    /// out of order for the session's state, or a rate the session is not
    /// allowed to send at.
    ProtocolViolation,
}

impl RejectReason {
    pub(crate) const fn tag(self) -> u8 {
        match self {
            Self::UnsupportedVersion => 0,
            Self::MatchFull => 1,
            Self::AlreadyStarted => 2,
            Self::ProtocolViolation => 3,
        }
    }

    pub(crate) const fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(Self::UnsupportedVersion),
            1 => Some(Self::MatchFull),
            2 => Some(Self::AlreadyStarted),
            3 => Some(Self::ProtocolViolation),
            _ => None,
        }
    }
}
