//! What a detector's score *means*, over drawn latencies rather than over two
//! examples.
//!
//! # Why these two and not others
//!
//! The mutation pass on `anticheat/src/features.rs` found that the choice of
//! spread statistic is load bearing — the median absolute deviation reports zero
//! for a player who varied, which is a false-positive generator on a low-tail
//! detector — and it found it against a hand-written list. A hand-written list
//! is evidence about that list. What the score *claims* is an equivalence:
//!
//! > the spread is zero exactly when every answer took the same number of ticks
//!
//! and an equivalence is a property, so it is stated as one. The floor's claim is
//! the same shape and cheaper: it is one of the latencies and no latency is
//! shorter.
//!
//! These are constructions rather than assumptions — every draw is a legal input
//! and nothing here calls `prop_assume!` — so the reject budget stays at zero
//! however high the case budget is raised, which is `docs/MILESTONES.md` M1's
//! lesson about proptest's global reject cap not scaling with the case count.
//! The budget is scaled to the case count anyway, in this workspace's usual
//! shape, because the argument holds until somebody adds a property that does
//! reject.
//!
//! # The committed counter-example, and where it came from
//!
//! `proptest-regressions/statistics.txt` holds one seed, and it is honest about
//! its provenance: it was produced by the **mutation** that replaces the mean
//! absolute deviation with the median one, not by a failure of shipped code. It
//! is kept because the case it shrank to is the minimal one that tells the two
//! statistics apart — `[0, 1]`, two answers a tick apart — and the hand-written
//! unit test beside the statistic needed five values to reach the same
//! distinction. Pinning it means that mutation is caught on the first case
//! rather than after a few dozen draws.

#![deny(unsafe_code)]

use anticheat::features::{Reaction, Reactions};
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use sim::{EntityId, Seat};

/// Where a counter-example is written, in this workspace's usual place.
const REGRESSIONS: &str = "proptest-regressions/statistics.txt";

fn config() -> ProptestConfig {
    let default = ProptestConfig::default();
    ProptestConfig {
        // Scaled to the case count for the reason `sim/tests/properties.rs`
        // gives: proptest's fixed default of 1024 turns a raised budget into an
        // aborted test rather than a longer one, and a test that stops running
        // looks nothing like a test that fails. Nothing here rejects.
        max_global_rejects: default.cases.saturating_mul(4),
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(REGRESSIONS))),
        ..default
    }
}

/// Latencies as a `Reactions`, which is the only thing the statistics read.
fn reactions(latencies: &[u32]) -> Reactions {
    Reactions {
        seat: Seat::Blue0,
        pairs: latencies
            .iter()
            .enumerate()
            .map(|(index, latency)| Reaction {
                target: EntityId(3),
                sighted_at: u32::try_from(index).unwrap_or(0).saturating_mul(100),
                answered_at: u32::try_from(index)
                    .unwrap_or(0)
                    .saturating_mul(100)
                    .saturating_add(*latency),
                latency_ticks: *latency,
            })
            .collect(),
        unsighted: 0,
        naming_orders: u32::try_from(latencies.len()).unwrap_or(u32::MAX),
    }
}

proptest! {
    #![proptest_config(config())]

    /// **The spread is zero exactly when every latency is the same.**
    ///
    /// Both directions, because only one of them is the dangerous one. A
    /// statistic that misses a scripted delay is a false negative; a statistic
    /// that reports zero about a player who varied is a false positive on a
    /// detector whose whole cost is false positives.
    ///
    /// The draw is a *list of latencies*, so a run that happens to draw a
    /// constant list is exercising the left-to-right half and a run that does
    /// not is exercising the right-to-left half. Both are reached at any case
    /// budget: a two-element list of a two-valued domain is constant a quarter
    /// of the time.
    #[test]
    fn the_spread_is_zero_exactly_when_every_answer_took_the_same_time(
        latencies in prop::collection::vec(0u32..12, 1..40)
    ) {
        let spread = reactions(&latencies)
            .dispersion_centiticks()
            .expect("a non-empty list has a spread");
        let constant = latencies.iter().all(|latency| *latency == latencies[0]);
        prop_assert_eq!(
            spread == 0,
            constant,
            "spread {} over {:?}",
            spread,
            latencies
        );
    }

    /// **The floor is one of the latencies, and none of them is shorter.**
    ///
    /// The definition of a minimum, and worth stating because the score is what
    /// a reviewer compares against a human floor: a value that were not itself
    /// an observed latency would be a summary of the distribution rather than a
    /// reaction anybody had.
    #[test]
    fn the_floor_is_the_shortest_answer_and_is_one_of_them(
        latencies in prop::collection::vec(0u32..40, 1..40)
    ) {
        let found = reactions(&latencies).floor().expect("a non-empty list has a floor");
        prop_assert!(latencies.contains(&found));
        prop_assert!(latencies.iter().all(|latency| *latency >= found));
    }

    /// **A spread is invariant under reordering**, which is what makes it a
    /// statement about the distribution rather than about the order the answers
    /// happened to arrive in.
    ///
    /// It matters here because the pairs are collected in arrival order and a
    /// statistic that read the order would be reading the match's shape — when
    /// the fights happened — rather than the hand's.
    #[test]
    fn the_spread_does_not_depend_on_the_order_the_answers_arrived_in(
        latencies in prop::collection::vec(0u32..12, 2..40),
        rotation in 1usize..40
    ) {
        let mut rotated = latencies.clone();
        rotated.rotate_left(rotation % latencies.len());
        prop_assert_eq!(
            reactions(&latencies).dispersion_centiticks(),
            reactions(&rotated).dispersion_centiticks()
        );
    }
}
