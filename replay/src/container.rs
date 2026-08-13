//! The replay file, sealing it, and verifying it.
//!
//! # What resimulation establishes here, and what it does not
//!
//! `docs/SCOPE.md` is explicit and this module must not read as though it had
//! forgotten: **resimulating the inputs of a fully authoritative server proves
//! that the server did not corrupt itself. It does not catch a cheating
//! client.** Every input in the log is one the server accepted, stamped with the
//! tick the server chose and the seat the session named; a client that aimed
//! with a script sent inputs that are in that log and that resimulate perfectly.
//!
//! What resimulation is evidence about is the surface where a **client-supplied
//! artefact asserts an outcome**, and that surface is exactly this file format.
//! Somebody hands you a replay saying "Blue won this match, here is the log".
//! Verifying it establishes four things and it is worth being precise about
//! each:
//!
//! 1. **A key in your registry sealed these bytes.** Not that the match was
//!    played fairly, and not that the server was honest — that the manifest is
//!    one your own build's key committed to. This is the whole of the
//!    provenance claim, and `docs/RISKS.md` R13 says the rest: a version and a
//!    commit order *this project's own builds* and are not evidence against an
//!    attacker who controls the build.
//! 2. **The log is the log the manifest names**, in order and in full. Not a
//!    subset, not a permutation, not a longer one.
//! 3. **The log, run through this build, reaches the digest and the outcome the
//!    manifest claims.** So a manifest asserting a result its own log does not
//!    produce is refused, which is the only thing standing between a signed
//!    replay and a signed lie about one.
//! 4. **The build that verifies is the build the match was played under**, or
//!    the verification stops with a distinct error rather than a digest
//!    mismatch.
//!
//! And what it does not establish, stated at the same volume:
//!
//! - **Nothing about the player.** A bot's inputs resimulate exactly.
//!   `docs/MILESTONES.md` M8's detectors read the same log for a different
//!   question, and they are a different milestone with a different standard of
//!   evidence.
//! - **Nothing against a signer.** An attacker holding a key in the registry can
//!   seal a replay of a match that never happened by simulating it themselves —
//!   the log would be internally consistent, because they produced it that way.
//!   The escalation below is the honest account of that.
//! - **Nothing this build cannot reproduce.** Verification runs `sim`, the same
//!   `sim` the server ran. That is the comparability question and it has its own
//!   heading.
//!
//! # The escalation, and why each check has its own error
//!
//! `docs/MILESTONES.md` M5's criterion asks for six tamper cases each rejected
//! with a distinct error. The reason distinct errors are possible at all is that
//! the checks below run **in order** and each one catches the attacker who
//! stopped one step short of the next:
//!
//! | The attacker | Caught by | Error |
//! | --- | --- | --- |
//! | Edits anything and cannot re-sign | the signature | [`VerifyError::Signature`] |
//! | Signs with a key of their own | the registry | [`VerifyError::UnknownKey`] |
//! | Re-signs, but the manifest is from another set of constants | the rules hash | [`VerifyError::RulesHash`] |
//! | Re-signs, but from another build of the same constants | the version | [`VerifyError::SimVersion`] |
//! | Re-signs a shortened log without adjusting its count | the count | [`VerifyError::Truncated`] |
//! | Re-signs a reordered or edited log without adjusting its digest | the log digest | [`VerifyError::InputLog`] |
//! | Re-signs a changed seed, or an adjusted log digest | the resimulation | [`VerifyError::FinalDigest`] |
//! | Re-signs a changed result and nothing else | the resimulation, again | [`VerifyError::Outcome`] |
//!
//! **And the last row of that table is the one this module cannot write.** An
//! attacker who holds a key the registry accepts, and who adjusts *every* field
//! consistently, has not tampered with a replay: they have produced a replay of
//! a different match, simulated honestly, and there is nothing in the bytes to
//! distinguish it from one that was played. That is not a hole in the format, it
//! is the boundary of what a signature over a self-consistent artefact can mean,
//! and the thing on the other side of it is key custody rather than
//! verification. `docs/RISKS.md` R4 is where the key half lives.
//!
//! # The comparability trap, named because this project has fallen into it
//!
//! Resimulation compares `sim` against `sim`. `docs/MILESTONES.md` M2 records
//! the same shape of mistake in the encoded-size bound, which re-executed the
//! same function on both sides and could not be made to fail; M2's visibility
//! test avoids it by re-deriving the visibility predicate rather than calling
//! `sim`'s.
//!
//! There is no equivalent move available here and pretending otherwise would be
//! worse than saying so: a second implementation of `step` is precisely what
//! `docs/ARCHITECTURE.md` refuses, because the same `step` running in the
//! server, the verifier, the determinism suite and eventually the RL environment
//! is the property the whole project rests on. So the honest statement of what
//! the comparison establishes is narrower than "the match was played correctly":
//!
//! - **It is a check on the recording, not on the rules.** A mutation inside
//!   `step` reddens nothing here, because both sides move together. A mutation
//!   in `Match::recording`, in the tick a log entry is stamped with, in the
//!   order the log is written, or in this module's own encoding reddens it
//!   immediately, because only one side moves.
//! - **What covers the rules is the tri-platform fixture**, which compares
//!   `step` against digests committed in this repository rather than against
//!   itself, and `replay/tests/sealed.rs`, which does the same for a sealed
//!   replay's bytes on all three targets.

use sim::{Digest, Outcome, State, input_log_digest, new_state, step};

use crate::keys::{KeyRegistry, KeyStatus, Signature, SigningKey, VerifyingKey};
use crate::manifest::{Build, Manifest, SessionFacts};
use crate::{ByteReader, INPUT_BYTES, Recording, TimedInput, read_input, write_input};

/// What every replay starts with, so that a file that is not one is refused by
/// its first eight bytes rather than by an arithmetic error further in.
///
/// Deliberately **not** the M3 recording's `MOBAREPL`. That container had no
/// signature, no identity and no version stamp, and a build that read one as if
/// it were this would be reading a development artefact as evidence.
const MAGIC: [u8; 8] = *b"MOBARPLY";

/// The replay container format this build writes.
///
/// Not the protocol version, not the container the M3 recording used, and not
/// `sim`'s: one is how two processes talk, one is how a file is laid out, one is
/// what the rules do, and they change for different reasons. It starts at 1
/// because this is the first format anybody keeps.
pub const FORMAT: u16 = 1;

/// A match, sealed.
///
/// The manifest and the signature over it, and the input log the manifest's
/// digest covers. Nothing else: no events, no frames, no snapshots — see
/// [`crate::manifest`] for the field-by-field account and for the absences.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replay {
    /// The signed half.
    pub manifest: Manifest,
    /// The signature over [`Replay::signed_bytes`].
    pub signature: Signature,
    /// Every input the server accepted, in the order it applied them.
    pub inputs: Vec<TimedInput>,
}

/// Seals a recording into a replay.
///
/// The signing happens **here** rather than inside `server::Match`, and the
/// placement is a decision. The authority has no clock, no socket and no
/// identity — that is what makes it a function of its inputs and what every
/// traffic-shape property is stated over — so a key inside it would be the first
/// secret in the one component that is supposed to have none. What signs is
/// whoever holds the key, which is the operator, and the facts the authority
/// cannot know arrive beside the recording in [`SessionFacts`].
#[must_use]
pub fn seal(recording: &Recording, session: &SessionFacts, key: &SigningKey) -> Replay {
    let log: Vec<sim::Input> = recording.inputs.iter().map(|timed| timed.input).collect();
    let manifest = Manifest {
        match_id: session.match_id,
        server_identity: key.verifying(),
        seed: recording.seed,
        rules_hash: recording.rules_hash,
        sim_version: sim::VERSION,
        sim_commit: session.sim_commit,
        started_at_unix_ms: session.started_at_unix_ms,
        participants: session.participants.clone(),
        ticks: recording.ticks,
        inputs: recording.inputs.len() as u64,
        input_log_digest: input_log_digest(&log),
        outcome: recording.outcome,
        final_state_digest: recording.final_state_digest,
    };
    let signature = key.sign(&signed_bytes(&manifest));
    Replay {
        manifest,
        signature,
        inputs: recording.inputs.clone(),
    }
}

/// The bytes a signature covers: the magic, the format, and the manifest.
///
/// The magic and the format are inside the signature rather than only in the
/// file header, so that a replay cannot be re-labelled as another format's file
/// and re-parsed under different rules while keeping a signature that verifies.
///
/// Public, deliberately. "What is signed" is the question `docs/MILESTONES.md`
/// M5 asks to be decided and documented, and a project whose answer lives only
/// inside a private function has documented it in the weakest available place. A
/// third party writing their own verifier needs these bytes; so does the tamper
/// suite, which has to be able to re-sign a manifest **verbatim** in order to
/// play the attacker who holds a key — `seal` recomputes the derived fields and
/// would quietly repair the tamper it was handed.
#[must_use]
pub fn signed_bytes(manifest: &Manifest) -> Vec<u8> {
    let mut out = Vec::with_capacity(crate::manifest::MANIFEST_MIN_BYTES + 10);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&FORMAT.to_be_bytes());
    out.extend_from_slice(&manifest.encode());
    out
}

/// What a verified replay establishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verified {
    /// The digest resimulation reached, which is the manifest's.
    pub final_state_digest: Digest,
    /// The outcome resimulation reached, which is the manifest's.
    pub outcome: Outcome,
    /// Ticks resimulated.
    pub ticks: u32,
    /// The key that sealed it.
    pub signer: VerifyingKey,
    /// Whether that key has been retired. Reported and not acted on: a retired
    /// key still verifies, or rotation would destroy evidence
    /// (`docs/RISKS.md` R4).
    pub retired: bool,
}

/// Why a replay is not what it says it is.
///
/// Each variant is a *different check*, in the order [`verify`] runs them, and
/// the ordering is what makes `docs/MILESTONES.md` M5's six tamper cases six
/// distinct answers rather than one. A verifier that conflated them would teach
/// its reader to distrust the loud one — "this replay is from another build" and
/// "this replay was tampered with" are different sentences with different
/// responses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerifyError {
    /// The manifest names an identity this registry does not hold.
    UnknownKey(VerifyingKey),
    /// The signature is not one that key made over these bytes.
    Signature,
    /// The recording was made under other constants, so resimulating it here
    /// would produce a different match rather than a different digest
    /// (`docs/RISKS.md` R2).
    RulesHash {
        /// What the replay was sealed under.
        recorded: Digest,
        /// What this build plays by.
        local: Digest,
    },
    /// The same constants, another build of the code that reads them
    /// (`docs/RISKS.md` R13).
    SimVersion {
        /// What the replay was sealed under.
        recorded: [u16; 3],
        /// What this build is.
        local: [u16; 3],
    },
    /// The log holds a different number of inputs than the manifest says.
    Truncated {
        /// What the manifest claims.
        claimed: u64,
        /// What the file holds.
        found: u64,
    },
    /// The log is not the log the manifest names: reordered, or edited.
    InputLog {
        /// What the manifest claims.
        claimed: Digest,
        /// What the log hashes to.
        computed: Digest,
    },
    /// Resimulating the log does not reach the state the manifest claims.
    FinalDigest {
        /// What the manifest claims.
        claimed: Digest,
        /// What resimulating produced.
        computed: Digest,
    },
    /// Resimulating the log reaches the right state and a different result.
    ///
    /// Its own case rather than part of the digest, because the outcome is the
    /// claim a replay is *submitted* to make — exploit class 2 is result
    /// forgery — and a forger who edits it and nothing else deserves an answer
    /// that names what they edited.
    Outcome {
        /// What the manifest claims.
        claimed: Outcome,
        /// What resimulating produced.
        computed: Outcome,
    },
}

impl core::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownKey(key) => write!(f, "sealed by {key}, which is not a key I accept"),
            Self::Signature => write!(f, "the signature is not that key's over these bytes"),
            Self::RulesHash { .. } => write!(f, "recorded under other constants"),
            Self::SimVersion { recorded, local } => write!(
                f,
                "recorded by sim {}.{}.{}, this build is {}.{}.{}",
                recorded[0], recorded[1], recorded[2], local[0], local[1], local[2]
            ),
            Self::Truncated { claimed, found } => {
                write!(
                    f,
                    "the manifest names {claimed} inputs, the log holds {found}"
                )
            }
            Self::InputLog { .. } => write!(f, "the log is not the log the manifest names"),
            Self::FinalDigest { .. } => {
                write!(f, "the log does not reproduce the state it claims")
            }
            Self::Outcome { claimed, computed } => {
                write!(
                    f,
                    "the manifest claims {claimed:?}, the log produces {computed:?}"
                )
            }
        }
    }
}

impl core::error::Error for VerifyError {}

/// Verifies a replay, and returns what it establishes.
///
/// The order of the checks is the substance; see this module's header for what
/// each one catches and for the attacker the last one cannot.
///
/// # Errors
///
/// [`VerifyError`], one variant per check, in the order they run.
pub fn verify(replay: &Replay, keys: &KeyRegistry, build: &Build) -> Result<Verified, VerifyError> {
    // 1. Provenance, before anything else. A replay from a key nobody accepts
    //    is not a replay whose contents are worth an opinion, and checking the
    //    contents first would be spending a resimulation on a stranger's file.
    let identity = replay.manifest.server_identity;
    let entry = keys
        .find(identity)
        .ok_or(VerifyError::UnknownKey(identity))?;

    // 2. The signature, over the magic, the format and the manifest.
    if !identity.verifies(&signed_bytes(&replay.manifest), &replay.signature) {
        return Err(VerifyError::Signature);
    }

    // 3 and 4. Is this build the build? Two questions, two answers, because
    //          `rules_hash` covers the constants and the version covers the code
    //          that reads them (`docs/RISKS.md` R2 and R13).
    if replay.manifest.rules_hash != build.rules_hash {
        return Err(VerifyError::RulesHash {
            recorded: replay.manifest.rules_hash,
            local: build.rules_hash,
        });
    }
    if replay.manifest.sim_version != build.sim_version {
        return Err(VerifyError::SimVersion {
            recorded: replay.manifest.sim_version,
            local: build.sim_version,
        });
    }

    // 5. The log is the length the manifest says. Checked before its digest so
    //    that a shortened log is reported as shortened rather than as different:
    //    both are true and only one of them is useful.
    let found = replay.inputs.len() as u64;
    if found != replay.manifest.inputs {
        return Err(VerifyError::Truncated {
            claimed: replay.manifest.inputs,
            found,
        });
    }

    // 6. …and it is the log the manifest names, in order. `sim::input_log_digest`
    //    is a function of the sequence, because `step` neither sorts nor
    //    deduplicates, so a permutation is a different log.
    let log: Vec<sim::Input> = replay.inputs.iter().map(|timed| timed.input).collect();
    let computed = input_log_digest(&log);
    if computed != replay.manifest.input_log_digest {
        return Err(VerifyError::InputLog {
            claimed: replay.manifest.input_log_digest,
            computed,
        });
    }

    // 7 and 8. The only two claims left are about what the log *does*, and the
    //          resimulation answers both. The digest first, because a changed
    //          seed moves both and the more specific answer is the state.
    let reached = resimulate(replay.manifest.seed, replay.manifest.ticks, &replay.inputs);
    let digest = reached.digest();
    if digest != replay.manifest.final_state_digest {
        return Err(VerifyError::FinalDigest {
            claimed: replay.manifest.final_state_digest,
            computed: digest,
        });
    }
    if reached.outcome() != replay.manifest.outcome {
        return Err(VerifyError::Outcome {
            claimed: replay.manifest.outcome,
            computed: reached.outcome(),
        });
    }

    Ok(Verified {
        final_state_digest: digest,
        outcome: reached.outcome(),
        ticks: replay.manifest.ticks,
        signer: identity,
        retired: entry.status == KeyStatus::Retired,
    })
}

/// The state a log reaches, without checking anything about it.
///
/// Inputs are bucketed by their own `tick` field, which is authoritative: `step`
/// ignores an input whose tick is not the state's, so a log fed in bulk would
/// mostly be discarded. Bucketing here rather than trusting the file's order
/// means the order *within* a tick's slice is the file's and the order across
/// ticks is the tick field — and `input_log_digest`, checked above, is what
/// makes the file's order part of its identity either way.
#[must_use]
pub fn resimulate(seed: u64, ticks: u32, inputs: &[TimedInput]) -> State {
    let mut buckets: Vec<Vec<sim::Input>> = vec![Vec::new(); ticks as usize];
    for timed in inputs {
        if let Some(bucket) = buckets.get_mut(timed.input.tick.0 as usize) {
            bucket.push(timed.input);
        }
    }
    let mut state = new_state(seed);
    for bucket in &buckets {
        state = step(&state, bucket);
    }
    state
}

// ---------------------------------------------------------------------------
// The container
// ---------------------------------------------------------------------------

/// Why a byte string is not a replay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The first eight bytes are not [`MAGIC`].
    NotAReplay,
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
            Self::NotAReplay => write!(f, "not a replay"),
            Self::UnsupportedFormat(found) => {
                write!(f, "container format {found}, this build reads {FORMAT}")
            }
            Self::Malformed => write!(f, "malformed replay"),
            Self::TrailingBytes => write!(f, "trailing bytes after the last input"),
        }
    }
}

impl core::error::Error for ReadError {}

impl Replay {
    /// The replay's bytes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = signed_bytes(&self.manifest);
        out.extend_from_slice(self.signature.as_bytes());
        out.extend_from_slice(&(self.inputs.len() as u64).to_be_bytes());
        for timed in &self.inputs {
            write_input(&mut out, timed);
        }
        out
    }

    /// Reads a replay.
    ///
    /// Total on every byte string. Note what this does **not** do: it does not
    /// check the signature, the constants, the version or the log. A replay that
    /// decodes is a replay whose bytes are laid out correctly and nothing more,
    /// and every claim about it comes from [`verify`]. The split is deliberate —
    /// a reader that verified would be a reader whose failures a caller could
    /// not tell apart from a corrupt download.
    ///
    /// # Errors
    ///
    /// [`ReadError`] for anything that is not exactly one well-formed replay,
    /// including trailing bytes after the last input.
    pub fn decode(bytes: &[u8]) -> Result<Self, ReadError> {
        let mut reader = ByteReader::new(bytes);
        if reader.array::<8>().ok_or(ReadError::Malformed)? != MAGIC {
            return Err(ReadError::NotAReplay);
        }
        let format = reader.u16().ok_or(ReadError::Malformed)?;
        if format != FORMAT {
            return Err(ReadError::UnsupportedFormat(format));
        }
        let manifest = Manifest::decode(&mut reader).ok_or(ReadError::Malformed)?;
        let signature = Signature::from_bytes(reader.array::<64>().ok_or(ReadError::Malformed)?);

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
            manifest,
            signature,
            inputs,
        })
    }
}
