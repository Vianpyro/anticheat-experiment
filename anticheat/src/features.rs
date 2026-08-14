//! The quantities the detectors read, extracted once and named.
//!
//! A feature and a detector are separate here because they fail differently. An
//! extraction can find nothing — a match in which nobody ever ordered an attack
//! holds no reactions — and a detector has to be able to tell that apart from a
//! player who reacted implausibly fast. Keeping the extraction as a value with
//! its own counters is what makes the abstention a first-class answer rather
//! than a zero (`docs/RISKS.md` R15: a statistic computed over an antecedent
//! that was never reached looks exactly like a statistic).
//!
//! # Two families, and both of them are times
//!
//! [`Reactions`] is the interval between a view showing somebody an enemy and
//! that seat naming the enemy in an order. [`ClockTrace`] is the rate at which a
//! client's own clock ran against the server's. **Neither reads a distance**, so
//! neither divides by `device_cpi` and neither is a measurement of a mouse
//! (`docs/SCHEMA.md` §4d.1). That is not an accident of what was easy: §4d
//! ranks the three families by comparability, and the timing family is the one
//! a per-participant scale factor cannot reach.
//!
//! # The units, and the resolution each of them has
//!
//! - A reaction is counted in **ticks**, because both ends of it are tick
//!   numbers the log carries. One tick is 33.3 ms, and that quantisation is the
//!   binding limit on the dispersion statistic — see
//!   `docs/detectors/reaction-dispersion.md`, which states it rather than
//!   working around it.
//! - A clock divergence is counted in **parts per million**, and its resolution
//!   is a function of how long the match was: both timestamps are whole
//!   milliseconds, so a span of `S` milliseconds cannot resolve a rate error
//!   below about `2 000 000 / S` ppm. [`ClockTrace::resolution_ppm`] reports it
//!   beside the score, because a score under its own resolution is noise with a
//!   number on it.

use std::collections::BTreeSet;

use sim::{EntityId, Seat, TICKS_PER_SECOND};

use crate::telemetry::{MatchTelemetry, names, seat_of};

/// A latency in ticks, rendered in milliseconds.
///
/// Multiplied before it is divided, and that ordering is the whole of the
/// function: a tick is 33.33 ms, so converting one tick at a time and adding
/// loses a third of a millisecond per tick — six ticks came out as 199 ms
/// rather than 200 the first time this was written the other way round. There
/// is no float to fall back on here (`anticheat/clippy.toml`), so the integer
/// arithmetic has to be right rather than nearly right.
#[must_use]
pub const fn ticks_to_ms(ticks: u32) -> u64 {
    (ticks as u64).saturating_mul(1000) / (TICKS_PER_SECOND as u64)
}

// ---------------------------------------------------------------------------
// Reactions
// ---------------------------------------------------------------------------

/// One answer to one appearance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reaction {
    /// The enemy champion the order named.
    pub target: EntityId,
    /// The view tick it entered this seat's vision on.
    pub sighted_at: u32,
    /// The tick the server stamped the answering order with.
    pub answered_at: u32,
    /// The difference. Zero is possible: the earliest tick an answer to the
    /// view carrying tick `v` can be stamped with is `v` itself, because the
    /// server's next tick is the one it buckets the intention into.
    pub latency_ticks: u32,
}

/// Every reaction one seat produced in one match.
///
/// # What counts as a reaction, and why it is only two of the five actions
///
/// `Attack(id)` and `Targeted(id)` **name an entity**. `Move` and `Skillshot`
/// carry a point and `Idle` carries nothing, so only those two are orders a
/// player could not have composed without having been shown a handle — which is
/// what makes the interval a *reaction* rather than a coincidence of walking.
///
/// The cost is stated rather than hidden: a player who answers everything with
/// skillshots produces no pairs at all, and the detectors reading this abstain
/// on them. A detector that abstains on a whole style of play is a detector
/// whose false-negative behaviour is a property of the game and not of the
/// threshold, and `docs/detectors/reaction-floor.md` says so.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reactions {
    /// The seat.
    pub seat: Seat,
    /// One entry per appearance that was answered, in the order the answers
    /// arrived.
    pub pairs: Vec<Reaction>,
    /// Orders naming an enemy champion this seat had **never been shown**.
    ///
    /// Counted and not scored. The rules do not require an attack order's
    /// target to be visible — `sim::step` discards an order naming an ally or
    /// nobody, and nothing else — so this is reachable, and what it is evidence
    /// of is class 1 rather than class 3. It is in the bundle because a
    /// reviewer reading a reaction floor is entitled to know the seat also
    /// named somebody it could not see.
    pub unsighted: u32,
    /// Orders that named an enemy champion at all, answered or not.
    pub naming_orders: u32,
}

impl Reactions {
    /// Extracts one seat's reactions from a match.
    #[must_use]
    pub fn extract(telemetry: &MatchTelemetry, seat: Seat) -> Self {
        let shown = telemetry.shown();
        let mut pairs = Vec::new();
        let mut unsighted = 0u32;
        let mut naming_orders = 0u32;
        // One pair per appearance: a player holding down an attack order sends
        // it every tick, and counting each repetition would report a hundred
        // reactions of increasing latency to one sighting.
        let mut answered: BTreeSet<(u16, u32)> = BTreeSet::new();

        for timed in telemetry.inputs_from(seat) {
            let Some(id) = names(timed.input.action) else {
                continue;
            };
            let Some(other) = seat_of(id) else {
                // A tower or a projectile handle. A tower stands where the
                // rules put it and never appears, so there is nothing to react
                // to.
                continue;
            };
            if other.team() == seat.team() {
                continue;
            }
            naming_orders = naming_orders.saturating_add(1);
            let at = timed.input.tick.0;
            match shown.entered_by(seat, id, at) {
                None => unsighted = unsighted.saturating_add(1),
                Some(sighted_at) => {
                    if answered.insert((id.0, sighted_at)) {
                        pairs.push(Reaction {
                            target: id,
                            sighted_at,
                            answered_at: at,
                            latency_ticks: at.saturating_sub(sighted_at),
                        });
                    }
                }
            }
        }

        Self {
            seat,
            pairs,
            unsighted,
            naming_orders,
        }
    }

    /// The latencies, in ticks, in the order they happened.
    #[must_use]
    pub fn latencies(&self) -> Vec<u32> {
        self.pairs.iter().map(|pair| pair.latency_ticks).collect()
    }

    /// The shortest one, or `None` if there were none.
    #[must_use]
    pub fn floor(&self) -> Option<u32> {
        self.latencies().into_iter().min()
    }

    /// The middle one, taking the lower of the two on an even count.
    #[must_use]
    pub fn median(&self) -> Option<u32> {
        let mut sorted = self.latencies();
        if sorted.is_empty() {
            return None;
        }
        sorted.sort_unstable();
        sorted.get(sorted.len().saturating_sub(1) / 2).copied()
    }

    /// The **mean absolute deviation from the median**, in hundredths of a
    /// tick.
    ///
    /// # Why not the standard deviation, and why not the MAD
    ///
    /// A standard deviation needs a square root and this crate has no floats,
    /// by `anticheat/clippy.toml`, because a published number must be the same
    /// on both of this project's platforms.
    ///
    /// The *median* absolute deviation would have been the robust choice and it
    /// is the wrong one **here**, for a reason that is specific to the
    /// quantisation: latencies are whole ticks and a plausible human range
    /// spans three or four of them, so more than half the values often equal
    /// the median exactly — and the median of the deviations is then zero for a
    /// player with real spread. A statistic that reports "no variation at all"
    /// about somebody who varied is a false positive generator, and this
    /// detector's whole cost is false positives.
    ///
    /// The mean absolute deviation keeps the outlier-tolerance that matters
    /// (deviations, not squares) and does not collapse.
    #[must_use]
    pub fn dispersion_centiticks(&self) -> Option<u64> {
        let median = u64::from(self.median()?);
        let latencies = self.latencies();
        let count = u64::try_from(latencies.len()).ok()?;
        if count == 0 {
            return None;
        }
        let total: u64 = latencies
            .iter()
            .map(|latency| u64::from(*latency).abs_diff(median))
            .sum();
        Some(total.saturating_mul(100) / count)
    }
}

// ---------------------------------------------------------------------------
// The two clocks
// ---------------------------------------------------------------------------

/// How one client's own clock ran against the server's.
///
/// `docs/SCOPE.md`'s adversary model: the client controls its clock and its
/// input timing, so `claimed_at_ms` is attacker-controlled by definition and
/// `received_at_ms` is the only clock in the system that is evidence of
/// anything. M7 established that **no rule reads the claimed field** — four
/// different claimed clocks produce one identical world digest — and left the
/// detector over the divergence to M8. This is it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockTrace {
    /// The seat.
    pub seat: Seat,
    /// How many intentions it sent.
    pub inputs: u32,
    /// How long the server watched it for, in milliseconds.
    pub observed_span_ms: u64,
    /// How much the client's own clock advanced over that, in milliseconds.
    /// Signed, because a client is free to run its clock backwards.
    pub claimed_span_ms: i128,
    /// `(claimed - observed) / observed`, in parts per million. Zero is a
    /// client whose clock ran at the server's rate; the constant offset between
    /// the two is divided out by construction, because a difference of two
    /// spans cannot see it.
    pub rate_error_ppm: i64,
    /// The smallest rate error this span could resolve, in parts per million.
    ///
    /// Both timestamps are whole milliseconds and there are two of them, so a
    /// span of `S` milliseconds cannot distinguish a rate error below about
    /// `2 000 000 / S`. A 33-second match resolves about 60 ppm; a
    /// seventeen-minute one, about 2.
    pub resolution_ppm: i64,
    /// Consecutive intentions whose claimed timestamp went backwards.
    ///
    /// Counted and not scored, and the reason is in
    /// `docs/detectors/clock-divergence.md`: a clock that lies **without
    /// changing its average rate** — one that jitters, or steps back and
    /// forward by the same amount — leaves the rate error near zero, and this
    /// detector does not reach it. Naming the gap is the deliverable.
    pub backwards: u32,
}

impl ClockTrace {
    /// Extracts one seat's two clocks from a match.
    #[must_use]
    pub fn extract(telemetry: &MatchTelemetry, seat: Seat) -> Self {
        let inputs = telemetry.inputs_from(seat);
        let count = u32::try_from(inputs.len()).unwrap_or(u32::MAX);

        let mut backwards = 0u32;
        for pair in inputs.windows(2) {
            if let [before, after] = pair
                && after.claimed_at_ms < before.claimed_at_ms
            {
                backwards = backwards.saturating_add(1);
            }
        }

        let (Some(first), Some(last)) = (inputs.first(), inputs.last()) else {
            return Self {
                seat,
                inputs: count,
                observed_span_ms: 0,
                claimed_span_ms: 0,
                rate_error_ppm: 0,
                resolution_ppm: 0,
                backwards,
            };
        };

        let observed_span_ms = last.received_at_ms.saturating_sub(first.received_at_ms);
        let claimed_span_ms = i128::from(last.claimed_at_ms) - i128::from(first.claimed_at_ms);
        let (rate_error_ppm, resolution_ppm) = if observed_span_ms == 0 {
            (0, 0)
        } else {
            let observed = i128::from(observed_span_ms);
            let error = (claimed_span_ms - observed).saturating_mul(1_000_000) / observed;
            let resolution = 2_000_000i128 / observed;
            (
                i64::try_from(error).unwrap_or(i64::MAX),
                i64::try_from(resolution).unwrap_or(i64::MAX),
            )
        };

        Self {
            seat,
            inputs: count,
            observed_span_ms,
            claimed_span_ms,
            rate_error_ppm,
            resolution_ppm,
            backwards,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Reaction, Reactions, ticks_to_ms};
    use sim::{EntityId, Seat};

    fn reactions(latencies: &[u32]) -> Reactions {
        Reactions {
            seat: Seat::Blue0,
            pairs: latencies
                .iter()
                .enumerate()
                .map(|(index, latency)| Reaction {
                    target: EntityId(3),
                    sighted_at: index as u32 * 100,
                    answered_at: index as u32 * 100 + latency,
                    latency_ticks: *latency,
                })
                .collect(),
            unsighted: 0,
            naming_orders: latencies.len() as u32,
        }
    }

    /// A tick is 33 milliseconds and the conversion says so without a float.
    #[test]
    fn a_latency_in_ticks_renders_in_milliseconds() {
        assert_eq!(ticks_to_ms(0), 0);
        assert_eq!(ticks_to_ms(1), 33);
        assert_eq!(ticks_to_ms(6), 200);
    }

    /// The case the median absolute deviation gets wrong, run against the one
    /// that is used instead.
    ///
    /// Five latencies with real spread, three of them equal to the median: the
    /// MAD is zero and would report a player who varied as a player who did
    /// not. The mean absolute deviation is not.
    #[test]
    fn the_dispersion_does_not_collapse_when_most_values_sit_on_the_median() {
        let varied = reactions(&[6, 6, 6, 5, 9]);
        // |6-6| + |6-6| + |6-6| + |5-6| + |9-6| = 4, over 5, times 100.
        assert_eq!(varied.dispersion_centiticks(), Some(80));

        let scripted = reactions(&[7, 7, 7, 7, 7]);
        assert_eq!(scripted.dispersion_centiticks(), Some(0));
    }

    /// Nothing to summarise is `None` rather than zero, which is the whole of
    /// why abstention is a first-class answer.
    #[test]
    fn an_empty_extraction_summarises_to_nothing_rather_than_to_zero() {
        let nothing = reactions(&[]);
        assert_eq!(nothing.floor(), None);
        assert_eq!(nothing.median(), None);
        assert_eq!(nothing.dispersion_centiticks(), None);
    }
}
