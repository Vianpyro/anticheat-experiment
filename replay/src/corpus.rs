//! The corpus on disk, and the command that destroys a participant's part of
//! it.
//!
//! # Why this is here and not in a crate of its own
//!
//! `docs/ARCHITECTURE.md` refuses an `xtask` crate and says a crate for two
//! commands is a crate to maintain. The corpus is a directory of recordings and
//! the records that make them lawful to hold, so it belongs beside the recording
//! format; the alternative is an eighth crate whose whole content is `std::fs`.
//!
//! # Why withdrawal is code rather than a paragraph
//!
//! `docs/RISKS.md` R3 and `docs/MILESTONES.md` M4 commit this project to Law 25:
//! high-resolution input telemetry tied to an account is personal information,
//! pseudonymisation does not change that, and a participant may withdraw at any
//! time, without justification, and have their data destroyed. A promise to
//! destroy data is worth what the destruction is worth, and a destruction
//! nobody can check is a promise. So there is a command, and there is a second
//! command whose only job is to fail if the first one left anything behind.
//!
//! # What withdrawal destroys, and why it is not surgical
//!
//! A match is one interleaved input log for nine players. Deleting one
//! participant's inputs leaves a log that no longer resimulates — the digest
//! stops matching, and the recording stops being evidence of anything. Surgical
//! removal is therefore not on offer, and the consent text says so **before**
//! recording, because a participant who learns it afterwards was not informed:
//! withdrawal destroys every match that participant played in, in full,
//! including the other participants' contributions to those matches.
//!
//! # The tombstone, and why one thing survives
//!
//! Withdrawal leaves a `withdrawals/<pseudonym>.withdrawn` file holding the
//! pseudonym and the date. Nothing else — no identity, no contact, no match, no
//! telemetry. It exists because "we destroyed it" is a claim this project has to
//! be able to demonstrate, and because a corpus that silently forgets a
//! participant ever existed cannot tell an honoured withdrawal from a file that
//! was never written. The identity mapping is destroyed in the same operation,
//! so the tombstone names nobody: it is an opaque string and the thing that made
//! it point at a person is gone.
//!
//! [`Corpus::audit`] treats `withdrawals/` as the one place the pseudonym may
//! still appear and reports every other occurrence, byte by byte, over every
//! file under the root.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::Recording;

/// Where the consent records live, one file per participant.
const PARTICIPANTS: &str = "participants";
/// Where the pseudonym mapping lives. The sensitive directory.
const IDENTITIES: &str = "identities";
/// Where the recordings live, one directory per match.
const MATCHES: &str = "matches";
/// Where a withdrawal leaves its tombstone.
const WITHDRAWALS: &str = "withdrawals";

/// One participant's consent, as recorded.
///
/// Deliberately holds **no identifying information**. The mapping from this
/// pseudonym to a person is a separate file in a separate directory, so that
/// destroying the mapping is one unlink and so that everything an analyst reads
/// is already pseudonymous. `docs/RISKS.md` R3 is explicit that this is a
/// security measure and not a change of legal category — the data is still
/// personal information and is treated as such.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsentRecord {
    /// The opaque identifier this participant is known by.
    pub pseudonym: String,
    /// The day consent was given, `YYYY-MM-DD`.
    pub consented_on: String,
    /// The day everything raw about this participant is destroyed even if they
    /// never withdraw. `docs/MILESTONES.md` M4: twenty-four months.
    pub retention_until: String,
    /// Whether this participant separately agreed that the **raw** corpus may
    /// be published. Refusable without refusing the rest, which is what "consent
    /// is per purpose" means.
    pub publication: bool,
}

impl ConsentRecord {
    /// The record as it is stored: one `key: value` per line.
    ///
    /// Written by hand rather than through a serialisation crate, for the reason
    /// the rest of this workspace writes its own encodings: the format is small,
    /// it has to be readable by a participant who asks what is held about them,
    /// and a derive would put the field list somewhere a reader cannot see it.
    #[must_use]
    pub fn encode(&self) -> String {
        let Self {
            pseudonym,
            consented_on,
            retention_until,
            publication,
        } = self;
        format!(
            "pseudonym: {pseudonym}\nconsented_on: {consented_on}\nretention_until: \
             {retention_until}\npublication: {publication}\n"
        )
    }

    /// Reads a record back, or `None` if the file is not one.
    #[must_use]
    pub fn decode(text: &str) -> Option<Self> {
        let field = |name: &str| -> Option<String> {
            text.lines()
                .find_map(|line| line.strip_prefix(&format!("{name}: ")))
                .map(str::to_owned)
        };
        Some(Self {
            pseudonym: field("pseudonym")?,
            consented_on: field("consented_on")?,
            retention_until: field("retention_until")?,
            publication: field("publication")? == "true",
        })
    }
}

/// What a withdrawal destroyed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Withdrawal {
    /// The matches deleted, by identifier.
    pub matches: Vec<String>,
    /// Whether a pseudonym mapping was found and destroyed.
    pub identity: bool,
    /// Whether a consent record was found and destroyed.
    pub consent: bool,
}

/// A corpus rooted at a directory.
///
/// The directory is **never** inside the repository. `.gitignore` refuses the
/// paths this module writes, and `ci` fails on a tracked recording, because
/// `docs/RISKS.md` R3 is about an irreversibility that git makes literal:
/// deleting a committed file does not delete it.
#[derive(Clone, Debug)]
pub struct Corpus {
    root: PathBuf,
}

impl Corpus {
    /// A corpus at this path. Nothing is created until something is written.
    #[must_use]
    pub fn open(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The directory this corpus lives in.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Records a participant's consent and their pseudonym mapping.
    ///
    /// `identity` is whatever makes the pseudonym resolvable to a person — a
    /// name, a contact address — and it is the single most sensitive string in
    /// the project. It goes in its own directory so that withdrawal can destroy
    /// it without touching anything else, and it is never read by anything that
    /// analyses telemetry.
    ///
    /// # Errors
    ///
    /// Anything the filesystem refuses.
    pub fn enrol(&self, record: &ConsentRecord, identity: &str) -> io::Result<()> {
        fs::create_dir_all(self.root.join(PARTICIPANTS))?;
        fs::create_dir_all(self.root.join(IDENTITIES))?;
        fs::write(
            self.root
                .join(PARTICIPANTS)
                .join(format!("{}.consent", record.pseudonym)),
            record.encode(),
        )?;
        fs::write(
            self.root
                .join(IDENTITIES)
                .join(format!("{}.identity", record.pseudonym)),
            format!("pseudonym: {}\nidentity: {identity}\n", record.pseudonym),
        )
    }

    /// Stores a recorded match and the pseudonyms that played it.
    ///
    /// # Errors
    ///
    /// Anything the filesystem refuses, and [`io::ErrorKind::PermissionDenied`]
    /// for a participant with no consent record — a match nobody consented to is
    /// a match this project may not hold, and refusing it here is cheaper than
    /// discovering it at M6.
    pub fn store(
        &self,
        match_id: &str,
        recording: &Recording,
        participants: &[String],
        recorded_on: &str,
    ) -> io::Result<()> {
        for pseudonym in participants {
            if !self.consent_path(pseudonym).exists() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("{pseudonym} has no consent record in this corpus"),
                ));
            }
        }
        let directory = self.root.join(MATCHES).join(match_id);
        fs::create_dir_all(&directory)?;
        fs::write(directory.join("recording.replay"), recording.encode())?;
        fs::write(
            directory.join("participants"),
            format!(
                "recorded_on: {recorded_on}\n{}",
                participants
                    .iter()
                    .map(|pseudonym| format!("participant: {pseudonym}\n"))
                    .collect::<String>()
            ),
        )
    }

    /// Every match identifier in the corpus.
    ///
    /// # Errors
    ///
    /// Anything the filesystem refuses. An absent `matches/` directory is an
    /// empty corpus rather than an error.
    pub fn matches(&self) -> io::Result<Vec<String>> {
        let directory = self.root.join(MATCHES);
        if !directory.exists() {
            return Ok(Vec::new());
        }
        let mut found: Vec<String> = fs::read_dir(directory)?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        found.sort();
        Ok(found)
    }

    /// The pseudonyms a match was played by.
    ///
    /// # Errors
    ///
    /// Anything the filesystem refuses.
    pub fn participants_of(&self, match_id: &str) -> io::Result<Vec<String>> {
        let path = self.root.join(MATCHES).join(match_id).join("participants");
        let text = fs::read_to_string(path)?;
        Ok(text
            .lines()
            .filter_map(|line| line.strip_prefix("participant: "))
            .map(str::to_owned)
            .collect())
    }

    /// Honours a withdrawal: destroys every match this participant played in,
    /// their pseudonym mapping, and their consent record, and leaves a
    /// tombstone.
    ///
    /// Idempotent. Withdrawing twice is not an error and destroys nothing the
    /// second time, because a participant repeating a request they were not sure
    /// had landed must not get an error message for it.
    ///
    /// # Errors
    ///
    /// Anything the filesystem refuses.
    pub fn withdraw(&self, pseudonym: &str, on: &str) -> io::Result<Withdrawal> {
        let mut destroyed = Withdrawal::default();

        // The matches first. If the process dies halfway, what is left is a
        // corpus with fewer matches and a live consent record — a state the next
        // run of this command repairs. The other order would leave telemetry
        // behind with nothing pointing at it, which is worse: it is data nobody
        // knows they are holding.
        for match_id in self.matches()? {
            let participants = self.participants_of(&match_id).unwrap_or_default();
            if !participants.iter().any(|who| who == pseudonym) {
                continue;
            }
            fs::remove_dir_all(self.root.join(MATCHES).join(&match_id))?;
            destroyed.matches.push(match_id);
        }

        let identity = self.identity_path(pseudonym);
        if identity.exists() {
            fs::remove_file(identity)?;
            destroyed.identity = true;
        }
        let consent = self.consent_path(pseudonym);
        if consent.exists() {
            fs::remove_file(consent)?;
            destroyed.consent = true;
        }

        fs::create_dir_all(self.root.join(WITHDRAWALS))?;
        fs::write(
            self.root
                .join(WITHDRAWALS)
                .join(format!("{pseudonym}.withdrawn")),
            format!(
                "pseudonym: {pseudonym}\nwithdrawn_on: {on}\nmatches_destroyed: {}\n",
                destroyed.matches.len()
            ),
        )?;
        Ok(destroyed)
    }

    /// Every file under the root that still mentions this pseudonym, outside the
    /// tombstone that is supposed to.
    ///
    /// The verification half, and it is deliberately crude: it reads every byte
    /// of every file under the root and looks for the pseudonym. A cleverer
    /// check would know where the pseudonym is *supposed* to appear and would
    /// therefore be blind in exactly the place a bug would put it — a temporary
    /// file, a backup, a directory a later milestone added and this function was
    /// never told about.
    ///
    /// An empty result is the only acceptable outcome after a withdrawal.
    ///
    /// # The orphan case, which searching for a name cannot reach
    ///
    /// A recording does **not** carry a pseudonym. It names seats — `player: 0`
    /// through `player: 8` — and the only thing tying a seat to a person is the
    /// match's `participants` file. So a match directory whose participant list
    /// was deleted while its recording survived is telemetry that no search for
    /// a name can find, in any corpus, for any participant.
    ///
    /// That is reported too, unconditionally: a match directory with no readable
    /// participant list is an orphan, it is somebody's input telemetry, and
    /// nobody can say whose. It appears in the result of every audit, which is
    /// deliberate — the question "is this corpus in a state I can defend" has to
    /// have one answer rather than nine.
    ///
    /// # Errors
    ///
    /// Anything the filesystem refuses. A missing root is an empty corpus, which
    /// trivially holds nothing.
    pub fn audit(&self, pseudonym: &str) -> io::Result<Vec<PathBuf>> {
        let mut traces = Vec::new();
        if !self.root.exists() {
            return Ok(traces);
        }
        for match_id in self.matches()? {
            if self.participants_of(&match_id).is_err() {
                traces.push(self.root.join(MATCHES).join(match_id));
            }
        }
        let tombstones = self.root.join(WITHDRAWALS);
        walk(&self.root, &mut |path| {
            if path.starts_with(&tombstones) {
                return Ok(());
            }
            let bytes = fs::read(path)?;
            if contains(&bytes, pseudonym.as_bytes()) || path.to_string_lossy().contains(pseudonym)
            {
                traces.push(path.to_path_buf());
            }
            Ok(())
        })?;
        traces.sort();
        Ok(traces)
    }

    fn consent_path(&self, pseudonym: &str) -> PathBuf {
        self.root
            .join(PARTICIPANTS)
            .join(format!("{pseudonym}.consent"))
    }

    fn identity_path(&self, pseudonym: &str) -> PathBuf {
        self.root
            .join(IDENTITIES)
            .join(format!("{pseudonym}.identity"))
    }
}

/// Calls `visit` on every file under `directory`, at any depth.
fn walk(directory: &Path, visit: &mut impl FnMut(&Path) -> io::Result<()>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            walk(&path, visit)?;
        } else {
            visit(&path)?;
        }
    }
    Ok(())
}

/// Whether `haystack` contains `needle`.
///
/// Written out rather than reached for, because `replay` has one dependency and
/// it is `sim`. Four lines against a crate is not a trade this workspace makes.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
