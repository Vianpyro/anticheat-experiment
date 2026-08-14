//! What a threshold rests on, and the control that refuses one resting on
//! nothing.
//!
//! # Why this is a type and not a number
//!
//! `docs/RISKS.md` R8 is the entry: a corpus of tens of matches cannot
//! substantiate a low false-positive rate, and no amount of later modelling
//! fixes it. `docs/MILESTONES.md` M6 fixes the people count at nine and states
//! the consequence — `3/9 ≈ 33%` for anything a person's style drives, and no
//! number of matches improves it. `docs/SCHEMA.md` §8 requires **both** bounds
//! to travel together everywhere a claim is made.
//!
//! Those are three documents saying the same thing, and a document is what a
//! threshold ignores. So the number and its basis are one value here: a
//! [`Fixed`] threshold cannot be written down without a [`CorpusBasis`], and a
//! [`CorpusBasis`] cannot be written down at all outside this crate — the only
//! way to obtain one is [`crate::evaluate::Evaluation::basis`], which refuses
//! synthetic play, refuses a mixture of supervision strata, and refuses a corpus
//! of fewer people than `docs/MILESTONES.md` M6's exit criterion asks for.
//!
//! # The state this repository is actually in
//!
//! Every detector returns [`Calibration::Uncalibrated`]. There is no corpus:
//! M6's machinery is built and its recordings are a calendar. A threshold chosen
//! on the exploit suite's bots, or on two evenings of test sessions, is worse
//! than no detector — it has the shape of a calibrated number and none of the
//! basis, and whoever inherits it will defend it. So the refusal is mechanical
//! and `anticheat/tests/calibration.rs` exercises it by trying every way in.

use core::fmt;

use crate::telemetry::Stratum;

/// The smallest number of distinct participants a threshold may be fixed on.
///
/// `docs/MILESTONES.md` M6's exit criterion, and the one number in it that its
/// own revision proposal refuses to trade: the match count may fall from forty
/// to twenty, and the **people** count may not fall at all, because the null
/// model a behavioural detector needs is a distribution over humans and four
/// people do not make one.
pub const MINIMUM_PEOPLE: usize = 9;

/// What fixed a detector's threshold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Calibration {
    /// Nothing has, and this is what every detector in this repository returns.
    Uncalibrated {
        /// What would have to exist first, in one clause, so that a reader who
        /// asks "why not" gets the answer rather than the state.
        blocked_on: &'static str,
    },
    /// A threshold, and the corpus that fixed it.
    Fixed(Fixed),
}

impl Calibration {
    /// The threshold, if there is one.
    #[must_use]
    pub const fn fixed(&self) -> Option<Fixed> {
        match self {
            Self::Uncalibrated { .. } => None,
            Self::Fixed(fixed) => Some(*fixed),
        }
    }

    /// Whether a person may act on a reading scored against this.
    ///
    /// Never means "may sanction". `docs/SCOPE.md` excludes every automatic
    /// sanction permanently; this is whether the number means anything at all.
    #[must_use]
    pub const fn is_calibrated(&self) -> bool {
        matches!(self, Self::Fixed(_))
    }
}

impl fmt::Display for Calibration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uncalibrated { blocked_on } => {
                write!(f, "UNCALIBRATED — {blocked_on}")
            }
            Self::Fixed(fixed) => write!(f, "{fixed}"),
        }
    }
}

/// A threshold and the corpus it was chosen on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fixed {
    /// The value a reading is compared against, in the detector's own unit.
    pub threshold: i64,
    /// The corpus it was chosen on, and therefore what may be claimed from it.
    pub basis: CorpusBasis,
    /// Why this value and not another. `docs/ENGINEERING.md` keeps choosing a
    /// threshold on the manual list for this reason: "the threshold and its
    /// justification are the deliverable, and a tuner that picks it optimises a
    /// number nobody has to defend".
    pub justification: &'static str,
}

impl fmt::Display for Fixed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "threshold {} on {} — {}",
            self.threshold, self.basis, self.justification
        )
    }
}

/// The corpus a threshold was chosen on: how many people, how many matches, and
/// which single stratum.
///
/// **Constructible only by [`crate::evaluate::Evaluation::basis`].** The fields
/// are readable and none of them can be written from outside this crate, which
/// is what makes "this threshold was fixed on nine people, twenty matches, all
/// of them supervised in person" a fact the type carries rather than a sentence
/// in a document that can drift from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CorpusBasis {
    people: usize,
    matches: usize,
    stratum: Stratum,
}

impl CorpusBasis {
    /// The only constructor, and it is not public.
    pub(crate) const fn new(people: usize, matches: usize, stratum: Stratum) -> Self {
        Self {
            people,
            matches,
            stratum,
        }
    }

    /// Distinct participants. The `N` for anything a person's style drives.
    #[must_use]
    pub const fn people(&self) -> usize {
        self.people
    }

    /// Matches. The `N` for anything a match's circumstances drive.
    #[must_use]
    pub const fn matches(&self) -> usize {
        self.matches
    }

    /// The one stratum this basis was computed over.
    #[must_use]
    pub const fn stratum(&self) -> Stratum {
        self.stratum
    }

    /// The two bounds, which travel together.
    #[must_use]
    pub const fn bounds(&self) -> Bounds {
        Bounds {
            people: self.people,
            matches: self.matches,
        }
    }
}

impl fmt::Display for CorpusBasis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} people, {} match(es), {}",
            self.people, self.matches, self.stratum
        )
    }
}

/// Why a corpus cannot fix a threshold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BasisError {
    /// The readings are the exploit suite's, not a corpus's.
    ///
    /// The refusal that matters most in this repository today, because it is the
    /// one a person in a hurry would want to route around: the bots are here,
    /// they run in CI, and their scores separate cleanly. They are not people,
    /// and a null model for human behaviour is a distribution over humans.
    Synthetic,
    /// There is nothing to compute a basis from.
    Empty,
    /// Fewer distinct participants than `docs/MILESTONES.md` M6 requires.
    TooFewPeople {
        /// How many the corpus holds.
        found: usize,
        /// [`MINIMUM_PEOPLE`].
        required: usize,
    },
}

impl fmt::Display for BasisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Synthetic => write!(
                f,
                "these readings are the exploit suite's own bots. A null model for \
                 human behaviour is a distribution over humans, and no number of \
                 bot matches is a draw from it (docs/SCOPE.md)"
            ),
            Self::Empty => write!(
                f,
                "no readings: there is no corpus, so there is nothing a threshold \
                 could be fixed on (docs/MILESTONES.md M6)"
            ),
            Self::TooFewPeople { found, required } => write!(
                f,
                "{found} distinct participant(s), and docs/MILESTONES.md M6's exit \
                 criterion asks for at least {required}. The match count is the one \
                 that may be revised; the people count is not, because it is what \
                 the null model is a distribution over"
            ),
        }
    }
}

impl core::error::Error for BasisError {}

/// The two upper bounds `docs/RISKS.md` R8's rule of three supports, computed
/// together because `docs/SCHEMA.md` §8 requires them to travel together.
///
/// Zero false positives observed over `N` independent trials supports an upper
/// bound of about `3/N` at 95% confidence. **What counts as `N` is the part
/// people get wrong and there are two answers**: the number of distinct *people*
/// for anything a person's style drives, and the number of *matches* for
/// anything a match's circumstances drive. The `9 × matches` scored
/// player-matches are not independent and are never `N`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    /// Distinct participants.
    pub people: usize,
    /// Matches.
    pub matches: usize,
}

/// The sentence `docs/SCHEMA.md` §8 requires beside every published statistic.
///
/// It is a constant rather than a habit for the reason `replay census` prints it
/// on every run: `docs/RISKS.md` R8 calls publishing a rate of that shape "the
/// single most credibility-damaging thing this project could do — precisely
/// because the audience is engineers who will check".
pub const NO_ZERO_RATE: &str = "No claim of the form \"0% false positives\" is supportable at any corpus \
     size this project can reach (docs/RISKS.md R8).";

impl Bounds {
    /// `3/N` in permille for the people count, or `None` when there are none.
    ///
    /// Permille and not a float: this number is published, and
    /// `anticheat/clippy.toml` says why a published number is an integer here.
    #[must_use]
    pub const fn style_permille(&self) -> Option<u64> {
        if self.people == 0 {
            return None;
        }
        Some(3000u64.saturating_div(self.people as u64))
    }

    /// `3/N` in permille for the match count, or `None` when there are none.
    #[must_use]
    pub const fn circumstance_permille(&self) -> Option<u64> {
        if self.matches == 0 {
            return None;
        }
        Some(3000u64.saturating_div(self.matches as u64))
    }
}

/// Renders permille as a percentage with one decimal, or the honest absence.
///
/// Above a hundred per cent the rule of three has stopped saying anything: `3/N`
/// exceeds 1 for fewer than three trials, and printing "150%" as an upper bound
/// on a rate invites a reader to treat it as a rate. Fewer than three
/// observations bound nothing, and the string says so.
fn percent(permille: Option<u64>) -> String {
    match permille {
        None => "nothing at all (no observations)".to_owned(),
        Some(value) if value > 1000 => {
            "nothing useful (fewer than three observations: 3/N exceeds 1)".to_owned()
        }
        Some(value) => format!("{}.{}%", value / 10, value % 10),
    }
}

impl fmt::Display for Bounds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "  what a person's style drives     : N = {} people  -> upper bound about {}",
            self.people,
            percent(self.style_permille())
        )?;
        writeln!(
            f,
            "  what a match's circumstances drive: N = {} match(es) -> upper bound about {}",
            self.matches,
            percent(self.circumstance_permille())
        )?;
        write!(f, "  {NO_ZERO_RATE}")
    }
}

#[cfg(test)]
mod tests {
    use super::{Bounds, percent};

    /// The two numbers `docs/SCHEMA.md` §8's table states, arrived at by the
    /// code that prints them rather than by the document that quotes them.
    #[test]
    fn the_rule_of_three_reproduces_the_table_in_the_schema() {
        let at_forty = Bounds {
            people: 9,
            matches: 40,
        };
        assert_eq!(percent(at_forty.style_permille()), "33.3%");
        assert_eq!(percent(at_forty.circumstance_permille()), "7.5%");

        let at_twenty = Bounds {
            people: 9,
            matches: 20,
        };
        // The people bound does not move with the match count, which is the
        // whole of `docs/MILESTONES.md` M6's first consequence.
        assert_eq!(percent(at_twenty.style_permille()), "33.3%");
        assert_eq!(percent(at_twenty.circumstance_permille()), "15.0%");
    }

    /// An empty corpus supports nothing, and says so rather than dividing.
    #[test]
    fn an_empty_corpus_reports_no_bound_rather_than_a_flattering_one() {
        let nothing = Bounds {
            people: 0,
            matches: 0,
        };
        assert_eq!(nothing.style_permille(), None);
        assert_eq!(nothing.circumstance_permille(), None);
        assert!(format!("{nothing}").contains("nothing at all"));
        assert!(format!("{nothing}").contains("0% false positives"));

        // And two observations bound nothing either, rather than bounding a
        // rate at 150%.
        let two = Bounds {
            people: 2,
            matches: 2,
        };
        assert!(format!("{two}").contains("nothing useful"), "{two}");
    }
}
