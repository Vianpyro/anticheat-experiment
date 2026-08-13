//! Authority: sessions, input intake, the tick loop, fog application, and the
//! recording a match leaves behind.
//!
//! # There is no clock in this file
//!
//! [`Match`] is driven rather than driving. It has no timer, no socket and no
//! runtime; [`Match::tick`] advances the world by exactly one tick when it is
//! called, and [`Match::deliver`] takes the arrival timestamp as an argument
//! rather than reading one. The clock and the sockets live in [`net`], which is
//! thirty lines of `quinn` around this.
//!
//! That split is not tidiness. The authority is the part whose behaviour has to
//! be a function of its inputs — it is what the replay resimulates, what the
//! exploit suite at M7 will drive, and what the traffic-shape properties are
//! stated over — and a function of its inputs is exactly what a component with a
//! clock in it is not. It also means the properties in `server/tests/` run in
//! milliseconds instead of at 30 Hz.
//!
//! # The traffic-shape invariant, and which half lives here
//!
//! `protocol` carries the size half in its types: a `ServerFrame` is a fixed
//! array, so there is no encoder that could return a shorter one. The half that
//! cannot be a type is the cadence, and it is this loop:
//!
//! **[`Match::tick`] returns exactly one frame for every occupied seat, every
//! tick, whatever happened.** There is no early return for a tick in which
//! nothing changed, no extra message when an event fires, and no message that
//! exists only when the match ends. An observer who counts packets and times
//! them learns the tick rate and the number of connected players, both of which
//! they already knew.
//!
//! # Everything a client sends is hostile
//!
//! `docs/SCOPE.md`'s axiom, applied at [`Match::deliver`], which is the only
//! door in:
//!
//! - **The seat comes from the session, never from the message.** There is no
//!   seat field in `ClientMessage` to ignore. `Input::player` is written here.
//! - **The tick comes from the server.** A client says what it wants, not when
//!   it happens; the input is stamped with the tick the server is about to run.
//!   The client's own claimed timestamp is recorded and never read by a rule.
//! - **Sequence numbers are strictly increasing.** A repeated or lowered `seq`
//!   is dropped, which is the application-level residue of exploit class 5 —
//!   QUIC has already defeated the naive packet replay beneath it, and
//!   `docs/SCOPE.md` requires this document to say which layer did what.
//! - **A seat may queue at most [`MAX_INPUTS_PER_TICK`] inputs per tick.**
//!   Beyond that they are dropped rather than disconnecting the session: a
//!   client that just recovered from a stall is indistinguishable from one
//!   trying to make the server allocate, and only one of them deserves to lose
//!   the match. The count is kept so that the difference is at least visible.

#![forbid(unsafe_code)]
#![deny(missing_docs, missing_debug_implementations)]

pub mod net;

use protocol::{
    ClientFrame, ClientMessage, DecodeError, EventBacklog, HandleSpace, RejectReason, ServerFrame,
    ServerMessage,
};
use replay::{Recording, TimedInput};
use sim::view::view_for;
use sim::{Action, Digest, Input, PLAYER_COUNT, Seat, State, Tick, new_state, rules_hash, step};

/// Inputs one seat may have queued for one tick.
///
/// The protocol's shape is one intention per tick, so anything above one is a
/// client catching up. Four leaves room for that and bounds the work a sender
/// can ask for; the excess is dropped and counted, never queued.
pub const MAX_INPUTS_PER_TICK: usize = 4;

/// How a match is set up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchConfig {
    /// The match seed. The only input to the initial world.
    pub seed: u64,
    /// How many seats must be filled and ready before the first tick runs.
    ///
    /// Three at M3, because the exit criterion is three headless clients: one
    /// team of the three. The seats nobody occupies still hold champions —
    /// `State` always has [`sim::PLAYER_COUNT`] — and they stand at their base
    /// taking no orders, which is a perfectly ordinary state for the rules and
    /// one the fixtures already cover.
    pub players: usize,
}

/// Why a frame from a client was not acted on.
///
/// Every one of these is the client's fault. They are distinguished for the
/// operator and for the tests; the transport's answer to all of them is the
/// same, and deliberately so — a caller that branched on the reason would be a
/// caller an attacker could steer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Violation {
    /// The bytes were not a frame.
    Frame(DecodeError),
    /// A message that does not belong in this session's state — a second
    /// `Join`, or anything at all from a seat that is not seated.
    OutOfOrder,
    /// A sequence number that was not strictly greater than the last accepted.
    /// A replayed or reordered input, dropped rather than applied twice.
    Sequence,
    /// More inputs in one tick than a seat may queue.
    RateLimit,
}

/// One connected player.
#[derive(Debug)]
struct Session {
    ready: bool,
    /// This recipient's private naming of the projectiles it has been shown.
    handles: HandleSpace,
    /// The events this recipient is owed and the frame budget could not carry.
    events: EventBacklog,
    /// The highest sequence number accepted from this seat.
    highest_seq: Option<u32>,
    /// The highest sequence number from this seat that has been drained into a
    /// tick, which is what the frame acknowledges.
    ///
    /// Distinct from `highest_seq` in what it means rather than, today, in what
    /// it holds: an input is *accepted* the moment it decodes and sequences, and
    /// *applied* when the loop below drains it, and since the loop drains its
    /// whole queue before emitting a frame the two coincide on the wire. What
    /// they do not share is the refusals — `highest_seq` is untouched by a
    /// refused input and so is this, which is the half a client depends on.
    ///
    /// Written where the meaning is rather than where it is currently
    /// convenient, so that a rate policy which one day holds an input across a
    /// tick does not silently start acknowledging intentions the world has not
    /// seen.
    applied_seq: Option<u32>,
    /// What this seat has asked for since the last tick.
    pending: Vec<Pending>,
    /// Inputs refused for sequencing or for rate. Never read by a rule; it is
    /// the number that tells an operator a session is behaving oddly.
    refused: u32,
}

/// An intention, before the server stamps a tick on it.
#[derive(Clone, Copy, Debug)]
struct Pending {
    seq: u32,
    claimed_at_ms: u64,
    received_at_ms: u64,
    action: Action,
}

/// An authoritative match.
#[derive(Debug)]
pub struct Match {
    seed: u64,
    expected_players: usize,
    state: State,
    seats: [Option<Session>; PLAYER_COUNT],
    started: bool,
    ticks_run: u32,
    log: Vec<TimedInput>,
}

impl Match {
    /// A match nobody has joined yet.
    #[must_use]
    pub fn new(config: MatchConfig) -> Self {
        Self {
            seed: config.seed,
            expected_players: config.players,
            state: new_state(config.seed),
            seats: [const { None }; PLAYER_COUNT],
            started: false,
            ticks_run: 0,
            log: Vec::new(),
        }
    }

    /// The tick the world is on. Advanced by exactly one per [`Match::tick`].
    #[must_use]
    pub const fn tick_number(&self) -> Tick {
        self.state.tick()
    }

    /// Whether the first tick has run.
    #[must_use]
    pub const fn started(&self) -> bool {
        self.started
    }

    /// The authoritative digest of the world.
    ///
    /// The only sanctioned way to compare two states, and what the offline
    /// resimulation has to reproduce.
    #[must_use]
    pub fn digest(&self) -> Digest {
        self.state.digest()
    }

    /// Which seats are occupied.
    #[must_use]
    pub fn seated(&self) -> Vec<Seat> {
        Seat::ALL
            .into_iter()
            .filter(|seat| self.seats[seat.index()].is_some())
            .collect()
    }

    /// How many inputs this seat has had refused, for sequencing or for rate.
    ///
    /// Not a detector and not evidence: it is the counter that makes the
    /// difference between "the client stalled" and "the client is probing"
    /// visible to whoever is reading a log. The detectors are M8's and they read
    /// telemetry rather than this.
    #[must_use]
    pub fn refused(&self, seat: Seat) -> u32 {
        self.seats[seat.index()]
            .as_ref()
            .map_or(0, |session| session.refused)
    }

    /// How many events this seat is owed that its frames could not carry, and
    /// how many it will never be told about.
    ///
    /// The frame's event budget is smaller than a tick's capacity
    /// (`sim::view::MAX_EVENTS_PER_VIEW`), so an overflow waits for the next
    /// frame rather than being lost. These two numbers are what make "waits"
    /// and "lost" tellable apart from outside, and the second is expected to
    /// stay at zero for the whole life of this project — it is not a budget, it
    /// is a tripwire.
    #[must_use]
    pub fn events_owed(&self, seat: Seat) -> (usize, u32) {
        self.seats[seat.index()].as_ref().map_or((0, 0), |session| {
            (session.events.deferred(), session.events.dropped())
        })
    }

    /// A new connection asks for a seat.
    ///
    /// The server picks which one — the lowest free — and tells the client.
    /// A client that could name its seat could name somebody else's, so the
    /// message has no field for it.
    ///
    /// Returns the seat if one was granted, and in every case the frame to send
    /// back. There is always a frame: a refusal a client cannot distinguish
    /// from a dropped packet is a refusal it retries forever.
    pub fn join(&mut self) -> (Option<Seat>, ServerFrame) {
        if self.started {
            return (
                None,
                ServerFrame::encode(&ServerMessage::Rejected(RejectReason::AlreadyStarted)),
            );
        }
        let Some(seat) = Seat::ALL
            .into_iter()
            .find(|seat| self.seats[seat.index()].is_none())
        else {
            return (
                None,
                ServerFrame::encode(&ServerMessage::Rejected(RejectReason::MatchFull)),
            );
        };

        self.seats[seat.index()] = Some(Session {
            ready: false,
            handles: HandleSpace::new(),
            events: EventBacklog::new(),
            highest_seq: None,
            applied_seq: None,
            pending: Vec::new(),
            refused: 0,
        });
        (
            Some(seat),
            ServerFrame::encode(&ServerMessage::Accepted {
                seat,
                seed: self.seed,
                rules_hash: rules_hash(),
            }),
        )
    }

    /// Bytes arrived on an established session.
    ///
    /// `received_at_ms` is the server's own observation of when, which is the
    /// only clock in the system that is evidence of anything. It is recorded
    /// beside the client's claim and read by no rule.
    ///
    /// # Errors
    ///
    /// [`Violation`], for a frame that did not decode or a message this session
    /// is not allowed to send right now. Nothing here is fatal to the match:
    /// the caller decides whether to close the session, and at M3 it does.
    pub fn deliver(
        &mut self,
        seat: Seat,
        bytes: &[u8],
        received_at_ms: u64,
    ) -> Result<(), Violation> {
        let message = ClientFrame::decode(bytes).map_err(Violation::Frame)?;
        let session = self.seats[seat.index()]
            .as_mut()
            .ok_or(Violation::OutOfOrder)?;

        match message {
            // The session already has a seat; a second request is a client
            // whose state machine does not match the server's.
            ClientMessage::Join => Err(Violation::OutOfOrder),
            ClientMessage::Ready => {
                session.ready = true;
                Ok(())
            }
            ClientMessage::Input {
                seq,
                claimed_at_ms,
                action,
            } => {
                // Strictly greater, so a replayed frame is a no-op rather than
                // a second command. QUIC has already rejected a duplicated
                // *packet* beneath this; what it cannot know is that two
                // distinct packets carry the same application-level intention,
                // which is what this is for.
                if session.highest_seq.is_some_and(|last| seq <= last) {
                    session.refused = session.refused.saturating_add(1);
                    return Err(Violation::Sequence);
                }
                if session.pending.len() >= MAX_INPUTS_PER_TICK {
                    session.refused = session.refused.saturating_add(1);
                    return Err(Violation::RateLimit);
                }
                session.highest_seq = Some(seq);
                session.pending.push(Pending {
                    seq,
                    claimed_at_ms,
                    received_at_ms,
                    action,
                });
                Ok(())
            }
            // Surrender frees the seat. It does not decide the match: whether a
            // team that has conceded loses is a *rule*, and rules live in `sim`
            // where a replay resimulates them. What the session layer can
            // honestly do is stop representing a player who has left, and that
            // is what this does. Their champion stays on the map taking no
            // orders, exactly as an unoccupied seat's does.
            ClientMessage::Surrender => {
                self.seats[seat.index()] = None;
                Ok(())
            }
        }
    }

    /// Advances the world by one tick and returns one frame per occupied seat.
    ///
    /// The return type is the cadence invariant: the length of this vector is
    /// the number of occupied seats and is a function of nothing else. Before
    /// the match starts there are no ticks, so it is empty — a client that has
    /// joined and is waiting knows it is waiting.
    #[must_use]
    pub fn tick(&mut self) -> Vec<(Seat, ServerFrame)> {
        if !self.started {
            if !self.everybody_ready() {
                return Vec::new();
            }
            self.started = true;
        }

        let now = self.state.tick();
        let mut inputs = Vec::new();
        for seat in Seat::ALL {
            let Some(session) = self.seats[seat.index()].as_mut() else {
                continue;
            };
            // Drained in seat order, and within a seat in arrival order, which
            // is sequence order because `deliver` accepts only strictly
            // increasing numbers. `step` requires inputs pre-sorted by
            // `(player, seq)` and neither sorts nor deduplicates, so this loop
            // is where that contract is met.
            for pending in session.pending.drain(..) {
                let input = Input {
                    tick: now,
                    seq: pending.seq,
                    player: seat,
                    action: pending.action,
                };
                // Recorded here rather than in `deliver`, because this is the
                // line at which the intention stops being queued and starts
                // being part of the world. The frames below carry it back.
                session.applied_seq = Some(pending.seq);
                inputs.push(input);
                self.log.push(TimedInput {
                    input,
                    claimed_at_ms: pending.claimed_at_ms,
                    received_at_ms: pending.received_at_ms,
                });
            }
        }

        self.state = step(&self.state, &inputs);
        self.ticks_run = self.ticks_run.saturating_add(1);

        // One frame per occupied seat, unconditionally. Nothing above this line
        // may become a reason not to send one.
        let mut frames = Vec::with_capacity(PLAYER_COUNT);
        for seat in Seat::ALL {
            let Some(session) = self.seats[seat.index()].as_mut() else {
                continue;
            };
            let view = view_for(&self.state, seat);
            let localized = session.handles.localize(&view, self.state.projectiles());
            // Shaped last, because the frame budget is the last thing between
            // the projection and the wire: `localize` decides what this
            // recipient calls things, `shape` decides how much of it fits in
            // one frame and holds the rest for the next.
            let shaped = session.events.shape(&localized);
            frames.push((
                seat,
                ServerFrame::encode(&ServerMessage::View {
                    view: shaped,
                    applied_through: session.applied_seq,
                }),
            ));
        }
        frames
    }

    /// The world, for tests that need to state a property about it.
    ///
    /// Handing out a `&State` is safe here in a way it would not be in most
    /// codebases, and the reason is the structural invariant rather than
    /// discipline: `State` implements no serialization anywhere in the
    /// workspace (`docs/RISKS.md` R5), so a caller holding one cannot put it on
    /// a socket or in a file. It can read it, which is exactly what
    /// `server/tests/traffic.rs` needs — the antecedent of the byte-equality
    /// property is what a player is *entitled* to know, derived from the same
    /// state the frames were produced from.
    ///
    /// This accessor is the clearest demonstration this repository has that R5
    /// bought something: making the state readable is a two-line decision
    /// instead of an audit.
    #[must_use]
    pub const fn world(&self) -> &State {
        &self.state
    }

    /// The match so far, as the seed and the log that reproduce it.
    ///
    /// **Unsigned, and it has no encoding.** `docs/MILESTONES.md` M5 gives this
    /// project one file format and it is `replay::Replay`, which is this bound
    /// to an identity by a signature over a manifest. The sealing happens in
    /// `replay::seal` and not here, because the authority has no clock, no
    /// socket and no identity — which is what makes it a function of its inputs,
    /// and a signing key inside it would be the first secret in the one
    /// component that is supposed to have none.
    ///
    /// `outcome` is a field rather than something a reader derives, because it
    /// is the claim a replay is submitted to make: exploit class 2 is result
    /// forgery, and the manifest has to carry the assertion in order for
    /// resimulation to be able to contradict it.
    #[must_use]
    pub fn recording(&self) -> Recording {
        Recording {
            seed: self.seed,
            rules_hash: rules_hash(),
            ticks: self.ticks_run,
            outcome: self.state.outcome(),
            final_state_digest: self.state.digest(),
            inputs: self.log.clone(),
        }
    }

    /// Events this match will never deliver to anybody, across every seat.
    ///
    /// **This number is a defect count, not a statistic.** A non-zero value
    /// means some client permanently missed something that happened to it: the
    /// frame's event budget defers its overflow rather than dropping it, and the
    /// queue that holds the deferral is a tick's capacity deep, so reaching the
    /// bound needs a sustained rate no reachable state under the game's
    /// constants produces. `docs/ARCHITECTURE.md` calls it a tripwire rather
    /// than a budget, and the tests that drive a whole match assert it is zero
    /// so that the wire is what trips rather than a metric nobody reads.
    #[must_use]
    pub fn events_lost(&self) -> u32 {
        Seat::ALL
            .into_iter()
            .map(|seat| self.events_owed(seat).1)
            .fold(0, u32::saturating_add)
    }

    fn everybody_ready(&self) -> bool {
        let seated = self.seated();
        seated.len() >= self.expected_players
            && seated.iter().all(|seat| {
                self.seats[seat.index()]
                    .as_ref()
                    .is_some_and(|session| session.ready)
            })
    }
}
