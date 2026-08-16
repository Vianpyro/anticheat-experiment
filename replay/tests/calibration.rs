//! The device profile: what accumulates, what verifies, and what none of it is
//! allowed to block.
//!
//! `replay/src/calibration.rs`'s unit tests cover the arithmetic on data built by
//! hand. This file covers the two things only a corpus can show:
//!
//! - **estimation accumulates**, so a participant's fifth evening is rated
//!   against all five and the last person to join does not have to be calibrated
//!   that night;
//! - **verification is cheap**, so a handful of movements is enough to say that
//!   a device is not the device on record — and saying so is a mark on a seat and
//!   never a refusal.
//!
//! And one measurement rather than an assertion: the estimator is given streams
//! generated at scales it is not told, and the error is printed.

#![deny(unsafe_code)]

use std::path::PathBuf;

use replay::attest::Attested;
use replay::calibration::{
    CalibrationState, DeviceProfileId, Estimate, Observations, Profile, SeatCalibration,
    rate_seats, sufficiency,
};
use replay::consent::{ConsentVersion, Permissions};
use replay::corpus::{ConsentRecord, Corpus};
use replay::keys::SigningKey;
use replay::manifest::{MatchId, Pseudonym, SessionFacts, SimCommit};
use replay::session::{
    Clock, Declared, Measured, Platform, SeatRecord, SessionRecord, Supervision,
};
use replay::{Recording, Replay};
use sim::{Outcome, PLAYER_COUNT, new_state, rules_hash};

/// A corpus in a temporary directory, removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("moba-calibration-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a scratch directory");
        Self(path)
    }

    fn corpus(&self) -> Corpus {
        Corpus::open(self.0.join("corpus"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The key one throwaway corpus is sealed with.
const SEAL_SEED: [u8; 32] = *b"moba calibration drill key.....\0";

fn label(text: &str) -> DeviceProfileId {
    DeviceProfileId::parse(text).expect("a device profile label")
}

fn consent(pseudonym: &str) -> ConsentRecord {
    ConsentRecord {
        pseudonym: pseudonym.to_owned(),
        consented_on: "2026-09-01".to_owned(),
        retention_until: "2028-09-01".to_owned(),
        permissions: Permissions::none(),
        adult: true,
        consent_version: ConsentVersion::current(),
    }
}

fn a_recording() -> Recording {
    let mut state = new_state(11);
    for _ in 0..8 {
        state = sim::step(&state, &[]);
    }
    Recording {
        seed: 11,
        rules_hash: rules_hash(),
        ticks: 8,
        outcome: Outcome::InProgress,
        final_state_digest: state.digest(),
        inputs: Vec::new(),
    }
}
/// The one way through [`Attested::of`], for tests whose subject is a different
/// refusal.
///
/// `Corpus::store` takes a value only `Attested::of` builds, and what that
/// constructor refuses is a seat the replay's input log shows playing that no
/// session record accounts for — a playtest bot, a headless client, a script
/// (`replay/src/attest.rs`). Every fixture here logs no seat the session record
/// leaves empty, so the gate opens; `client/tests/playtest_bots.rs` is where it
/// does not.
fn attested<'a>(replay: &'a Replay, session: &'a SessionRecord) -> Attested<'a> {
    Attested::of(replay, session, None).expect("every seat that played is a person")
}

fn a_replay(match_id: &str, participants: &[&str]) -> Replay {
    let mut identifier = [0u8; 16];
    for (slot, byte) in identifier.iter_mut().zip(match_id.bytes()) {
        *slot = byte;
    }
    let mut slots: [Option<Pseudonym>; PLAYER_COUNT] = [const { None }; PLAYER_COUNT];
    for (slot, who) in slots.iter_mut().zip(participants) {
        *slot = Pseudonym::parse(who);
    }
    replay::seal(
        &a_recording(),
        &SessionFacts {
            match_id: MatchId(identifier),
            started_at_unix_ms: 1_786_000_000_000,
            participants: slots,
            sim_commit: SimCommit::Unknown,
            telemetry: replay::Commitment::Absent,
        },
        &SigningKey::from_seed(SEAL_SEED),
    )
}

fn filed_as(match_id: &str) -> String {
    a_replay(match_id, &[]).manifest.match_id.to_string()
}

/// One seat, on the device `device`, having measured `calibration`.
fn a_seat(device: &str, calibration: SeatCalibration) -> SeatRecord {
    SeatRecord::Human {
        declared: Declared {
            device_profile_id: label(device),
            device_cpi: 800,
            device_polling_hz: 1000,
            pointer_acceleration: false,
        },
        measured: Measured {
            platform: Platform::Linux,
            clock: Clock::Dequeue,
            world_units_per_count_e6: 50_000,
            samples: 91_234,
            motions: 90_880,
            coincident: 0,
            median_gap_ns: 1_000_000,
            budget_ns: 33_333_333,
            passes: 24_010,
            passes_over_budget: 0,
            worst_overrun_ns: 0,
            worst_pass_ns: 5_144_000,
        },
        calibration,
    }
}

fn a_session(match_id: &str, seats: Vec<SeatRecord>) -> SessionRecord {
    let mut slots = [const { SeatRecord::Empty }; PLAYER_COUNT];
    for (slot, seat) in slots.iter_mut().zip(seats) {
        *slot = seat;
    }
    SessionRecord {
        match_id: a_replay(match_id, &[]).manifest.match_id,
        consent_version: ConsentVersion::current(),
        recorded_on: "2026-09-01".to_owned(),
        supervision: Supervision::InPerson,
        seats: slots,
    }
}

/// A deterministic generator, written out because `replay` has one dependency
/// and it is `sim`. Four lines against a crate is not a trade this workspace
/// makes.
struct Draws(u64);

impl Draws {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }

    /// A number in `-spread..spread`.
    fn noise(&mut self, spread: f64) -> f64 {
        (self.next() - 0.5) * 2.0 * spread
    }
}

/// Observations from a hand crossing `reaches` known distances on a device whose
/// true scale is `counts_per_unit`.
///
/// The landing slop and the correction cost go in as a fixed `arrival` term plus
/// noise, which is what the regression has to separate out — and the distances
/// sweep a range, because a set of equal ones is a ratio rather than a fit.
fn a_crossing(
    counts_per_unit: f64,
    arrival: f64,
    reaches: u64,
    seed: u64,
    octants: u8,
) -> Observations {
    let mut draws = Draws(seed);
    let mut out = Observations::new();
    for index in 0..reaches {
        let distance = 20.0 + (index as f64) * 18.0 + draws.noise(4.0);
        let counts = distance * counts_per_unit + arrival + draws.noise(arrival);
        fold(&mut out, distance, counts);
    }
    out.octants = octants;
    out.fast_reaches = reaches / 2;
    out.fast_motions = out.fast_reaches * 100;
    out.fast_ns = out.fast_reaches * 800_000_000;
    out.quantum_e6 = 1_000_000;
    out
}

/// Folds one `(distance, counts)` pair into a set of observations, exactly as
/// `client::lobby::Observations::record` does on the other side of the boundary.
fn fold(out: &mut Observations, distance: f64, counts: f64) {
    let e3 = |value: f64| (value * 1e3).round() as u64;
    if out.min_distance_e3 == 0 || e3(distance) < out.min_distance_e3 {
        out.min_distance_e3 = e3(distance);
    }
    out.max_distance_e3 = out.max_distance_e3.max(e3(distance));
    out.reaches += 1;
    out.sum_distance_e3 += e3(distance);
    out.sum_counts_e3 += e3(counts);
    out.sum_distance_sq_e3 += e3(distance * distance);
    out.sum_distance_counts_e3 += e3(distance * counts);
    out.sum_counts_sq_e3 += e3(counts * counts);
}

/// **The estimator recovers a scale it was not told, and here are the errors.**
///
/// A measurement rather than an assertion. Three devices, three arrival costs,
/// noise on every reach, and the recovered slope printed against the truth that
/// generated it. The tolerance is loose on purpose — what this establishes is
/// that the estimate is the *scale* and not the arrival cost, and a check tight
/// enough to be about the noise would be a check about the generator.
#[test]
fn the_estimator_recovers_a_scale_it_was_never_told() {
    let mut worst: f64 = 0.0;
    for (truth, arrival, seed) in [
        (20.0, 8.0, 1),
        (13.7, 3.0, 2),
        (41.3, 21.0, 3),
        (5.25, 1.5, 4),
    ] {
        let observations = a_crossing(truth, arrival, 24, seed, 0xff);
        let estimate = Estimate::of(&observations).expect("a crossing supports a fit");
        let error = (estimate.counts_per_unit - truth).abs() / truth;
        worst = worst.max(error);
        println!(
            "calibration: true {truth:.2} counts per unit, arrival {arrival:.1} — \
             recovered {:.4} ({:+.4}%), arrival {:.2}, fit {:.5}",
            estimate.counts_per_unit,
            (estimate.counts_per_unit - truth) / truth * 100.0,
            estimate.arrival_counts,
            estimate.fit
        );
        assert!(
            error < 0.01,
            "recovered {:.4} counts per unit against a true {truth:.4}",
            estimate.counts_per_unit
        );
        // And the arrival cost is where the arrival cost went, rather than being
        // folded into the slope — which is what a ratio would have done.
        assert!(
            (estimate.arrival_counts - arrival).abs() < arrival.max(2.0),
            "the arrival cost came out at {:.2} against a true {arrival:.1}",
            estimate.arrival_counts
        );
    }
    println!(
        "calibration: worst relative error over four devices {:.4}%",
        worst * 100.0
    );
}

/// **A participant's profile is the sum of their sessions**, so the evening they
/// arrive late is rated against every evening before it.
#[test]
fn a_profile_accumulates_across_a_participants_sessions() {
    let scratch = Scratch::new("accumulates");
    let corpus = scratch.corpus();
    corpus
        .enrol(&consent("alizarin"), "alizarin@example.invalid")
        .expect("enrol");

    // Two evenings, each of them a partial crossing: enough reaches to fit a
    // scale and not enough to be sufficient on its own.
    let thin = || {
        let mut observations = a_crossing(20.0, 8.0, 6, 5, 0b0000_0111);
        observations.fast_reaches = 1;
        observations.fast_motions = 100;
        observations.fast_ns = 800_000_000;
        observations
    };
    for id in ["2026-09-03-a", "2026-09-11-a"] {
        let seat = a_seat(
            "mouse-a",
            SeatCalibration {
                observations: thin(),
                state: CalibrationState::Partial,
            },
        );
        corpus
            .store(&attested(
                &a_replay(id, &["alizarin"]),
                &a_session(id, vec![seat]),
            ))
            .expect("store");
    }

    let profile = corpus
        .profile_of("alizarin", &label("mouse-a"), None)
        .expect("a profile");
    assert_eq!(profile.sessions, 2, "the two evenings did not both fold in");
    assert_eq!(profile.observations.reaches, 12);
    assert!(
        !profile.sufficient(),
        "two thin evenings were rated as a calibrated device: {:?}",
        profile.shortfall()
    );

    // The third evening is thin too, and is rated `Sufficient` because the first
    // two are on record — which is the whole of what the separation buys.
    let third = a_crossing(20.0, 8.0, 8, 7, 0xff);
    let mut session = a_session(
        "2026-09-18-a",
        vec![a_seat(
            "mouse-a",
            SeatCalibration {
                observations: third,
                state: CalibrationState::Absent,
            },
        )],
    );
    rate_seats(&mut session, &|_, device| {
        corpus
            .profile_of("alizarin", device, None)
            .expect("a profile")
    });
    assert_eq!(
        session.seats[0].calibration(),
        CalibrationState::Sufficient,
        "a third evening on top of two was not enough: {:?}",
        {
            let mut pooled = profile.clone();
            pooled.fold(third);
            pooled.shortfall()
        }
    );

    // …and the same evening, rated on its own, is not.
    assert_eq!(
        CalibrationState::rate(&third, &Profile::empty(label("mouse-a"))),
        CalibrationState::Partial,
        "one evening was enough on its own, so the accumulation above proves \
         nothing"
    );
    println!(
        "calibration: three thin evenings — {} reach(es) pooled, {} octant(s), \
         sufficient at the third",
        profile.observations.reaches + third.reaches,
        (profile.observations.merge(third)).octants_covered()
    );
}

/// **A profile is never pooled across two devices**, which is what the label is
/// for.
#[test]
fn two_devices_under_one_participant_are_two_profiles() {
    let scratch = Scratch::new("two-devices");
    let corpus = scratch.corpus();
    corpus
        .enrol(&consent("bistre"), "bistre@example.invalid")
        .expect("enrol");

    for (id, device) in [("2026-09-03-a", "mouse-a"), ("2026-09-11-a", "mouse-b")] {
        let seat = a_seat(
            device,
            SeatCalibration {
                observations: a_crossing(20.0, 8.0, 10, 9, 0xff),
                state: CalibrationState::Partial,
            },
        );
        corpus
            .store(&attested(
                &a_replay(id, &["bistre"]),
                &a_session(id, vec![seat]),
            ))
            .expect("store");
    }

    for device in ["mouse-a", "mouse-b"] {
        let profile = corpus
            .profile_of("bistre", &label(device), None)
            .expect("a profile");
        assert_eq!(
            profile.sessions, 1,
            "{device} pooled a session recorded on the other device"
        );
        assert_eq!(profile.observations.reaches, 10);
    }
}

/// **A device that changed is a seat marked out of tune, and the match is stored
/// anyway.**
///
/// The cheap half of the separation, and the clause that matters most about it:
/// `Mismatched` is not an accusation and blocks nothing. A mouse replaced
/// between two evenings produces exactly this, and what the corpus does about it
/// is decline to pool two devices under one profile.
#[test]
fn a_session_on_another_device_is_marked_out_of_tune_and_is_still_stored() {
    let scratch = Scratch::new("out-of-tune");
    let corpus = scratch.corpus();
    corpus
        .enrol(&consent("celadon"), "celadon@example.invalid")
        .expect("enrol");

    // A calibrated profile on record: one long evening at twenty counts per unit.
    let calibrated = a_crossing(20.0, 8.0, sufficiency::REACHES + 4, 13, 0xff);
    let first = a_seat(
        "mouse-a",
        SeatCalibration {
            observations: calibrated,
            state: CalibrationState::Sufficient,
        },
    );
    corpus
        .store(&attested(
            &a_replay("2026-09-03-a", &["celadon"]),
            &a_session("2026-09-03-a", vec![first]),
        ))
        .expect("store");
    let profile = corpus
        .profile_of("celadon", &label("mouse-a"), None)
        .expect("a profile");
    assert!(profile.sufficient(), "the fixture is not calibrated");

    // The next evening, on a device reporting half as many counts per unit, and
    // declared under the same label — which is exactly the mistake this exists to
    // catch: a participant who changed mouse and did not say so.
    let changed = a_crossing(10.0, 8.0, 8, 17, 0xff);
    let mut session = a_session(
        "2026-09-11-a",
        vec![a_seat(
            "mouse-a",
            SeatCalibration {
                observations: changed,
                state: CalibrationState::Absent,
            },
        )],
    );
    let filed = filed_as("2026-09-11-a");
    rate_seats(&mut session, &|_, device| {
        corpus
            .profile_of("celadon", device, Some(&filed))
            .expect("a profile")
    });
    assert_eq!(
        session.seats[0].calibration(),
        CalibrationState::Mismatched,
        "a device reporting half as many counts per world unit matched the \
         profile on record"
    );

    // **And nothing is refused.** The match goes into the corpus with the mark
    // on it, because blocking a player for a calibration reason is the shortest
    // path to an anti-cheat that degrades honest play (`docs/SCOPE.md`).
    corpus
        .store(&attested(&a_replay("2026-09-11-a", &["celadon"]), &session))
        .expect("a mismatched seat must not stop a match being stored");
    assert_eq!(
        corpus.session_of(&filed).expect("the stored record").seats[0].calibration(),
        CalibrationState::Mismatched,
        "the state was not the state that was filed"
    );

    // The same evening on the *same* device is not a mismatch, or the check
    // above is a check that fires on everything.
    let same = a_crossing(20.0, 8.0, 8, 19, 0xff);
    assert_eq!(
        CalibrationState::rate(&same, &profile),
        CalibrationState::Sufficient
    );
    println!(
        "calibration: profile {:.3} counts per unit; a session at {:.3} is out of \
         tune and a session at {:.3} is not",
        profile.estimate().expect("a fit").counts_per_unit,
        Estimate::of(&changed).expect("a fit").counts_per_unit,
        Estimate::of(&same).expect("a fit").counts_per_unit
    );
}

/// **A seat nobody has calibrated never blocks anything**, which is the standing
/// decision rather than a property of this implementation.
#[test]
fn a_seat_with_no_calibration_at_all_is_stored_without_complaint() {
    let scratch = Scratch::new("absent");
    let corpus = scratch.corpus();
    corpus
        .enrol(&consent("alizarin"), "alizarin@example.invalid")
        .expect("enrol");

    let mut session = a_session(
        "2026-09-03-a",
        vec![a_seat("mouse-a", SeatCalibration::absent())],
    );
    rate_seats(&mut session, &|_, device| Profile::empty(device.clone()));
    assert_eq!(session.seats[0].calibration(), CalibrationState::Absent);
    assert!(!session.seats[0].calibration().scale_is_known());

    corpus
        .store(&attested(
            &a_replay("2026-09-03-a", &["alizarin"]),
            &session,
        ))
        .expect("an uncalibrated seat must not stop a match being stored");
    assert_eq!(corpus.matches().expect("matches").len(), 1);
}

/// A session record round-trips its two new fields, byte for byte.
#[test]
fn a_record_reads_back_the_device_and_the_state_it_was_written_with() {
    for state in [
        CalibrationState::Sufficient,
        CalibrationState::Partial,
        CalibrationState::Absent,
        CalibrationState::Mismatched,
    ] {
        let observations = a_crossing(20.0, 8.0, 9, 23, 0b1010_1010);
        let session = a_session(
            "2026-09-03-a",
            vec![a_seat(
                "mouse-a-1",
                SeatCalibration {
                    observations,
                    state,
                },
            )],
        );
        let text = session.encode();
        let back = SessionRecord::decode(&text).expect("a record this crate wrote");
        assert_eq!(
            back, session,
            "a session record did not survive a round trip"
        );
        assert_eq!(back.seats[0].calibration(), state);

        // Absence does not decode, which is the rule the rest of this schema
        // already holds: a record written before the field existed must not be
        // readmitted as a record that measured nothing.
        for missing in ["seat.0.calibration_state", "seat.0.device_profile_id"] {
            let stripped: String = text
                .lines()
                .filter(|line| !line.starts_with(missing))
                .map(|line| format!("{line}\n"))
                .collect();
            assert!(
                SessionRecord::decode(&stripped).is_none(),
                "a record with no `{missing}` line decoded"
            );
        }
    }
}
