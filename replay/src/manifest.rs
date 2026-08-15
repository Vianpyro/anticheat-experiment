//! The manifest: the thing that gets signed, field by field.
//!
//! `docs/MILESTONES.md` M5 is the milestone that freezes a record format, and
//! the sentence that governs every decision in this file is that **whatever is
//! missing here is missing from the whole corpus**. A field left out can be
//! added later only by a format version, and every replay recorded before that
//! version does not have it — so the cost of an absent field is not "add it
//! next month", it is "the matches you already collected cannot answer this".
//!
//! # What is signed, and why it is this and not the log
//!
//! `docs/RISKS.md` R4 states the failure modes and this is the answer to all of
//! them at once: **the signature covers the manifest, and the manifest covers
//! the log by carrying its digest.** Signing the log alone would let a genuine
//! log be resubmitted under another match identity; omitting the server
//! identity would let any party mint replays; omitting a per-match identifier
//! would make a replay indistinguishable from its own copy. So the identity, the
//! match id, the seed, the constants, the build, the participants, the log's
//! digest and the outcome are all inside one signature, and the log rides
//! outside it with its digest inside.
//!
//! # Field by field, and the absences are decisions
//!
//! | Field | Why it is here |
//! | --- | --- |
//! | `match_id` | A replay has to be distinguishable from a copy of itself submitted as a second match. Sixteen bytes, chosen by whoever runs the match |
//! | `server_identity` | Without it the signature says only "somebody signed this". It is the field that turns a replay into evidence *from* somebody (`docs/RISKS.md` R4) |
//! | `seed` | The whole initial world. Resimulation cannot start without it and altering it is the cheapest way to change what a log means |
//! | `rules_hash` | The constants the match was played under (`docs/RISKS.md` R2). Its absence is a silent failure rather than a missing feature: a replay resimulated under other constants does not error, it produces a different match |
//! | `sim_version` | The code that reads those constants (`docs/RISKS.md` R13). Two builds can agree on every constant and resolve a tick differently |
//! | `sim_commit` | The version says *something* changed; only the commit says *what*, which is the difference between a verification failure you can bisect and one you can only file |
//! | `started_at_unix_ms` | The retention clock in `docs/CONSENT.md` runs from it, and a corpus that cannot say when a match was recorded cannot honour a retention date |
//! | `participants` | Per seat, so that a withdrawal can find the matches a person played in without a second index to keep in step. The pseudonym and nothing else — the mapping to a person lives outside the corpus |
//! | `ticks` | How long the match ran, which is what bounds resimulation. A verifier that took this from the log would take it from the part an attacker can shorten |
//! | `inputs` | How many inputs the log holds. This is what makes a truncated log a *distinct* error rather than a digest mismatch |
//! | `input_log_digest` | The log, in one field inside the signature. Order is part of it, because `step` neither sorts nor deduplicates |
//! | `outcome` | **The claim a replay actually makes.** Exploit class 2 is result forgery, and what a forger wants to assert is that they won. It is independently checkable by resimulation, which is what gives the check its own error |
//! | `final_state_digest` | What resimulating the log has to reproduce |
//! | `telemetry` | **The digest of the device-telemetry companion, or the named absence of one.** See [`Commitment`] |
//!
//! And what is deliberately **not** in it:
//!
//! - **No events, and no frames.** A recording carries the seed and the input
//!   log; resimulation derives the events by running the same `step` the server
//!   ran. `server/tests/produced_not_delivered.rs` is the demonstration that
//!   this matters — the delivered sequence and the produced sequence differ on
//!   every busy tick — and there is no field here for delivery order to get
//!   into.
//! - **No per-tick telemetry, and no device stream.** `docs/MILESTONES.md` M6
//!   wants a client-claimed and a server-observed timestamp for every input, and
//!   both are in the log's `TimedInput`, one per input. What is *not* here is
//!   anything at a higher rate than that: `sim` consumes one intention per tick
//!   at 30 Hz and that invariant does not move, so the stream of raw device
//!   deltas is a **separate sealed file** and this manifest carries its
//!   **digest** rather than its contents (`crate::telemetry`). Putting the
//!   stream itself here would have made the manifest a telemetry container and
//!   the input log a subset of it, and the resimulation could no longer be a
//!   function of the file.
//! - **No player identity beyond the pseudonym.** `docs/RISKS.md` R3: the
//!   mapping is held outside the corpus, and a replay that carried a name would
//!   make every published replay a disclosure.
//! - **No score, no statistics, no derived summary.** Everything of that shape
//!   is a function of the resimulation, and a field that restates a derivable
//!   fact is a field that can disagree with it.
//! - **No client version.** The client is assumed compromised
//!   (`docs/SCOPE.md`), so a version it reported would be a field an attacker
//!   writes. What matters is the build that *resolved* the match, which is the
//!   server's, and that is `sim_version` and `sim_commit`.

use sim::{Digest, Outcome, PLAYER_COUNT, Tick, rules_hash};

use crate::keys::VerifyingKey;

/// A match's identifier. Sixteen bytes, opaque.
///
/// Not a UUID type, because a dependency for a sixteen-byte array is a
/// dependency for a sixteen-byte array. It is rendered as hex rather than in
/// UUID's grouped form, so that nothing about it invites a reader to think a
/// version or a variant nibble means something here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MatchId(pub [u8; 16]);

impl core::fmt::Display for MatchId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl MatchId {
    /// The identifier these hex characters name, or `None`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if text.len() != 32 {
            return None;
        }
        let mut out = [0u8; 16];
        for (index, slot) in out.iter_mut().enumerate() {
            let at = index.checked_mul(2)?;
            *slot = u8::from_str_radix(text.get(at..at.checked_add(2)?)?, 16).ok()?;
        }
        Some(Self(out))
    }
}

/// The longest a pseudonym may be, in bytes.
pub const MAX_PSEUDONYM_BYTES: usize = 32;

/// One participant, as the corpus knows them.
///
/// Constrained to `A-Z a-z 0-9 _ -` and to [`MAX_PSEUDONYM_BYTES`], and the
/// constraint is not tidiness. Two things rest on it. The corpus audit
/// (`Corpus::audit`) finds a withdrawn participant by searching every byte of
/// every file for their pseudonym, and a pseudonym that could contain a space, a
/// newline or a path separator would be a pseudonym that could be split across a
/// line or collide with an unrelated string. And a free-form string is a place a
/// real name ends up by accident, which `docs/RISKS.md` R3 is the reason not to
/// allow.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pseudonym(String);

impl Pseudonym {
    /// The pseudonym this text names, or `None` if it is not one.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if text.is_empty() || text.len() > MAX_PSEUDONYM_BYTES {
            return None;
        }
        if !text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            return None;
        }
        Some(Self(text.to_owned()))
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for Pseudonym {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The commit a build came from, or the admission that it is not known.
///
/// `docs/RISKS.md` R13: the version is a claim and the commit is a fact. It is
/// allowed to be absent because a locally built server is a real case and a
/// manifest that lies about its provenance is worse than one that admits it —
/// `Unknown` is a smaller claim than a commit that is not the one the binary was
/// built from, and `Dirty` is smaller still than a clean commit that does not
/// describe the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimCommit {
    /// Built from this commit, with no uncommitted changes.
    Sha([u8; 20]),
    /// Built from this commit, with a working tree that differed from it.
    Dirty([u8; 20]),
    /// Built somewhere with no git history to ask.
    Unknown,
}

impl SimCommit {
    /// What this build came from, as `build.rs` observed it.
    ///
    /// Read at compile time from an environment variable the build script sets,
    /// so a server binary carries the commit it was built from rather than
    /// whatever the machine it runs on happens to have checked out.
    #[must_use]
    pub fn of_this_build() -> Self {
        let Some(text) = option_env!("MOBA_SIM_COMMIT") else {
            return Self::Unknown;
        };
        let (hex, dirty) = match text.strip_suffix("-dirty") {
            Some(hex) => (hex, true),
            None => (text, false),
        };
        let Some(bytes) = unhex20(hex) else {
            return Self::Unknown;
        };
        if dirty {
            Self::Dirty(bytes)
        } else {
            Self::Sha(bytes)
        }
    }
}

impl core::fmt::Display for SimCommit {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Sha(bytes) | Self::Dirty(bytes) => {
                for byte in bytes {
                    write!(f, "{byte:02x}")?;
                }
                if matches!(self, Self::Dirty(_)) {
                    write!(f, "-dirty")?;
                }
                Ok(())
            }
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

fn unhex20(text: &str) -> Option<[u8; 20]> {
    if text.len() != 40 {
        return None;
    }
    let mut out = [0u8; 20];
    for (index, slot) in out.iter_mut().enumerate() {
        let at = index.checked_mul(2)?;
        *slot = u8::from_str_radix(text.get(at..at.checked_add(2)?)?, 16).ok()?;
    }
    Some(out)
}

/// What a verifier is, so that verification can say what it disagrees with.
///
/// A parameter rather than a pair of ambient constants, and the reason is that
/// "this replay is from another build" has to be *testable* without changing the
/// build under test. `Build::current()` is what the tool uses and what every
/// caller in this workspace passes except two: the tamper suite, which
/// constructs a mismatch, and the cross-platform fixture, whose `sim_version` is
/// a constant of the fixture rather than of whoever is running it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Build {
    /// The constants this build plays by.
    pub rules_hash: Digest,
    /// The `sim` version this build was compiled from.
    pub sim_version: [u16; 3],
}

impl Build {
    /// What the build running this code is.
    #[must_use]
    pub fn current() -> Self {
        Self {
            rules_hash: rules_hash(),
            sim_version: sim::VERSION,
        }
    }
}

/// Whether a match recorded device telemetry, and if so which bytes are its.
///
/// # Why the digest and not the stream
///
/// `crate::telemetry` carries the reasoning at length and the short form belongs
/// here, because this is the field a reader meets first. The companion holds the
/// device-event stream at its native 125 Hz to 1 kHz; a replay holds one
/// intention per tick at 30 Hz. Keeping the two in one file would make a
/// resimulation a function of a stream no rule reads, which is the invariant M5
/// froze. Keeping them in two files with nothing between them would make a
/// companion substitutable — and a smoothed trajectory sealed by a key the
/// registry accepts is exactly the artefact exploit class 2 is about, one level
/// down.
///
/// So the manifest names thirty-two bytes and nothing else. A replay verifies
/// with or without the companion beside it; the companion verifies only against
/// the replay that named it.
///
/// # Why absence is a variant rather than a missing file
///
/// **A replay with no companion is a complete replay**, not a damaged one. A
/// development run, a match nobody was recording, a session whose parts never
/// arrived: all of them are matches, and a verifier that treated them as errors
/// would be teaching its reader to ignore the error. So the state is named, it is
/// inside the signature like everything else, and `verify` reports it in those
/// words.
///
/// The consequence that gives it teeth: because the absence is *signed*,
/// attaching a companion afterwards to a replay that recorded `Absent` is a
/// refusal ([`crate::telemetry::TelemetryError::NotCommitted`]) rather than an
/// upgrade.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Commitment {
    /// This match recorded no device telemetry, and that is the whole of it.
    Absent,
    /// The companion file whose bytes hash to this digest, and no other.
    Sealed(Digest),
}

impl Commitment {
    /// The digest committed to, if there is one.
    #[must_use]
    pub const fn digest(&self) -> Option<Digest> {
        match self {
            Self::Absent => None,
            Self::Sealed(digest) => Some(*digest),
        }
    }
}

impl core::fmt::Display for Commitment {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Absent => write!(f, "none"),
            Self::Sealed(digest) => write!(f, "{digest}"),
        }
    }
}

/// What a match's session knows and its authority does not.
///
/// `Match` has no clock, no socket and no identity — that is what makes it a
/// function of its inputs and what the whole traffic-shape argument rests on —
/// so the fields here are supplied by whoever is operating the match. They are
/// separated from [`crate::Recording`] rather than folded into it precisely so
/// that the authority cannot acquire them by accident.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionFacts {
    /// This match's identifier.
    pub match_id: MatchId,
    /// When it started, in milliseconds since the Unix epoch.
    pub started_at_unix_ms: u64,
    /// Who sat in each seat, where anybody did.
    pub participants: [Option<Pseudonym>; PLAYER_COUNT],
    /// The commit the server was built from.
    pub sim_commit: SimCommit,
    /// The telemetry companion this match produced, or its named absence.
    ///
    /// A session fact rather than an authority fact for the same reason the
    /// participants are: `Match` has no clock, no socket and no identity, and the
    /// device streams are the clients'. Whoever seals resolves the companion
    /// first and hands the digest here, which is what fixes the order — the
    /// companion is sealed, then the replay commits to it, and never the reverse.
    pub telemetry: Commitment,
}

impl SessionFacts {
    /// A session with nobody in it and no telemetry, for a match that records no
    /// participants — which is every match in this repository's own tests.
    #[must_use]
    pub fn anonymous(match_id: MatchId, started_at_unix_ms: u64) -> Self {
        Self {
            match_id,
            started_at_unix_ms,
            participants: [const { None }; PLAYER_COUNT],
            sim_commit: SimCommit::of_this_build(),
            telemetry: Commitment::Absent,
        }
    }
}

/// The signed half of a replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    /// This match's identifier.
    pub match_id: MatchId,
    /// The key that sealed it.
    pub server_identity: VerifyingKey,
    /// The match seed.
    pub seed: u64,
    /// The constants it was played under.
    pub rules_hash: Digest,
    /// The `sim` version that resolved it.
    pub sim_version: [u16; 3],
    /// The commit that build came from.
    pub sim_commit: SimCommit,
    /// When the match started.
    pub started_at_unix_ms: u64,
    /// Who sat where.
    pub participants: [Option<Pseudonym>; PLAYER_COUNT],
    /// Ticks the server ran.
    pub ticks: u32,
    /// Inputs the log holds.
    pub inputs: u64,
    /// The digest of that log, in order.
    pub input_log_digest: Digest,
    /// How the match ended. The claim a forger wants to make.
    pub outcome: Outcome,
    /// The digest the server ended on.
    pub final_state_digest: Digest,
    /// The device-telemetry companion this match produced, or its named absence.
    pub telemetry: Commitment,
}

impl Manifest {
    /// The manifest's bytes.
    ///
    /// Hand-written by **exhaustive destructuring**, exactly as
    /// `sim::canonical` and `sim::view::PlayerView::encode` are, and for the
    /// same reason turned up one level: a field added to a manifest and not
    /// encoded is a field outside the signature, which means a field an
    /// attacker can change for free. The `let Manifest { .. } = self` below
    /// stops compiling when somebody adds one, and `unused_variables` — denied
    /// at this crate's root — catches binding it and forgetting to write it.
    ///
    /// Big-endian and fixed-width throughout, with a tag byte before every
    /// enum and every `Option`, so that the bytes are a function of the values
    /// and not of the machine. That is what makes a replay sealed on one target
    /// verify on another, and `replay/tests/sealed.rs` is where the claim is
    /// checked on all three.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let Self {
            match_id,
            server_identity,
            seed,
            rules_hash,
            sim_version,
            sim_commit,
            started_at_unix_ms,
            participants,
            ticks,
            inputs,
            input_log_digest,
            outcome,
            final_state_digest,
            telemetry,
        } = self;

        let mut out = Vec::with_capacity(MANIFEST_MIN_BYTES);
        out.extend_from_slice(&match_id.0);
        out.extend_from_slice(server_identity.as_bytes());
        out.extend_from_slice(&seed.to_be_bytes());
        out.extend_from_slice(rules_hash.as_bytes());
        for component in sim_version {
            out.extend_from_slice(&component.to_be_bytes());
        }
        match sim_commit {
            SimCommit::Unknown => {
                out.push(0);
                out.extend_from_slice(&[0u8; 20]);
            }
            SimCommit::Sha(bytes) => {
                out.push(1);
                out.extend_from_slice(bytes);
            }
            SimCommit::Dirty(bytes) => {
                out.push(2);
                out.extend_from_slice(bytes);
            }
        }
        out.extend_from_slice(&started_at_unix_ms.to_be_bytes());
        // One fixed-width slot per seat, present or not, so that the manifest's
        // length does not report how many people played. Not a traffic-shape
        // obligation — a file is not a packet — but the same reasoning applies
        // to a published replay, and a fixed layout costs nothing here.
        for seat in participants {
            match seat {
                None => out.push(0),
                Some(pseudonym) => {
                    let bytes = pseudonym.as_str().as_bytes();
                    out.push(bytes.len() as u8);
                    out.extend_from_slice(bytes);
                }
            }
            let written = seat.as_ref().map_or(0, |name| name.as_str().len());
            out.resize(
                out.len()
                    .saturating_add(MAX_PSEUDONYM_BYTES.saturating_sub(written)),
                0,
            );
        }
        out.extend_from_slice(&ticks.to_be_bytes());
        out.extend_from_slice(&inputs.to_be_bytes());
        out.extend_from_slice(input_log_digest.as_bytes());
        match outcome {
            Outcome::InProgress => {
                out.push(0);
                out.push(0);
                out.extend_from_slice(&0u32.to_be_bytes());
            }
            Outcome::Decided { winner, at } => {
                out.push(1);
                out.push(winner.index() as u8);
                out.extend_from_slice(&at.0.to_be_bytes());
            }
        }
        out.extend_from_slice(final_state_digest.as_bytes());
        // A tag and a fixed thirty-two bytes, zero when there is nothing to
        // name, so that the manifest's length does not report whether a match
        // recorded telemetry.
        match telemetry {
            Commitment::Absent => {
                out.push(0);
                out.extend_from_slice(&[0u8; 32]);
            }
            Commitment::Sealed(digest) => {
                out.push(1);
                out.extend_from_slice(digest.as_bytes());
            }
        }
        out
    }

    /// Reads a manifest, or `None` if these are not the bytes of one.
    ///
    /// Total on every byte string, for the reason `protocol`'s decoders are: a
    /// replay is something a third party hands you, and this milestone's whole
    /// subject is what happens when they hand you a tampered one. A padding byte
    /// that is not zero is refused, so that one manifest has exactly one
    /// encoding — otherwise two verifiers could disagree about what a file said
    /// while both accepting the same signature.
    #[must_use]
    pub fn decode(reader: &mut crate::ByteReader<'_>) -> Option<Self> {
        let match_id = MatchId(reader.array::<16>()?);
        let server_identity = VerifyingKey::from_bytes(reader.array::<32>()?);
        let seed = reader.u64()?;
        let rules_hash = Digest::from_bytes(reader.array::<32>()?);
        let sim_version = [reader.u16()?, reader.u16()?, reader.u16()?];
        let sim_commit = match reader.u8()? {
            0 => {
                if reader.array::<20>()? != [0u8; 20] {
                    return None;
                }
                SimCommit::Unknown
            }
            1 => SimCommit::Sha(reader.array::<20>()?),
            2 => SimCommit::Dirty(reader.array::<20>()?),
            _ => return None,
        };
        let started_at_unix_ms = reader.u64()?;

        let mut participants: [Option<Pseudonym>; PLAYER_COUNT] = [const { None }; PLAYER_COUNT];
        for slot in &mut participants {
            let length = usize::from(reader.u8()?);
            if length > MAX_PSEUDONYM_BYTES {
                return None;
            }
            let bytes = reader.take(MAX_PSEUDONYM_BYTES)?;
            let (named, padding) = bytes.split_at_checked(length)?;
            if padding.iter().any(|byte| *byte != 0) {
                return None;
            }
            if length > 0 {
                *slot = Some(Pseudonym::parse(core::str::from_utf8(named).ok()?)?);
            }
        }

        let ticks = reader.u32()?;
        let inputs = reader.u64()?;
        let input_log_digest = Digest::from_bytes(reader.array::<32>()?);
        let outcome = match reader.u8()? {
            0 => {
                if reader.u8()? != 0 || reader.u32()? != 0 {
                    return None;
                }
                Outcome::InProgress
            }
            1 => {
                let winner = *sim::Team::ALL.get(usize::from(reader.u8()?))?;
                Outcome::Decided {
                    winner,
                    at: Tick(reader.u32()?),
                }
            }
            _ => return None,
        };
        let final_state_digest = Digest::from_bytes(reader.array::<32>()?);
        let telemetry = match reader.u8()? {
            0 => {
                if reader.array::<32>()? != [0u8; 32] {
                    return None;
                }
                Commitment::Absent
            }
            1 => Commitment::Sealed(Digest::from_bytes(reader.array::<32>()?)),
            _ => return None,
        };

        Some(Self {
            match_id,
            server_identity,
            seed,
            rules_hash,
            sim_version,
            sim_commit,
            started_at_unix_ms,
            participants,
            ticks,
            inputs,
            input_log_digest,
            outcome,
            final_state_digest,
            telemetry,
        })
    }

    /// The pseudonyms this match names, in seat order.
    #[must_use]
    pub fn participants(&self) -> Vec<&Pseudonym> {
        self.participants.iter().flatten().collect()
    }
}

/// The manifest's encoded width, which is constant.
///
/// Written as a derivation rather than a number for the reason
/// `sim::derived_max_events` is: a comment that adds up the fields goes stale on
/// the Tuesday somebody adds one, and the assertion below is what keeps this
/// honest.
pub const MANIFEST_MIN_BYTES: usize = 16      // match_id
    + 32                                      // server_identity
    + 8                                       // seed
    + 32                                      // rules_hash
    + 6                                       // sim_version
    + 21                                      // sim_commit tag + 20 bytes
    + 8                                       // started_at_unix_ms
    + PLAYER_COUNT * (1 + MAX_PSEUDONYM_BYTES) // participants
    + 4                                       // ticks
    + 8                                       // inputs
    + 32                                      // input_log_digest
    + 6                                       // outcome tag + team + tick
    + 32                                      // final_state_digest
    + 33; // telemetry commitment tag + digest
