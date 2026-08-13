//! A headless client: input scripts in, digests out.
//!
//! `docs/MILESTONES.md` M3 asks for headless clients only. Rendering, input
//! capture and prediction are M4's, and the exit criterion this crate has to
//! serve is narrow and precise: three clients join one match, run a thousand
//! ticks of scripted input, and report **identical digests of their reconciled
//! local view at every checkpoint tick**.
//!
//! # What "reconciled" means here, and what it does not
//!
//! At this milestone it means two things and neither of them is prediction:
//!
//! - Views are applied in tick order, and one that is not newer than what the
//!   client already holds is discarded. That is what makes the local world a
//!   function of the match rather than of delivery order.
//! - The server's view for a tick **replaces** whatever the client believed
//!   about it. The client holds no opinion the server has to argue with,
//!   because the server is the authority and the client has no world model of
//!   its own to predict from.
//!
//! What is deliberately absent is client-side prediction, and finding out *why*
//! it cannot be built yet was worth the trip. Prediction needs the client to
//! know **which of its inputs the server applied to which tick**, so that it can
//! re-apply the ones that are still outstanding on top of the authoritative
//! state. The protocol does not tell it: the client sends an intention with a
//! sequence number, the server buckets it into whichever tick it happens to be
//! about to run, and `PlayerView` carries no acknowledgement — no last-applied
//! sequence number, no echo of the standing order. `docs/ARCHITECTURE.md` left
//! the order out on purpose, because it names an `EntityId` the player may no
//! longer be able to see. So M4 needs one more field or one more message, and
//! the shape of it is a decision M4 has to make rather than one M3 should make
//! for it by accident.
//!
//! # The digest, and why the three clients can be compared at all
//!
//! A client's local world is what it was told, merged: the entity list from
//! `visible`, plus its own champion — which travels in `own` and is never in
//! `visible` — folded in as a champion at the same fidelity. The three seats of
//! one team receive the same visible set (vision is a team property) and their
//! projectile handles agree (`protocol::HandleSpace` allocates in handle order,
//! which is the same order for all three), so after the merge the three worlds
//! are the same object and their digests are the same 32 bytes.
//!
//! Two consequences of that are not accidents and would break the comparison if
//! they changed. A client that joined late would legitimately disagree about
//! projectile handles, and two clients on *opposite* teams are supposed to
//! disagree about everything — the exit criterion compares a team, not a match.

#![forbid(unsafe_code)]
#![deny(missing_docs, missing_debug_implementations)]

pub mod draw;
pub mod gfx;
pub mod health;
pub mod input;
pub mod net;
pub mod play;
pub mod predict;

use std::collections::BTreeMap;

use protocol::{ClientFrame, ClientMessage, DecodeError, ServerFrame, ServerMessage};
use sim::view::{EntityView, PlayerView, VisibleEvent};
use sim::{Action, Digest, Fx, FxVec2, Liveness, Outcome, Seat, Tick, digest_bytes};

/// Why a frame from the server was not applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientError {
    /// The bytes were not a frame this build reads.
    Frame(DecodeError),
    /// The server refused the session.
    Rejected(protocol::RejectReason),
    /// A message that does not belong in this session's state: a view before
    /// the seat was assigned, or a second `Accepted`.
    OutOfOrder,
    /// The server plays by constants this build does not have. Reported rather
    /// than absorbed: a client that plays on regardless is a client rendering a
    /// different game (`docs/RISKS.md` R2).
    RulesHash,
}

impl core::fmt::Display for ClientError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Frame(error) => write!(f, "{error}"),
            Self::Rejected(reason) => write!(f, "the server refused the session: {reason:?}"),
            Self::OutOfOrder => write!(f, "a message this session was not expecting"),
            Self::RulesHash => write!(f, "the server plays by other constants"),
        }
    }
}

impl core::error::Error for ClientError {}

/// One thing the client currently believes is on the map.
///
/// Its own champion is folded in as a [`Sighting::Champion`], which is what
/// makes two clients on one team comparable: the seat that owns it is the only
/// one that receives it through `own`, and after the merge nobody can tell which
/// entry came from where.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sighting {
    Champion { position: FxVec2, hp: Fx },
    Tower { position: FxVec2, hp: Fx },
    Projectile { position: FxVec2, velocity: FxVec2 },
}

/// Everything a client currently believes about the match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalWorld {
    tick: Tick,
    outcome: Outcome,
    /// Keyed by the handle the client was given, so the map's order is the
    /// handle's order and the digest does not depend on arrival order.
    entities: BTreeMap<u16, Sighting>,
    /// This tick's derived signals, in the order the server sent them.
    events: Vec<VisibleEvent>,
    /// Whether the client's own champion is on the map, and its respawn tick if
    /// not. Held apart from `entities` because a dead champion is not on the
    /// map — for its own team either — and folding a corpse in would make this
    /// world disagree with a teammate's about a square nobody is standing on.
    ///
    /// **Not hashed**, and that is a correction rather than an omission; see
    /// [`LocalWorld::digest`].
    own_liveness: Liveness,
}

impl LocalWorld {
    /// The tick this world describes.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// How the match stands.
    #[must_use]
    pub const fn outcome(&self) -> Outcome {
        self.outcome
    }

    /// How many entities the client currently believes are on the map.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Whether the client believes nothing is on the map, which in a match with
    /// a living teammate should never happen.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// The 32-byte fingerprint of the part of this world two teammates are
    /// supposed to agree about.
    ///
    /// Hand-written and exhaustively destructured, exactly as `sim`'s canonical
    /// encoding is and for the same reason: a field added to the local world and
    /// not hashed would leave three clients agreeing about a world they had
    /// stopped fully comparing. `sim::digest_bytes` is the hash, so that the
    /// client and everything else in the workspace compute SHA-256 the same way
    /// rather than through a second implementation.
    ///
    /// # Why `own_liveness` is deliberately outside it
    ///
    /// This digest exists for one purpose: `docs/MILESTONES.md` M3's exit
    /// criterion, which compares the reconciled worlds of three clients on one
    /// team. That comparison is only meaningful over what the three are
    /// *entitled to the same view of*, and `own_liveness` is the one field that
    /// is not — it is this player's own hit points and its own respawn timer,
    /// and `docs/ARCHITECTURE.md` is explicit that an ally's respawn timer is
    /// information about a player rather than about the world and is withheld
    /// from teammates on purpose.
    ///
    /// Hashing it made the criterion false the first time anybody took damage,
    /// and it went unnoticed for a milestone because M3's scripted match
    /// produces no damage at all: the three clients walk a lane and nothing
    /// touches them, so their own hit points stayed equal by accident. M4's loss
    /// tests walk them into a tower's range instead, and the criterion reported
    /// `Blue0 disagrees with Blue1 about the world at tick 620` — with the two
    /// worlds identical except for Blue0's own 555 hit points against Blue1's
    /// 600, which every one of the three could see and only one of them was
    /// hashing.
    ///
    /// Nothing is lost by the exclusion. A living champion is folded into
    /// `entities` with the hit points every teammate is shown, and a dead one is
    /// absent there — which is exactly what a teammate observes, because a dead
    /// champion is not on the map for its own team either. What leaves the
    /// digest is the respawn timer, which no teammate is entitled to and which
    /// therefore had no business in a cross-client comparison.
    #[must_use]
    pub fn digest(&self) -> Digest {
        let LocalWorld {
            tick,
            outcome,
            entities,
            events,
            // Bound and deliberately not hashed. Named rather than skipped with
            // `..`, so that the exclusion is a decision somebody made and not a
            // field somebody forgot: `..` here would let the *next* field be
            // dropped by accident, which is the failure `sim::canonical` bans
            // the pattern for.
            own_liveness: _,
        } = self;

        let mut out = Vec::with_capacity(256);
        out.extend_from_slice(b"moba/client/world/v2");
        out.extend_from_slice(&tick.0.to_be_bytes());
        encode_outcome(&mut out, *outcome);

        out.extend_from_slice(&(entities.len() as u64).to_be_bytes());
        for (handle, sighting) in entities {
            out.extend_from_slice(&handle.to_be_bytes());
            match *sighting {
                Sighting::Champion { position, hp } => {
                    out.push(0);
                    encode_point(&mut out, position);
                    out.extend_from_slice(&hp.to_raw().to_be_bytes());
                }
                Sighting::Tower { position, hp } => {
                    out.push(1);
                    encode_point(&mut out, position);
                    out.extend_from_slice(&hp.to_raw().to_be_bytes());
                }
                Sighting::Projectile { position, velocity } => {
                    out.push(2);
                    encode_point(&mut out, position);
                    encode_point(&mut out, velocity);
                }
            }
        }

        out.extend_from_slice(&(events.len() as u64).to_be_bytes());
        for event in events {
            match *event {
                VisibleEvent::Cast {
                    caster,
                    ability,
                    at,
                } => {
                    out.push(0);
                    out.extend_from_slice(&caster.0.to_be_bytes());
                    out.push(match ability {
                        sim::Ability::Skillshot => 0,
                        sim::Ability::Targeted => 1,
                    });
                    encode_point(&mut out, at);
                }
                VisibleEvent::Damage { target, amount, at } => {
                    out.push(1);
                    out.extend_from_slice(&target.0.to_be_bytes());
                    out.extend_from_slice(&amount.to_raw().to_be_bytes());
                    encode_point(&mut out, at);
                }
                VisibleEvent::Death { entity, at } => {
                    out.push(2);
                    out.extend_from_slice(&entity.0.to_be_bytes());
                    encode_point(&mut out, at);
                }
            }
        }

        digest_bytes(&out)
    }
}

fn encode_point(out: &mut Vec<u8>, point: FxVec2) {
    out.extend_from_slice(&point.x.to_raw().to_be_bytes());
    out.extend_from_slice(&point.y.to_raw().to_be_bytes());
}

fn encode_outcome(out: &mut Vec<u8>, outcome: Outcome) {
    match outcome {
        Outcome::InProgress => out.push(0),
        Outcome::Decided { winner, at } => {
            out.push(1);
            out.push(match winner {
                sim::Team::Blue => 0,
                sim::Team::Red => 1,
                sim::Team::Green => 2,
            });
            out.extend_from_slice(&at.0.to_be_bytes());
        }
    }
}

/// A headless client: a session, a local world, and a sequence number.
#[derive(Clone, Debug)]
pub struct Headless {
    seat: Option<Seat>,
    seed: u64,
    world: LocalWorld,
    seq: u32,
    /// The highest sequence number of this client's own inputs that the server
    /// says it has folded into the world.
    ///
    /// The field M3 could not build against and M4 added — see
    /// `protocol::ServerMessage::View`. The headless client does not predict, so
    /// it records the acknowledgement and nothing more; `Predicted` is what
    /// consumes it.
    applied_through: Option<u32>,
    /// Views discarded for being no newer than what the client already holds.
    /// The transport is reliable and ordered, so a non-zero count here means
    /// something upstream is wrong rather than that the network reordered.
    stale: u32,
    /// The last view applied, kept whole for the renderer.
    ///
    /// `LocalWorld` is the *comparable* world — a merged entity map built for
    /// M3's digest — and it deliberately forgets which handle was whose and what
    /// the events were about. A renderer needs both, so it reads this instead.
    /// It is the view the server sent, unmodified: nothing here accumulates
    /// across ticks, because a client that remembered where an enemy used to be
    /// would be reconstructing what the fog withheld (`crate::draw`).
    last_view: Option<PlayerView>,
}

impl Default for Headless {
    fn default() -> Self {
        Self::new()
    }
}

impl Headless {
    /// A client that has not been given a seat yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            seat: None,
            seed: 0,
            world: LocalWorld {
                tick: Tick(0),
                outcome: Outcome::InProgress,
                entities: BTreeMap::new(),
                events: Vec::new(),
                own_liveness: Liveness::Alive { hp: Fx::ZERO },
            },
            seq: 0,
            applied_through: None,
            stale: 0,
            last_view: None,
        }
    }

    /// The last view the server sent, whole.
    #[must_use]
    pub const fn view(&self) -> Option<&PlayerView> {
        self.last_view.as_ref()
    }

    /// The sequence number the next [`Headless::intend`] will use.
    ///
    /// A caller that has to record an intention *before* sending it — a
    /// prediction, which must know the number the acknowledgement will name —
    /// needs this. It is not an allocator: calling it twice returns the same
    /// number, and only `intend` advances it.
    #[must_use]
    pub const fn next_seq(&self) -> u32 {
        self.seq
    }

    /// The highest sequence number of this client's own inputs the server says
    /// it has applied.
    #[must_use]
    pub const fn applied_through(&self) -> Option<u32> {
        self.applied_through
    }

    /// The seat the server assigned, once it has.
    #[must_use]
    pub const fn seat(&self) -> Option<Seat> {
        self.seat
    }

    /// The match seed the server named.
    #[must_use]
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    /// The world the client has reconciled.
    #[must_use]
    pub const fn world(&self) -> &LocalWorld {
        &self.world
    }

    /// How many views were discarded for not being newer.
    #[must_use]
    pub const fn stale(&self) -> u32 {
        self.stale
    }

    /// The frame that asks for a seat.
    #[must_use]
    pub fn join(&self) -> ClientFrame {
        ClientFrame::encode(&ClientMessage::Join)
    }

    /// The frame that says this client is ready.
    #[must_use]
    pub fn ready(&self) -> ClientFrame {
        ClientFrame::encode(&ClientMessage::Ready)
    }

    /// The frame carrying one intention, and advances the sequence number.
    ///
    /// `claimed_at_ms` is this client's own clock, which the server records and
    /// never trusts. A headless client has no reason to lie, and it is passed in
    /// rather than read here so that the cheat client at M7 can lie about it
    /// through the same protocol without needing a second one.
    pub fn intend(&mut self, action: Action, claimed_at_ms: u64) -> ClientFrame {
        let frame = ClientFrame::encode(&ClientMessage::Input {
            seq: self.seq,
            claimed_at_ms,
            action,
        });
        self.seq = self.seq.saturating_add(1);
        frame
    }

    /// Applies a frame from the server.
    ///
    /// # Errors
    ///
    /// [`ClientError`] for a frame that did not decode, a refusal, a message
    /// out of order for this session, or a server playing by other constants.
    pub fn receive(&mut self, bytes: &[u8]) -> Result<(), ClientError> {
        match ServerFrame::decode(bytes).map_err(ClientError::Frame)? {
            ServerMessage::Accepted {
                seat,
                seed,
                rules_hash,
            } => {
                if self.seat.is_some() {
                    return Err(ClientError::OutOfOrder);
                }
                if rules_hash != sim::rules_hash() {
                    return Err(ClientError::RulesHash);
                }
                self.seat = Some(seat);
                self.seed = seed;
                Ok(())
            }
            ServerMessage::Rejected(reason) => Err(ClientError::Rejected(reason)),
            ServerMessage::View {
                view,
                applied_through,
            } => {
                if self.seat.is_none() {
                    return Err(ClientError::OutOfOrder);
                }
                self.applied_through = applied_through;
                self.reconcile(&view);
                Ok(())
            }
        }
    }

    /// Replaces the local world with what the server says, if it is newer.
    fn reconcile(&mut self, view: &PlayerView) {
        // The first view carries tick 1, so the initial `Tick(0)` is not a view
        // that has been applied. Anything at or below the current tick is a
        // view the client has already accounted for.
        if view.tick <= self.world.tick && self.world.tick != Tick(0) {
            self.stale = self.stale.saturating_add(1);
            return;
        }

        let mut entities = BTreeMap::new();
        for entity in &view.visible {
            let (handle, sighting) = match *entity {
                EntityView::Champion { id, position, hp } => {
                    (id.0, Sighting::Champion { position, hp })
                }
                EntityView::Tower { id, position, hp } => (id.0, Sighting::Tower { position, hp }),
                EntityView::Projectile {
                    id,
                    position,
                    velocity,
                } => (id.0, Sighting::Projectile { position, velocity }),
            };
            entities.insert(handle, sighting);
        }
        // The client's own champion, folded in at the fidelity a teammate would
        // have been given for it. While it is dead it is absent, which is what a
        // teammate sees too — a dead champion is not on the map.
        if let Liveness::Alive { hp } = view.own.liveness {
            entities.insert(
                view.own.id.0,
                Sighting::Champion {
                    position: view.own.position,
                    hp,
                },
            );
        }

        self.world = LocalWorld {
            tick: view.tick,
            outcome: view.outcome,
            entities,
            events: view.events.clone(),
            own_liveness: view.own.liveness,
        };
        self.last_view = Some(view.clone());
    }
}
