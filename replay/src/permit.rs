//! The gates: one per separable purpose, and nothing reaches a use without
//! passing through one.
//!
//! # Why this module exists at all
//!
//! `docs/CONSENT.md` offers four boxes a participant may refuse without refusing
//! to take part. A box recorded and not applied is decoration, and a granular
//! consent regime whose granularity lives in a paragraph is worse than a coarse
//! one, because it claims something it does not do.
//!
//! So the shape here is the shape `docs/RISKS.md` R8 already gave thresholds and
//! M5 gave the participant list: **the check is the only constructor of the value
//! the use needs.** [`Publishable`] is the only thing that can be written to a
//! publication directory; [`TrainingSet`] is the only thing that hands out
//! matches for training. Neither has a public constructor that skips the consent
//! records, so "publish a corpus somebody refused" is not a thing to remember not
//! to do — it is a value that cannot be built.
//!
//! # The permissions are read at the moment of use, never carried
//!
//! Every function here calls [`Corpus::permits`], which reads the consent record
//! off the disk. Nothing caches. That is what makes a partial withdrawal
//! mechanical rather than a second bookkeeping problem: revoking a permission is
//! an edit to one file, and the next publication or training set is computed
//! against the edited one with nothing to invalidate.
//!
//! # And what this cannot reach, which is one of the four
//!
//! [`crate::consent::Purpose::NamedAttribution`] is gated by
//! [`Corpus::attribution`] and that gate is real — the identity mapping is the
//! only machine-readable path from a pseudonym to a person, and it refuses. It is
//! also not sufficient, and the document says so to the participant rather than
//! implying otherwise: a name somebody remembers and types into a report passes
//! through no gate at all. That is the one participant choice in this regime kept
//! by a promise instead of by a control, and naming it is part of the deliverable
//! in the same register `docs/SCOPE.md` names the ceiling of behavioural
//! detection.
//!
//! # Publication is irreversible, and the gate is therefore *before* it
//!
//! A withdrawal of [`crate::consent::Purpose::Publication`] after a publication
//! recalls nothing: `docs/RISKS.md` R3's irreversibility is git history and
//! forks, and it applies to a published data set exactly as it applies to a
//! committed recording. So the guarantee this module offers is precise and
//! narrower than "publication can be undone": **a refusal in force when
//! `publish` runs is honoured, and a refusal arriving afterwards is a
//! conversation with a person.** `docs/CONSENT.md` says that in the
//! participant's own words, beside the box.

use std::fs;
use std::io;
use std::path::Path;

use crate::consent::Purpose;
use crate::corpus::Corpus;
use crate::session::SessionRecord;
use crate::{Replay, Telemetry};

/// Why a use of the corpus was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermitError {
    /// A participant in the match has no readable consent record.
    ///
    /// The same answer as a refusal and deliberately a separate variant: the
    /// *operator* needs to tell "they said no" from "their record does not
    /// decode", because the second is a corpus in a state somebody has to fix
    /// and the first is somebody's decision.
    Unconsented {
        /// Who.
        pseudonym: String,
    },
    /// A participant in the match refused this purpose.
    Refused {
        /// Who.
        pseudonym: String,
        /// Which purpose.
        purpose: Purpose,
    },
    /// The match does not read, so nobody can say who is in it.
    Unaccountable {
        /// Which match.
        match_id: String,
    },
}

impl core::fmt::Display for PermitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unconsented { pseudonym } => write!(
                f,
                "{pseudonym} has no readable consent record, so nothing about \
                 them may be used for anything (docs/CONSENT.md)"
            ),
            Self::Refused { pseudonym, purpose } => write!(
                f,
                "{pseudonym} refused `{purpose}`, so {}",
                purpose.refusing_means()
            ),
            Self::Unaccountable { match_id } => write!(
                f,
                "{match_id} does not read, so nobody can say who is in it \
                 (docs/SCHEMA.md §9)"
            ),
        }
    }
}

impl core::error::Error for PermitError {}

impl From<PermitError> for io::Error {
    fn from(error: PermitError) -> Self {
        Self::new(io::ErrorKind::PermissionDenied, error.to_string())
    }
}

/// Whether every participant in a match permits a purpose.
///
/// The predicate all four gates are built out of, and it is **`all`, not
/// `any`**: a match is one interleaved log of nine people's inputs, so there is
/// no way to publish, train on, or otherwise use one seat of it. One refusal
/// withholds the whole match, which `docs/SCHEMA.md` §10 states in advance as
/// the practical consequence — a publishable subset that will in practice be
/// small or empty, and no plan that depends on it existing.
///
/// # Errors
///
/// [`PermitError::Unaccountable`] for a match whose replay does not read,
/// [`PermitError::Unconsented`] and [`PermitError::Refused`] naming the first
/// participant that stops it.
pub fn everyone_in(corpus: &Corpus, match_id: &str, purpose: Purpose) -> Result<(), PermitError> {
    let participants =
        corpus
            .participants_of(match_id)
            .map_err(|_| PermitError::Unaccountable {
                match_id: match_id.to_owned(),
            })?;
    for pseudonym in participants {
        let Some(record) = corpus.consent_of(&pseudonym) else {
            return Err(PermitError::Unconsented { pseudonym });
        };
        if !record.permissions.granted(purpose) {
            return Err(PermitError::Refused { pseudonym, purpose });
        }
    }
    Ok(())
}

/// A match every participant of which agreed to have published raw.
///
/// **The only value this crate will write to a publication directory**, and it
/// has no constructor but [`Publishable::of`]. That is the whole mechanism:
/// publishing a match somebody refused is not a mistake to avoid, it is a value
/// that does not exist.
#[derive(Clone, Debug)]
pub struct Publishable {
    match_id: String,
    replay: Replay,
    session: SessionRecord,
    telemetry: Option<Telemetry>,
}

impl Publishable {
    /// This match, if every participant in it granted publication.
    ///
    /// # Errors
    ///
    /// Whatever [`everyone_in`] refuses, and
    /// [`PermitError::Unaccountable`] for a match whose files do not all read —
    /// because a match this corpus cannot account for is not one it may hand to
    /// anybody, which is the same rule `Corpus::audit` applies for every
    /// pseudonym at once.
    pub fn of(corpus: &Corpus, match_id: &str) -> Result<Self, PermitError> {
        everyone_in(corpus, match_id, Purpose::Publication)?;
        let unaccountable = || PermitError::Unaccountable {
            match_id: match_id.to_owned(),
        };
        if !corpus.accountable(match_id) {
            return Err(unaccountable());
        }
        Ok(Self {
            match_id: match_id.to_owned(),
            replay: corpus.replay_of(match_id).map_err(|_| unaccountable())?,
            session: corpus.session_of(match_id).map_err(|_| unaccountable())?,
            telemetry: corpus.telemetry_of(match_id).map_err(|_| unaccountable())?,
        })
    }

    /// Which match this is.
    #[must_use]
    pub fn match_id(&self) -> &str {
        &self.match_id
    }

    /// Writes the match into a publication directory.
    ///
    /// Takes `&self` rather than the three files, which is the point: there is no
    /// path from a `Replay` on disk to this directory that does not go through
    /// [`Publishable::of`]. The layout is the corpus's own — one directory per
    /// match, the same three names — so a published set is a corpus somebody
    /// else can point every tool in this crate at.
    ///
    /// # Errors
    ///
    /// Anything the filesystem refuses.
    pub fn write_to(&self, destination: &Path) -> io::Result<()> {
        let directory = destination.join("matches").join(&self.match_id);
        fs::create_dir_all(&directory)?;
        fs::write(directory.join("match.replay"), self.replay.encode())?;
        fs::write(directory.join("match.session"), self.session.encode())?;
        if let Some(companion) = &self.telemetry {
            fs::write(directory.join("match.telemetry"), companion.encode())?;
        }
        Ok(())
    }
}

/// The matches a bot may be trained on.
///
/// **The only value that hands out corpus data for training**, for the reason
/// [`Publishable`] is the only one that hands it out for publication. A trainer
/// takes one of these; there is no constructor that skips the consent records,
/// so a model fitted on a session whose participant refused
/// [`Purpose::BotTraining`] is not something to catch in review.
///
/// # Why exclusion rather than refusal, and where the loud refusal lives
///
/// [`TrainingSet::of`] **excludes** the matches it may not use rather than
/// refusing the whole corpus, because training on the permitted subset is
/// legitimate and a corpus of nine people cannot afford to be all-or-nothing
/// about a purpose four of them might refuse. What is loud instead is
/// [`TrainingSet::refusal`], which answers *why* a named match is not in the set
/// — because "my matches are quietly missing" and "my matches were refused by
/// name" are different things for whoever is operating this.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrainingSet {
    matches: Vec<String>,
}

impl TrainingSet {
    /// Every match in this corpus every participant of which permits training.
    ///
    /// # Errors
    ///
    /// Anything the filesystem refuses while listing the matches.
    pub fn of(corpus: &Corpus) -> io::Result<Self> {
        let mut matches = Vec::new();
        for match_id in corpus.matches()? {
            if everyone_in(corpus, &match_id, Purpose::BotTraining).is_ok()
                && corpus.accountable(&match_id)
            {
                matches.push(match_id);
            }
        }
        Ok(Self { matches })
    }

    /// Why this match is not in the set, or `None` because it is.
    #[must_use]
    pub fn refusal(corpus: &Corpus, match_id: &str) -> Option<PermitError> {
        match everyone_in(corpus, match_id, Purpose::BotTraining) {
            Err(error) => Some(error),
            Ok(()) if !corpus.accountable(match_id) => Some(PermitError::Unaccountable {
                match_id: match_id.to_owned(),
            }),
            Ok(()) => None,
        }
    }

    /// The identifiers, in corpus order.
    #[must_use]
    pub fn matches(&self) -> &[String] {
        &self.matches
    }

    /// What a model trained on this set has to carry with it.
    ///
    /// # Why a trained model needs one at all
    ///
    /// `docs/CONSENT.md` records the M5 lesson: the way a destruction promise
    /// fails is a **derived artefact that outlives what it was derived from**. A
    /// trained model is exactly that shape, and it is the first one this project
    /// will produce that a `remove_dir_all` cannot reach — a corpus can delete
    /// the matches and the weights stay.
    ///
    /// There is no un-training, so the rule is destruction rather than
    /// correction: a model trained on this corpus is destroyed by the same
    /// withdrawal that destroys the matches it learned from. What makes that
    /// reachable rather than remembered is this string. It names the
    /// **pseudonyms**, deliberately — everything else in a corpus file avoids
    /// naming a person, and this one must, because the machinery that already
    /// works is `Corpus::audit` reading every byte under the root for a name. A
    /// model stored beside its provenance is reported the first time one of its
    /// participants withdraws, exactly as a planted index is.
    ///
    /// **What this does not do**, stated because a provenance file invites a
    /// reader to conclude more: nothing forces a future model to be stored where
    /// the audit looks. That is one of the points `docs/CONSENT.md` sends to a
    /// human review rather than claiming to have closed.
    ///
    /// # Errors
    ///
    /// Anything the filesystem refuses while reading the matches.
    pub fn provenance(&self, corpus: &Corpus) -> io::Result<String> {
        let mut people: Vec<String> = Vec::new();
        for match_id in &self.matches {
            for pseudonym in corpus.participants_of(match_id)? {
                if !people.contains(&pseudonym) {
                    people.push(pseudonym);
                }
            }
        }
        people.sort();
        Ok(format!(
            "format: moba/training-provenance/v1\nconsent_version: {}\npurpose: \
             {}\nmatches: {}\nparticipants: {}\n\nAnything trained on this set is \
             destroyed by the withdrawal of any participant named above. There is \
             no un-training (docs/CONSENT.md).\n",
            crate::consent::ConsentVersion::current(),
            Purpose::BotTraining.tag(),
            self.matches.join(", "),
            people.join(", ")
        ))
    }

    /// The matches themselves.
    ///
    /// **The only accessor that yields data**, and it takes `&self` — so the
    /// signature of anything that trains is `fn fit(… , &TrainingSet)` and a
    /// caller holding a `Corpus` and a list of identifiers cannot reach it.
    ///
    /// # Errors
    ///
    /// Anything the filesystem refuses. A match that stopped reading between the
    /// set being built and this being called is skipped rather than fatal, for
    /// the reason `Corpus::profile_of` skips one: the alternative is a training
    /// run that dies on a corpus somebody is midway through repairing.
    pub fn load(&self, corpus: &Corpus) -> io::Result<Vec<(Replay, SessionRecord)>> {
        let mut loaded = Vec::new();
        for match_id in &self.matches {
            if let (Ok(replay), Ok(session)) =
                (corpus.replay_of(match_id), corpus.session_of(match_id))
            {
                loaded.push((replay, session));
            }
        }
        Ok(loaded)
    }
}
