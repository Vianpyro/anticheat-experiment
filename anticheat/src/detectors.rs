//! The detectors, and the three candidate signals that are not among them.
//!
//! `docs/MILESTONES.md` M8 lists five candidate signals, each to arrive with a
//! null model that can be stated in one sentence. Three of them are here. The
//! other two, and the third that turned out to be a fourth, are named in
//! `docs/detectors/README.md` with the reason each is not buildable — the short
//! version being that the quantity a curvature detector needs is **not in the
//! corpus at any resolution**, and that account belongs in a document a reader
//! meets rather than in a module they have to go looking for.
//!
//! # Every detector here is uncalibrated, and that is the milestone's finding
//!
//! Not one of the thresholds below is a number, because there is no corpus to
//! choose one on. What each detector ships is the statistic, the null model, the
//! evidence bundle, and [`crate::Calibration::Uncalibrated`] with the clause
//! that says what would have to exist first. `docs/MILESTONES.md` M8 is
//! therefore **built and not reached**, which is the same shape M4 and M6 are
//! in and is recorded the same way.
//!
//! # Each of them was born with the exploit that moves it
//!
//! `docs/SCOPE.md`'s rule, and it does not bend here: a detector without a
//! matching exploit is not a delivered detector. `cheat-client::bot` carries one
//! variant per detector plus the two that matter more than either —
//! a **control**, which plays the same match without the behaviour, and a
//! **ceiling**, which does the thing with human-plausible noise on it and which
//! nothing here catches. `anticheat/tests/detectors.rs` runs all four against
//! all three.
//!
//! What that pairing establishes is narrower than it looks and the narrowness is
//! the point: it shows a detector **responds to a behaviour** and does not
//! respond to its absence. It is not a false-positive measurement, because a
//! control bot is not a person. The false-positive half is exactly what the
//! absent corpus owes, and no arrangement of bots pays it.

use sim::{Seat, TICKS_PER_SECOND};

use crate::calibration::Calibration;
use crate::features::{ClockTrace, Reactions, ticks_to_ms};
use crate::telemetry::MatchTelemetry;
use crate::{Detector, Evidence, Reading, Score, Tail};

/// Reactions needed before a floor means anything.
///
/// `docs/RISKS.md` R15 with a number on it. The minimum of one sample is that
/// sample, so a "floor" over one reaction is a reaction with a different name —
/// and the fastest of three is still a weak order statistic, which is why the
/// pair count is in the evidence beside the score rather than only in this
/// constant.
const FLOOR_MINIMUM_PAIRS: usize = 3;

/// Reactions needed before a spread means anything.
///
/// Higher than the floor's, because a dispersion is a statement about a
/// distribution and three points do not describe one. Five is the smallest
/// count at which "every one of them was identical" is a sentence worth reading
/// rather than a coincidence, and the page carries the arithmetic.
const DISPERSION_MINIMUM_PAIRS: usize = 5;

/// Every detector this crate ships, in the order a report prints them.
#[must_use]
pub fn all() -> Vec<&'static dyn Detector> {
    vec![&ClockDivergence, &ReactionFloor, &ReactionDispersion]
}

// ---------------------------------------------------------------------------
// Class 4: the clock the client controls, against the one it does not
// ---------------------------------------------------------------------------

/// How fast a client's own clock ran against the server's.
#[derive(Clone, Copy, Debug, Default)]
pub struct ClockDivergence;

impl Detector for ClockDivergence {
    fn name(&self) -> &'static str {
        "clock-divergence"
    }

    fn null_model(&self) -> &'static str {
        "Two clocks measuring the same seconds run at the same rate: an honest \
         client's claimed timestamps differ from the server's observations by an \
         offset and by the drift of a quartz crystal, which is tens to hundreds of \
         parts per million and does not accumulate into a trend."
    }

    fn tail(&self) -> Tail {
        Tail::High
    }

    fn calibration(&self) -> Calibration {
        Calibration::Uncalibrated {
            blocked_on: "the spread of real clock drift across nine participants' \
                         machines over a recorded session. The null model bounds it \
                         at hundreds of ppm from the physics of a crystal; what a \
                         threshold needs is what an unsynchronised laptop with a \
                         sleeping scheduler actually does, and this project has \
                         never watched one (docs/MILESTONES.md M6)",
        }
    }

    fn read(&self, telemetry: &MatchTelemetry, seat: Seat) -> Reading {
        let trace = ClockTrace::extract(telemetry, seat);
        let evidence = Evidence::new()
            .with("match", telemetry.match_id)
            .with("intentions", trace.inputs)
            .with("server observed (ms)", trace.observed_span_ms)
            .with("client claimed (ms)", trace.claimed_span_ms)
            .with("rate error (ppm, signed)", trace.rate_error_ppm)
            .with("resolution (ppm)", trace.resolution_ppm)
            .with("claimed timestamp went backwards", trace.backwards);

        if trace.inputs < 2 || trace.observed_span_ms == 0 {
            return Reading::abstained(
                self.name(),
                seat,
                format!(
                    "{} intention(s) over an observed span of {} ms: two timestamps \
                     a measurable distance apart are what a rate is",
                    trace.inputs, trace.observed_span_ms
                ),
                evidence,
            );
        }

        Reading::scored(
            self.name(),
            seat,
            Score {
                value: trace.rate_error_ppm.saturating_abs(),
                unit: "ppm of rate error",
            },
            evidence,
        )
    }
}

// ---------------------------------------------------------------------------
// Class 3: how fast a hand can answer something it has just been shown
// ---------------------------------------------------------------------------

/// The shortest interval between an enemy appearing and this seat naming it.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReactionFloor;

impl Detector for ReactionFloor {
    fn name(&self) -> &'static str {
        "reaction-floor"
    }

    fn null_model(&self) -> &'static str {
        "A person cannot answer something before they have seen it, and the \
         interval between a stimulus reaching a screen and a hand reaching a \
         button is bounded below by visual and motor latency — a property of \
         people rather than of this game, and one no amount of practice takes to \
         zero."
    }

    fn tail(&self) -> Tail {
        Tail::Low
    }

    fn calibration(&self) -> Calibration {
        Calibration::Uncalibrated {
            blocked_on: "nine people's own floors, measured through this client and \
                         this transport. The literature's number is about a \
                         laboratory and a button; what a threshold needs is what \
                         these participants do through a 33 ms tick and a lossy \
                         datagram path, and the loss is the half a resimulation \
                         cannot recover (docs/MILESTONES.md M6, docs/RISKS.md R6)",
        }
    }

    fn read(&self, telemetry: &MatchTelemetry, seat: Seat) -> Reading {
        let reactions = Reactions::extract(telemetry, seat);
        let (ticks_examined, sightings) = telemetry.shown().counts();
        let evidence = reaction_evidence(telemetry, &reactions, ticks_examined, sightings);

        if reactions.pairs.len() < FLOOR_MINIMUM_PAIRS {
            return Reading::abstained(
                self.name(),
                seat,
                format!(
                    "{} answered appearance(s), and a floor over fewer than \
                     {FLOOR_MINIMUM_PAIRS} is the fastest of a handful rather than a \
                     floor. A seat that fights with skillshots names nobody and lands \
                     here, which is a property of the game and not of this player",
                    reactions.pairs.len()
                ),
                evidence,
            );
        }

        let Some(floor) = reactions.floor() else {
            return Reading::abstained(self.name(), seat, "no reactions", evidence);
        };
        Reading::scored(
            self.name(),
            seat,
            Score {
                value: i64::from(floor),
                unit: "ticks",
            },
            evidence,
        )
    }
}

/// How much this seat's reaction latencies varied.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReactionDispersion;

impl Detector for ReactionDispersion {
    fn name(&self) -> &'static str {
        "reaction-dispersion"
    }

    fn null_model(&self) -> &'static str {
        "A human reaction time is a random variable, not a constant: the same \
         person answering the same stimulus twice takes two different amounts of \
         time, and the trial-to-trial variability is irreducible. A scripted delay \
         has none."
    }

    fn tail(&self) -> Tail {
        Tail::Low
    }

    fn calibration(&self) -> Calibration {
        Calibration::Uncalibrated {
            blocked_on: "a corpus, and a decision this project cannot take without \
                         one: a human spread of about 40 ms is 1.2 ticks, and this \
                         record quantises to whole ticks, so the honest separation \
                         between a person and a constant delay is barely more than \
                         one unit wide. What would settle it is nine people's \
                         measured spreads — and if they come out under a tick, this \
                         detector is withdrawn rather than thresholded \
                         (docs/detectors/reaction-dispersion.md)",
        }
    }

    fn read(&self, telemetry: &MatchTelemetry, seat: Seat) -> Reading {
        let reactions = Reactions::extract(telemetry, seat);
        let (ticks_examined, sightings) = telemetry.shown().counts();
        let evidence = reaction_evidence(telemetry, &reactions, ticks_examined, sightings);

        if reactions.pairs.len() < DISPERSION_MINIMUM_PAIRS {
            return Reading::abstained(
                self.name(),
                seat,
                format!(
                    "{} answered appearance(s), and a spread is a statement about a \
                     distribution: fewer than {DISPERSION_MINIMUM_PAIRS} points do not \
                     describe one",
                    reactions.pairs.len()
                ),
                evidence,
            );
        }

        let Some(spread) = reactions.dispersion_centiticks() else {
            return Reading::abstained(self.name(), seat, "no reactions", evidence);
        };
        Reading::scored(
            self.name(),
            seat,
            Score {
                value: i64::try_from(spread).unwrap_or(i64::MAX),
                unit: "hundredths of a tick, mean absolute deviation",
            },
            evidence,
        )
    }
}

/// The bundle both reaction detectors hand a reviewer.
///
/// Shared because it is the same evidence: the two statistics are read off one
/// list of latencies, and a reviewer comparing a floor against a spread would
/// otherwise be comparing two renderings of the same pairs.
fn reaction_evidence(
    telemetry: &MatchTelemetry,
    reactions: &Reactions,
    ticks_examined: u32,
    sightings: u32,
) -> Evidence {
    let latencies = reactions.latencies();
    let rendered: Vec<String> = reactions
        .pairs
        .iter()
        .map(|pair| {
            format!(
                "{}->{} on handle {} ({} ticks, {} ms)",
                pair.sighted_at,
                pair.answered_at,
                pair.target.0,
                pair.latency_ticks,
                ticks_to_ms(pair.latency_ticks)
            )
        })
        .collect();

    Evidence::new()
        .with("match", telemetry.match_id)
        .with("answered appearances", latencies.len())
        .with("orders naming an enemy", reactions.naming_orders)
        .with(
            "orders naming an enemy never shown",
            format!(
                "{} (class 1's shape, counted here and scored by nothing)",
                reactions.unsighted
            ),
        )
        .with("latencies (ticks)", format!("{latencies:?}"))
        // The pairs themselves, because a reviewer's first question about a
        // fast floor is *which* appearance, and the answer is a tick number
        // they can seek to in a replay.
        .with("sighted -> answered", rendered.join("; "))
        .with(
            "floor",
            reactions.floor().map_or_else(
                || "-".to_owned(),
                |ticks| format!("{ticks} ticks, {} ms", ticks_to_ms(ticks)),
            ),
        )
        .with(
            "median",
            reactions.median().map_or_else(
                || "-".to_owned(),
                |ticks| format!("{ticks} ticks, {} ms", ticks_to_ms(ticks)),
            ),
        )
        .with(
            "tick",
            format!("{} ms, and that is the resolution", ticks_to_ms(1)),
        )
        .with(
            "resimulated",
            format!("{ticks_examined} tick(s), {sightings} enemy sighting(s)"),
        )
        .with(
            "tick rate",
            format!("{TICKS_PER_SECOND} Hz (docs/ARCHITECTURE.md, frozen by R2)"),
        )
}

#[cfg(test)]
mod tests {
    use super::all;

    /// **Every detector this crate ships is uncalibrated**, and the assertion is
    /// here rather than only in the integration suite because it is the one
    /// claim the milestone actually makes.
    #[test]
    fn nothing_shipped_here_carries_a_threshold() {
        for detector in all() {
            assert!(
                !detector.calibration().is_calibrated(),
                "{} ships a threshold, and no corpus exists to have fixed one \
                 (docs/MILESTONES.md M6, M8)",
                detector.name()
            );
        }
    }

    /// A null model that cannot be stated in a sentence is a null model nobody
    /// will check, so the sentence has to exist.
    #[test]
    fn every_detector_states_a_null_model_and_a_tail() {
        for detector in all() {
            assert!(
                detector.null_model().len() > 40,
                "{}'s null model is not a sentence",
                detector.name()
            );
            println!(
                "detector {}: {} tail — {}",
                detector.name(),
                detector.tail(),
                detector.calibration()
            );
        }
    }

    /// Names are the page stems under `docs/detectors/`, and two detectors with
    /// one name would be two pages nobody could tell apart.
    #[test]
    fn the_names_are_distinct_and_are_page_stems() {
        let mut names: Vec<&str> = all().iter().map(|detector| detector.name()).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "two detectors share a name");
        for name in names {
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '-') && !name.is_empty(),
                "{name} is not a page stem"
            );
        }
    }
}
