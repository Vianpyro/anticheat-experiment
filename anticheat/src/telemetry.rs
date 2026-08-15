//! Everything a detector may look at, and where each part of it comes from.
//!
//! # Three sources, and only one of them is a measurement of a hand
//!
//! | Source | What it holds | Trusted? |
//! | --- | --- | --- |
//! | the sealed replay | seed, and one intention per tick with both clocks | the server's, except `claimed_at_ms` |
//! | the session record | what the match was recorded *on*, per seat | a declaration, except what the client measured |
//! | resimulation | what each seat was **shown**, tick by tick | derived, and only as good as `sim` |
//!
//! The third is the one worth being careful about. A replay records no views —
//! `docs/ARCHITECTURE.md` is explicit that a recording carries the seed and the
//! log and nothing else, so that there is no field for delivery order to get
//! into — so "when did this player first see that enemy" has to be re-derived by
//! running the same `step` the server ran and applying `sim::view::view_for` to
//! the result.
//!
//! **That is deliberately *not* `docs/ARCHITECTURE.md` invariant 5's situation,
//! and the difference is worth stating because it looks identical.** Invariant 5
//! forbids a *test* of the projection from calling the projection's own
//! predicate, because a projection that leaks everything satisfies a test that
//! agrees with it. Nothing here is a test of the projection. A detector's claim
//! is "this player could not have known before the server told them", and what
//! the server told them **is** `view_for`'s output — so re-deriving a second
//! visibility rule would be modelling a game the players did not play.
//!
//! # What resimulation cannot recover, and which way the error runs
//!
//! State travels on QUIC datagrams since M3 (`docs/RISKS.md` R6), so **a client
//! can miss a tick**. A resimulation says an enemy was in the view for tick `v`;
//! the player may have seen it for the first time at `v + 1` or `v + 2` because
//! the frame for `v` never arrived. Every reaction latency computed from this is
//! therefore an **under**estimate, and under-estimating a reaction latency is
//! the direction that produces a false positive rather than a miss — which is
//! the expensive direction (`docs/SCOPE.md`: a false positive is worse than a
//! missed cheater).
//!
//! The loss count is a client-side number and the corpus does not carry it. On
//! loopback the observed loss is zero and `client/tests/m3_exit.rs` prints it;
//! over a network it is not. This is one of the reasons `docs/detectors/`
//! records the reaction thresholds as uncalibrated even in the presence of a
//! corpus that has not been recorded yet: what fixes them has to be a corpus
//! that carries both halves.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use replay::session::{Clock, Platform, SeatRecord, SessionRecord, Supervision};
use replay::split::{Split, split_of};
use replay::{MatchId, Recording, Replay, TimedInput};
use sim::view::{EntityView, view_for};
use sim::{Action, EntityId, PLAYER_COUNT, Seat, new_state, step};

/// A distribution's provenance stratum: the three things `docs/SCHEMA.md`
/// refuses to pool.
///
/// One type rather than three fields scattered through the pipeline, because
/// the rule is the same rule in all three cases and a reader has to see it as
/// one:
///
/// - **§5a**, supervision: what makes a match human is that somebody was
///   watching, which is a fact about a person and not a property of a file.
/// - **§5**, degradation: a client that fell behind wrote a *delay* into the
///   record, and an intention decided one pass late looks exactly like a hand
///   that hesitated.
/// - **§6**, occupancy: a match with three absent champions has different
///   fights in it, so anything reading the situation a player was in must not
///   pool it with a full one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stratum {
    /// What the operator wrote down about who was watching.
    pub supervision: Supervision,
    /// Whether any seat's capture loop ever exceeded one tick.
    pub degraded: bool,
    /// Whether all nine seats were occupied.
    pub full: bool,
}

impl core::fmt::Display for Stratum {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{} / {} / {}",
            self.supervision.tag(),
            if self.degraded { "degraded" } else { "healthy" },
            if self.full {
                "nine seats"
            } else {
                "partially filled"
            }
        )
    }
}

/// Where a match came from, which is what decides what may be claimed from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Provenance {
    /// A recorded match from the corpus, with the conditions it was recorded
    /// under and the half of the frozen split it falls in.
    Corpus {
        /// The three things a distribution may not be pooled across.
        stratum: Stratum,
        /// Train or holdout, from `replay::split` — a function of the match
        /// identifier and a frozen salt, stored nowhere.
        split: Split,
    },
    /// A match this repository generated. The exploit suite's bots.
    ///
    /// **Nothing about people may be computed from these**, and that is a
    /// refusal in [`crate::evaluate::Evaluation::basis`] rather than a rule
    /// somebody follows: the bots are here, they run in CI, their scores
    /// separate cleanly, and routing a threshold through them is the shortest
    /// path to a number with no basis at all.
    Synthetic {
        /// What this match was: the exploit it demonstrates, or the control.
        label: String,
    },
}

/// What a seat was played on. `docs/SCHEMA.md` §4.
///
/// The covariate half of `docs/RISKS.md` R14: a mouse at 400 counts per inch
/// and one at 1600 describe the same hand differently, so a detector that reads
/// a **distance or a speed** must divide by [`SeatFacts::device_cpi`] and must
/// say in its page that it rests on a declaration nobody checked. A detector
/// that reads only *times* — which is both of this crate's families — reads
/// none of this, and its page says that instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeatFacts {
    /// Counts per inch, as the participant reported their mouse configured. A
    /// declaration (§4a).
    pub device_cpi: u32,
    /// The device's report rate in hertz, as declared.
    pub device_polling_hz: u32,
    /// World units per device count, in millionths. A build constant.
    pub world_units_per_count_e6: u64,
    /// Which of `docs/ENGINEERING.md`'s three targets.
    pub platform: Platform,
    /// Which clock the client's own samples were stamped from.
    pub clock: Clock,
    /// The median device inter-arrival time, in nanoseconds. The only check
    /// available on [`SeatFacts::device_polling_hz`], and the **whole** of what
    /// the corpus holds about the device stream.
    pub median_gap_ns: u64,
    /// **How well this seat's device is known** (`docs/SCHEMA.md` §4e).
    ///
    /// The one field here that governs whether a detector may answer at all.
    /// A statistic that reads a distance or a speed does so in *device counts*,
    /// and a count is not comparable between two participants until it has been
    /// divided by something — which used to be `device_cpi`, a declaration
    /// nobody checked. Since `client::lobby` there is a measured conversion
    /// instead, and this state says whether the corpus has enough of it.
    ///
    /// The rule, and it is the treatment M8 already gives an uncalibrated
    /// threshold: **a detector that depends on the scale returns `None` for a
    /// seat whose state is not `Sufficient`**, through
    /// [`crate::Reading::abstained`] rather than by scoring it anyway. Nothing
    /// here blocks a match, refuses a store, or acts against anybody; an
    /// insufficiently calibrated seat is one no distance-shaped statistic has an
    /// opinion about.
    ///
    /// Neither of this crate's two detector families reads it, because both read
    /// only *times* — and a page that says so is `docs/detectors/README.md`'s
    /// job rather than a field's.
    pub calibration: replay::calibration::CalibrationState,
}

/// Why a replay and a session record cannot be read together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TelemetryError {
    /// The two files describe different matches.
    MatchIdMismatch {
        /// What the signed manifest says.
        replay: MatchId,
        /// What the session record says.
        session: MatchId,
    },
}

impl core::fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MatchIdMismatch { replay, session } => write!(
                f,
                "the replay names match {replay} and the session record names \
                 {session}: a record filed beside the wrong replay describes the \
                 wrong hardware (docs/SCHEMA.md §4)"
            ),
        }
    }
}

impl core::error::Error for TelemetryError {}

/// Everything a detector may look at, for one match.
#[derive(Clone, Debug)]
pub struct MatchTelemetry {
    /// The match. Named in evidence; a pseudonym never is.
    pub match_id: MatchId,
    /// The seed the world was generated from.
    pub seed: u64,
    /// How many ticks the server ran.
    pub ticks: u32,
    /// Every intention the server accepted, in the order it applied them.
    pub inputs: Vec<TimedInput>,
    /// Which seats a person sat in.
    pub occupied: [bool; PLAYER_COUNT],
    /// The pseudonyms, **for counting distinct participants and nothing else**.
    /// `N` for the style bound is the number of distinct people, so this crate
    /// has to be able to count them; no evidence bundle carries one.
    pub participants: [Option<String>; PLAYER_COUNT],
    /// What each seat was played on.
    pub seats: [Option<SeatFacts>; PLAYER_COUNT],
    /// Where this match came from.
    pub provenance: Provenance,
    /// What each seat was shown, derived on first use.
    ///
    /// A cache and not a second source. Resimulating a thousand ticks costs
    /// milliseconds and two detectors read it for nine seats each, so computing
    /// it eighteen times a match would be paying eighteen times for one answer.
    /// Memoisation does not make this crate impure: the value is a function of
    /// the fields above and of `sim`, and `OnceLock` is what lets `&self` fill
    /// it.
    shown: OnceLock<Sightings>,
}

impl MatchTelemetry {
    /// Reads a stored match: the sealed replay and the session record beside
    /// it.
    ///
    /// # Errors
    ///
    /// [`TelemetryError::MatchIdMismatch`] when the two files describe different
    /// matches. `Corpus::store` refuses that at the door, and this refuses it
    /// again, because a detector handed two files by somebody else is a detector
    /// that should not assume they came through the door.
    pub fn from_corpus(replay: &Replay, session: &SessionRecord) -> Result<Self, TelemetryError> {
        let manifest = &replay.manifest;
        if manifest.match_id != session.match_id {
            return Err(TelemetryError::MatchIdMismatch {
                replay: manifest.match_id,
                session: session.match_id,
            });
        }

        let mut occupied = [false; PLAYER_COUNT];
        let mut seats: [Option<SeatFacts>; PLAYER_COUNT] = [None; PLAYER_COUNT];
        for (index, record) in session.seats.iter().enumerate() {
            if let SeatRecord::Human {
                declared,
                measured,
                calibration,
            } = record
            {
                occupied[index] = true;
                seats[index] = Some(SeatFacts {
                    device_cpi: declared.device_cpi,
                    device_polling_hz: declared.device_polling_hz,
                    world_units_per_count_e6: measured.world_units_per_count_e6,
                    platform: measured.platform,
                    clock: measured.clock,
                    median_gap_ns: measured.median_gap_ns,
                    calibration: calibration.state,
                });
            }
        }

        let participants = core::array::from_fn(|index| {
            manifest
                .participants
                .get(index)
                .and_then(|slot| slot.as_ref())
                .map(ToString::to_string)
        });

        Ok(Self {
            match_id: manifest.match_id,
            seed: manifest.seed,
            ticks: manifest.ticks,
            inputs: replay.inputs.clone(),
            occupied,
            participants,
            seats,
            provenance: Provenance::Corpus {
                stratum: Stratum {
                    supervision: session.supervision,
                    degraded: session.degraded(),
                    full: session.occupied().len() == PLAYER_COUNT,
                },
                split: split_of(manifest.match_id),
            },
            shown: OnceLock::new(),
        })
    }

    /// A match this repository generated, labelled as such.
    ///
    /// The exploit suite's entry point. It takes a [`Recording`] — the
    /// authority's in-memory product — rather than a sealed replay, because
    /// sealing is a signature over a claim about a match somebody played and a
    /// bot match is not one.
    #[must_use]
    pub fn synthetic(recording: &Recording, label: impl Into<String>) -> Self {
        let mut occupied = [false; PLAYER_COUNT];
        for timed in &recording.inputs {
            occupied[timed.input.player.index()] = true;
        }
        Self {
            // A synthetic match is not a corpus match and has no identifier
            // anybody assigned, so it gets one derived from its own seed. That
            // keeps it printable in a report without inventing a fact.
            match_id: MatchId(synthetic_id(recording.seed)),
            seed: recording.seed,
            ticks: recording.ticks,
            inputs: recording.inputs.clone(),
            occupied,
            participants: core::array::from_fn(|_| None),
            seats: [None; PLAYER_COUNT],
            provenance: Provenance::Synthetic {
                label: label.into(),
            },
            shown: OnceLock::new(),
        }
    }

    /// The seats a person sat in.
    #[must_use]
    pub fn seated(&self) -> Vec<Seat> {
        Seat::ALL
            .into_iter()
            .filter(|seat| self.occupied[seat.index()])
            .collect()
    }

    /// What each seat was shown, tick by tick, re-derived from the log.
    #[must_use]
    pub fn shown(&self) -> &Sightings {
        self.shown.get_or_init(|| Sightings::of(self))
    }

    /// This match's inputs from one seat, in order.
    #[must_use]
    pub fn inputs_from(&self, seat: Seat) -> Vec<&TimedInput> {
        self.inputs
            .iter()
            .filter(|timed| timed.input.player == seat)
            .collect()
    }
}

/// A printable identifier for a match nobody assigned one to.
fn synthetic_id(seed: u64) -> [u8; 16] {
    let mut bytes = *b"synthetic-000000";
    bytes[10..].copy_from_slice(&seed.to_be_bytes()[2..]);
    bytes
}

/// When each seat was shown each enemy champion, and when it stopped being.
///
/// Stored as **entries** rather than as a per-tick set: what a reaction is
/// measured from is the moment somebody appeared, and a champion that dies and
/// respawns appears twice. Recording every tick a champion was visible would be
/// a thousand bits per pair to answer a question about a handful of moments.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sightings {
    /// `(observer seat index, target handle) -> the view ticks it entered on`.
    entries: BTreeMap<(usize, u16), Vec<u32>>,
    /// `docs/RISKS.md` R15: how much of the match this was derived over, and
    /// how much of it there was to find. A `Sightings` computed over a match in
    /// which nobody ever saw anybody is not evidence that nobody reacted.
    ticks_examined: u32,
    sightings: u32,
}

impl Sightings {
    /// Re-derives what every occupied seat was shown, by resimulation.
    #[must_use]
    pub fn of(telemetry: &MatchTelemetry) -> Self {
        let ticks = telemetry.ticks as usize;
        let mut buckets: Vec<Vec<sim::Input>> = vec![Vec::new(); ticks];
        for timed in &telemetry.inputs {
            if let Some(bucket) = buckets.get_mut(timed.input.tick.0 as usize) {
                bucket.push(timed.input);
            }
        }

        let seated = telemetry.seated();
        let mut visible_last: Vec<Vec<u16>> = vec![Vec::new(); seated.len()];
        let mut found = Self::default();
        let mut state = new_state(telemetry.seed);

        for bucket in &buckets {
            state = step(&state, bucket);
            // The view a client receives after the step that consumed the
            // inputs stamped `t` carries tick `t + 1`, because `step` raises
            // the tick at the end. So this is the tick the recipient read off
            // the frame, and the earliest tick an answer to it can be stamped
            // with is the same number — the server's next tick.
            let at = state.tick().0;
            found.ticks_examined = found.ticks_examined.saturating_add(1);

            for (index, seat) in seated.iter().enumerate() {
                let view = view_for(&state, *seat);
                let mut now: Vec<u16> = Vec::new();
                for entity in &view.visible {
                    // Champions only. A tower stands at a position the rules
                    // publish and never moves, so "a tower came into view" is a
                    // fact about where the observer walked rather than about
                    // anything appearing; and a projectile is not something an
                    // order can name.
                    if let EntityView::Champion { id, .. } = *entity
                        && is_enemy_champion(*seat, id)
                    {
                        now.push(id.0);
                    }
                }
                for handle in &now {
                    if !visible_last[index].contains(handle) {
                        found
                            .entries
                            .entry((seat.index(), *handle))
                            .or_default()
                            .push(at);
                        found.sightings = found.sightings.saturating_add(1);
                    }
                }
                visible_last[index] = now;
            }
        }
        found
    }

    /// The most recent view tick at or before `tick` on which `target` entered
    /// `observer`'s vision, or `None` if it never had.
    ///
    /// `None` is not "no reaction": it is an order naming somebody this seat
    /// was never shown, which is a different thing and is counted separately.
    #[must_use]
    pub fn entered_by(&self, observer: Seat, target: EntityId, tick: u32) -> Option<u32> {
        self.entries
            .get(&(observer.index(), target.0))?
            .iter()
            .rev()
            .find(|entered| **entered <= tick)
            .copied()
    }

    /// How many ticks were examined, and how many enemy sightings were found in
    /// them.
    ///
    /// `docs/RISKS.md` R15's counter: a detector that abstains because a match
    /// held no reactions and a detector that abstains because the resimulation
    /// found nothing to react *to* are different failures, and only one of them
    /// is about the player.
    #[must_use]
    pub const fn counts(&self) -> (u32, u32) {
        (self.ticks_examined, self.sightings)
    }
}

/// Whether a champion handle names somebody on another team.
///
/// A champion's handle *is* its seat (`docs/ARCHITECTURE.md`), which is what
/// makes this a lookup rather than an inference.
fn is_enemy_champion(observer: Seat, id: EntityId) -> bool {
    seat_of(id).is_some_and(|other| other.team() != observer.team())
}

/// The seat a handle names, if it names one.
pub(crate) fn seat_of(id: EntityId) -> Option<Seat> {
    u8::try_from(id.0).ok().and_then(Seat::from_index)
}

/// The entity an action names, when it names one.
///
/// `Move` and `Skillshot` carry a point and `Idle` carries nothing, so only two
/// of the five actions can be an answer to *somebody* rather than to somewhere
/// — and those two are the ones a player cannot compose without having been
/// shown a handle.
pub(crate) const fn names(action: Action) -> Option<EntityId> {
    match action {
        Action::Attack(id) | Action::Targeted(id) => Some(id),
        Action::Idle | Action::Move(_) | Action::Skillshot(_) => None,
    }
}
