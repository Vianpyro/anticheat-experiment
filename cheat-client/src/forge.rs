//! Exploit class 2: submitting the replay of a match you did not play.
//!
//! **This is the exploit that matters most in M7.** `docs/MILESTONES.md` M5
//! delivered the format a replay is kept in and stated its own limit; what it
//! could not deliver was the attacker. `docs/SCOPE.md` reserves the word
//! *delivered* for a defence with a matching exploit failing against it in CI, so
//! until this module existed the replay container was a format with a table of
//! hand-edited structs beside it. This is that table rewritten as a program that
//! produces files.
//!
//! # The attacker this is written from
//!
//! The one who does **not** hold a key the victim's registry accepts. That is not
//! a weakening, it is where the interesting boundary is: `docs/RISKS.md` R4 is
//! explicit that a holder of an accepted key who adjusts every field consistently
//! has produced a replay of a different match, honestly simulated, and that there
//! is nothing in the bytes to tell it from one that was played. What lies past
//! that point is key custody, not verification, and no exploit written here can
//! reach it — `replay/tests/tamper.rs::the_escalation_ends_where_key_custody_begins`
//! already executes that limit from the inside.
//!
//! So the attacker here has two things and only two: **the published format**,
//! and **a signature library**. It has a key of its own, which nobody accepts. It
//! writes the container out by hand — this module is the entire replay file
//! format, reimplemented from `docs/ARCHITECTURE.md` and `replay/src/manifest.rs`
//! — because linking the victim's writer would be an exploit that assumes the
//! victim's cooperation.
//!
//! That reimplementation is also the strongest available check on the format
//! being what the documents say: `tests/forgery.rs` requires the attacker's own
//! bytes to decode, in the victim's reader, into exactly the replay the victim's
//! writer would have produced. A format whose only writer is its own reader is a
//! format nobody has independently read.
//!
//! # What the exploit succeeds against, so that failing means something
//!
//! A registry that accepts the attacker's key. That is not a strawman: it is the
//! shape of every key-custody failure, and `docs/RISKS.md` R4's whole argument is
//! that the registry is what the format's guarantee reduces to. Against such a
//! registry the forged file **verifies** — the outcome it claims, the digest it
//! claims, the log it carries, all internally perfect, describing a match nobody
//! played. Against the victim's own registry the same bytes are refused, and the
//! only thing that refused them is provenance.
//!
//! Stating it that way is what keeps the milestone honest in the other direction
//! too. The contents did not save anybody. **The registry did.**

use ed25519_dalek::Signer as _;
use protocol::{Action, Outcome, Seat, Team};

/// What every replay starts with.
///
/// Read out of the format, not out of `replay`: an attacker holds the eight
/// bytes because a published file starts with them.
const MAGIC: [u8; 8] = *b"MOBARPLY";

/// The container format this attacker writes.
///
/// 2 since the manifest gained a telemetry commitment. The attacker reads that
/// out of the published documents like everything else here, and the fact that
/// the number had to move is itself the check working: a forger still writing 1
/// produces a file the victim's reader refuses by its format field, which is a
/// weaker attack and would have made the byte-equality assertion in
/// `tests/forgery.rs` red rather than silently passing.
const FORMAT: u16 = 2;

/// The longest a pseudonym may be, which is the width of every participant slot.
const MAX_PSEUDONYM_BYTES: usize = 32;

/// Seats in a match.
const SEATS: usize = protocol::PLAYER_COUNT;

/// The manifest's width, as the attacker adds it up from the field list.
const MANIFEST_BYTES: usize = 16      // match_id
    + 32                              // server_identity
    + 8                               // seed
    + 32                              // rules_hash
    + 6                               // sim_version
    + 21                              // sim_commit: tag and twenty bytes
    + 8                               // started_at_unix_ms
    + SEATS * (1 + MAX_PSEUDONYM_BYTES)
    + 4                               // ticks
    + 8                               // inputs
    + 32                              // input_log_digest
    + 6                               // outcome: tag, team, tick
    + 32                              // final_state_digest
    + 33; // telemetry commitment: tag and thirty-two bytes

/// Where the manifest starts in a file: after the magic and the format.
const MANIFEST_AT: usize = MAGIC.len() + 2;

/// Where each field the attacker rewrites in place begins, from the start of the
/// file.
///
/// Byte surgery rather than decode-edit-encode, because that is what somebody
/// holding a published replay actually has: a file. It is also the sharper
/// demonstration — an attacker who has to re-encode is an attacker using the
/// victim's writer.
const SEED_AT: usize = MANIFEST_AT + 16 + 32;
const MATCH_ID_AT: usize = MANIFEST_AT;
const OUTCOME_AT: usize = MANIFEST_AT + MANIFEST_BYTES - 33 - 32 - 6;
const SIGNATURE_AT: usize = MANIFEST_AT + MANIFEST_BYTES;
const LOG_COUNT_AT: usize = SIGNATURE_AT + 64;
const LOG_AT: usize = LOG_COUNT_AT + 8;

/// One log entry's width: tick, sequence, seat, both clocks, and an action
/// padded to its widest variant.
const INPUT_BYTES: usize = 4 + 4 + 1 + 8 + 8 + 9;

/// The commit a manifest claims its build came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Commit {
    /// No git history to ask.
    Unknown,
    /// A clean tree at this commit.
    Sha([u8; 20]),
    /// A tree that differed from this commit.
    Dirty([u8; 20]),
}

/// What a forged manifest claims about a telemetry companion.
///
/// The attacker reads this field out of `docs/SCHEMA.md` §11 and
/// `replay/src/manifest.rs` like every other one. It matters to a forger for a
/// reason worth stating: a forged replay of a match nobody played has no device
/// stream to point at, so it claims [`Commitment::None`] — and the fact that the
/// absence is a *signed* field is what stops the forger attaching somebody
/// else's genuine companion to it afterwards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Commitment {
    /// This file claims the match recorded no device telemetry.
    None,
    /// This file claims the companion whose bytes hash to these thirty-two.
    Sealed([u8; 32]),
}

/// One entry of a forged input log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForgedInput {
    /// The tick the entry claims to belong to.
    pub tick: u32,
    /// The sequence number it claims.
    pub seq: u32,
    /// The seat it claims to be from.
    pub seat: Seat,
    /// What the client claimed. Untrusted by anybody, including here.
    pub claimed_at_ms: u64,
    /// What the server claims to have observed.
    pub received_at_ms: u64,
    /// The intention.
    pub action: Action,
}

/// Everything a forged manifest asserts.
///
/// The two digests are supplied rather than computed, and that is the honest
/// division: an attacker who wants a *self-consistent* forgery has to simulate
/// the match they are claiming, and the rules of this game are published, so
/// their simulator is the same one everybody else runs. Reimplementing `step`
/// here to prove that an attacker can run published rules would be
/// reimplementing the game to demonstrate a claim about a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgedManifest {
    /// The match this file claims to be of.
    pub match_id: [u8; 16],
    /// The seed the claimed match was played from.
    pub seed: u64,
    /// The constants it claims to have been played under.
    pub rules_hash: [u8; 32],
    /// The `sim` version it claims resolved it.
    pub sim_version: [u16; 3],
    /// The commit it claims that build came from.
    pub sim_commit: Commit,
    /// When it claims the match started.
    pub started_at_unix_ms: u64,
    /// Who it claims sat where.
    pub participants: [Option<String>; SEATS],
    /// How many ticks it claims were run.
    pub ticks: u32,
    /// The digest of the log it carries, in order.
    pub input_log_digest: [u8; 32],
    /// **The claim the whole file exists to make.**
    pub outcome: Outcome,
    /// The state it claims the log reaches.
    pub final_state_digest: [u8; 32],
    /// The telemetry companion it claims, or the absence it claims.
    pub telemetry: Commitment,
}

/// An attacker with a key nobody accepts and a copy of the file format.
#[derive(Debug)]
pub struct Forger {
    key: ed25519_dalek::SigningKey,
}

impl Forger {
    /// A forger whose key is derived from these thirty-two bytes.
    ///
    /// A written-down seed rather than a generated key, for the reason
    /// `replay/tests/tamper.rs` gives about its own: Ed25519 signing is
    /// deterministic, so a forged file is a function of the claim rather than of
    /// the moment it was forged, and two runs of the suite produce the same
    /// bytes. It is not a secret and nothing here treats it as one.
    #[must_use]
    pub fn with_seed(seed: [u8; 32]) -> Self {
        Self {
            key: ed25519_dalek::SigningKey::from_bytes(&seed),
        }
    }

    /// The public half, which is what a manifest names and a registry either
    /// holds or does not.
    #[must_use]
    pub fn identity(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    /// Points a genuine file's `server_identity` field at this forger.
    ///
    /// A manifest names the key that is supposed to have signed it, and
    /// [`Forger::reseal`] signs with the forger's key — so an insider taking over
    /// a genuine replay has to rewrite the named identity to match, or the
    /// signature it makes verifies against the wrong key and dies at
    /// [`replay::VerifyError::Signature`] before anything else runs. The identity
    /// is the second field of the manifest, right after the sixteen-byte match id.
    pub fn point_identity_at_self(&self, file: &mut [u8]) {
        put(file, MANIFEST_AT + 16, &self.identity());
    }

    /// Re-signs a file whose manifest bytes were edited in place.
    ///
    /// This is what turns `docs/MILESTONES.md` M5's tamper table into executable
    /// *file-level* attacks. An [`Edit`] to a field inside the manifest — the
    /// outcome, the seed, the match id — leaves the signature stale, so verify
    /// stops at [`replay::VerifyError::Signature`] before the field's own check
    /// runs. An attacker who holds a key resigns, and then the deeper check is the
    /// one that catches them: which is `docs/RISKS.md` R4's whole point, that the
    /// escalation is only visible against somebody who can re-sign. The forger
    /// signs the manifest region — the magic, the format and the manifest, which
    /// is exactly what [`Forger::seal`] signed — and writes the new signature over
    /// the old.
    pub fn reseal(&self, file: &mut [u8]) {
        let Some(region) = file.get(..MANIFEST_AT + MANIFEST_BYTES) else {
            return;
        };
        let signature = self.key.sign(region).to_bytes();
        put(file, SIGNATURE_AT, &signature);
    }

    /// Writes a complete replay file, signed by this forger.
    ///
    /// The `inputs` count in the manifest is taken from `log`, because a forger
    /// producing a file that contradicts itself is a forger caught by a check
    /// that is not the one under test.
    #[must_use]
    pub fn seal(&self, manifest: &ForgedManifest, log: &[ForgedInput]) -> Vec<u8> {
        self.seal_as(self.identity(), manifest, log)
    }

    /// The same, naming somebody else's identity in the manifest.
    ///
    /// The attacker who wants their file to be *from the server* rather than
    /// from a stranger. The manifest then names a key the registry does hold and
    /// the signature is not that key's, which is the check that catches it — and
    /// it is a different check from the one that catches the honest forger, which
    /// is the point of running both.
    #[must_use]
    pub fn seal_as(
        &self,
        identity: [u8; 32],
        manifest: &ForgedManifest,
        log: &[ForgedInput],
    ) -> Vec<u8> {
        let signed = signed_bytes(identity, manifest, log.len() as u64);
        let signature = self.key.sign(&signed).to_bytes();

        let mut out = signed;
        out.extend_from_slice(&signature);
        out.extend_from_slice(&(log.len() as u64).to_be_bytes());
        for entry in log {
            write_input(&mut out, entry);
        }
        out
    }
}

/// The bytes a signature covers: the magic, the format and the manifest.
fn signed_bytes(identity: [u8; 32], manifest: &ForgedManifest, inputs: u64) -> Vec<u8> {
    let ForgedManifest {
        match_id,
        seed,
        rules_hash,
        sim_version,
        sim_commit,
        started_at_unix_ms,
        participants,
        ticks,
        input_log_digest,
        outcome,
        final_state_digest,
        telemetry,
    } = manifest;

    let mut out = Vec::with_capacity(MANIFEST_AT + MANIFEST_BYTES);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&FORMAT.to_be_bytes());

    out.extend_from_slice(match_id);
    out.extend_from_slice(&identity);
    out.extend_from_slice(&seed.to_be_bytes());
    out.extend_from_slice(rules_hash);
    for component in sim_version {
        out.extend_from_slice(&component.to_be_bytes());
    }
    match sim_commit {
        Commit::Unknown => {
            out.push(0);
            out.extend_from_slice(&[0u8; 20]);
        }
        Commit::Sha(bytes) => {
            out.push(1);
            out.extend_from_slice(bytes);
        }
        Commit::Dirty(bytes) => {
            out.push(2);
            out.extend_from_slice(bytes);
        }
    }
    out.extend_from_slice(&started_at_unix_ms.to_be_bytes());
    for seat in participants {
        let named = seat.as_deref().unwrap_or("");
        out.push(named.len() as u8);
        out.extend_from_slice(named.as_bytes());
        out.resize(
            out.len()
                .saturating_add(MAX_PSEUDONYM_BYTES.saturating_sub(named.len())),
            0,
        );
    }
    out.extend_from_slice(&ticks.to_be_bytes());
    out.extend_from_slice(&inputs.to_be_bytes());
    out.extend_from_slice(input_log_digest);
    match outcome {
        Outcome::InProgress => {
            out.push(0);
            out.push(0);
            out.extend_from_slice(&0u32.to_be_bytes());
        }
        Outcome::Decided { winner, at } => {
            out.push(1);
            out.push(team_index(*winner));
            out.extend_from_slice(&at.0.to_be_bytes());
        }
    }
    out.extend_from_slice(final_state_digest);
    match telemetry {
        Commitment::None => {
            out.push(0);
            out.extend_from_slice(&[0u8; 32]);
        }
        Commitment::Sealed(digest) => {
            out.push(1);
            out.extend_from_slice(digest);
        }
    }
    out
}

const fn team_index(team: Team) -> u8 {
    match team {
        Team::Blue => 0,
        Team::Red => 1,
        Team::Green => 2,
    }
}

fn write_input(out: &mut Vec<u8>, entry: &ForgedInput) {
    out.extend_from_slice(&entry.tick.to_be_bytes());
    out.extend_from_slice(&entry.seq.to_be_bytes());
    out.push(entry.seat.index() as u8);
    out.extend_from_slice(&entry.claimed_at_ms.to_be_bytes());
    out.extend_from_slice(&entry.received_at_ms.to_be_bytes());

    let before = out.len();
    match entry.action {
        Action::Idle => out.push(0),
        Action::Move(point) => {
            out.push(1);
            out.extend_from_slice(&point.x.to_raw().to_be_bytes());
            out.extend_from_slice(&point.y.to_raw().to_be_bytes());
        }
        Action::Skillshot(direction) => {
            out.push(2);
            out.extend_from_slice(&direction.x.to_raw().to_be_bytes());
            out.extend_from_slice(&direction.y.to_raw().to_be_bytes());
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
    out.resize(before.saturating_add(9), 0);
}

// ---------------------------------------------------------------------------
// Byte surgery on a file somebody published
// ---------------------------------------------------------------------------

/// Edits an attacker can make to a replay they hold without re-encoding it.
///
/// Each of these is one row of `docs/MILESTONES.md` M5's tamper table, performed
/// the way the attacker it was written about would perform it: on the bytes of a
/// file, from outside, with no access to the writer that produced them. M5 asked
/// what a verifier does with a tampered file and answered it by constructing the
/// structs; this answers it by handing the verifier a file.
///
/// Every one of them leaves the signature untouched, because **the attacker
/// cannot re-sign** — that is the whole of what distinguishes this attacker from
/// the one `docs/RISKS.md` R4 says cannot be caught.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    /// Claim a different result.
    Outcome(Outcome),
    /// Claim the file is a recording of a different match, which is the
    /// resubmission R4 names: a genuine log, offered a second time under another
    /// identity.
    MatchId([u8; 16]),
    /// Claim a different initial world.
    Seed(u64),
    /// Drop entries from the end of the log.
    TruncateLog(usize),
    /// Exchange two entries, leaving the multiset alone.
    SwapInputs(usize, usize),
    /// Flip a bit of the signature.
    ForgeSignature,
}

/// Applies an edit to a replay file's bytes.
///
/// Returns the file unchanged if it is too short to hold the field being edited,
/// which is not a silent failure: the exploit suite requires every edited file to
/// still decode and to be *refused*, so an edit that did nothing would fail the
/// refusal assertion rather than pass it.
#[must_use]
pub fn edit(file: &[u8], edit: Edit) -> Vec<u8> {
    let mut bytes = file.to_vec();
    match edit {
        Edit::Outcome(outcome) => {
            let mut field = [0u8; 6];
            match outcome {
                Outcome::InProgress => {}
                Outcome::Decided { winner, at } => {
                    field[0] = 1;
                    field[1] = team_index(winner);
                    field[2..6].copy_from_slice(&at.0.to_be_bytes());
                }
            }
            put(&mut bytes, OUTCOME_AT, &field);
        }
        Edit::MatchId(id) => put(&mut bytes, MATCH_ID_AT, &id),
        Edit::Seed(seed) => put(&mut bytes, SEED_AT, &seed.to_be_bytes()),
        Edit::TruncateLog(drop) => {
            let Some(count) = read_u64(&bytes, LOG_COUNT_AT) else {
                return bytes;
            };
            let keep = (count as usize).saturating_sub(drop);
            // The *log* is shortened and the manifest's count is not, which is
            // what makes "truncated" a different answer from "different". The
            // count in the manifest stays because the attacker cannot re-sign it
            // anyway, and the outer count is the file's own framing.
            put(&mut bytes, LOG_COUNT_AT, &(keep as u64).to_be_bytes());
            bytes.truncate(LOG_AT.saturating_add(keep.saturating_mul(INPUT_BYTES)));
        }
        Edit::SwapInputs(left, right) => {
            let (at, to) = (entry_at(left), entry_at(right));
            if bytes.len() < to.saturating_add(INPUT_BYTES) {
                return bytes;
            }
            for offset in 0..INPUT_BYTES {
                bytes.swap(at.saturating_add(offset), to.saturating_add(offset));
            }
        }
        Edit::ForgeSignature => {
            if let Some(byte) = bytes.get_mut(SIGNATURE_AT) {
                *byte ^= 0x01;
            }
        }
    }
    bytes
}

/// Where a log entry begins.
const fn entry_at(index: usize) -> usize {
    LOG_AT.saturating_add(index.saturating_mul(INPUT_BYTES))
}

fn put(bytes: &mut [u8], at: usize, value: &[u8]) {
    let Some(slot) = bytes.get_mut(at..at.saturating_add(value.len())) else {
        return;
    };
    slot.copy_from_slice(value);
}

fn read_u64(bytes: &[u8], at: usize) -> Option<u64> {
    let slice = bytes.get(at..at.checked_add(8)?)?;
    Some(u64::from_be_bytes(slice.try_into().ok()?))
}
