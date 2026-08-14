//! Exploit classes 3 and 4: a client that plays the protocol without a person,
//! and one that lies about its own clock.
//!
//! # Class 3, and the ceiling it runs into
//!
//! `docs/SCOPE.md` and `docs/MILESTONES.md` M6 both say this in advance and this
//! module is where it becomes executable: **there is a bot, and no delivered
//! defence catches it, and that is correct.** A scripted client that drives the
//! protocol is real — [`Bot`] is one, it composes intentions the way a player's
//! client does and the server cannot tell them from a person's — and the only
//! mechanical thing a *file* can say about it is whether any device event was
//! recorded. `replay`'s corpus refuses a seat with zero device events, so a
//! headless bot is refused there; a bot that moved a real mouse would record as
//! many samples as a person and is not reachable from any file.
//!
//! So `tests/botting.rs` writes the exploit and asserts that **it is not
//! caught** — the server accepts every frame, the resulting replay verifies, and
//! the match is indistinguishable in the artefact from a human one. An exploit
//! that fails against a defence that does not exist would be `docs/RISKS.md` R15
//! inverted: a red that proves nothing. This one is green on purpose, and green
//! *documents a limit* rather than a defence. The behavioural detectors that
//! narrow the gap are M8's, they read telemetry rather than reject frames, and
//! they will carry their own error bounds; nothing here claims one.
//!
//! # Class 4: the clock the client controls, and the one it does not
//!
//! `docs/SCOPE.md`: the client's own timestamp is attacker-controlled by
//! definition; only the server's arrival time is evidence. [`Bot::intend_at`]
//! lets an attacker write any `claimed_at_ms` it likes, and the protocol carries
//! it — the exploit is that a client can claim to have acted at a time it did
//! not, a slowed clock, a frozen one, a future one.
//!
//! The defence at M7 is not a detector; it is that **no rule reads the field**.
//! `Match::deliver` records `claimed_at_ms` beside the server's own
//! `received_at_ms` and stamps the input with the server's tick, so a lie in it
//! changes the telemetry and changes nothing about the match. `tests/clock.rs`
//! establishes both halves: the claimed clock can be made arbitrarily false, and
//! the world the log resimulates to does not move when it is. The *divergence*
//! between the two clocks is the class-4 signal M8 will read; M7's job is to show
//! it is recorded and inert, and that a client cannot make the server act on its
//! own clock.

use protocol::{Action, ClientFrame, ClientMessage, FxVec2};

/// A client with no person behind it.
///
/// It holds exactly what a real client's session holds and nothing the victim's
/// internals would give it: a sequence number it increments, and a standing
/// intention it repeats. `docs/ARCHITECTURE.md`'s "one intention per tick" is
/// the shape it produces, because a bot that produced a different shape would be
/// detectable by the traffic invariant rather than by anything behavioural, and
/// this is the class-3 attacker, not the class-1 one.
#[derive(Clone, Debug)]
pub struct Bot {
    seq: u32,
    standing: Action,
}

impl Default for Bot {
    fn default() -> Self {
        Self::new()
    }
}

impl Bot {
    /// A bot that has said nothing and is holding position.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            seq: 0,
            standing: Action::Idle,
        }
    }

    /// Asks for a seat. The server picks it; a client that named its own seat is
    /// class 5, and `abuse` is where that lives.
    #[must_use]
    pub fn join(&self) -> ClientFrame {
        ClientFrame::encode(&ClientMessage::Join)
    }

    /// Declares readiness.
    #[must_use]
    pub fn ready(&self) -> ClientFrame {
        ClientFrame::encode(&ClientMessage::Ready)
    }

    /// The next intention, with an honest-looking claimed timestamp.
    ///
    /// `claimed_at_ms` is set to the argument, which a caller driving the bot at
    /// the tick rate passes the tick's own time — an ordinary, truthful-looking
    /// value. [`Bot::intend_at`] is the same call with the lie made explicit.
    pub fn intend(&mut self, action: Action, claimed_at_ms: u64) -> ClientFrame {
        self.standing = follow(self.standing, action);
        let frame = ClientFrame::encode(&ClientMessage::Input {
            seq: self.seq,
            claimed_at_ms,
            action,
        });
        self.seq = self.seq.saturating_add(1);
        frame
    }

    /// An intention whose claimed timestamp is whatever the attacker wants.
    ///
    /// This is the class-4 primitive. The value bears no required relationship to
    /// the server's clock, to the tick, or to the previous claim; it can go
    /// backwards, stand still, or jump to the far future. The server records it
    /// and no rule reads it.
    pub fn intend_at(&mut self, action: Action, claimed_at_ms: u64) -> ClientFrame {
        self.intend(action, claimed_at_ms)
    }

    /// A frame carrying an arbitrary sequence number, for the class-5 attacker
    /// in `abuse` that wants to replay or reorder its own inputs.
    ///
    /// It does not touch the bot's own counter, because the attacker using it is
    /// deliberately not playing by the increment rule.
    #[must_use]
    pub fn intend_raw(&self, seq: u32, action: Action, claimed_at_ms: u64) -> ClientFrame {
        ClientFrame::encode(&ClientMessage::Input {
            seq,
            claimed_at_ms,
            action,
        })
    }

    /// The sequence number the next [`Bot::intend`] will use.
    #[must_use]
    pub const fn next_seq(&self) -> u32 {
        self.seq
    }

    /// A walk toward a point, which is what a bot with a goal repeats.
    #[must_use]
    pub const fn walk_to(point: FxVec2) -> Action {
        Action::Move(point)
    }
}

/// The standing order after an action, mirroring the rule a client tracks: a
/// move replaces it, an idle clears it, a cast leaves it alone.
const fn follow(standing: Action, action: Action) -> Action {
    match action {
        Action::Idle => Action::Idle,
        Action::Move(_) => action,
        Action::Skillshot(_) | Action::Targeted(_) | Action::Attack(_) => standing,
    }
}
