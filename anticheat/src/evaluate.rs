//! The evaluation pipeline: score a set of matches, keep the strata apart, and
//! render the page `docs/MILESTONES.md` M8 asks each detector for.
//!
//! # It groups, and it will not pool
//!
//! `docs/SCHEMA.md` states three separate refusals and this module implements
//! them as one: a distribution is computed over **one** [`Group`], and there is
//! no function here that returns a distribution over more than one. §5a keeps
//! supervision strata apart because what makes a match human is that somebody
//! was watching; §5 keeps degraded sessions apart because a client that fell
//! behind wrote a delay into the record; §6 keeps partially filled matches apart
//! because a match with three absent champions has different fights in it. The
//! frozen train/holdout split is the fourth axis, and it is here because a
//! threshold quoted against the half it was chosen on is not a holdout.
//!
//! The cost is visible and is meant to be: a corpus recorded over several
//! evenings under different conditions produces several small distributions
//! rather than one comfortable one, and the report prints every one of them with
//! its own `N` and its own pair of bounds. That is the honest arithmetic, and a
//! reader who finds it disappointing has understood it.
//!
//! # It refuses to let bots fix a threshold
//!
//! [`Evaluation::basis`] is the only way to obtain a [`CorpusBasis`], and it
//! says no three ways: to synthetic play, to an empty corpus, and to a corpus of
//! fewer distinct people than `docs/MILESTONES.md` M6's exit criterion asks for.
//! The first is the one that matters here and now — the exploit suite's bots
//! exist, they run in CI, and their scores separate cleanly from their controls
//! — because it is the shortest available path to a number with the shape of a
//! calibration and none of the basis.
//!
//! # No I/O
//!
//! `docs/ARCHITECTURE.md` puts `anticheat` outside the filesystem: it is a pure
//! function from telemetry to scores, which is what makes every number here
//! reproducible from a stored match rather than from a server that was running
//! at the time. Reading a corpus directory is `src/bin/anticheat.rs`'s job and
//! it is twenty lines of `replay::Corpus`.

use core::fmt;
use std::collections::BTreeSet;

use replay::MatchId;
use replay::split::Split;
use sim::Seat;

use crate::calibration::{BasisError, Bounds, Calibration, CorpusBasis, MINIMUM_PEOPLE};
use crate::telemetry::{MatchTelemetry, Provenance, Stratum};
use crate::{Detector, Reading, Tail};

/// A set of matches a distribution may be computed over, and nothing wider.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    /// Recorded matches, in one stratum and one half of the frozen split.
    Corpus {
        /// Supervision, degradation and occupancy — the three `docs/SCHEMA.md`
        /// refuses to pool.
        stratum: Stratum,
        /// Train or holdout.
        split: Split,
    },
    /// This repository's own bots, by what each was built to demonstrate.
    Synthetic {
        /// The label the exploit suite gave the match.
        label: String,
    },
}

impl Group {
    /// The group a match belongs to.
    #[must_use]
    pub fn of(telemetry: &MatchTelemetry) -> Self {
        match &telemetry.provenance {
            Provenance::Corpus { stratum, split } => Self::Corpus {
                stratum: *stratum,
                split: *split,
            },
            Provenance::Synthetic { label } => Self::Synthetic {
                label: label.clone(),
            },
        }
    }

    /// Whether this group is the exploit suite's rather than the corpus's.
    #[must_use]
    pub const fn is_synthetic(&self) -> bool {
        matches!(self, Self::Synthetic { .. })
    }
}

impl fmt::Display for Group {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corpus { stratum, split } => write!(f, "{stratum} / {}", split.tag()),
            Self::Synthetic { label } => write!(f, "synthetic: {label}"),
        }
    }
}

/// One scored unit: one detector, one seat, one match.
///
/// `docs/SCHEMA.md` §8's warning made concrete: there are nine of these per
/// match and they are **not** nine independent observations, so nothing in this
/// module ever uses their count as an `N`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unit {
    /// Which match.
    pub match_id: MatchId,
    /// Which seat.
    pub seat: Seat,
    /// Which group it falls in.
    pub group: Group,
    /// What the detector read.
    pub reading: Reading,
}

/// What one detector is, for a report that has to state it beside every number.
#[derive(Clone, Copy, Debug)]
pub struct Card {
    /// The detector's name and page stem.
    pub name: &'static str,
    /// Its null model, in one sentence.
    pub null_model: &'static str,
    /// Which tail a reviewer looks at.
    pub tail: Tail,
    /// What fixed its threshold, or the fact that nothing has.
    pub calibration: Calibration,
}

/// What one group held: which matches, and how many distinct people.
#[derive(Clone, Debug, PartialEq, Eq)]
struct GroupContents {
    group: Group,
    matches: Vec<MatchId>,
    people: BTreeSet<String>,
}

/// Every detector's reading of every seat of every match, kept apart by group.
#[derive(Clone, Debug)]
pub struct Evaluation {
    cards: Vec<Card>,
    units: Vec<Unit>,
    groups: Vec<GroupContents>,
}

/// Scores a set of matches with a set of detectors.
#[must_use]
pub fn evaluate(detectors: &[&dyn Detector], corpus: &[MatchTelemetry]) -> Evaluation {
    let cards = detectors
        .iter()
        .map(|detector| Card {
            name: detector.name(),
            null_model: detector.null_model(),
            tail: detector.tail(),
            calibration: detector.calibration(),
        })
        .collect();

    let mut units = Vec::new();
    let mut groups: Vec<GroupContents> = Vec::new();

    for telemetry in corpus {
        let group = Group::of(telemetry);
        let contents = match groups.iter_mut().find(|held| held.group == group) {
            Some(held) => held,
            None => {
                groups.push(GroupContents {
                    group: group.clone(),
                    matches: Vec::new(),
                    people: BTreeSet::new(),
                });
                groups.last_mut().unwrap_or_else(|| unreachable!())
            }
        };
        if !contents.matches.contains(&telemetry.match_id) {
            contents.matches.push(telemetry.match_id);
        }
        for pseudonym in telemetry.participants.iter().flatten() {
            contents.people.insert(pseudonym.clone());
        }

        for seat in telemetry.seated() {
            for detector in detectors {
                units.push(Unit {
                    match_id: telemetry.match_id,
                    seat,
                    group: group.clone(),
                    reading: detector.read(telemetry, seat),
                });
            }
        }
    }

    groups.sort_by(|left, right| left.group.cmp(&right.group));
    Evaluation {
        cards,
        units,
        groups,
    }
}

/// One detector's scores over one group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Distribution {
    /// The detector.
    pub detector: &'static str,
    /// The group.
    pub group: Group,
    /// The scores, sorted.
    pub scored: Vec<i64>,
    /// The unit they are in, if anything was scored.
    pub unit: Option<&'static str>,
    /// Seats the detector declined to score.
    pub abstained: usize,
}

impl Distribution {
    /// The smallest score.
    #[must_use]
    pub fn min(&self) -> Option<i64> {
        self.scored.first().copied()
    }

    /// The largest.
    #[must_use]
    pub fn max(&self) -> Option<i64> {
        self.scored.last().copied()
    }

    /// The middle one, taking the lower of two on an even count.
    #[must_use]
    pub fn median(&self) -> Option<i64> {
        self.scored
            .get(self.scored.len().saturating_sub(1) / 2)
            .copied()
    }

    /// Scored seats.
    #[must_use]
    pub fn count(&self) -> usize {
        self.scored.len()
    }
}

impl fmt::Display for Distribution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} scored, {} abstained",
            self.group,
            self.count(),
            self.abstained
        )?;
        if let (Some(min), Some(median), Some(max), Some(unit)) =
            (self.min(), self.median(), self.max(), self.unit)
        {
            write!(f, " — min {min}, median {median}, max {max} ({unit})")?;
            // The whole list, because a corpus this size has a distribution a
            // reader can hold in their head, and five summary numbers over
            // nine points is a summary that hides more than it says.
            if self.scored.len() <= 32 {
                write!(f, "\n        all: {:?}", self.scored)?;
            }
        }
        Ok(())
    }
}

impl Evaluation {
    /// Every reading, in the order they were taken.
    #[must_use]
    pub fn units(&self) -> &[Unit] {
        &self.units
    }

    /// Every group, in a stable order.
    #[must_use]
    pub fn groups(&self) -> Vec<Group> {
        self.groups.iter().map(|held| held.group.clone()).collect()
    }

    /// One detector's distribution over one group.
    #[must_use]
    pub fn distribution(&self, detector: &str, group: &Group) -> Distribution {
        let mut scored = Vec::new();
        let mut abstained = 0usize;
        let mut unit = None;
        for held in &self.units {
            if held.reading.detector != detector || held.group != *group {
                continue;
            }
            match held.reading.score {
                Some(score) => {
                    unit = Some(score.unit);
                    scored.push(score.value);
                }
                None => abstained = abstained.saturating_add(1),
            }
        }
        scored.sort_unstable();
        Distribution {
            detector: self
                .cards
                .iter()
                .find(|card| card.name == detector)
                .map_or("unknown", |card| card.name),
            group: group.clone(),
            scored,
            unit,
            abstained,
        }
    }

    /// What may be claimed from one group — or why nothing may.
    ///
    /// **The only constructor of a [`CorpusBasis`], and therefore the only way a
    /// threshold in this repository can ever be written down.**
    ///
    /// # Errors
    ///
    /// [`BasisError::Synthetic`] for the exploit suite's own matches,
    /// [`BasisError::Empty`] for a group with no matches in it, and
    /// [`BasisError::TooFewPeople`] for a corpus below
    /// [`MINIMUM_PEOPLE`] — which is every corpus this project holds today,
    /// there being none.
    pub fn basis(&self, group: &Group) -> Result<CorpusBasis, BasisError> {
        if group.is_synthetic() {
            return Err(BasisError::Synthetic);
        }
        let Some(contents) = self.groups.iter().find(|held| held.group == *group) else {
            return Err(BasisError::Empty);
        };
        if contents.matches.is_empty() {
            return Err(BasisError::Empty);
        }
        let people = contents.people.len();
        if people < MINIMUM_PEOPLE {
            return Err(BasisError::TooFewPeople {
                found: people,
                required: MINIMUM_PEOPLE,
            });
        }
        let Group::Corpus { stratum, .. } = group else {
            return Err(BasisError::Synthetic);
        };
        Ok(CorpusBasis::new(people, contents.matches.len(), *stratum))
    }

    /// The two bounds a group supports, whether or not it could fix a threshold.
    ///
    /// Separate from [`Evaluation::basis`] deliberately: a reader is entitled to
    /// see what a corpus would support even when it is refused, because "this
    /// corpus supports an upper bound of 33% and is still not enough to fix a
    /// threshold" is the sentence `docs/MILESTONES.md` M6's arithmetic is about.
    #[must_use]
    pub fn bounds(&self, group: &Group) -> Bounds {
        self.groups.iter().find(|held| held.group == *group).map_or(
            Bounds {
                people: 0,
                matches: 0,
            },
            |held| Bounds {
                people: held.people.len(),
                matches: held.matches.len(),
            },
        )
    }
}

impl fmt::Display for Evaluation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let corpus_groups: Vec<&GroupContents> = self
            .groups
            .iter()
            .filter(|held| !held.group.is_synthetic())
            .collect();
        let matches: usize = corpus_groups.iter().map(|held| held.matches.len()).sum();

        writeln!(
            f,
            "detectors: {} over {} recorded match(es) in {} stratum-half(s)",
            self.cards.len(),
            matches,
            corpus_groups.len()
        )?;
        if matches == 0 {
            writeln!(
                f,
                "\nThere is no corpus. Every threshold below is UNCALIBRATED and every\n\
                 score below is a number rather than a judgement. `replay census` says\n\
                 what an empty corpus means for a claim; this says what it means for a\n\
                 threshold — nothing here has one, and nothing here can get one from the\n\
                 synthetic groups, because a null model for human behaviour is a\n\
                 distribution over humans (docs/MILESTONES.md M6, docs/RISKS.md R8)."
            )?;
        }

        for card in &self.cards {
            writeln!(f)?;
            writeln!(f, "{} — {} tail", card.name, card.tail)?;
            writeln!(f, "  null model: {}", card.null_model)?;
            writeln!(f, "  calibration: {}", card.calibration)?;
            if self.groups.is_empty() {
                writeln!(f, "  (nothing scored)")?;
                continue;
            }
            for held in &self.groups {
                writeln!(f, "    {}", self.distribution(card.name, &held.group))?;
            }
        }

        // The two bounds, per stratum-half, never pooled. `docs/SCHEMA.md` §8:
        // a reader shown one of them has been shown the friendlier one.
        writeln!(f)?;
        writeln!(
            f,
            "what each stratum can support, at 95% confidence and zero observed \
             false positives:"
        )?;
        if corpus_groups.is_empty() {
            writeln!(f, "  no recorded matches, so no stratum and no bound.")?;
            writeln!(f, "  {}", crate::calibration::NO_ZERO_RATE)?;
        } else {
            for held in corpus_groups {
                writeln!(f, "  {}", held.group)?;
                writeln!(f, "{}", self.bounds(&held.group))?;
                match self.basis(&held.group) {
                    Ok(basis) => writeln!(f, "  a threshold may be fixed on {basis}")?,
                    Err(error) => writeln!(f, "  no threshold may be fixed here: {error}")?,
                }
            }
        }
        write!(
            f,
            "\nNo detector emits an action. A finding is a score and an evidence \
             bundle;\nacting on one is a human decision, permanently \
             (docs/SCOPE.md)."
        )
    }
}
