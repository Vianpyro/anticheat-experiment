//! The session record: what a match was recorded *on*, seat by seat.
//!
//! `docs/SCHEMA.md` is the schema this type implements and the document to read
//! first. What belongs here is the reasoning that is about the code.
//!
//! # Why there is a second file beside the replay at all
//!
//! A replay answers "what happened". It cannot answer "what was it recorded
//! through", and M6 is the milestone at which that question stops being idle:
//! **a mouse at 400 counts per inch and one at 1600 describe the same hand
//! differently**, and a corpus that does not record which is which will read a
//! difference of hardware as a difference of style. The same goes for the
//! platform, for the sensitivity the build applied, and for whether the client
//! kept up with the tick while it recorded (`docs/RISKS.md` R16).
//!
//! None of that can go in the manifest. M5 froze that format, the signature
//! covers it, and every field in it is something the *authority* knows — the
//! server has no idea what mouse anybody is using, and a field the operator
//! filled in inside the server's signature would be the server attesting to
//! something it did not observe.
//!
//! # Why it is not an index, which is the question M5 taught this project to ask
//!
//! `docs/CONSENT.md` records the M5 lesson: the way a destruction promise fails
//! is a *derived* artefact that outlives what it was derived from. This file is
//! not derived from anything — it is primary, and it is the only statement of
//! facts nothing else holds — and it is filed **inside the match directory**, so
//! the single `remove_dir_all` a withdrawal already performs destroys it.
//!
//! And it is indexed by **seat, never by pseudonym**. The manifest is the one
//! place a person is named and it is inside the signature; a second naming here
//! would be exactly the `participants` file M5 deleted, in a new place. What
//! connects a seat to a person is the manifest, and the two are read together or
//! not at all.
//!
//! The guard against a future version of this file drifting from its replay is
//! not a rule anybody has to remember. `Corpus::store` refuses a session record
//! that names another match, and `Corpus::audit` reports a match directory whose
//! replay or session record does not decode — unconditionally, for every
//! pseudonym, because a seat record with no manifest beside it is somebody's
//! session and nobody can say whose.
//!
//! # What is deliberately not in it
//!
//! - **No pseudonym**, for the reason above.
//! - **No device identifier, model or serial.** A mouse model is close to a
//!   hardware identifier and `docs/CONSENT.md` promises none is collected. What a
//!   detector needs is the scale, and the scale is a number.
//! - **No score, no summary, no derived statistic.** Everything of that shape is
//!   a function of the corpus and is computed by `replay census` when somebody
//!   asks, printed and not stored — a stored summary is an index with a friendly
//!   name.
//! - **No wall-clock times beyond the day.** The manifest already carries the
//!   match's start in milliseconds; a second clock here would be a second thing
//!   to keep in step.

use sim::PLAYER_COUNT;

use crate::consent::ConsentVersion;
use crate::manifest::MatchId;

/// The magic line a session record carries.
pub const FORMAT: &str = "moba/session/v1";

/// The magic line one client's part carries, written by `client::health`.
pub const PART_FORMAT: &str = "moba/session-part/v1";

/// What was sitting in a seat.
///
/// Two values, and the absence of a third is the point. There is no `Bot`, no
/// `Script` and no `Replayed`: `docs/SCHEMA.md` excludes them from the corpus
/// outright, so the schema has no way to say them and `Corpus::store` has no case
/// to accept. A synthetic seat is not a seat this corpus can describe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provenance {
    /// A person, playing.
    Human,
    /// Nobody. The champion stands at its base taking no orders, which is an
    /// ordinary state for the rules and is what M4's own criterion ran on.
    Empty,
}

/// What a participant was asked, and could not be measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Declared {
    /// Counts per inch, as the participant reports their mouse configured.
    pub device_cpi: u32,
    /// The device's report rate in hertz, as the participant reports it.
    pub device_polling_hz: u32,
    /// Whether the operating system's pointer acceleration was left on. Required
    /// to be `false`; see [`SessionRecord::accelerated_seats`].
    pub pointer_acceleration: bool,
}

/// What the client observed about its own session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Measured {
    /// `linux`, `windows`, `macos` or `other`, as a fixed tag.
    pub platform: Platform,
    /// Which clock the sample timestamps came from
    /// (`client::input::CLOCK`).
    pub clock: Clock,
    /// World units per device count, in millionths. A build constant rather than
    /// a setting, recorded because a build that changes it changes what a
    /// recorded aim means.
    pub world_units_per_count_e6: u64,
    /// Device events recorded, of every kind. **Zero is refused**: a seat that
    /// produced no device event was not a person at a keyboard.
    pub samples: u64,
    /// Motion events among them.
    pub motions: u64,
    /// Consecutive identical motions closer together than a device produces,
    /// which is a platform delivering one event twice
    /// (`client::input::TraceStats::coincident`).
    pub coincident: u64,
    /// The median inter-arrival time, in nanoseconds. Read against
    /// [`Declared::device_polling_hz`], which is the only check available on a
    /// declaration.
    pub median_gap_ns: u64,
    /// The budget one pass of the capture loop was measured against.
    pub budget_ns: u64,
    /// Passes of the loop.
    pub passes: u64,
    /// Passes that exceeded the budget. Non-zero makes the session degraded.
    pub passes_over_budget: u64,
    /// The worst overrun, in nanoseconds beyond the budget.
    pub worst_overrun_ns: u64,
    /// The longest single pass.
    pub worst_pass_ns: u64,
}

/// The platform a session was recorded on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    /// Linux, X11 or Wayland.
    Linux,
    /// Windows.
    Windows,
    /// macOS.
    MacOs,
    /// Something outside `docs/ENGINEERING.md`'s matrix.
    Other,
}

impl Platform {
    /// The tag this platform is written as.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::MacOs => "macos",
            Self::Other => "other",
        }
    }

    /// The platform this tag names, or `None`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "linux" => Some(Self::Linux),
            "windows" => Some(Self::Windows),
            "macos" => Some(Self::MacOs),
            "other" => Some(Self::Other),
            _ => None,
        }
    }
}

/// Where a sample's timestamp came from, as the corpus records it.
///
/// The mirror of `client::input::Clock`, and it exists here for the reason that
/// constant exists there: a corpus spanning a build that gained a device
/// timestamp can be split rather than silently pooled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clock {
    /// The device's own timestamp. Nothing produces one today.
    Device,
    /// The moment the client dequeued the event from the platform.
    Dequeue,
}

impl Clock {
    /// The tag this clock is written as.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Device => "device",
            Self::Dequeue => "dequeue",
        }
    }

    /// The clock this tag names, or `None`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "device" => Some(Self::Device),
            "dequeue" => Some(Self::Dequeue),
            _ => None,
        }
    }
}

/// One seat of one match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeatRecord {
    /// Nobody sat here.
    Empty,
    /// A person did, and this is what they were playing on.
    Human {
        /// What they were asked.
        declared: Declared,
        /// What their client observed.
        measured: Measured,
    },
}

impl SeatRecord {
    /// Whether this seat fell behind the tick at any point.
    #[must_use]
    pub const fn degraded(&self) -> bool {
        match self {
            Self::Empty => false,
            Self::Human { measured, .. } => measured.passes_over_budget > 0,
        }
    }

    /// One client's part, as `client::health::SessionPart::encode` writes it.
    ///
    /// Total on every string, in the same spirit as `protocol`'s decoders and
    /// `Manifest::decode`: this file arrives from another process, and the
    /// answer to a file that is not one is `None` rather than a partial record.
    ///
    /// # The seat is returned rather than assumed
    ///
    /// A part names its own seat and the caller is the one that knows which seat
    /// it asked for, so returning the pair is what lets `SessionRecord::assemble`
    /// refuse a part filed under the wrong name — which is the mistake an
    /// operator collecting nine files from nine machines will actually make.
    #[must_use]
    pub fn decode_part(text: &str) -> Option<(usize, Self)> {
        let field = |name: &str| -> Option<&str> {
            text.lines()
                .find_map(|line| line.strip_prefix(&format!("{name}: ")))
                .map(str::trim)
        };
        let number = |name: &str| -> Option<u64> { field(name)?.parse::<u64>().ok() };

        if field("format")? != PART_FORMAT {
            return None;
        }
        let seat = usize::try_from(number("seat")?).ok()?;
        if seat >= PLAYER_COUNT {
            return None;
        }
        if field("provenance")? != "human" {
            // Deliberately not a variant. `docs/SCHEMA.md` excludes synthetic
            // play from this corpus, so a part that claims to be anything but a
            // person is refused at the parser rather than carried to a check
            // somebody might forget to run.
            return None;
        }
        let declared = Declared {
            device_cpi: u32::try_from(number("device_cpi")?).ok()?,
            device_polling_hz: u32::try_from(number("device_polling_hz")?).ok()?,
            pointer_acceleration: match field("pointer_acceleration")? {
                "on" => true,
                "off" => false,
                _ => return None,
            },
        };
        let measured = Measured {
            platform: Platform::parse(field("platform")?)?,
            clock: Clock::parse(field("clock")?)?,
            world_units_per_count_e6: number("world_units_per_count_e6")?,
            samples: number("samples")?,
            motions: number("motions")?,
            coincident: number("coincident")?,
            median_gap_ns: number("median_gap_ns")?,
            budget_ns: number("budget_ns")?,
            passes: number("passes")?,
            passes_over_budget: number("passes_over_budget")?,
            worst_overrun_ns: number("worst_overrun_ns")?,
            worst_pass_ns: number("worst_pass_ns")?,
        };
        Some((seat, Self::Human { declared, measured }))
    }
}

/// A match's session record: one per match, filed beside the replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRecord {
    /// The match this describes. Checked against the manifest's, so a record
    /// filed beside the wrong replay is a refusal rather than a silent mismatch.
    pub match_id: MatchId,
    /// The consent document this session was operated under.
    pub consent_version: ConsentVersion,
    /// The day it was recorded, `YYYY-MM-DD`. The manifest carries the
    /// millisecond; this is what an operator writes on a form.
    pub recorded_on: String,
    /// One entry per seat, in seat order.
    pub seats: [SeatRecord; PLAYER_COUNT],
}

impl SessionRecord {
    /// Builds a record from the parts nine clients wrote.
    ///
    /// # Errors
    ///
    /// A message naming what was wrong: a part that is not one, a part filed
    /// under a seat it does not claim, or two parts claiming one seat. Seats no
    /// part was collected for are [`SeatRecord::Empty`], which is what makes a
    /// partially filled match expressible — and `Corpus::store` is what checks
    /// that the empty seats are the ones the manifest also leaves empty.
    pub fn assemble(
        match_id: MatchId,
        consent_version: ConsentVersion,
        recorded_on: &str,
        parts: &[(String, String)],
    ) -> Result<Self, String> {
        let mut seats = [SeatRecord::Empty; PLAYER_COUNT];
        for (name, text) in parts {
            let (seat, record) = SeatRecord::decode_part(text)
                .ok_or_else(|| format!("{name} is not a session part"))?;
            let slot = seats
                .get_mut(seat)
                .ok_or_else(|| format!("{name} claims seat {seat}, which does not exist"))?;
            if !matches!(slot, SeatRecord::Empty) {
                return Err(format!(
                    "two parts claim seat {seat}; one of them is {name}"
                ));
            }
            *slot = record;
        }
        Ok(Self {
            match_id,
            consent_version,
            recorded_on: recorded_on.to_owned(),
            seats,
        })
    }

    /// The record as it is stored: one `key: value` per line, seats prefixed.
    ///
    /// Hand-written by exhaustive destructuring, in this workspace's usual style
    /// and for its usual reason: a field added to the schema and not written is a
    /// field the corpus does not have, and the `let … = ` patterns below stop
    /// compiling when somebody adds one.
    #[must_use]
    pub fn encode(&self) -> String {
        let Self {
            match_id,
            consent_version,
            recorded_on,
            seats,
        } = self;
        let mut out = String::new();
        out.push_str(&format!("format: {FORMAT}\n"));
        out.push_str(&format!("match_id: {match_id}\n"));
        out.push_str(&format!("consent_version: {consent_version}\n"));
        out.push_str(&format!("recorded_on: {recorded_on}\n"));
        for (index, seat) in seats.iter().enumerate() {
            match seat {
                SeatRecord::Empty => {
                    out.push_str(&format!("seat.{index}.provenance: empty\n"));
                }
                SeatRecord::Human { declared, measured } => {
                    let Declared {
                        device_cpi,
                        device_polling_hz,
                        pointer_acceleration,
                    } = declared;
                    let Measured {
                        platform,
                        clock,
                        world_units_per_count_e6,
                        samples,
                        motions,
                        coincident,
                        median_gap_ns,
                        budget_ns,
                        passes,
                        passes_over_budget,
                        worst_overrun_ns,
                        worst_pass_ns,
                    } = measured;
                    let mut put = |key: &str, value: &str| {
                        out.push_str(&format!("seat.{index}.{key}: {value}\n"));
                    };
                    put("provenance", "human");
                    put("device_cpi", &device_cpi.to_string());
                    put("device_polling_hz", &device_polling_hz.to_string());
                    put(
                        "pointer_acceleration",
                        if *pointer_acceleration { "on" } else { "off" },
                    );
                    put("platform", platform.tag());
                    put("clock", clock.tag());
                    put(
                        "world_units_per_count_e6",
                        &world_units_per_count_e6.to_string(),
                    );
                    put("samples", &samples.to_string());
                    put("motions", &motions.to_string());
                    put("coincident", &coincident.to_string());
                    put("median_gap_ns", &median_gap_ns.to_string());
                    put("budget_ns", &budget_ns.to_string());
                    put("passes", &passes.to_string());
                    put("passes_over_budget", &passes_over_budget.to_string());
                    put("worst_overrun_ns", &worst_overrun_ns.to_string());
                    put("worst_pass_ns", &worst_pass_ns.to_string());
                }
            }
        }
        out
    }

    /// Reads a record back, or `None` if these are not the lines of one.
    #[must_use]
    pub fn decode(text: &str) -> Option<Self> {
        let field = |name: &str| -> Option<&str> {
            text.lines()
                .find_map(|line| line.strip_prefix(&format!("{name}: ")))
                .map(str::trim)
        };
        if field("format")? != FORMAT {
            return None;
        }
        let match_id = MatchId::parse(field("match_id")?)?;
        let consent_version = ConsentVersion::parse(field("consent_version")?)?;
        let recorded_on = field("recorded_on")?.to_owned();

        let mut seats = [SeatRecord::Empty; PLAYER_COUNT];
        for (index, slot) in seats.iter_mut().enumerate() {
            let at = |key: &str| -> Option<&str> { field(&format!("seat.{index}.{key}")) };
            let number = |key: &str| -> Option<u64> { at(key)?.parse::<u64>().ok() };
            match at("provenance")? {
                "empty" => {}
                "human" => {
                    *slot = SeatRecord::Human {
                        declared: Declared {
                            device_cpi: u32::try_from(number("device_cpi")?).ok()?,
                            device_polling_hz: u32::try_from(number("device_polling_hz")?).ok()?,
                            pointer_acceleration: match at("pointer_acceleration")? {
                                "on" => true,
                                "off" => false,
                                _ => return None,
                            },
                        },
                        measured: Measured {
                            platform: Platform::parse(at("platform")?)?,
                            clock: Clock::parse(at("clock")?)?,
                            world_units_per_count_e6: number("world_units_per_count_e6")?,
                            samples: number("samples")?,
                            motions: number("motions")?,
                            coincident: number("coincident")?,
                            median_gap_ns: number("median_gap_ns")?,
                            budget_ns: number("budget_ns")?,
                            passes: number("passes")?,
                            passes_over_budget: number("passes_over_budget")?,
                            worst_overrun_ns: number("worst_overrun_ns")?,
                            worst_pass_ns: number("worst_pass_ns")?,
                        },
                    };
                }
                _ => return None,
            }
        }
        Some(Self {
            match_id,
            consent_version,
            recorded_on,
            seats,
        })
    }

    /// The seats a person sat in.
    #[must_use]
    pub fn occupied(&self) -> Vec<usize> {
        self.seats
            .iter()
            .enumerate()
            .filter(|(_, seat)| matches!(seat, SeatRecord::Human { .. }))
            .map(|(index, _)| index)
            .collect()
    }

    /// Whether any seat fell behind the tick.
    ///
    /// **One seat is enough.** A match is one interleaved log and the seats in it
    /// are not independent observations, so a session in which one client
    /// stuttered is a session whose *timing* is contaminated wherever that client
    /// appears — and telling the operator "seat 4 only" would invite exactly the
    /// partial pooling `docs/SCHEMA.md` refuses.
    #[must_use]
    pub fn degraded(&self) -> bool {
        self.seats.iter().any(SeatRecord::degraded)
    }

    /// Seats whose participant declared pointer acceleration left on.
    ///
    /// Refused by `Corpus::store` rather than flagged, and it is the one
    /// declaration that is refused rather than recorded: acceleration makes the
    /// map from device counts to world units a function of speed, so the
    /// trajectory in the record is the operating system's curve as much as the
    /// hand's — and unlike a sensitivity, no covariate stored here recovers it.
    #[must_use]
    pub fn accelerated_seats(&self) -> Vec<usize> {
        self.seats
            .iter()
            .enumerate()
            .filter(|(_, seat)| {
                matches!(seat, SeatRecord::Human { declared, .. } if declared.pointer_acceleration)
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// Seats whose client recorded no device event at all.
    ///
    /// **This is the corpus's one mechanical defence against synthetic play**,
    /// and it is worth being precise about how narrow it is. A scripted or
    /// headless client drives the protocol and touches no device, so it records
    /// zero samples and is refused here. A *bot that moves a real mouse* records
    /// exactly as many samples as a person and is not reachable from this file —
    /// which is `docs/SCOPE.md`'s stated ceiling for behavioural detection,
    /// arriving early. What keeps that hole closed at M6 is the operator, who is
    /// in the room.
    #[must_use]
    pub fn silent_seats(&self) -> Vec<usize> {
        self.seats
            .iter()
            .enumerate()
            .filter(|(_, seat)| {
                matches!(seat, SeatRecord::Human { measured, .. } if measured.samples == 0)
            })
            .map(|(index, _)| index)
            .collect()
    }
}
