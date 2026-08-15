//! The replay container: what a match leaves behind, and what a third party can
//! establish from it.
//!
//! # What this is at M5, and what changed
//!
//! At M3 and M4 this crate held a *recording*: a container, a reader total on
//! hostile bytes, and a resimulation. It carried `rules_hash` and nothing else,
//! so a digest mismatch meant "these bytes do not describe that match" and could
//! not distinguish tampering from a build difference; it had no signature, so it
//! proved that some server wrote it and nothing about who. It was a development
//! artefact and its own documentation said so.
//!
//! `docs/MILESTONES.md` M5 is the milestone that turns it into evidence, and it
//! is the milestone that **freezes a record format**: whatever is missing from
//! [`Manifest`] is missing from every match this project will ever record.
//! [`crate::manifest`] carries the field-by-field account and, more importantly,
//! the absences and why each of them is a decision.
//!
//! **There is now exactly one file format.** The unsigned container is gone
//! rather than kept beside the signed one. Two formats would be two things to
//! verify, two things to parse, and — the reason that decided it — a question
//! with no good answer the first time somebody hands you the unsigned one: a
//! reader that accepts both accepts the weaker, and a corpus that holds both
//! holds files nobody can tell apart at a glance. [`Recording`] survives as the
//! authority's in-memory product and has no encoding of its own; [`seal`] is the
//! only way it reaches a disk.
//!
//! # The three layers, and which one makes which claim
//!
//! - [`Recording`] — what `server::Match` produced. Facts about a match, held in
//!   memory, signed by nobody.
//! - [`Replay`] — a recording bound to an identity, a build and a moment by a
//!   signature over a [`Manifest`]. The only thing this project writes to disk
//!   and the only thing it will publish.
//! - [`verify`] — the one function that turns bytes into a claim, and
//!   [`crate::container`]'s header is the honest account of which claims and
//!   which not. Short version: it establishes that a key you accept sealed this
//!   manifest, that the log is the one the manifest names, and that the log
//!   reaches the state and the result it claims. It establishes **nothing about
//!   whether a player cheated**, which is `docs/SCOPE.md`'s standing note on
//!   exploit class 2 and is the reason M7's exploit rather than this milestone
//!   is what makes the defence delivered.
//!
//! # Why the log holds inputs and not frames
//!
//! A [`TimedInput`] names a seat, a tick, a sequence number and an action. It
//! names no projectile and no view. That matters because `protocol` gives every
//! recipient its own projectile handles: a log of what clients were *told* would
//! be a log in nine mutually untranslatable namings, and resimulating it would
//! mean choosing one. A log of what clients *asked for* is in the one namespace
//! the server and `sim` share, which is why resimulation is a function of it.
//!
//! It also settles a question `docs/ARCHITECTURE.md` answers at length: a replay
//! records the events the rules **produced**, never the ones a client was
//! **delivered**, and it does so structurally rather than by convention —
//! because it records no events at all. Resimulation derives them.
//!
//! # And the corpus, which arrives at M4 with the consent regime
//!
//! [`corpus`] is a directory of replays and the consent records that make them
//! lawful to hold, plus the command that destroys a participant's part of it and
//! the command that fails if anything was left behind. It lives here rather than
//! in a crate of its own because `docs/ARCHITECTURE.md` refuses an `xtask` crate
//! and a corpus is a directory of the thing this crate defines; the alternative
//! is an eighth crate whose whole content is `std::fs`.
//!
//! # And what M6 adds to it, which is a schema rather than a format
//!
//! The replay format is frozen and this milestone does not touch it. What it adds
//! is the three things a corpus of *people* needs and a corpus of matches does
//! not: [`session`], the per-seat record of what each match was recorded on —
//! hardware, sensitivity, platform, and whether the client kept up with the tick;
//! [`consent`], which turns "they signed the document" into a version a program
//! can refuse; and [`split`], the train/holdout assignment, frozen before the
//! first detector and computed rather than stored. `docs/SCHEMA.md` is the
//! document; these modules are what makes each of its rules a refusal.
//!
//! # And the companion, which is the one thing M5's freeze did have to move
//!
//! [`telemetry`] is a **second sealed file** carrying the device-event stream at
//! its native 125 Hz to 1 kHz, per seat. It exists because two of
//! `docs/MILESTONES.md` M8's five candidate signals — the inter-arrival
//! distribution and aim-correction curvature — were not detectors with
//! uncalibrated thresholds but detectors whose **quantity was not in the corpus**
//! at any resolution, and the recording policy that excluded it was a decision
//! rather than a property of the system.
//!
//! Three things about it are the whole design, and each of them is a refusal:
//!
//! - **It is not in the replay**, so M5's invariant does not move: a resimulation
//!   is a function of the seed and the log alone, and nothing no rule reads can
//!   influence it.
//! - **The replay's manifest carries its digest**
//!   ([`crate::manifest::Commitment`]), so a companion cannot be swapped for
//!   another and a replay stays verifiable without one.
//! - **Its absence is a named state rather than an error.** A replay committing
//!   to [`Commitment::Absent`] is a complete replay of a match that recorded no
//!   device telemetry, and [`verify`] says so in those words.

#![forbid(unsafe_code)]
#![deny(missing_docs, missing_debug_implementations, unused_variables)]

pub mod consent;
pub mod container;
pub mod corpus;
pub mod keys;
pub mod manifest;
pub mod session;
pub mod split;
pub mod telemetry;

pub use crate::consent::ConsentVersion;
pub use crate::container::{
    FORMAT, ReadError, Replay, Verified, VerifyError, resimulate, seal, signed_bytes, verify,
};
pub use crate::keys::{
    KeyEntry, KeyRegistry, KeyStatus, RegistryError, Signature, SigningKey, VerifyingKey,
};
pub use crate::manifest::{
    Build, Commitment, Manifest, MatchId, Pseudonym, SessionFacts, SimCommit,
};
pub use crate::session::SessionRecord;
pub use crate::split::{Split, split_of};
pub use crate::telemetry::{Telemetry, TelemetryError, TelemetryLog, TelemetryPart};

use sim::{Action, Digest, EntityId, Fx, FxVec2, Input, Outcome, Seat, Tick};

/// One input, with both clocks.
///
/// `claimed_at_ms` is what the client said and is attacker-controlled by
/// definition; `received_at_ms` is what the server observed and is the only
/// clock in the system that is evidence of anything (`docs/SCOPE.md`, adversary
/// model). Both are recorded, neither is read by [`resimulate`], and the
/// divergence between them is exploit class 4's signal at M8.
///
/// These two fields are also the whole of the per-input telemetry this format
/// carries, and `docs/MILESTONES.md` M6 asks for exactly them. Anything at a
/// higher rate — the raw device deltas `client::input::InputTrace` holds — lives
/// beside a replay rather than inside one, because `sim` consumes one intention
/// per tick at 30 Hz and folding a kilohertz stream into the artefact
/// resimulation is a function of would have made the resimulation a function of
/// something the rules never read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimedInput {
    /// The input as the server accepted it. Its `player` and `tick` are the
    /// server's, not the sender's.
    pub input: Input,
    /// When the client said it produced the input. Untrusted.
    pub claimed_at_ms: u64,
    /// When the server observed it arrive.
    pub received_at_ms: u64,
}

/// A match, as the authority produced it.
///
/// **This type has no encoding**, and that is the M5 decision rather than an
/// omission: the only thing that reaches a disk is a [`Replay`], which is this
/// bound to an identity by a signature. A `Recording::encode` would be a second
/// file format, unsigned, indistinguishable from the real one at a glance, and
/// exactly the artefact somebody would hand you as evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recording {
    /// The match seed.
    pub seed: u64,
    /// The constants the match was played under (`docs/RISKS.md` R2).
    pub rules_hash: Digest,
    /// How many ticks the server ran.
    pub ticks: u32,
    /// How the match stood when the server stopped. The claim a forged replay
    /// exists to make, and the reason it is a field rather than a derivation.
    pub outcome: Outcome,
    /// The digest the server ended on. What [`resimulate`] has to reproduce.
    pub final_state_digest: Digest,
    /// Every input the server accepted, in the order it applied them.
    pub inputs: Vec<TimedInput>,
}

/// A `TimedInput` on the wire: tick, seq, seat, both clocks, and an action
/// padded to its widest variant so that every entry is the same width.
pub(crate) const INPUT_BYTES: usize = 4 + 4 + 1 + 8 + 8 + 9;

pub(crate) fn write_input(out: &mut Vec<u8>, timed: &TimedInput) {
    let TimedInput {
        input,
        claimed_at_ms,
        received_at_ms,
    } = timed;
    let Input {
        tick,
        seq,
        player,
        action,
    } = input;
    out.extend_from_slice(&tick.0.to_be_bytes());
    out.extend_from_slice(&seq.to_be_bytes());
    out.push(player.index() as u8);
    out.extend_from_slice(&claimed_at_ms.to_be_bytes());
    out.extend_from_slice(&received_at_ms.to_be_bytes());

    let before = out.len();
    match *action {
        Action::Idle => out.push(0),
        Action::Move(destination) => {
            out.push(1);
            write_point(out, destination);
        }
        Action::Skillshot(direction) => {
            out.push(2);
            write_point(out, direction);
        }
        Action::Targeted(target) => {
            out.push(3);
            out.extend_from_slice(&target.0.to_be_bytes());
        }
        Action::Attack(target) => {
            out.push(4);
            out.extend_from_slice(&target.0.to_be_bytes());
        }
    }
    // Fixed-width entries, so that the reader can bound the count against the
    // buffer before allocating and so that a file's size is a function of how
    // many inputs it holds rather than of which ones.
    out.resize(before + 9, 0);
}

fn write_point(out: &mut Vec<u8>, point: FxVec2) {
    out.extend_from_slice(&point.x.to_raw().to_be_bytes());
    out.extend_from_slice(&point.y.to_raw().to_be_bytes());
}

pub(crate) fn read_input(reader: &mut ByteReader<'_>) -> Option<TimedInput> {
    let tick = Tick(reader.u32()?);
    let seq = reader.u32()?;
    let player = Seat::from_index(reader.u8()?)?;
    let claimed_at_ms = reader.u64()?;
    let received_at_ms = reader.u64()?;

    let action_bytes = reader.array::<9>()?;
    let mut action_reader = ByteReader::new(&action_bytes);
    let action = match action_reader.u8()? {
        0 => Action::Idle,
        1 => Action::Move(read_point(&mut action_reader)?),
        2 => Action::Skillshot(read_point(&mut action_reader)?),
        3 => Action::Targeted(EntityId(action_reader.u16()?)),
        4 => Action::Attack(EntityId(action_reader.u16()?)),
        _ => return None,
    };
    // The action's own padding is checked for the reason `protocol` checks a
    // frame's: one input must have exactly one encoding, or the digest of a log
    // stops being a function of the log.
    if action_reader.rest().iter().any(|byte| *byte != 0) {
        return None;
    }

    Some(TimedInput {
        input: Input {
            tick,
            seq,
            player,
            action,
        },
        claimed_at_ms,
        received_at_ms,
    })
}

fn read_point(reader: &mut ByteReader<'_>) -> Option<FxVec2> {
    Some(FxVec2::new(
        Fx::from_raw(reader.i32()?),
        Fx::from_raw(reader.i32()?),
    ))
}

/// A cursor that never reads past the end.
#[derive(Debug)]
pub struct ByteReader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> ByteReader<'a> {
    /// A cursor at the start of these bytes.
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    /// How many bytes are left.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }

    /// Everything left, without consuming it.
    #[must_use]
    pub fn rest(&self) -> &'a [u8] {
        self.bytes.get(self.at..).unwrap_or(&[])
    }

    /// The next `count` bytes, or nothing.
    #[must_use]
    pub fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(count)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }

    /// The next `N` bytes as an array.
    #[must_use]
    pub fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    /// The next byte.
    #[must_use]
    pub fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }

    /// The next big-endian `u16`.
    #[must_use]
    pub fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.array::<2>()?))
    }

    /// The next big-endian `u32`.
    #[must_use]
    pub fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.array::<4>()?))
    }

    /// The next big-endian `u64`.
    #[must_use]
    pub fn u64(&mut self) -> Option<u64> {
        Some(u64::from_be_bytes(self.array::<8>()?))
    }

    /// The next big-endian `i32`.
    #[must_use]
    pub fn i32(&mut self) -> Option<i32> {
        Some(i32::from_be_bytes(self.array::<4>()?))
    }
}
