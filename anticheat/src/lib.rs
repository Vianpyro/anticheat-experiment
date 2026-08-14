//! Detection: what a recorded match says about how it was played, and what it
//! does not.
//!
//! `docs/MILESTONES.md` M8 is this crate's milestone and `docs/detectors/` is
//! its document. What belongs here is the reasoning that is about the code.
//!
//! # The one sentence this crate exists to be able to say
//!
//! **No threshold in this repository has been calibrated, and every detector
//! says so in a type.** `docs/MILESTONES.md` M6 is built and not reached: there
//! is no human corpus, because assembling nine adults on the same evening is a
//! calendar rather than a task. A threshold chosen on synthetic play or on a
//! handful of test sessions is worse than no detector at all, because it looks
//! calibrated — so [`calibration::Calibration`] has an `Uncalibrated` variant, it
//! is what every detector here returns, and [`Finding::for_review`] answers
//! `None` rather than `false`. A detector that cannot decide must not be able to
//! *say* it decided.
//!
//! That is why [`Detector`] does not have the `fn threshold(&self) -> Score`
//! that `docs/ARCHITECTURE.md` sketched. A signature returning a threshold
//! unconditionally is a signature in which "there is no threshold" cannot be
//! expressed, and the only honest thing this milestone has to report is exactly
//! that.
//!
//! # Detectors flag for review. Nothing here acts
//!
//! `docs/SCOPE.md` and `docs/MILESTONES.md` M6 both fix this as a decision
//! rather than as an unfinished feature: **no threshold calibrated on nine
//! people supports an automatic sanction of any kind** — not a ban, not a
//! suspension, not a queue restriction, not a silent match-quality adjustment. A
//! `3/9 ≈ 33%` upper bound on the false-positive rate means one flagged player in
//! three could be innocent and the corpus cannot rule it out.
//!
//! So this crate produces a [`Score`], an [`Evidence`] bundle and a [`Finding`],
//! and a person decides. There is no verb in this crate that does anything to
//! anybody, and there is no name in it that suggests one.
//!
//! # What a detector is allowed to read
//!
//! [`telemetry::MatchTelemetry`], and it is deliberately narrow. A replay holds
//! the seed and one intention per tick with both clocks
//! (`docs/SCHEMA.md` §3); a session record holds what the match was recorded
//! *on* (§4). Everything else a detector needs is **re-derived by resimulation**
//! — the same `step` that resolved the match, so that a detector is a function
//! of a stored file and of nothing that was true only while a server was
//! running.
//!
//! Two consequences, and both are limits rather than choices:
//!
//! - **The kilohertz device stream is not here.** `client::input::InputTrace`
//!   records every device event at 125 Hz to 1 kHz, and `replay/src/manifest.rs`
//!   deliberately keeps it out of the artefact resimulation is a function of. It
//!   reaches the corpus as four summary numbers in a session record. So M8's
//!   *first* candidate signal — "input inter-arrival distribution and
//!   quantisation" — has no distribution to read, and the aim *trajectory* a
//!   curvature detector would need is not in a replay at any resolution.
//!   `docs/detectors/README.md` carries that verdict at length.
//! - **A detector's clock is the millisecond and its ruler is the tick.**
//!   `received_at_ms` and `claimed_at_ms` are milliseconds; a reaction is
//!   counted in ticks, which are 33.3 ms. `docs/RISKS.md` R14 measured the
//!   client's own contribution to a timestamp at 16 µs of standard deviation,
//!   which is sixty times finer than the field it is written into — so nothing
//!   in this crate is limited by the capture path, and nothing in it reopens
//!   `evdev`.
//!
//! # Scores are integers, and that is enforced
//!
//! `anticheat/clippy.toml` disallows `f32` and `f64` in this crate. A detector's
//! score is a **published** number (`docs/SCHEMA.md` §10), and a float is a
//! number that can come out differently on two of this project's platforms. Every
//! statistic here is an integer in a unit the detector names — parts per million,
//! ticks, hundredths of a tick — and [`Score`] carries the unit beside the value
//! so that a report cannot print one without the other.
//!
//! # Evidence names a seat and a match, never a person
//!
//! `docs/SCHEMA.md` §2: the signed manifest is the one place a pseudonym is
//! written, and the session record beside it is indexed by seat precisely so
//! that there is one naming of a person to destroy. An evidence bundle is a
//! derived artefact by definition, so it names the match and the seat and stops
//! there; whoever is reading it already holds the manifest, and a bundle that
//! carried a pseudonym would be the derived index `docs/CONSENT.md` records the
//! lesson about.

#![forbid(unsafe_code)]
#![deny(missing_docs, missing_debug_implementations, unused_variables)]

pub mod calibration;
pub mod detectors;
pub mod evaluate;
pub mod features;
pub mod telemetry;

use core::fmt;

use sim::Seat;

pub use crate::calibration::{Bounds, Calibration, CorpusBasis, Fixed};
pub use crate::detectors::all;
pub use crate::telemetry::{MatchTelemetry, Provenance, Stratum};

/// One detector's statistic, in the unit that detector's page states.
///
/// The unit is a field rather than documentation because a score without one is
/// a number a reader will guess at: 7 is a plausible reaction floor in ticks and
/// an implausible one in seconds, and the two differ by the tick rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Score {
    /// The statistic.
    pub value: i64,
    /// What it is measured in.
    pub unit: &'static str,
}

impl fmt::Display for Score {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.value, self.unit)
    }
}

/// Which tail of a statistic a reviewer would be looking at.
///
/// Not every detector is anomalous upwards. A reaction *floor* is suspicious
/// when it is small and a clock divergence when it is large, so a report that
/// printed "above threshold" for both would be wrong about half of them — and a
/// threshold nobody can orient is a threshold nobody can argue with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tail {
    /// A large value is the one worth a look.
    High,
    /// A small value is.
    Low,
}

impl fmt::Display for Tail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::High => write!(f, "high"),
            Self::Low => write!(f, "low"),
        }
    }
}

/// The supporting facts behind a reading, in the order a reader wants them.
///
/// A bundle rather than a number, because `docs/SCOPE.md` fixes what a detector
/// ships as "a score and an evidence bundle, a human reads them, and the
/// decision is the human's" — and a human handed a score alone has been handed
/// an assertion.
///
/// An ordered list rather than a map, for the reason `anticheat/clippy.toml`
/// bans the randomized-hasher collections: two runs must produce the same
/// bundle, line for line, or two readers cannot compare what they were shown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Evidence {
    lines: Vec<(&'static str, String)>,
}

impl Evidence {
    /// An empty bundle.
    #[must_use]
    pub const fn new() -> Self {
        Self { lines: Vec::new() }
    }

    /// Appends one fact.
    #[must_use]
    pub fn with(mut self, key: &'static str, value: impl fmt::Display) -> Self {
        self.lines.push((key, value.to_string()));
        self
    }

    /// What this bundle says about `key`, if anything.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.lines
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| value.as_str())
    }

    /// Every fact, in order.
    #[must_use]
    pub fn lines(&self) -> &[(&'static str, String)] {
        &self.lines
    }
}

impl fmt::Display for Evidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, (key, value)) in self.lines.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            write!(f, "    {key}: {value}")?;
        }
        Ok(())
    }
}

/// What one detector produced for one seat of one match.
///
/// The score is an [`Option`] and the absence is a first-class answer rather
/// than a zero. A detector that reads reaction latencies and finds a match with
/// no reactions in it has not observed a suspiciously fast player; it has
/// observed nothing, and scoring that as an extreme value is how a detector
/// flags the quietest person in the corpus. So it **abstains**, and says why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reading {
    /// The detector that produced it.
    pub detector: &'static str,
    /// The seat it is about. Never a person — see this crate's header.
    pub seat: Seat,
    /// The statistic, or `None` when the detector abstained.
    pub score: Option<Score>,
    /// Why it abstained, when it did.
    pub abstained: Option<String>,
    /// What the score rests on.
    pub evidence: Evidence,
}

impl Reading {
    /// A reading with a score.
    #[must_use]
    pub const fn scored(
        detector: &'static str,
        seat: Seat,
        score: Score,
        evidence: Evidence,
    ) -> Self {
        Self {
            detector,
            seat,
            score: Some(score),
            abstained: None,
            evidence,
        }
    }

    /// A reading that declines to score, and says why.
    #[must_use]
    pub fn abstained(
        detector: &'static str,
        seat: Seat,
        why: impl Into<String>,
        evidence: Evidence,
    ) -> Self {
        Self {
            detector,
            seat,
            score: None,
            abstained: Some(why.into()),
            evidence,
        }
    }
}

/// A reading together with what fixed its threshold — or with the fact that
/// nothing has.
///
/// This is the type `docs/ARCHITECTURE.md` calls a finding, and the calibration
/// is in it rather than reachable from it because the two are read together or
/// not at all. A score of 0 ticks means one thing beside a threshold justified
/// on forty matches and another beside `Uncalibrated`, and a reader handed the
/// first without the second has been handed the friendlier half.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// What the detector read.
    pub reading: Reading,
    /// What the detector's threshold rests on.
    pub calibration: Calibration,
    /// Which tail the threshold is on.
    pub tail: Tail,
}

impl Finding {
    /// Whether a **person should look at this match**, or `None` when nothing
    /// has fixed a threshold.
    ///
    /// Three answers rather than two, and the third is the one this milestone
    /// ships. `Some(true)` is *a reviewer should read the evidence below*.
    /// `Some(false)` is *this reading is inside what the corpus that fixed the
    /// threshold showed*. `None` is *no corpus has fixed a threshold, so this
    /// score is a number and not a judgement* — which is every detector in this
    /// repository today, and it is deliberately not `Some(false)`: a detector
    /// that cannot decide must not be able to report that it decided in
    /// anybody's favour either.
    ///
    /// Nothing acts on the answer. `docs/SCOPE.md` excludes every automatic
    /// sanction, and at nine participants the arithmetic forbids the
    /// alternative outright.
    #[must_use]
    pub fn for_review(&self) -> Option<bool> {
        let fixed = self.calibration.fixed()?;
        let score = self.reading.score?;
        Some(match self.tail {
            Tail::High => score.value >= fixed.threshold,
            Tail::Low => score.value <= fixed.threshold,
        })
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} / {:?}: ", self.reading.detector, self.reading.seat)?;
        match (self.reading.score, &self.reading.abstained) {
            (Some(score), _) => write!(f, "{score}")?,
            (None, Some(why)) => write!(f, "abstained — {why}")?,
            (None, None) => write!(f, "abstained")?,
        }
        match self.for_review() {
            Some(true) => write!(f, " — for review")?,
            Some(false) => write!(f, " — inside the calibrated range")?,
            None => write!(f, " — no threshold: this is a number, not a judgement")?,
        }
        if !self.reading.evidence.lines().is_empty() {
            write!(f, "\n{}", self.reading.evidence)?;
        }
        Ok(())
    }
}

/// One statistic over one match, with the null model it is read against.
///
/// A trait because there is more than one implementation and the evaluation
/// pipeline iterates over a collection, which is the bar `docs/ARCHITECTURE.md`
/// sets for an abstraction here.
pub trait Detector: fmt::Debug + Sync {
    /// The name this detector is quoted by, and the stem of its page under
    /// `docs/detectors/`.
    fn name(&self) -> &'static str;

    /// **The null model, in one sentence**, because `docs/RISKS.md` R8 prefers
    /// a detector with a stated physical null model to a fitted classifier, and
    /// a null model that cannot be stated in a sentence is a null model nobody
    /// will check.
    fn null_model(&self) -> &'static str;

    /// Which tail a reviewer looks at.
    fn tail(&self) -> Tail;

    /// What fixed this detector's threshold, or the fact that nothing has.
    fn calibration(&self) -> Calibration;

    /// The statistic for one seat of one match.
    fn read(&self, telemetry: &MatchTelemetry, seat: Seat) -> Reading;

    /// The reading, with the calibration beside it.
    ///
    /// Provided rather than implemented, so that no detector can hand out a
    /// score without the thing that says what it is worth.
    fn finding(&self, telemetry: &MatchTelemetry, seat: Seat) -> Finding {
        Finding {
            reading: self.read(telemetry, seat),
            calibration: self.calibration(),
            tail: self.tail(),
        }
    }
}
