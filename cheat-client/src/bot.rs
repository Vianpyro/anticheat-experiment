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
//!
//! # M8's variants, and the two that are not exploits
//!
//! `docs/MILESTONES.md` M8 asks for the bot first and then *variants that add
//! human-plausible noise*. [`Reflexes`] and [`ClaimedClock`] are those variants
//! and [`Reactor`] is what plays them, and only two of the five values below are
//! attacks:
//!
//! | Variant | What it is |
//! | --- | --- |
//! | [`Reflexes::Immediate`] | the exploit for `anticheat`'s reaction floor |
//! | [`Reflexes::Scripted`] | the exploit for the dispersion detector, and the **control** for the floor |
//! | [`Reflexes::Jittered`] | the **ceiling**: plausible and varied, and nothing catches it |
//! | [`ClaimedClock::Scaled`] | the exploit for the clock-divergence detector |
//! | [`ClaimedClock::Honest`] | its **control** |
//!
//! The control arms are not decoration. A detector that responds to an exploit
//! and also to its absence has not detected anything, and a suite that only ran
//! the attacks would be `docs/RISKS.md` R15 with the assertion pointing the
//! other way: it would look exactly like a detector that works.
//!
//! And the ceiling arm is the honest one. `docs/SCOPE.md` puts hardware input
//! injection with statistically human timing outside the adversary model
//! outright; [`Reflexes::Jittered`] is the reachable lower bound on it, it
//! defeats both reaction detectors, and `anticheat/tests/detectors.rs` asserts
//! that it does.

use std::collections::BTreeSet;

use protocol::{
    Action, ClientFrame, ClientMessage, EntityId, EntityView, FxVec2, PlayerView, Seat,
};

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

// ---------------------------------------------------------------------------
// M8: the variants, and the line they are deliberately on the wrong side of
// ---------------------------------------------------------------------------

/// How long a bot waits between being shown something and answering it.
///
/// `docs/MILESTONES.md` M8: *write the bot first, then variants that add
/// human-plausible noise.* These are those variants, and the three of them are
/// one exploit each for one detector each — plus the one nothing catches, which
/// is the point of having three rather than one.
///
/// # The noise is on a decision and never on a device
///
/// `docs/RISKS.md` R7 settles this before the code rather than after it. What
/// would turn a bot into a *tool* is not that it plays well; it is a layer that
/// synthesises **device** input, because such a layer drives the operating
/// system rather than a protocol and is game-independent by construction. That
/// layer is also, exactly, `docs/SCOPE.md`'s stated ceiling of behavioural
/// detection — a bot moving a real mouse records as many samples as a person
/// and no file can tell them apart.
///
/// So the honest way to test a detector against the ceiling would be to build
/// the thing this repository refuses to publish, and it is not built. [`Jittered`]
/// is as close as this project goes: plausible variability applied to *when a
/// decision is taken*, composed into an intention and sent over the wire. It is
/// a lower bound on the ceiling and it is named as one.
///
/// [`Jittered`]: Reflexes::Jittered
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reflexes {
    /// Answers on the very view that showed it. Latency zero, every time.
    ///
    /// The exploit for `anticheat`'s reaction floor. Nothing behind a hand is
    /// this fast: the earliest tick an answer to the view carrying tick `v` can
    /// be stamped with is `v` itself, and this takes it.
    Immediate,
    /// Waits a fixed number of ticks, chosen to look like a person.
    ///
    /// The exploit for the dispersion detector and the **control** for the
    /// floor: seven ticks is 233 ms, which is an unremarkable human reaction,
    /// so a floor detector has nothing to say about it. What it has none of is
    /// variability — every answer takes exactly as long as every other, which
    /// no hand does.
    Scripted(u32),
    /// Waits a drawn number of ticks around a plausible centre.
    ///
    /// **The ceiling, executed.** It defeats the floor because its shortest
    /// draw is still a human interval, and it defeats the dispersion because it
    /// varies. `anticheat/tests/detectors.rs` asserts that neither detector
    /// separates it from the control, and that green is the honest half of this
    /// milestone.
    Jittered {
        /// The middle of the range, in ticks.
        centre: u32,
        /// How far either side it may fall, in ticks.
        spread: u32,
        /// The generator's seed, so that a CI run is reproducible.
        seed: u64,
    },
}

/// A generator the attacker wrote out.
///
/// `cheat-client` links `protocol` and a signature library and nothing else, so
/// `sim::Rng` is on the other side of the line `docs/ARCHITECTURE.md` draws.
/// Eight lines of xorshift is the same trade `sim` makes for SHA-256 and the
/// opposite of the one `replay` refuses for a signature: the failure mode of a
/// weak generator here is a bot whose delays are less varied than intended,
/// which the test would see.
#[derive(Clone, Copy, Debug)]
struct Xorshift(u64);

impl Xorshift {
    const fn new(seed: u64) -> Self {
        // Zero is the fixed point of xorshift and would make every delay the
        // centre, which is the *other* variant.
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    const fn next(&mut self) -> u64 {
        let mut state = self.0;
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        self.0 = state;
        state
    }
}

/// A bot that answers what it is shown, on the reflexes it was given.
///
/// It holds what a real client's session holds and nothing the victim's
/// internals would give it: its own seat, the enemy handles the last view named,
/// and a standing order. Everything it knows about the world arrived in a
/// [`PlayerView`] the server chose to send it — so a target it has not been
/// shown is a target it does not know about, which is what makes the reaction
/// it produces a reaction rather than an oracle.
#[derive(Clone, Debug)]
pub struct Reactor {
    bot: Bot,
    own: Seat,
    reflexes: Reflexes,
    rng: Xorshift,
    /// Enemy champion handles the previous view named.
    visible: BTreeSet<u16>,
    /// Answers scheduled but not yet due: the target, and the view tick to
    /// answer on.
    due: Vec<(EntityId, u32)>,
    /// The order to repeat when nothing is due.
    standing: Action,
    /// A target being attacked, and the tick that stops.
    holding: Option<(EntityId, u32)>,
    /// How many appearances this bot has answered.
    answers: u32,
}

/// How long a bot keeps attacking after it answers, in ticks.
///
/// Long enough that the order does something — a basic attack has a cooldown
/// and a match in which nobody ever damaged anybody is a match with no events
/// in it (`docs/RISKS.md` R15) — and short enough that the bot returns to its
/// walk and lets enemies leave and re-enter its vision, which is what produces
/// more than one appearance per pair of champions.
const HOLD_TICKS: u32 = 20;

impl Reactor {
    /// A bot seated in `own`, with these reflexes, holding position.
    #[must_use]
    pub fn new(own: Seat, reflexes: Reflexes) -> Self {
        let seed = match reflexes {
            Reflexes::Jittered { seed, .. } => seed,
            Reflexes::Immediate | Reflexes::Scripted(_) => 1,
        };
        Self {
            bot: Bot::new(),
            own,
            reflexes,
            rng: Xorshift::new(seed),
            visible: BTreeSet::new(),
            due: Vec::new(),
            standing: Action::Idle,
            holding: None,
            answers: 0,
        }
    }

    /// The session underneath, for `Join`, `Ready` and the frames it sends.
    pub const fn bot(&mut self) -> &mut Bot {
        &mut self.bot
    }

    /// Sets the order this bot repeats when it has nothing to answer.
    pub const fn walk_to(&mut self, point: FxVec2) {
        self.standing = Action::Move(point);
    }

    /// Folds in one view and returns the intention for this tick.
    ///
    /// Always returns something, because `docs/ARCHITECTURE.md`'s one intention
    /// per tick is the traffic shape a person's client produces and a bot that
    /// went quiet would be caught by the traffic invariant rather than by
    /// anything behavioural — which is class 1, not class 3.
    pub fn observe(&mut self, view: &PlayerView) -> Action {
        let tick = view.tick.0;

        let mut now = BTreeSet::new();
        for entity in &view.visible {
            if let EntityView::Champion { id, .. } = *entity
                && enemy_of(self.own, id)
            {
                now.insert(id.0);
            }
        }
        for handle in &now {
            if !self.visible.contains(handle) {
                let delay = self.delay();
                self.due
                    .push((EntityId(*handle), tick.saturating_add(delay)));
            }
        }
        self.visible = now;

        // An answer that has come due takes precedence, oldest first — and any
        // *other* answer that came due at the same moment is **dropped rather
        // than deferred**.
        //
        // That is a decision about what this bot is, not a convenience. One
        // intention per tick is the protocol's shape, so two enemies appearing
        // together cannot both be answered on time; deferring the second would
        // make its recorded latency the scripted delay *plus* however long the
        // first one took, and a variant called `Scripted` whose latencies vary
        // is a variant that does not demonstrate what it is named for. Dropping
        // is also the more human of the two — a person who is answering one
        // thing does not queue the other for exactly 233 milliseconds later.
        self.due.sort_by_key(|(_, at)| *at);
        if let Some(index) = self.due.iter().position(|(_, at)| *at <= tick) {
            let (target, _) = self.due.remove(index);
            self.due.retain(|(_, at)| *at > tick);
            self.answers = self.answers.saturating_add(1);
            self.holding = Some((target, tick.saturating_add(HOLD_TICKS)));
            return Action::Attack(target);
        }

        match self.holding {
            Some((target, until)) if tick < until => Action::Attack(target),
            _ => {
                self.holding = None;
                self.standing
            }
        }
    }

    /// How many appearances this bot has answered.
    #[must_use]
    pub const fn answers(&self) -> u32 {
        self.answers
    }

    /// The delay this bot's reflexes call for, in ticks.
    fn delay(&mut self) -> u32 {
        match self.reflexes {
            Reflexes::Immediate => 0,
            Reflexes::Scripted(ticks) => ticks,
            Reflexes::Jittered {
                centre,
                spread,
                seed: _,
            } => {
                if spread == 0 {
                    return centre;
                }
                let width = u64::from(spread).saturating_mul(2).saturating_add(1);
                let drawn = u32::try_from(self.rng.next() % width).unwrap_or(0);
                centre.saturating_add(drawn).saturating_sub(spread)
            }
        }
    }
}

/// Whether a champion handle names somebody on another team.
///
/// A champion's handle *is* its seat, which is a deliberate disclosure
/// (`docs/ARCHITECTURE.md`) and is why this attacker needs no learning phase.
fn enemy_of(own: Seat, id: EntityId) -> bool {
    u8::try_from(id.0)
        .ok()
        .and_then(Seat::from_index)
        .is_some_and(|other| other.team() != own.team())
}

/// A clock a client is free to lie with.
///
/// `docs/SCOPE.md`'s adversary model puts the client's clock under the
/// attacker's control by definition, and M7 established that no rule reads
/// `claimed_at_ms` — four different claimed clocks produce one identical world
/// digest. What is left is a *record* in which the two clocks disagree, and this
/// is the thing that writes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimedClock {
    /// The client's clock is the server's, plus a constant offset.
    ///
    /// The **control**. Every real client has an offset — two machines do not
    /// agree on the epoch — and a detector that read the offset rather than the
    /// rate would flag every honest player in the corpus, which is why
    /// `anticheat`'s clock detector differences two spans.
    Honest {
        /// How far ahead of the server this client's epoch is, in milliseconds.
        offset_ms: u64,
    },
    /// The client's clock runs at a fraction of the server's.
    ///
    /// The exploit for `anticheat`'s clock divergence. `numerator` over
    /// `denominator`: `1/2` is a clock running at half speed, `0/1` a frozen
    /// one, `3/2` one running fast.
    Scaled {
        /// The epoch offset, as [`ClaimedClock::Honest`].
        offset_ms: u64,
        /// The rate's numerator.
        numerator: u64,
        /// The rate's denominator. Zero is read as one.
        denominator: u64,
    },
}

impl ClaimedClock {
    /// What this client claims the time is, given what the server observes.
    #[must_use]
    pub const fn claim(&self, observed_ms: u64) -> u64 {
        match self {
            Self::Honest { offset_ms } => observed_ms.saturating_add(*offset_ms),
            Self::Scaled {
                offset_ms,
                numerator,
                denominator,
            } => {
                let denominator = if *denominator == 0 { 1 } else { *denominator };
                observed_ms
                    .saturating_mul(*numerator)
                    .saturating_div(denominator)
                    .saturating_add(*offset_ms)
            }
        }
    }
}
