//! The recording a match leaves behind, and the offline resimulation of it.
//!
//! # What this is at M3, and what it is not
//!
//! `docs/MILESTONES.md` M3 asks for replay recording and for an exit criterion
//! that resimulates the recorded input log **in a separate process** and gets
//! the server's own final digest back. That is what is here: a container, a
//! reader that is total on hostile bytes, and [`resimulate`].
//!
//! What is deliberately absent is everything M5 owns, and the list is worth
//! writing down so that nothing here is mistaken for it. There is **no
//! signature**, so a recording proves that some server wrote this file and
//! nothing about who; `docs/RISKS.md` R4 is explicit that signing the log alone
//! would not be enough anyway, and the manifest it describes — match id, server
//! identity, participants, start time — is M5's object rather than a subset of
//! this one. There is **no `sim_version` and no commit**, so two builds that
//! reordered a rule resimulate to different digests with nothing in the file to
//! say which was right (`docs/RISKS.md` R13). A digest mismatch out of
//! [`resimulate`] at this milestone therefore means "these bytes do not
//! describe that match" and cannot yet distinguish tampering from a build
//! difference. M5 is where that distinction becomes a distinct error, and until
//! then a recording is a development artefact rather than evidence.
//!
//! What *is* here is `rules_hash`, and it is here rather than deferred because
//! its absence is a silent failure rather than a missing feature: a recording
//! resimulated under other constants does not error, it produces a different
//! match (`docs/RISKS.md` R2).
//!
//! # Why the log holds inputs and not frames
//!
//! A `TimedInput` names a seat, a tick, a sequence number and an action. It
//! names no projectile and no view. That matters because `protocol` gives every
//! recipient its own projectile handles: a log of what clients were *told*
//! would be a log in six mutually untranslatable namings, and resimulating it
//! would mean choosing one. A log of what clients *asked for* is in the one
//! namespace the server and `sim` share, which is why resimulation is a
//! function of it.
//!
//! For the same reason this crate depends on `sim` alone. `docs/ARCHITECTURE.md`
//! permits a dependency on `protocol` and it is deliberately not taken, exactly
//! as `sim`'s permission to use `serde` is not: nothing here reads a wire
//! format, and a dependency in the graph that nothing uses is a dependency
//! nobody is watching. It arrives at M5 with the manifest, which does record
//! the session that produced the file.

#![forbid(unsafe_code)]
#![deny(missing_docs, missing_debug_implementations)]

use sim::{Action, Digest, EntityId, Fx, FxVec2, Input, Seat, State, Tick, new_state, step};

/// The container format this build writes.
///
/// Not the protocol version and not `sim`'s: one is how two processes talk to
/// each other, one is how a file is laid out, one is what the rules do, and
/// they change for different reasons.
pub const FORMAT: u16 = 1;

/// What every recording starts with, so that a file that is not one is refused
/// by its first eight bytes rather than by an arithmetic error further in.
const MAGIC: [u8; 8] = *b"MOBAREPL";

/// One input, with both clocks.
///
/// `claimed_at_ms` is what the client said and is attacker-controlled by
/// definition; `received_at_ms` is what the server observed and is the only
/// clock in the system that is evidence of anything (`docs/SCOPE.md`, adversary
/// model). Both are recorded, neither is read by [`resimulate`], and the
/// divergence between them is exploit class 4's signal at M8.
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

/// A match, as the seed and the log that reproduce it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recording {
    /// The match seed.
    pub seed: u64,
    /// The constants the match was played under (`docs/RISKS.md` R2).
    pub rules_hash: Digest,
    /// How many ticks the server ran.
    pub ticks: u32,
    /// The digest the server ended on. What [`resimulate`] has to reproduce.
    pub final_state_digest: Digest,
    /// Every input the server accepted, in the order it applied them.
    pub inputs: Vec<TimedInput>,
}

/// Why a byte string is not a recording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The first eight bytes are not [`MAGIC`].
    NotARecording,
    /// A container format this build does not read.
    UnsupportedFormat(u16),
    /// The bytes ran out, or a field held a value that names nothing.
    Malformed,
    /// Bytes after the last input.
    TrailingBytes,
}

impl core::fmt::Display for ReadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotARecording => write!(f, "not a recording"),
            Self::UnsupportedFormat(found) => {
                write!(f, "container format {found}, this build reads {FORMAT}")
            }
            Self::Malformed => write!(f, "malformed recording"),
            Self::TrailingBytes => write!(f, "trailing bytes after the last input"),
        }
    }
}

impl core::error::Error for ReadError {}

/// Why a recording did not resimulate to what it claims.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyError {
    /// The recording was made under other constants, so resimulating it here
    /// would produce a different match rather than a different digest.
    ///
    /// Its own case rather than a digest mismatch because the two have
    /// different answers, and a verifier that conflates them teaches its reader
    /// to distrust the loud one (`docs/RISKS.md` R2).
    RulesHash {
        /// What the recording was made under.
        recorded: Digest,
        /// What this build plays by.
        local: Digest,
    },
    /// The log does not reproduce the digest the recording claims.
    ///
    /// At M3 this cannot yet distinguish a tampered log from a build that
    /// resolves a tick differently; see the module documentation and
    /// `docs/RISKS.md` R13.
    Digest {
        /// What the recording claims.
        claimed: Digest,
        /// What resimulating produced.
        computed: Digest,
    },
}

impl core::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::RulesHash { .. } => write!(f, "recorded under other constants"),
            Self::Digest { .. } => write!(f, "the log does not reproduce the claimed digest"),
        }
    }
}

impl core::error::Error for VerifyError {}

/// Replays the log through `sim` and returns the digest it ends on.
///
/// The one function this crate exists for at M3, and it is deliberately as thin
/// as it looks: the same `step` the server ran, over the same inputs, from the
/// same seed. Anything cleverer would be a second simulation to keep in
/// agreement with the first, which is the whole reason `sim` has no workspace
/// dependencies.
///
/// # Errors
///
/// [`VerifyError::RulesHash`] if the recording was made under other constants,
/// and [`VerifyError::Digest`] if the log does not reproduce what the recording
/// claims.
pub fn resimulate(recording: &Recording) -> Result<Digest, VerifyError> {
    let local = sim::rules_hash();
    if recording.rules_hash != local {
        return Err(VerifyError::RulesHash {
            recorded: recording.rules_hash,
            local,
        });
    }

    let computed = run(recording).digest();
    if computed == recording.final_state_digest {
        Ok(computed)
    } else {
        Err(VerifyError::Digest {
            claimed: recording.final_state_digest,
            computed,
        })
    }
}

/// The state the log reaches, without checking anything about it.
///
/// Inputs are bucketed by their own `tick` field, which is authoritative:
/// `step` ignores an input whose tick is not the state's, so a log fed in bulk
/// would mostly be discarded. Bucketing here rather than trusting the log's
/// order means a reordered log resimulates to the same digest as an ordered one
/// *within a tick's slice only* — across ticks the order is the tick field, and
/// `sim`'s `input_log_digest` is what makes the order of the log itself part of
/// its identity.
fn run(recording: &Recording) -> State {
    let mut buckets: Vec<Vec<Input>> = vec![Vec::new(); recording.ticks as usize];
    for timed in &recording.inputs {
        if let Some(bucket) = buckets.get_mut(timed.input.tick.0 as usize) {
            bucket.push(timed.input);
        }
    }

    let mut state = new_state(recording.seed);
    for bucket in &buckets {
        state = step(&state, bucket);
    }
    state
}

// ---------------------------------------------------------------------------
// The container
// ---------------------------------------------------------------------------

impl Recording {
    /// The recording's bytes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64 + self.inputs.len() * 32);
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&FORMAT.to_be_bytes());
        out.extend_from_slice(&self.seed.to_be_bytes());
        out.extend_from_slice(self.rules_hash.as_bytes());
        out.extend_from_slice(&self.ticks.to_be_bytes());
        out.extend_from_slice(self.final_state_digest.as_bytes());
        out.extend_from_slice(&(self.inputs.len() as u64).to_be_bytes());
        for timed in &self.inputs {
            write_input(&mut out, timed);
        }
        out
    }

    /// Reads a recording.
    ///
    /// Total on every byte string, for the same reason `protocol`'s decoders
    /// are: a replay file is something a third party hands you, and M5's whole
    /// subject is what happens when they hand you a tampered one.
    ///
    /// # Errors
    ///
    /// [`ReadError`] for anything that is not exactly one well-formed
    /// recording, including trailing bytes after the last input.
    pub fn decode(bytes: &[u8]) -> Result<Self, ReadError> {
        let mut reader = ByteReader::new(bytes);
        if reader.array::<8>().ok_or(ReadError::Malformed)? != MAGIC {
            return Err(ReadError::NotARecording);
        }
        let format = reader.u16().ok_or(ReadError::Malformed)?;
        if format != FORMAT {
            return Err(ReadError::UnsupportedFormat(format));
        }
        let seed = reader.u64().ok_or(ReadError::Malformed)?;
        let rules_hash = Digest::from_bytes(reader.array::<32>().ok_or(ReadError::Malformed)?);
        let ticks = reader.u32().ok_or(ReadError::Malformed)?;
        let final_state_digest =
            Digest::from_bytes(reader.array::<32>().ok_or(ReadError::Malformed)?);

        let count = reader.u64().ok_or(ReadError::Malformed)?;
        // Bounded against what is left in the buffer before anything is
        // allocated for it: a header claiming four billion inputs is malformed,
        // not large, and reserving for it first would make that a memory
        // exhaustion rather than an error.
        let count = usize::try_from(count).map_err(|_| ReadError::Malformed)?;
        if count.saturating_mul(INPUT_BYTES) > reader.remaining() {
            return Err(ReadError::Malformed);
        }
        let mut inputs = Vec::with_capacity(count);
        for _ in 0..count {
            inputs.push(read_input(&mut reader).ok_or(ReadError::Malformed)?);
        }
        if reader.remaining() != 0 {
            return Err(ReadError::TrailingBytes);
        }

        Ok(Self {
            seed,
            rules_hash,
            ticks,
            final_state_digest,
            inputs,
        })
    }
}

/// A `TimedInput` on the wire: tick, seq, seat, both clocks, and an action
/// padded to its widest variant so that every entry is the same width.
const INPUT_BYTES: usize = 4 + 4 + 1 + 8 + 8 + 9;

fn write_input(out: &mut Vec<u8>, timed: &TimedInput) {
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

fn read_input(reader: &mut ByteReader<'_>) -> Option<TimedInput> {
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
struct ByteReader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> ByteReader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }

    fn rest(&self) -> &'a [u8] {
        self.bytes.get(self.at..).unwrap_or(&[])
    }

    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(count)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.array::<2>()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.array::<4>()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_be_bytes(self.array::<8>()?))
    }

    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_be_bytes(self.array::<4>()?))
    }
}
