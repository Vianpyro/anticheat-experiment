//! `docs/MILESTONES.md` M8's detectors, each against the exploit it was born
//! with and against the control that is the same match without the behaviour.
//!
//! # What this suite establishes, and the half it cannot
//!
//! Every assertion below is of the form **this detector responds to this
//! behaviour and does not respond to its absence**. That is the R15 discipline
//! applied to detection, and it is the mirror of what `cheat-client`'s own suite
//! does for defences: an exploit that fails against a defence without ever
//! having worked proves nothing, and a detector that fires on an exploit without
//! ever having been quiet proves nothing either. A detector that scored
//! everything would pass a suite made only of attacks.
//!
//! **It is not a false-positive measurement, and no reading of it is.** A
//! control bot is a bot. `docs/SCOPE.md`'s three consequences of a nine-person
//! corpus are what a false-positive number would have to come from, and there is
//! no corpus — so every threshold in `anticheat` is `Uncalibrated`, the last
//! test here asserts that none of them can say whether a reading is for review,
//! and `docs/detectors/` states the same thing in prose beside the two bounds.
//!
//! # The three variants and what each of them is for
//!
//! | Variant | reaction-floor | reaction-dispersion | clock-divergence |
//! | --- | --- | --- | --- |
//! | `Immediate` | **the exploit** | responds too | — |
//! | `Scripted(7)` | control | **the exploit** | — |
//! | `Jittered { 8, 2 }` | quiet | quiet | — |
//! | `Scaled { 1/2 }` | — | — | **the exploit** |
//! | `Honest` | — | — | control |
//!
//! The third row is the one worth reading twice. `Jittered` is
//! `docs/SCOPE.md`'s ceiling as close as `docs/RISKS.md` R7 lets this project
//! come to it: a bot whose decisions carry plausible variability, composed into
//! intentions and sent over the wire, with **no synthesised device input
//! anywhere** — because the layer that would move a real mouse is the one
//! component of a cheat that generalises, and it is deliberately not written.
//! Nothing here catches it, and that green is the honest half of this milestone.

#![deny(unsafe_code)]

use anticheat::telemetry::MatchTelemetry;
use anticheat::{Detector, Score};
use cheat_client::bot::{ClaimedClock, Reflexes};
use replay::manifest::Build;
use sim::Seat;

#[path = "harness/played.rs"]
mod played;

use played::{Played, TICK_MS, TICKS, play};

/// The seed every variant plays, so that the four matches differ in the
/// behaviour under test and in nothing else.
const SEED: u64 = 0x0BEE_5EED_0BEE_5EED;

/// The offset every claimed clock carries.
///
/// Two machines do not agree on the epoch, so an honest client's claimed
/// timestamps are the server's plus some constant. It is here on **both** the
/// exploit and the control precisely so that a detector reading the offset
/// rather than the rate would fail this suite: such a detector would flag every
/// honest player in a corpus.
const EPOCH_OFFSET_MS: u64 = 1_786_000_000_000;

/// A plausible-looking scripted delay: seven ticks is 233 ms.
const SCRIPTED_TICKS: u32 = 7;

/// The ceiling's centre and spread: eight ticks either side by two, so 200 ms
/// to 333 ms.
const CEILING_CENTRE: u32 = 8;
const CEILING_SPREAD: u32 = 2;

const HONEST: ClaimedClock = ClaimedClock::Honest {
    offset_ms: EPOCH_OFFSET_MS,
};

fn immediate() -> Played {
    play("reflex bot", SEED, Reflexes::Immediate, HONEST)
}

fn scripted() -> Played {
    play(
        "scripted-delay bot",
        SEED,
        Reflexes::Scripted(SCRIPTED_TICKS),
        HONEST,
    )
}

fn ceiling() -> Played {
    play(
        "jittered bot (the ceiling)",
        SEED,
        Reflexes::Jittered {
            centre: CEILING_CENTRE,
            spread: CEILING_SPREAD,
            seed: 0x5EED_1234_5678_9ABC,
        },
        HONEST,
    )
}

fn slow_clock() -> Played {
    play(
        "half-speed-clock bot",
        SEED,
        Reflexes::Scripted(SCRIPTED_TICKS),
        ClaimedClock::Scaled {
            offset_ms: EPOCH_OFFSET_MS,
            numerator: 1,
            denominator: 2,
        },
    )
}

fn telemetry(played: &Played) -> MatchTelemetry {
    MatchTelemetry::synthetic(&played.recording, played.label)
}

/// Every seat's score under one detector, abstentions dropped.
fn scores(detector: &dyn Detector, telemetry: &MatchTelemetry) -> Vec<i64> {
    let mut found: Vec<i64> = telemetry
        .seated()
        .into_iter()
        .filter_map(|seat| {
            detector
                .read(telemetry, seat)
                .score
                .map(|score: Score| score.value)
        })
        .collect();
    found.sort_unstable();
    found
}

/// How many seats a detector declined to score.
fn abstentions(detector: &dyn Detector, telemetry: &MatchTelemetry) -> usize {
    telemetry
        .seated()
        .into_iter()
        .filter(|seat| detector.read(telemetry, *seat).score.is_none())
        .count()
}

/// Prints one variant's scores under one detector, with the abstentions.
fn report(detector: &dyn Detector, telemetry: &MatchTelemetry, label: &str) -> Vec<i64> {
    let found = scores(detector, telemetry);
    println!(
        "{}: {label} -> {found:?} ({} abstained)",
        detector.name(),
        abstentions(detector, telemetry)
    );
    found
}

// ---------------------------------------------------------------------------
// The antecedent, before any detector is allowed to mean anything
// ---------------------------------------------------------------------------

/// **The exploits work.** Every variant plays a whole match, the server accepts
/// every frame, and the replay verifies.
///
/// `docs/RISKS.md` R15 applied to attacks, which is `docs/SCOPE.md`'s standing
/// requirement since M7: a detector responding to a bot the server would have
/// refused anyway is a detector responding to a class-5 exploit wearing a
/// class-3 label. Nothing delivered catches any of these, which is class 3's
/// verdict in `docs/SCOPE.md` and is why it is still "No, and correctly".
///
/// It also asserts what the fixture *reached*, because a match in which nobody
/// ever saw anybody would make every detector below abstain, and nine
/// abstentions look exactly like nine clean seats.
#[test]
fn every_variant_plays_a_whole_match_that_nothing_delivered_catches() {
    let key = replay::SigningKey::from_seed(*b"moba m8 detector harness key\0\0\0\0");
    let mut registry = replay::KeyRegistry::new();
    registry.insert(key.verifying(), replay::KeyStatus::Active, "harness");

    for (index, played) in [immediate(), scripted(), ceiling(), slow_clock()]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            played.refused, 0,
            "{}: the server refused {} frame(s); a bot sends nothing illegal",
            played.label, played.refused
        );
        assert!(
            played.recording.ticks >= TICKS,
            "{}: the match ran {} ticks",
            played.label,
            played.recording.ticks
        );

        let mut identifier = *b"m8-variant-00000";
        identifier[11] = b'0' + u8::try_from(index).unwrap_or(0);
        let sealed = replay::seal(
            &played.recording,
            &replay::SessionFacts::anonymous(replay::MatchId(identifier), EPOCH_OFFSET_MS),
            &key,
        );
        replay::verify(&sealed, &registry, &Build::current()).unwrap_or_else(|error| {
            panic!(
                "{}: the bot's replay did not verify ({error}) — a bot's inputs \
                 resimulate perfectly and that is class 3's whole verdict",
                played.label
            )
        });

        // What the fixture reached (docs/RISKS.md R15).
        let telemetry = telemetry(&played);
        let (ticks_examined, sightings) = telemetry.shown().counts();
        let per_seat: Vec<usize> = telemetry
            .seated()
            .into_iter()
            .map(|seat| {
                anticheat::features::Reactions::extract(&telemetry, seat)
                    .pairs
                    .len()
            })
            .collect();
        let pairs: usize = per_seat.iter().sum();
        let leanest = per_seat.iter().copied().min().unwrap_or(0);
        println!(
            "{}: {} ticks resimulated, {sightings} enemy sighting(s), {} answered by \
             the bots, {pairs} reaction pair(s) extracted {per_seat:?}, replay VERIFIED",
            played.label, ticks_examined, played.answers
        );
        assert!(
            sightings >= 27,
            "{}: only {sightings} enemy sighting(s) in {ticks_examined} ticks, so the \
             detectors below would be abstaining rather than agreeing \
             (docs/RISKS.md R15)",
            played.label
        );
        // Per seat rather than in total, because a total of forty-five over
        // nine seats is satisfied by one seat that fought and eight that did
        // not — and a detector that abstains on eight seats looks exactly like
        // a detector that cleared them. Five is the dispersion detector's own
        // minimum, so this is the antecedent of the assertions below rather
        // than a number chosen to be met.
        assert!(
            leanest >= 5,
            "{}: the leanest seat produced {leanest} reaction pair(s) of {pairs} \
             across nine ({per_seat:?}), and the dispersion detector needs five \
             before it will score at all — so this fixture would be measuring \
             abstentions (docs/RISKS.md R15)",
            played.label
        );
    }
}

// ---------------------------------------------------------------------------
// One detector, one exploit, one control
// ---------------------------------------------------------------------------

/// **reaction-floor.** A bot that answers on the view that showed it has a floor
/// of zero ticks; the same bot with a scripted delay does not.
#[test]
fn the_reaction_floor_responds_to_an_instant_answer_and_not_to_a_delayed_one() {
    let detector = anticheat::detectors::ReactionFloor;

    let exploit = telemetry(&immediate());
    let control = telemetry(&scripted());
    let fast = report(&detector, &exploit, "reflex bot");
    let slow = report(&detector, &control, "scripted-delay bot (control)");

    assert!(!fast.is_empty() && !slow.is_empty(), "nothing was scored");
    assert!(
        fast.iter().all(|floor| *floor == 0),
        "the reflex bot's floors are {fast:?} and every one of them should be zero: \
         it answers on the very view that showed it"
    );
    assert!(
        slow.iter().all(|floor| *floor >= i64::from(SCRIPTED_TICKS)),
        "the control's floors are {slow:?} against a scripted delay of \
         {SCRIPTED_TICKS} ticks; a control that scored like the exploit would mean \
         this detector reads something other than the delay"
    );

    // The separation, stated in the unit a reader thinks in. It is a gap and
    // not a threshold: what would turn it into one is nine people's own floors
    // (docs/MILESTONES.md M6).
    let gap = slow[0] - fast[fast.len() - 1];
    println!(
        "reaction-floor: exploit max {} ticks, control min {} ticks, gap {gap} ticks \
         ({} ms) — and no threshold, because no corpus",
        fast[fast.len() - 1],
        slow[0],
        anticheat::features::ticks_to_ms(u32::try_from(gap).unwrap_or(0))
    );
    assert!(gap > 0, "the exploit and its control are not separated");
}

/// **reaction-dispersion.** A scripted delay has no trial-to-trial variability;
/// a drawn one does.
#[test]
fn the_reaction_dispersion_responds_to_a_constant_delay_and_not_to_a_drawn_one() {
    let detector = anticheat::detectors::ReactionDispersion;

    let exploit = telemetry(&scripted());
    let control = telemetry(&ceiling());
    let constant = report(&detector, &exploit, "scripted-delay bot");
    let drawn = report(&detector, &control, "jittered bot (control)");

    assert!(
        !constant.is_empty() && !drawn.is_empty(),
        "nothing was scored"
    );
    assert!(
        constant.iter().all(|spread| *spread == 0),
        "the scripted bot's spreads are {constant:?} and every one should be zero: \
         every answer took exactly the same number of ticks"
    );
    assert!(
        drawn.iter().all(|spread| *spread > 0),
        "the drawn bot's spreads are {drawn:?} and a draw over five ticks that \
         produced no variation at all would mean the generator, not the detector, \
         is what this test is measuring"
    );
    println!(
        "reaction-dispersion: exploit {constant:?}, control min {} hundredths of a \
         tick — and no threshold, because a human spread of 40 ms is 1.2 ticks and \
         this record quantises to whole ones",
        drawn[0]
    );
}

/// **clock-divergence.** A client whose clock runs at half the server's rate
/// reports a rate error of about −500 000 ppm; an honest one with a wildly
/// different epoch reports none.
#[test]
fn the_clock_divergence_responds_to_a_slowed_clock_and_not_to_an_offset_one() {
    let detector = anticheat::detectors::ClockDivergence;

    let exploit = telemetry(&slow_clock());
    let control = telemetry(&scripted());
    let slowed = report(&detector, &exploit, "half-speed-clock bot");
    let honest = report(&detector, &control, "honest-clock bot (control)");

    assert!(
        !slowed.is_empty() && !honest.is_empty(),
        "nothing was scored"
    );
    assert!(
        slowed.iter().all(|ppm| (*ppm - 500_000).abs() < 10_000),
        "the slowed clock's rate errors are {slowed:?} ppm and a clock running at \
         half the server's rate is −500 000 ppm"
    );
    // The control's epoch is a trillion milliseconds away from the server's and
    // its rate error is nonetheless under the resolution of the measurement.
    // That is the assertion that makes this detector about a *rate*: a version
    // reading the offset would put the control at 10^12 and pass every test that
    // only looked at the exploit.
    let resolution = 2_000_000i64 / i64::try_from(u64::from(TICKS) * TICK_MS).unwrap_or(1);
    assert!(
        honest.iter().all(|ppm| *ppm <= resolution),
        "the honest clock's rate errors are {honest:?} ppm against a measurement \
         resolution of {resolution} ppm, and its epoch is {EPOCH_OFFSET_MS} ms from \
         the server's: this detector must read the rate and not the offset"
    );
    println!(
        "clock-divergence: exploit {slowed:?} ppm, control {honest:?} ppm, \
         resolution {resolution} ppm over a {}-tick match — and no threshold, \
         because nobody has watched nine real machines drift",
        TICKS
    );
}

/// **The ceiling.** The bot with human-plausible reflexes is caught by neither
/// reaction detector, and that is `docs/SCOPE.md`'s stated limit executed rather
/// than asserted.
#[test]
fn neither_reaction_detector_separates_the_ceiling_from_plausible_play() {
    let ceiling = telemetry(&ceiling());
    let floors = report(&anticheat::detectors::ReactionFloor, &ceiling, "ceiling");
    let spreads = report(
        &anticheat::detectors::ReactionDispersion,
        &ceiling,
        "ceiling",
    );

    let lowest = CEILING_CENTRE.saturating_sub(CEILING_SPREAD);
    assert!(
        floors.iter().all(|floor| *floor >= i64::from(lowest)),
        "the ceiling's floors are {floors:?} and its shortest draw is {lowest} ticks"
    );
    assert!(
        spreads.iter().all(|spread| *spread > 0),
        "the ceiling's spreads are {spreads:?}"
    );
    println!(
        "the ceiling: floors {floors:?} ticks ({} ms at the fastest), spreads \
         {spreads:?} hundredths of a tick. Both detectors are quiet, and a \
         threshold placed to catch this would have to sit inside the range a \
         person occupies — which is the limit docs/SCOPE.md states and does not \
         defend.",
        anticheat::features::ticks_to_ms(lowest)
    );
}

// ---------------------------------------------------------------------------
// And the thing none of them may do
// ---------------------------------------------------------------------------

/// **No detector here can say whether a reading is for review**, because no
/// corpus has fixed a threshold.
///
/// The separations above are real and they are not a calibration. This is the
/// assertion that keeps the two apart, and it is deliberately the last thing in
/// the file: a reader who has just seen four clean separations is exactly the
/// reader who would conclude the wrong thing.
#[test]
fn no_detector_can_say_whether_a_reading_is_for_review() {
    let variants = [immediate(), scripted(), ceiling(), slow_clock()];
    let mut checked = 0u32;
    for played in &variants {
        let telemetry = telemetry(played);
        for detector in anticheat::all() {
            for seat in telemetry.seated() {
                let finding = detector.finding(&telemetry, seat);
                assert_eq!(
                    finding.for_review(),
                    None,
                    "{} answered a review question about {:?} in {}. No threshold \
                     calibrated on nine people supports a decision and no corpus \
                     exists to have calibrated one (docs/SCOPE.md, \
                     docs/MILESTONES.md M6)",
                    detector.name(),
                    seat,
                    played.label
                );
                checked = checked.saturating_add(1);
            }
        }
    }
    assert!(
        checked >= 4 * 3 * 9,
        "only {checked} findings were checked (docs/RISKS.md R15)"
    );
    println!(
        "{checked} findings across four variants, three detectors and nine seats: \
         every one of them answers `None` to \"should a person look at this\", \
         because nothing here has a threshold"
    );
    // And the evidence bundle is what a person would read instead.
    let example = telemetry(&variants[0]);
    let finding = anticheat::detectors::ReactionFloor.finding(&example, Seat::Blue0);
    println!("an evidence bundle, which is what ships instead of a verdict:\n{finding}");
    assert!(
        format!("{finding}").contains("not a judgement"),
        "a finding rendered without saying it is not a judgement"
    );
}
