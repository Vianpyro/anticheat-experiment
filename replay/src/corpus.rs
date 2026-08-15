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
//!
//! # There is no derived index, and that is the answer rather than an omission
//!
//! `docs/CONSENT.md` promises that withdrawal destroys a participant's data, and
//! the obvious way for that promise to fail is not a match directory somebody
//! forgot to unlink: it is a *derived* artefact — a summary, a cache, a list of
//! who played what — that outlives the thing it was derived from. A corpus with
//! an index has two places a pseudonym lives and one command that deletes one of
//! them.
//!
//! Until M5 this corpus had exactly that. `store` took a participant list and
//! wrote it into a `participants` file beside the recording, because a recording
//! named seats and not people and there was nowhere else to put it. That file
//! was an index: derived from what the operator passed in, able to drift from
//! the recording it sat next to, and — the case `audit` had to grow a second job
//! for — able to be deleted while the telemetry it pointed at survived, leaving
//! somebody's inputs in a corpus with nobody able to say whose.
//!
//! A sealed replay carries its participants **inside the signature**
//! (`crate::manifest`), so the index has no reason to exist and it is gone.
//! [`Corpus::participants_of`] reads the manifest. There is one place a
//! pseudonym is written and one thing to delete, and [`Corpus::audit`] — which
//! reads every byte of every file under the root rather than the places a
//! pseudonym is supposed to be — is what refuses a future one added quietly.
//!
//! # M8 adds a second file, and it is the one a withdrawal must not miss
//!
//! [`crate::telemetry::Telemetry`] is the device-event stream at its native
//! cadence, sealed, filed as `match.telemetry` **inside the match directory** —
//! so the single `remove_dir_all` a withdrawal already performs destroys it, and
//! so a corpus cannot end up holding a stream of somebody's hand movements in a
//! place `withdraw` was never told about.
//!
//! It is the richest personal information in this corpus and it names **no
//! pseudonym**, exactly as the session record does not, which means a search for
//! a name cannot find one left behind. [`Corpus::accountable`] is the answer, and
//! it grew two clauses for it: the telemetry state has to be *coherent* — the
//! replay commits to a companion and that companion is there, or it commits to
//! none and there is none — and the directory has to hold **nothing else**, which
//! is `docs/SCHEMA.md` §1's rule enforced for the first time and the only check
//! that reaches a client's telemetry part left behind by an interrupted
//! collection.
//!
//! # M6 adds one file to a match directory, and it is not an index either
//!
//! [`crate::session::SessionRecord`] is filed beside the replay, and it holds the
//! facts a *replay* structurally cannot: what hardware each seat was played on,
//! what the client's sensitivity was, which platform, and whether the client kept
//! up with the tick while it recorded (`docs/RISKS.md` R16). It is primary rather
//! than derived — nothing else in the corpus holds any of it — it is indexed by
//! **seat and never by pseudonym**, and it lives inside the match directory, so
//! the single `remove_dir_all` a withdrawal already performs destroys it.
//!
//! That last point is what a search for a pseudonym cannot check, so the audit's
//! unaccountable-match case grew to cover it: a match directory whose replay
//! **or** whose session record does not decode is reported for every pseudonym,
//! because a seat record with no manifest in front of it describes somebody's
//! session and nobody can say whose.
//!
//! # What [`Corpus::store`] refuses, and why each refusal is here
//!
//! Every one of these is cheaper to enforce at the door than to discover in a
//! distribution, and every one of them is a thing `docs/SCHEMA.md` states as a
//! rule about the corpus:
//!
//! | Refused | Because |
//! | --- | --- |
//! | a participant with no consent record | a match nobody consented to is a match this project may not hold |
//! | a consent record from another version of the consent document | what they signed is not what this session was recorded under |
//! | a session record naming another match | a record filed beside the wrong replay describes the wrong hardware |
//! | a session record under another consent version | the operator ran a session against a document that is no longer current |
//! | a seat the manifest fills and the session leaves empty, or the reverse | the two files disagree about who was playing |
//! | one pseudonym in two seats | one person filling several seats is not nine people, and `docs/SCHEMA.md` excludes it |
//! | a seat that recorded no device event | a seat with no device behind it is a script, and the corpus is a human corpus |
//! | a seat declaring pointer acceleration on | no covariate recorded here recovers the operating system's curve |
//! | a telemetry companion the replay does not commit to, or a commitment with no companion | a companion nobody named cannot be bound to a match, and a promise with no file behind it is a corpus that cannot account for itself |
//! | a companion that does not verify against the replay that named it | `crate::telemetry::verify`, at the door rather than at the first reader |
//! | a seat the session record and the companion describe differently | the two files hold the same numbers about each seat and neither is derived from the other, so both can drift |
//! | a traced seat whose stream holds no view anchor | a seat that played a match received frames; a stream with none is a client whose anchor wiring is broken, and it is the one part of that wiring no test can reach |

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::calibration::{DeviceProfileId, Profile};
use crate::consent::ConsentVersion;
use crate::manifest::Commitment;
use crate::session::{SeatRecord, SessionRecord};
use crate::telemetry::Telemetry;
use crate::{Replay, keys::VerifyingKey};

/// Where the consent records live, one file per participant.
const PARTICIPANTS: &str = "participants";
/// Where the pseudonym mapping lives. The sensitive directory.
const IDENTITIES: &str = "identities";
/// Where the replays live, one directory per match.
const MATCHES: &str = "matches";
/// Where a withdrawal leaves its tombstone.
const WITHDRAWALS: &str = "withdrawals";
/// The file a match's replay is stored as.
const REPLAY_FILE: &str = "match.replay";
/// The file a match's session record is stored as.
const SESSION_FILE: &str = "match.session";
/// The file a match's telemetry companion is stored as, when it has one.
const TELEMETRY_FILE: &str = "match.telemetry";

/// Everything a match directory is allowed to hold.
///
/// A list rather than a convention, because `docs/SCHEMA.md` §1's "there is no
/// other file" is the rule the whole no-derived-index argument rests on and
/// nothing enforced it. [`Corpus::audit`] reports a directory holding anything
/// else — a client's telemetry part left behind, a summary somebody cached, a
/// copy made during an interrupted store — which is the *only* check that can
/// reach an artefact naming no pseudonym.
const MATCH_FILES: [&str; 3] = [REPLAY_FILE, SESSION_FILE, TELEMETRY_FILE];

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
    /// **Which version of `docs/CONSENT.md` this participant signed.**
    ///
    /// The field that turns a signature on paper into something a program can
    /// refuse. A consent record from another version of the document is a record
    /// of somebody agreeing to a text that is no longer the one being operated,
    /// and `Corpus::store` treats it as no consent at all rather than as a
    /// warning — see [`crate::consent`].
    ///
    /// It is not optional, and a record written before this field existed does
    /// not decode. That is deliberate: "absent" and "stale" have to fail the same
    /// way, or a corpus assembled under an older regime is readmitted by the
    /// silence of its own files.
    pub consent_version: ConsentVersion,
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
            consent_version,
        } = self;
        format!(
            "pseudonym: {pseudonym}\nconsented_on: {consented_on}\nretention_until: \
             {retention_until}\npublication: {publication}\nconsent_version: \
             {consent_version}\n"
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
            consent_version: ConsentVersion::parse(&field("consent_version")?)?,
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

    /// Stores a sealed match and the record of the session it was recorded in.
    ///
    /// The participants are **read out of the replay's manifest** rather than
    /// passed in, which is the M5 change and the whole of why this corpus has no
    /// index: there is one statement of who played a match, it is inside the
    /// signature, and a second one kept beside it could disagree with it.
    ///
    /// The match's directory is named by its `match_id`, which is also inside
    /// the signature — so a replay filed under somebody else's identifier is a
    /// mismatch this can see, and refuses.
    ///
    /// The session record is the M6 addition and this module's header carries the
    /// table of what it makes refusable. Nothing is written until every check has
    /// passed, because a corpus that half-stores a match it then refuses is a
    /// corpus holding telemetry it has already decided it may not hold.
    ///
    /// # Errors
    ///
    /// Anything the filesystem refuses; [`io::ErrorKind::PermissionDenied`] when
    /// the consent regime cannot account for the match — no consent record, or
    /// one from another version of the consent document — and
    /// [`io::ErrorKind::InvalidInput`] when the two files disagree, when one
    /// person occupies two seats, when a seat recorded no device event, or when a
    /// participant declared pointer acceleration left on.
    pub fn store(
        &self,
        replay: &Replay,
        session: &SessionRecord,
        telemetry: Option<&Telemetry>,
    ) -> io::Result<()> {
        let manifest = &replay.manifest;
        let refuse = |kind: io::ErrorKind, message: String| -> io::Result<()> {
            Err(io::Error::new(kind, message))
        };

        // 1. The consent regime, per participant. `decode` is what makes an
        //    absent version indistinguishable from a stale one: a record written
        //    under an older format simply is not a consent record.
        for pseudonym in manifest.participants() {
            let path = self.consent_path(pseudonym.as_str());
            let Some(record) = fs::read_to_string(&path)
                .ok()
                .as_deref()
                .and_then(ConsentRecord::decode)
            else {
                return refuse(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "{pseudonym} has no readable consent record in this corpus \
                         (docs/CONSENT.md)"
                    ),
                );
            };
            if !record.consent_version.is_current() {
                return refuse(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "{pseudonym} consented to consent document {} and this build \
                         records against {}: the text they signed is not the text this \
                         session was recorded under (docs/RISKS.md R3)",
                        record.consent_version,
                        ConsentVersion::current()
                    ),
                );
            }
        }

        // 2. One person, one seat. Nine seats filled by four people is four
        //    people's telemetry wearing nine labels, and every count this corpus
        //    reports would be wrong in the direction that flatters it.
        let mut seen: Vec<&str> = Vec::new();
        for pseudonym in manifest.participants() {
            if seen.contains(&pseudonym.as_str()) {
                return refuse(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "{pseudonym} occupies more than one seat in this match, which \
                         is one person filling several seats rather than several \
                         people (docs/SCHEMA.md)"
                    ),
                );
            }
            seen.push(pseudonym.as_str());
        }

        // 3. The session record describes this match, under this document.
        if session.match_id != manifest.match_id {
            return refuse(
                io::ErrorKind::InvalidInput,
                format!(
                    "the session record names match {} and the replay names {}",
                    session.match_id, manifest.match_id
                ),
            );
        }
        if !session.consent_version.is_current() {
            return refuse(
                io::ErrorKind::PermissionDenied,
                format!(
                    "the session was operated under consent document {} and this build \
                     records against {}",
                    session.consent_version,
                    ConsentVersion::current()
                ),
            );
        }

        // 4. The two files agree, seat by seat, about who was playing.
        for (index, slot) in manifest.participants.iter().enumerate() {
            let named = slot.is_some();
            let recorded = session.occupied().contains(&index);
            if named != recorded {
                return refuse(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "seat {index} is {} in the replay and {} in the session record",
                        if named { "occupied" } else { "empty" },
                        if recorded { "occupied" } else { "empty" }
                    ),
                );
            }
        }

        // 5. Nothing synthetic, and nothing accelerated.
        let silent = session.silent_seats();
        if !silent.is_empty() {
            return refuse(
                io::ErrorKind::InvalidInput,
                format!(
                    "seat(s) {silent:?} recorded no device event at all, so nothing \
                     with a mouse was sitting there. A human corpus contaminated with \
                     synthetic play is not a human corpus (docs/SCHEMA.md)"
                ),
            );
        }
        let accelerated = session.accelerated_seats();
        if !accelerated.is_empty() {
            return refuse(
                io::ErrorKind::InvalidInput,
                format!(
                    "seat(s) {accelerated:?} declare the operating system's pointer \
                     acceleration left on, and no covariate in this schema recovers \
                     the curve (docs/SCHEMA.md)"
                ),
            );
        }

        // 6. The telemetry companion, if this replay commits to one — and the
        //    absence of one, if it does not. Both directions are refusals: a
        //    corpus holding a replay whose companion is missing cannot account
        //    for a file it promises, and a companion filed beside a replay that
        //    named none is the substitution `crate::telemetry` exists to refuse
        //    arriving through a directory rather than through a verifier.
        match (manifest.telemetry, telemetry) {
            (Commitment::Absent, None) => {}
            (Commitment::Absent, Some(_)) => {
                return refuse(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "the replay for {} commits to no telemetry companion and one \
                         was offered: a companion a replay did not name is a \
                         companion nobody can bind to this match \
                         (docs/SCHEMA.md §11)",
                        manifest.match_id
                    ),
                );
            }
            (Commitment::Sealed(digest), None) => {
                return refuse(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "the replay for {} commits to telemetry companion {digest} and \
                         none was given: a corpus that files the promise without the \
                         file cannot account for the match (docs/SCHEMA.md §11)",
                        manifest.match_id
                    ),
                );
            }
            (Commitment::Sealed(_), Some(companion)) => {
                // The registry is the replay's own signer, because the companion
                // must be sealed by the key that sealed the replay and
                // `crate::telemetry::verify` is what says so. Building it from
                // the manifest rather than taking one as an argument keeps
                // `store` a function of the two files in front of it.
                let mut keys = crate::KeyRegistry::new();
                keys.insert(
                    manifest.server_identity,
                    crate::KeyStatus::Active,
                    "the key that sealed this replay",
                );
                if let Err(error) = crate::telemetry::verify(replay, companion, &keys) {
                    return refuse(
                        io::ErrorKind::InvalidInput,
                        format!("the telemetry companion is refused: {error}"),
                    );
                }
                // …and it describes the same seats, on the same hardware, as the
                // session record does. The two files hold the same four numbers
                // about each seat — `docs/SCHEMA.md` §4b's summary and §11's
                // stream — and the summary is what survives when there is no
                // companion, so neither is derived from the other and both can
                // drift. This is the refusal that stops them.
                for (index, (seat, facts)) in session
                    .seats
                    .iter()
                    .zip(companion.manifest.seats.iter())
                    .enumerate()
                {
                    let disagreement = match (seat, facts) {
                        (SeatRecord::Empty, None) => None,
                        (SeatRecord::Empty, Some(_)) => Some(
                            "empty in the session record and traced in the companion".to_owned(),
                        ),
                        (SeatRecord::Human { .. }, None) => Some(
                            "occupied in the session record and absent from the companion"
                                .to_owned(),
                        ),
                        (SeatRecord::Human { measured, .. }, Some(trace)) => {
                            if measured.samples != trace.samples
                                || measured.motions != trace.motions
                            {
                                Some(format!(
                                    "counted {} device event(s) of which {} are motions in \
                                     the session record, and {} of which {} are in the \
                                     companion",
                                    measured.samples,
                                    measured.motions,
                                    trace.samples,
                                    trace.motions
                                ))
                            } else if trace.views == 0 {
                                Some(
                                    "recorded no view anchor at all. A seat that \
                                     played a match received frames, so a stream \
                                     with none in it is a client whose anchor \
                                     wiring is broken rather than a session \
                                     (docs/SCHEMA.md §11c)"
                                        .to_owned(),
                                )
                            } else if measured.clock != trace.clock
                                || measured.platform != trace.platform
                                || measured.world_units_per_count_e6
                                    != trace.world_units_per_count_e6
                            {
                                Some(
                                    "was recorded on a different clock, platform or \
                                     sensitivity in the two files"
                                        .to_owned(),
                                )
                            } else {
                                None
                            }
                        }
                    };
                    if let Some(disagreement) = disagreement {
                        return refuse(
                            io::ErrorKind::InvalidInput,
                            format!("seat {index} {disagreement} (docs/SCHEMA.md §11)"),
                        );
                    }
                }
            }
        }

        let directory = self.root.join(MATCHES).join(manifest.match_id.to_string());
        fs::create_dir_all(&directory)?;
        fs::write(directory.join(REPLAY_FILE), replay.encode())?;
        fs::write(directory.join(SESSION_FILE), session.encode())?;
        if let Some(companion) = telemetry {
            fs::write(directory.join(TELEMETRY_FILE), companion.encode())?;
        }
        Ok(())
    }

    /// The telemetry companion a match directory holds, if it holds one.
    ///
    /// `None` is the legitimate answer for a match that recorded none, and this
    /// function does not consult the replay to find out which case it is in —
    /// [`Corpus::accountable`] is where the two are compared, because "there is
    /// no file" and "there should have been a file" are different questions.
    ///
    /// # Errors
    ///
    /// Anything the filesystem refuses beyond the file being absent, and
    /// [`io::ErrorKind::InvalidData`] for a file that is not a companion.
    pub fn telemetry_of(&self, match_id: &str) -> io::Result<Option<Telemetry>> {
        let path = self.root.join(MATCHES).join(match_id).join(TELEMETRY_FILE);
        match fs::read(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
            Ok(bytes) => Telemetry::decode(&bytes).map(Some).map_err(|error| {
                io::Error::new(io::ErrorKind::InvalidData, format!("{match_id}: {error}"))
            }),
        }
    }

    /// A participant's device profile: every session they have already recorded
    /// on this device, folded.
    ///
    /// **Computed, never stored.** A profile file would be a derived artefact
    /// with two failure modes this corpus has spent two milestones removing: it
    /// can disagree with the matches it was derived from, and it outlives a
    /// withdrawal that destroyed them. `replay::split::split_of` is a function
    /// rather than a file for the same reason and `census` prints rather than
    /// writes for the same reason. The cost is a walk over the corpus, which is
    /// milliseconds on the dozens of matches `docs/SCOPE.md` puts in scope.
    ///
    /// `skip` is the match being filed, so that rating a seat compares it against
    /// the profile as it stood **before** this session — a check that folds the
    /// session in first is a check agreeing with itself.
    ///
    /// A match this corpus cannot account for contributes nothing, and neither
    /// does a seat that declares another device: two devices under one profile is
    /// exactly the pooling the label exists to prevent.
    ///
    /// # Errors
    ///
    /// Anything the filesystem refuses while listing the matches.
    pub fn profile_of(
        &self,
        pseudonym: &str,
        device: &DeviceProfileId,
        skip: Option<&str>,
    ) -> io::Result<Profile> {
        let mut profile = Profile::empty(device.clone());
        for match_id in self.matches()? {
            if skip == Some(match_id.as_str()) {
                continue;
            }
            let (Ok(replay), Ok(session)) = (self.replay_of(&match_id), self.session_of(&match_id))
            else {
                continue;
            };
            for (index, slot) in replay.manifest.participants.iter().enumerate() {
                if slot.as_ref().map(ToString::to_string).as_deref() != Some(pseudonym) {
                    continue;
                }
                let Some(SeatRecord::Human {
                    declared,
                    calibration,
                    ..
                }) = session.seats.get(index)
                else {
                    continue;
                };
                if &declared.device_profile_id == device {
                    profile.fold(calibration.observations);
                }
            }
        }
        Ok(profile)
    }

    /// The session record a match directory holds.
    ///
    /// # Errors
    ///
    /// Anything the filesystem refuses, and [`io::ErrorKind::InvalidData`] for a
    /// file that is not a session record.
    pub fn session_of(&self, match_id: &str) -> io::Result<SessionRecord> {
        let text = fs::read_to_string(self.root.join(MATCHES).join(match_id).join(SESSION_FILE))?;
        SessionRecord::decode(&text).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{match_id}: the session record does not decode"),
            )
        })
    }

    /// The replay a match directory holds.
    ///
    /// # Errors
    ///
    /// Anything the filesystem refuses, and [`io::ErrorKind::InvalidData`] for a
    /// file that is not a replay.
    pub fn replay_of(&self, match_id: &str) -> io::Result<Replay> {
        let bytes = fs::read(self.root.join(MATCHES).join(match_id).join(REPLAY_FILE))?;
        Replay::decode(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
    }

    /// The key that sealed a match, for an operator checking a corpus against a
    /// published registry.
    ///
    /// # Errors
    ///
    /// Anything [`Corpus::replay_of`] refuses.
    pub fn sealed_by(&self, match_id: &str) -> io::Result<VerifyingKey> {
        Ok(self.replay_of(match_id)?.manifest.server_identity)
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

    /// The pseudonyms a match was played by, read out of its manifest.
    ///
    /// Not out of a file beside the replay, which is what this used to do: that
    /// file was an index derived from what an operator passed to `store`, and an
    /// index is a second place a pseudonym lives and a second thing a withdrawal
    /// has to reach. The manifest is inside the signature, so this cannot drift
    /// from what the match actually was and cannot be edited without breaking
    /// verification.
    ///
    /// # Errors
    ///
    /// Anything the filesystem refuses, and [`io::ErrorKind::InvalidData`] for a
    /// file that is not a replay.
    pub fn participants_of(&self, match_id: &str) -> io::Result<Vec<String>> {
        Ok(self
            .replay_of(match_id)?
            .manifest
            .participants()
            .into_iter()
            .map(ToString::to_string)
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
    /// A *log* does not carry a pseudonym. It names seats — `player: 0` through
    /// `player: 8` — so telemetry with no manifest in front of it is telemetry
    /// no search for a name can find, in any corpus, for any participant.
    ///
    /// M5 narrowed this case rather than closing it. The participants are inside
    /// the manifest now and the manifest is inside the signature, so there is no
    /// longer a separate file that can be deleted while the log survives; what
    /// is left is a match directory whose replay does not decode at all — a
    /// truncated write, a half-finished copy, a file somebody edited. That is
    /// still somebody's inputs and still nobody can say whose, so it is still
    /// reported, unconditionally, in the result of every audit. The question "is
    /// this corpus in a state I can defend" has to have one answer rather than
    /// nine.
    ///
    /// **M6 widened it back**, because the session record it adds names no
    /// pseudonym either — deliberately, so that there is one naming of a person
    /// and it is the signed one. A record whose replay is gone is a description of
    /// somebody's hardware and somebody's session with nothing left to say whose,
    /// and a search for a name structurally cannot reach it. So a match directory
    /// counts as unaccountable when the replay **or** the session record fails to
    /// read, and either way the directory is reported.
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
            if !self.accountable(&match_id) {
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

    /// Whether this match directory is one somebody can give an account of.
    ///
    /// Four conditions, and the last two are what M8's companion added:
    ///
    /// 1. the replay reads — otherwise the log names seats and nobody can say
    ///    whose seats they were;
    /// 2. the session record reads — a seat record with no manifest in front of
    ///    it describes somebody's session and nobody can say whose;
    /// 3. **the telemetry state is coherent** — the replay commits to a
    ///    companion and the companion is there and is that one, or the replay
    ///    commits to none and there is none. A stream of somebody's hand
    ///    movements beside a replay that never named it is the same orphan in a
    ///    richer form, and a promise with no file behind it is a corpus that
    ///    cannot account for itself;
    /// 4. **the directory holds nothing else.** `docs/SCHEMA.md` §1 says there is
    ///    no other file, and until this check nothing enforced it. It is the only
    ///    thing that can reach an artefact naming no pseudonym — a client's
    ///    telemetry part left behind by an interrupted collection, a cached
    ///    summary, a copy — which is exactly the derived-index failure
    ///    `docs/CONSENT.md` records and which a search for a name structurally
    ///    cannot find.
    ///
    /// Reported unconditionally, for every pseudonym, because the question "is
    /// this corpus in a state I can defend" has to have one answer rather than
    /// nine.
    #[must_use]
    pub fn accountable(&self, match_id: &str) -> bool {
        let Ok(replay) = self.replay_of(match_id) else {
            return false;
        };
        if self.session_of(match_id).is_err() {
            return false;
        }
        let coherent = match (replay.manifest.telemetry, self.telemetry_of(match_id)) {
            (Commitment::Absent, Ok(None)) => true,
            (Commitment::Sealed(digest), Ok(Some(companion))) => companion.digest() == digest,
            _ => false,
        };
        if !coherent {
            return false;
        }
        let Ok(entries) = fs::read_dir(self.root.join(MATCHES).join(match_id)) else {
            return false;
        };
        entries.flatten().all(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| MATCH_FILES.contains(&name))
        })
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
