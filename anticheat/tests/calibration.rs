//! The control that refuses a threshold, exercised in both directions.
//!
//! `docs/MILESTONES.md` M8 says a detector ships with a threshold and its
//! justification, and M6 says no threshold calibrated on nine people supports a
//! sanction. This project has no corpus at all, so the thing M8 actually has to
//! deliver is the **refusal**: a threshold cannot be written down in this
//! workspace without a [`CorpusBasis`], and a `CorpusBasis` cannot be obtained
//! except from an evaluation over recorded human matches in one stratum, in one
//! half of the frozen split, with at least nine distinct participants.
//!
//! # Both directions, because a gate that has never opened is a gate nobody has
//! checked
//!
//! Four refusals and one success. The success is the important half and it is
//! the one that would be easy to omit: a control asserted only by watching it
//! say no is `docs/RISKS.md` R15 wearing a permission check — it looks identical
//! to a control that says no to everything, including to a corpus that should
//! have passed.
//!
//! Nothing here fixes a threshold for a shipped detector.
//! `anticheat/tests/detectors.rs` asserts that every one of them is
//! uncalibrated, and this file only shows the machinery that keeps it that way
//! is real.

#![deny(unsafe_code)]

use anticheat::calibration::{BasisError, Calibration, Fixed, MINIMUM_PEOPLE};
use anticheat::evaluate::{Group, evaluate};
use anticheat::telemetry::Stratum;
use anticheat::{Detector as _, Finding, Reading, Score, Tail};
use replay::session::Supervision;
use replay::split::Split;
use sim::Seat;

#[path = "harness/stored.rs"]
mod stored;

use stored::{NINE, identifiers_in, stored, synthetic_match};

/// One match of nine people, all supervised in person, all healthy.
fn nine_people_in_person(count: usize) -> Vec<anticheat::MatchTelemetry> {
    identifiers_in(Split::Train, count)
        .into_iter()
        .map(|identifier| stored(identifier, &NINE, Supervision::InPerson, false))
        .collect()
}

/// **A bot cannot fix a threshold**, and this is the refusal that matters here
/// and now.
///
/// The exploit suite's variants exist, they run in CI, and their scores separate
/// from their controls cleanly enough to be tempting. A null model for human
/// behaviour is a distribution over humans, so no arrangement of bots is a draw
/// from it — and this is that sentence with a compiler behind it.
#[test]
fn synthetic_play_cannot_fix_a_threshold_however_cleanly_it_separates() {
    // The bot match is **in** the evaluation, which is the antecedent this test
    // was missing the first time it was written: asking for the basis of a
    // synthetic group that is not there answers `Empty`, and a refusal that
    // fires for the wrong reason is `docs/RISKS.md` R15 with a green tick on
    // it. Removing the synthetic check from `basis` had to fail this, and it
    // did not until the group existed.
    let corpus = vec![
        synthetic_match("reflex bot"),
        // …and nine real people beside it, so that the evaluation is not
        // refusing everything: the corpus group below can fix a threshold and
        // the bot group cannot, in one evaluation.
        stored(
            identifiers_in(Split::Train, 1)[0],
            &NINE,
            Supervision::InPerson,
            false,
        ),
    ];
    let detectors = anticheat::all();
    let evaluation = evaluate(&detectors, &corpus);

    let bots = Group::Synthetic {
        label: "reflex bot".to_owned(),
    };
    assert!(
        evaluation.groups().contains(&bots),
        "the bot match is not in this evaluation, so refusing it proves nothing"
    );
    assert!(
        !evaluation
            .distribution("clock-divergence", &bots)
            .scored
            .is_empty(),
        "the bot group scored nothing, so this test would pass against a basis \
         that refused empty groups rather than synthetic ones"
    );
    assert_eq!(evaluation.basis(&bots), Err(BasisError::Synthetic));
    println!(
        "calibration: a synthetic group with {} scored seat(s) is refused — {}",
        evaluation.distribution("clock-divergence", &bots).count(),
        BasisError::Synthetic
    );
}

/// An empty corpus supports nothing and says so rather than dividing.
#[test]
fn an_empty_corpus_fixes_nothing() {
    let detectors = anticheat::all();
    let evaluation = evaluate(&detectors, &[]);
    let nowhere = Group::Corpus {
        stratum: Stratum {
            supervision: Supervision::InPerson,
            degraded: false,
            full: true,
        },
        split: Split::Train,
    };
    assert_eq!(evaluation.basis(&nowhere), Err(BasisError::Empty));
    let bounds = evaluation.bounds(&nowhere);
    assert_eq!(bounds.style_permille(), None);
    assert_eq!(bounds.circumstance_permille(), None);
}

/// **Fewer than nine people is refused**, and the message says which number is
/// the one that may not be revised.
///
/// `docs/MILESTONES.md` M6's own revision proposal drops the match count from
/// forty to twenty and holds the people count at nine, because the people count
/// is what a behavioural null model is a distribution *over*. A corpus of four
/// people and forty matches costs the same to collect and supports nothing.
#[test]
fn a_corpus_of_fewer_than_nine_people_is_refused_by_name() {
    let few: Vec<&str> = NINE.iter().copied().take(4).collect();
    let corpus: Vec<anticheat::MatchTelemetry> = identifiers_in(Split::Train, 12)
        .into_iter()
        .map(|identifier| stored(identifier, &few, Supervision::InPerson, false))
        .collect();
    let detectors = anticheat::all();
    let evaluation = evaluate(&detectors, &corpus);

    let group = evaluation
        .groups()
        .into_iter()
        .find(|group| !group.is_synthetic())
        .expect("the corpus has a group");
    assert_eq!(
        evaluation.basis(&group),
        Err(BasisError::TooFewPeople {
            found: 4,
            required: MINIMUM_PEOPLE
        }),
        "twelve matches from four people fixed a threshold"
    );
    // …and the bound the corpus *would* support is printed anyway, because
    // "this corpus supports 75% and is still not enough" is the sentence M6's
    // arithmetic is about.
    println!(
        "calibration: 12 matches, 4 people — refused, and the bounds it would have \
         carried are\n{}",
        evaluation.bounds(&group)
    );
}

/// **The gate opens.** Nine people, one stratum, one half of the split.
///
/// The half of this file that has to exist: a refusal nobody has seen accept
/// anything is a refusal that could be refusing everything.
#[test]
fn nine_people_in_one_stratum_can_fix_a_threshold_and_it_carries_both_bounds() {
    let corpus = nine_people_in_person(20);
    let detectors = anticheat::all();
    let evaluation = evaluate(&detectors, &corpus);

    let group = evaluation
        .groups()
        .into_iter()
        .find(|group| !group.is_synthetic())
        .expect("the corpus has a group");
    let basis = evaluation
        .basis(&group)
        .expect("nine people in one stratum-half is a basis");

    assert_eq!(basis.people(), MINIMUM_PEOPLE);
    assert_eq!(basis.matches(), 20);
    assert_eq!(basis.stratum().supervision, Supervision::InPerson);
    assert!(!basis.stratum().degraded && basis.stratum().full);

    // `docs/SCHEMA.md` §8's table, arrived at by the code rather than quoted
    // from the document: 3/9 and 3/20.
    let bounds = basis.bounds();
    assert_eq!(bounds.style_permille(), Some(333));
    assert_eq!(bounds.circumstance_permille(), Some(150));
    println!("calibration: a basis of {basis}\n{bounds}");

    // And with a basis, a threshold can be written down — which is what turns
    // `for_review` from `None` into an answer. Nothing shipped does this.
    let calibrated = Finding {
        reading: Reading::scored(
            "reaction-floor",
            Seat::Blue0,
            Score {
                value: 1,
                unit: "ticks",
            },
            anticheat::Evidence::new(),
        ),
        calibration: Calibration::Fixed(Fixed {
            threshold: 3,
            basis,
            justification: "an illustration in a test, and the only threshold in this \
                            workspace: no detector ships one",
        }),
        tail: Tail::Low,
    };
    assert_eq!(
        calibrated.for_review(),
        Some(true),
        "a calibrated finding below its threshold did not ask for a look"
    );
    let inside = Finding {
        reading: Reading::scored(
            "reaction-floor",
            Seat::Blue0,
            Score {
                value: 9,
                unit: "ticks",
            },
            anticheat::Evidence::new(),
        ),
        ..calibrated
    };
    assert_eq!(inside.for_review(), Some(false));
}

/// **Two supervision strata are two groups, and there is no call that pools
/// them.**
///
/// `docs/SCHEMA.md` §5a: what makes a match human is that somebody was
/// watching, so a distribution over a mixture carries a provenance covariate.
/// The pipeline does not offer a way to build one — a distribution takes a
/// single [`Group`] — and this is the assertion that the mixture really does
/// split rather than quietly landing in one bucket.
#[test]
fn a_mixture_of_supervision_conditions_is_several_distributions_and_never_one() {
    let mut corpus = Vec::new();
    let identifiers = identifiers_in(Split::Train, 4);
    corpus.push(stored(identifiers[0], &NINE, Supervision::InPerson, false));
    corpus.push(stored(identifiers[1], &NINE, Supervision::Remote, false));
    corpus.push(stored(
        identifiers[2],
        &NINE,
        Supervision::Unsupervised,
        false,
    ));
    // …and one that fell behind the tick, which is a fourth group under §5
    // rather than a variation inside the first.
    corpus.push(stored(identifiers[3], &NINE, Supervision::InPerson, true));

    let detectors = anticheat::all();
    let evaluation = evaluate(&detectors, &corpus);
    let groups = evaluation.groups();
    assert_eq!(
        groups.len(),
        4,
        "four matches under four provenances landed in {} group(s): {groups:?}",
        groups.len()
    );
    for group in &groups {
        let bounds = evaluation.bounds(group);
        assert_eq!(
            bounds.matches, 1,
            "{group} pooled more than one match's worth of provenance"
        );
        println!(
            "calibration: {group} -> N = 1 match, {} people",
            bounds.people
        );
    }
}

/// **A quiet seat is an abstention and never a zero**, which is the one place a
/// detector on a low tail would manufacture false positives out of nothing.
///
/// Both reaction detectors read a *low* score as the interesting one, so a seat
/// that produced no reactions at all would score zero on each of them — the same
/// number a bot answering instantly produces — if the absence were not a
/// first-class answer. The dispersion detector is the worse of the two: a player
/// who never right-clicks an enemy has no latencies, and "no variation" is
/// exactly what a scripted delay looks like.
///
/// The matches here have no inputs in them at all, which is the extreme version
/// of the same case and is also what a seat that fights only with skillshots
/// produces.
#[test]
fn a_seat_that_produced_nothing_is_an_abstention_and_never_a_score_of_zero() {
    let corpus = nine_people_in_person(1);
    let detectors = anticheat::all();
    let evaluation = evaluate(&detectors, &corpus);
    let group = evaluation
        .groups()
        .into_iter()
        .find(|group| !group.is_synthetic())
        .expect("the corpus has a group");

    for detector in &detectors {
        let distribution = evaluation.distribution(detector.name(), &group);
        assert_eq!(
            distribution.count(),
            0,
            "{} scored {} seat(s) in a match with no inputs in it; a low-tail \
             detector that scores an absence as zero flags the quietest player in \
             the corpus",
            detector.name(),
            distribution.count()
        );
        assert_eq!(distribution.abstained, 9, "{}", detector.name());
    }

    // …and the abstention says why, because a reviewer handed "no score" and no
    // reason cannot tell a quiet player from a broken extractor.
    let reading = anticheat::detectors::ReactionDispersion.read(&corpus[0], Seat::Blue0);
    let why = reading.abstained.expect("an abstention carries a reason");
    println!("calibration: a quiet seat abstains — {why}");
    assert!(why.contains("appearance"), "{why}");
}

/// The report says the two things `docs/SCHEMA.md` §8 requires of every
/// published statistic, on an empty corpus and on a full one.
#[test]
fn the_report_carries_both_bounds_and_the_sentence_refusing_a_rate_of_zero() {
    let detectors = anticheat::all();

    let empty = format!("{}", evaluate(&detectors, &[]));
    assert!(empty.contains("UNCALIBRATED"), "{empty}");
    assert!(empty.contains("0% false positives"), "{empty}");
    assert!(
        empty.contains("no stratum and no bound"),
        "an empty corpus reported a bound"
    );

    let full = format!("{}", evaluate(&detectors, &nine_people_in_person(20)));
    assert!(
        full.contains("33.3%"),
        "the style bound is missing:\n{full}"
    );
    assert!(
        full.contains("15.0%"),
        "the circumstance bound is missing:\n{full}"
    );
    assert!(full.contains("0% false positives"), "{full}");
    assert!(
        full.contains("No detector emits an action"),
        "the report does not say that a finding is not an action"
    );
    println!("{full}");
}

/// The operator's tool runs, and on an empty corpus it prints the milestone's
/// actual state.
///
/// `docs/RISKS.md` R15's first instance was a binary no test target needed, so
/// `cargo test --workspace` never built it and it existed locally only by
/// accident. This is the test that keeps `anticheat report` from becoming that.
#[test]
fn the_report_binary_runs_on_an_empty_corpus_and_says_so() {
    let directory = std::env::temp_dir().join("moba-m8-empty-corpus");
    std::fs::create_dir_all(&directory).expect("a directory to point at");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_anticheat"))
        .arg("report")
        .arg(&directory)
        .output()
        .expect("run the anticheat tool");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "anticheat report failed on an empty corpus:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("There is no corpus"), "{stdout}");
    for detector in anticheat::all() {
        assert!(
            stdout.contains(detector.name()),
            "{} is missing from the report",
            detector.name()
        );
    }
    println!("{stdout}");
}
