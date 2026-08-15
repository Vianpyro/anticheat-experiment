//! Device calibration: the profile a participant accumulates, and the state a
//! seat is filed under.
//!
//! `docs/SCHEMA.md` §4e is the schema. What belongs here is the reasoning about
//! the code, and the arithmetic.
//!
//! # The confound this exists for
//!
//! Nine people on nine mice, which `docs/SCOPE.md` fixes and `docs/RISKS.md` R17
//! prices: **person and device are perfectly confounded, so nothing separates a
//! style from a hardware response.** The parade is not to standardise the
//! hardware — a production anti-cheat does not choose its players' mice — but to
//! measure its contribution, so that a detector reading a distance or a speed
//! reads normalised units rather than raw device counts. `client::lobby` is the
//! instrument; this module is the arithmetic and the bookkeeping.
//!
//! # Two operations that were one, and separating them is the point
//!
//! **Estimating** a scale is expensive: it needs many movements, in many
//! directions, over a spread of distances, and no single evening owes anybody
//! that. **Verifying** that the device has not changed is cheap: a handful of
//! movements is enough to say whether a signature still matches a profile
//! already on record.
//!
//! So an estimate **accumulates**. [`Observations`] are the sufficient statistics
//! of a least-squares fit rather than the fit itself, they add by `+`, and
//! [`Profile`] is the sum over every session a participant has recorded on one
//! device. Three consequences, and they are the reason the separation is worth
//! machinery:
//!
//! - the last person to join does not have to be calibrated that evening; their
//!   earlier sessions already did it;
//! - a participant who spends the wait doing something else loses nothing, they
//!   merely defer the refinement of their own profile;
//! - the first session of a participant is explicitly a calibration session, and
//!   is filed as [`CalibrationState::Partial`] rather than pretended otherwise.
//!
//! # Nothing here blocks anything, and that is a decision
//!
//! A seat whose calibration is insufficient **never** stops a match starting and
//! never stops a match being stored. It is *marked*, and a detector that depends
//! on the scale answers `None` for it — which is the treatment `docs/SCOPE.md`
//! and M8 already apply to a detector with no calibrated threshold, arriving one
//! level down. Blocking a player for a calibration reason is the shortest path to
//! an anti-cheat that degrades the experience of honest players, and this project
//! has written that down about bans and means it about menus.
//!
//! # What is measured, and what stays a declaration
//!
//! [`Estimate::counts_per_unit`] is the map from **recorded device counts to
//! world units**: the conversion a distance-shaped statistic needs in order to
//! stop being a count, measured against geometry the build fixes rather than
//! taken from a number the client wrote about itself.
//!
//! **It is not `device_cpi` and it does not become one.** A mouse reports counts;
//! nothing in any stream this project records says what physical distance
//! produced them, and no menu geometry changes that. `docs/SCHEMA.md` §4c keeps
//! the true CPI in the unknown column where it has always been.

use crate::session::{SeatRecord, SessionRecord};

/// The longest a device profile label may be, in bytes.
pub const MAX_PROFILE_ID_BYTES: usize = 32;

/// A participant's label for the device they are playing on.
///
/// Constrained to the same character set as [`crate::Pseudonym`] and for exactly
/// the same reason: these strings are written into a text record that
/// `Corpus::audit` reads byte by byte, so one containing a space, a newline or a
/// path separator could be split across a line or collide with unrelated text —
/// and a free-form field is where a real name ends up by accident. A profile
/// label is chosen by the operator from the same kind of list a pseudonym is.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceProfileId(String);

impl DeviceProfileId {
    /// The label this text names, or `None` if it is not one.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if text.is_empty() || text.len() > MAX_PROFILE_ID_BYTES {
            return None;
        }
        if !text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            return None;
        }
        Some(Self(text.to_owned()))
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for DeviceProfileId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What one session's crossing of the lobby measured, as the corpus stores it.
///
/// The mirror of `client::lobby::Observations`, here for the reason
/// [`crate::session::Clock`] mirrors `client::input::Clock`: `client` may not
/// link `replay`, so the two crates cannot share a type and the record crosses as
/// text. `client/tests/session_part.rs` is what keeps the writer and this reader
/// in step, field for field.
///
/// **These are sufficient statistics, not an estimate.** Distances and counts are
/// held in thousandths and the device's resolution in millionths, so the record
/// carries exact integers rather than rendered floats and two builds reading one
/// corpus agree byte for byte.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Observations {
    /// Clicks on an element of known position, with a measured crossing behind
    /// them.
    pub reaches: u64,
    /// A bit per compass octant covered.
    ///
    /// Eight directions rather than a count, because a measurement aligned on one
    /// axis has already hidden an anisotropy in this project once
    /// (`docs/RISKS.md` R14) and a coverage mask is the cheapest guard against
    /// doing it twice.
    pub octants: u8,
    /// Legs abandoned because the cursor reached the map clamp during them.
    pub clamped: u64,
    /// The shortest and longest reach, in thousandths of a world unit.
    pub min_distance_e3: u64,
    /// See [`Observations::min_distance_e3`].
    pub max_distance_e3: u64,
    /// `Σ d`, in thousandths of a world unit.
    pub sum_distance_e3: u64,
    /// `Σ n`, in thousandths of a device count.
    pub sum_counts_e3: u64,
    /// `Σ d²`, in thousandths of a squared world unit.
    pub sum_distance_sq_e3: u64,
    /// `Σ d·n`, in thousandths of a world-unit count.
    pub sum_distance_counts_e3: u64,
    /// `Σ n²`, in thousandths of a squared device count.
    ///
    /// Carried only so that [`Estimate::fit`] exists. A slope and an intercept
    /// need the four sums above; how *well* the line fits needs this one, and a
    /// scale reported without a spread is the point estimate `docs/RISKS.md` R8
    /// spends a page refusing everywhere else in this project.
    pub sum_counts_sq_e3: u64,
    /// Reaches crossed fast enough to read a report rate off
    /// (`client::lobby::FAST_UNITS_PER_SECOND`).
    pub fast_reaches: u64,
    /// Motion events recorded during those.
    pub fast_motions: u64,
    /// Nanoseconds they took, in total.
    pub fast_ns: u64,
    /// The finest non-zero delta component observed, in millionths of a device
    /// count.
    ///
    /// The hardware's own resolution and the one number here that is neither
    /// style nor geometry: a mouse reporting whole counts gives `1_000_000`, a
    /// Wayland compositor's fixed-point relative motion gives `3_906`, and
    /// neither is anything a player can choose to do differently.
    pub quantum_e6: u64,
}

impl Observations {
    /// Nothing observed.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            reaches: 0,
            octants: 0,
            clamped: 0,
            min_distance_e3: 0,
            max_distance_e3: 0,
            sum_distance_e3: 0,
            sum_counts_e3: 0,
            sum_distance_sq_e3: 0,
            sum_distance_counts_e3: 0,
            sum_counts_sq_e3: 0,
            fast_reaches: 0,
            fast_motions: 0,
            fast_ns: 0,
            quantum_e6: 0,
        }
    }

    /// Whether anything at all was measured.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.reaches == 0
    }

    /// The sum of two sessions' observations.
    ///
    /// **This is the whole of what makes an estimate accumulate**, and it is
    /// addition because the statistics were chosen so that it could be. Nothing
    /// is stored, nothing is recomputed and no derived artefact outlives a
    /// withdrawal: a profile is a fold over the sessions still in the corpus, in
    /// exactly the register `replay::split::split_of` is a function rather than a
    /// file (`docs/SCHEMA.md` §7).
    #[must_use]
    pub const fn merge(self, other: Self) -> Self {
        Self {
            reaches: self.reaches.saturating_add(other.reaches),
            octants: self.octants | other.octants,
            clamped: self.clamped.saturating_add(other.clamped),
            min_distance_e3: match (self.min_distance_e3, other.min_distance_e3) {
                (0, value) | (value, 0) => value,
                (a, b) if a < b => a,
                (_, b) => b,
            },
            max_distance_e3: if self.max_distance_e3 > other.max_distance_e3 {
                self.max_distance_e3
            } else {
                other.max_distance_e3
            },
            sum_distance_e3: self.sum_distance_e3.saturating_add(other.sum_distance_e3),
            sum_counts_e3: self.sum_counts_e3.saturating_add(other.sum_counts_e3),
            sum_distance_sq_e3: self
                .sum_distance_sq_e3
                .saturating_add(other.sum_distance_sq_e3),
            sum_distance_counts_e3: self
                .sum_distance_counts_e3
                .saturating_add(other.sum_distance_counts_e3),
            sum_counts_sq_e3: self.sum_counts_sq_e3.saturating_add(other.sum_counts_sq_e3),
            fast_reaches: self.fast_reaches.saturating_add(other.fast_reaches),
            fast_motions: self.fast_motions.saturating_add(other.fast_motions),
            fast_ns: self.fast_ns.saturating_add(other.fast_ns),
            quantum_e6: match (self.quantum_e6, other.quantum_e6) {
                (0, value) | (value, 0) => value,
                (a, b) if a < b => a,
                (_, b) => b,
            },
        }
    }

    /// How many of eight octants are covered.
    #[must_use]
    pub const fn octants_covered(&self) -> u32 {
        self.octants.count_ones()
    }

    /// The ratio of the longest reach to the shortest, or `0.0` if nothing was
    /// measured.
    ///
    /// The leverage the regression has. A set of reaches all of one length fits a
    /// slope and an intercept through one cloud of points, which is a ratio
    /// rather than a regression, and the fixed cost of arriving at a target would
    /// be indistinguishable from the cost of crossing to it.
    #[must_use]
    pub fn distance_ratio(&self) -> f64 {
        if self.min_distance_e3 == 0 {
            return 0.0;
        }
        (self.max_distance_e3 as f64) / (self.min_distance_e3 as f64)
    }

    /// The lines this is written as in a session record, given a key prefix.
    #[must_use]
    pub fn encode(&self, prefix: &str) -> String {
        let Self {
            reaches,
            octants,
            clamped,
            min_distance_e3,
            max_distance_e3,
            sum_distance_e3,
            sum_counts_e3,
            sum_distance_sq_e3,
            sum_distance_counts_e3,
            sum_counts_sq_e3,
            fast_reaches,
            fast_motions,
            fast_ns,
            quantum_e6,
        } = self;
        let mut out = String::new();
        for (key, value) in [
            ("reaches", reaches),
            ("octants", &u64::from(*octants)),
            ("clamped", clamped),
            ("min_distance_e3", min_distance_e3),
            ("max_distance_e3", max_distance_e3),
            ("sum_distance_e3", sum_distance_e3),
            ("sum_counts_e3", sum_counts_e3),
            ("sum_distance_sq_e3", sum_distance_sq_e3),
            ("sum_distance_counts_e3", sum_distance_counts_e3),
            ("sum_counts_sq_e3", sum_counts_sq_e3),
            ("fast_reaches", fast_reaches),
            ("fast_motions", fast_motions),
            ("fast_ns", fast_ns),
            ("quantum_e6", quantum_e6),
        ] {
            out.push_str(&format!("{prefix}calibration.{key}: {value}\n"));
        }
        out
    }

    /// Reads the observations a record carries under `prefix`.
    ///
    /// `field` answers a key, and `None` from it is a missing line. Absence does
    /// not decode, in the register `docs/SCHEMA.md` §5a states about supervision
    /// and `docs/RISKS.md` R3 about the consent version: a record written before
    /// this field existed must not be readmitted as a record that measured
    /// nothing, because "nothing was measured" and "nobody was measuring" are
    /// different facts and only one of them is about the participant.
    #[must_use]
    pub fn decode(field: &impl Fn(&str) -> Option<u64>) -> Option<Self> {
        Some(Self {
            reaches: field("calibration.reaches")?,
            octants: u8::try_from(field("calibration.octants")?).ok()?,
            clamped: field("calibration.clamped")?,
            min_distance_e3: field("calibration.min_distance_e3")?,
            max_distance_e3: field("calibration.max_distance_e3")?,
            sum_distance_e3: field("calibration.sum_distance_e3")?,
            sum_counts_e3: field("calibration.sum_counts_e3")?,
            sum_distance_sq_e3: field("calibration.sum_distance_sq_e3")?,
            sum_distance_counts_e3: field("calibration.sum_distance_counts_e3")?,
            sum_counts_sq_e3: field("calibration.sum_counts_sq_e3")?,
            fast_reaches: field("calibration.fast_reaches")?,
            fast_motions: field("calibration.fast_motions")?,
            fast_ns: field("calibration.fast_ns")?,
            quantum_e6: field("calibration.quantum_e6")?,
        })
    }
}

/// What makes a measurement sufficient.
///
/// Four clauses, each of them the antecedent of something the estimate claims,
/// and stated as constants rather than as prose so that a change to one is a
/// change somebody has to make on purpose.
pub mod sufficiency {
    /// Reaches, pooled across a participant's sessions on one device.
    ///
    /// Sixteen rather than a handful because the fit has two parameters and its
    /// residual is the landing slop, which is comparable with a button's radius:
    /// a dozen points spread over a range of distances is where the slope stops
    /// moving when one of them is removed.
    pub const REACHES: u64 = 16;

    /// Compass octants that must be covered, of eight.
    ///
    /// Six, not eight: requiring all eight would make sufficiency a property of
    /// whether somebody happened to finish the dummy's schedule, and the failure
    /// this guards against is a measurement taken along **one** axis
    /// (`docs/RISKS.md` R14's anisotropic character cell), which six rules out.
    pub const OCTANTS: u32 = 6;

    /// The ratio of the longest reach to the shortest.
    ///
    /// Four. Below that the regression's two parameters are not separately
    /// identified by the data and the slope absorbs the fixed cost of arriving.
    pub const DISTANCE_RATIO: f64 = 4.0;

    /// Reaches crossed fast enough for the report rate to be readable.
    ///
    /// The rate is the one quantity here that a slow session cannot produce at
    /// all: a hand that creeps reports at the same rate but spends most of the
    /// interval stationary, and a stationary hand reports nothing.
    pub const FAST_REACHES: u64 = 4;
}

/// How far a session's own signature may sit from the profile's before the two
/// are not the same device.
///
/// **Ten per cent on the scale.** A tolerance rather than an equality because
/// every term in the estimate is noisy: the landing slop is bounded by a
/// button's radius, a session is a handful of reaches, and a person who is tired
/// clicks differently. Ten per cent is far outside that and far inside the
/// difference between two mice — a 400-count-per-inch device and an 800 differ by
/// a factor of two, and a sensitivity change by more.
pub const SCALE_TOLERANCE: f64 = 0.10;

/// And on the measured report rate: a factor of one and a half.
///
/// Wider than the scale's because the rate is measured over fewer reaches and is
/// bounded above by the client's own capture loop, which `docs/RISKS.md` R14
/// measures rather than fixes. What it separates is a 125 Hz device from a 500 Hz
/// one, which is the difference that matters (`docs/SCHEMA.md` §11f).
pub const RATE_TOLERANCE: f64 = 1.5;

/// A fit of device counts against known distances, and the two numbers beside
/// it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Estimate {
    /// **Device counts per world unit**: the slope.
    ///
    /// The number a distance-shaped statistic is divided by to stop being a
    /// count. It is not a counts-per-inch and does not become one; see this
    /// module's header.
    pub counts_per_unit: f64,
    /// The fixed cost of arriving at a target, in device counts: the intercept.
    ///
    /// **Style, not hardware**, and it is here to be *excluded* from the
    /// signature rather than to be used. A click lands somewhere inside a radius
    /// rather than on a point and a hand overshoots and corrects; both are
    /// distance-independent, so a regression puts them here and keeps the slope
    /// clean. A ratio taken from one movement cannot separate the two, which is
    /// the whole argument for a fit over a spread of distances.
    pub arrival_counts: f64,
    /// The coefficient of determination, `0.0` to `1.0`.
    pub fit: f64,
    /// The device's measured report rate in hertz, over the fast reaches only.
    ///
    /// `0.0` when none were fast enough. Read against `device_polling_hz`, which
    /// `docs/SCHEMA.md` §4a can only ever hold as a declaration.
    pub report_hz: f64,
    /// The finest non-zero delta component observed, in device counts.
    pub quantum: f64,
}

impl Estimate {
    /// The least-squares fit of `n = a·d + b` over the pooled reaches, or `None`
    /// when the observations cannot support one.
    ///
    /// `None` rather than a number with a wide interval, for the reason
    /// `anticheat::Calibration` has an `Uncalibrated` variant: a signature that
    /// always returns an estimate is a signature in which "there is no estimate"
    /// cannot be said, and the thing this module most has to be able to say is
    /// that a participant has not been measured yet.
    #[must_use]
    pub fn of(observations: &Observations) -> Option<Self> {
        let n = observations.reaches;
        if n < 2 {
            return None;
        }
        let count = n as f64;
        let sum_d = (observations.sum_distance_e3 as f64) / 1e3;
        let sum_n = (observations.sum_counts_e3 as f64) / 1e3;
        let sum_dd = (observations.sum_distance_sq_e3 as f64) / 1e3;
        let sum_dn = (observations.sum_distance_counts_e3 as f64) / 1e3;

        let sum_nn = (observations.sum_counts_sq_e3 as f64) / 1e3;

        // The centred sums. `s_dd` is zero exactly when every reach was the same
        // length, which is a ratio rather than a regression: there is one cloud
        // of points, a line through it is not identified, and answering with the
        // ratio would be answering with the arrival cost folded into the scale.
        let s_dd = sum_dd - sum_d * sum_d / count;
        let s_dn = sum_dn - sum_d * sum_n / count;
        let s_nn = sum_nn - sum_n * sum_n / count;
        if !s_dd.is_finite() || s_dd <= 0.0 {
            return None;
        }
        let slope = s_dn / s_dd;
        let intercept = (sum_n - slope * sum_d) / count;
        if !slope.is_finite() || slope <= 0.0 || !intercept.is_finite() {
            return None;
        }

        // R², clamped, and it is a **diagnostic rather than a claim**. It is
        // computed by differencing sums held in thousandths, so it loses
        // significant figures exactly where a residual is small — which is the
        // direction that flatters it. What it is good for is noticing a fit that
        // is not one; it is not an interval and nothing in this repository may
        // quote it as one (`docs/RISKS.md` R8).
        let fit = if s_nn > 0.0 {
            (s_dn * s_dn / (s_dd * s_nn)).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let report_hz = if observations.fast_ns == 0 {
            0.0
        } else {
            (observations.fast_motions as f64) * 1e9 / (observations.fast_ns as f64)
        };

        Some(Self {
            counts_per_unit: slope,
            arrival_counts: intercept,
            fit,
            report_hz,
            quantum: (observations.quantum_e6 as f64) / 1e6,
        })
    }

    /// Whether this reading is the same device as `profile`, within the stated
    /// tolerances.
    ///
    /// Three clauses, and each of them is a hardware property rather than a
    /// style one: the scale, the report rate and the device's own resolution.
    /// The arrival cost is deliberately not among them — it is the term the
    /// regression exists to put somewhere the scale is not.
    #[must_use]
    pub fn matches(&self, profile: &Self) -> bool {
        let within = |a: f64, b: f64, tolerance: f64| -> bool {
            if b <= 0.0 {
                return a <= 0.0;
            }
            (a - b).abs() / b <= tolerance
        };
        if !within(
            self.counts_per_unit,
            profile.counts_per_unit,
            SCALE_TOLERANCE,
        ) {
            return false;
        }
        if self.report_hz > 0.0
            && profile.report_hz > 0.0
            && (self.report_hz / profile.report_hz > RATE_TOLERANCE
                || profile.report_hz / self.report_hz > RATE_TOLERANCE)
        {
            return false;
        }
        // The resolution is exact rather than approximate: a device reports whole
        // counts or it does not, and a change here is a change of platform or of
        // device and never of hand. Compared in the record's millionths so that
        // two equal readings compare equal.
        if self.quantum > 0.0 && profile.quantum > 0.0 {
            let ratio = self.quantum / profile.quantum;
            if !(0.5..=2.0).contains(&ratio) {
                return false;
            }
        }
        true
    }
}

/// One participant's device profile: every session they have recorded on one
/// device, folded.
///
/// Not a file. `Corpus::profile_of` computes it from the matches on disk when
/// somebody asks, exactly as `replay census` recomputes everything it prints and
/// for the same reason (`docs/CONSENT.md`, "There is no derived index"): a stored
/// profile would be an artefact derived from the corpus, able to disagree with
/// it, and able to outlive a withdrawal that destroyed what it was derived from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    /// The device the sessions were recorded on.
    pub device: DeviceProfileId,
    /// Sessions folded in.
    pub sessions: u32,
    /// Their observations, summed.
    pub observations: Observations,
}

impl Profile {
    /// A profile with nothing in it.
    #[must_use]
    pub fn empty(device: DeviceProfileId) -> Self {
        Self {
            device,
            sessions: 0,
            observations: Observations::new(),
        }
    }

    /// Folds one session's observations in.
    pub fn fold(&mut self, observations: Observations) {
        self.sessions = self.sessions.saturating_add(1);
        self.observations = self.observations.merge(observations);
    }

    /// Whether the pooled observations meet every clause of [`sufficiency`].
    #[must_use]
    pub fn sufficient(&self) -> bool {
        let observations = &self.observations;
        observations.reaches >= sufficiency::REACHES
            && observations.octants_covered() >= sufficiency::OCTANTS
            && observations.distance_ratio() >= sufficiency::DISTANCE_RATIO
            && observations.fast_reaches >= sufficiency::FAST_REACHES
    }

    /// Which clauses are not met yet, for an operator reading a census.
    #[must_use]
    pub fn shortfall(&self) -> Vec<String> {
        let observations = &self.observations;
        let mut missing = Vec::new();
        if observations.reaches < sufficiency::REACHES {
            missing.push(format!(
                "{} of {} reaches",
                observations.reaches,
                sufficiency::REACHES
            ));
        }
        if observations.octants_covered() < sufficiency::OCTANTS {
            missing.push(format!(
                "{} of {} octants",
                observations.octants_covered(),
                sufficiency::OCTANTS
            ));
        }
        if observations.distance_ratio() < sufficiency::DISTANCE_RATIO {
            missing.push(format!(
                "distance ratio {:.2} of {:.1}",
                observations.distance_ratio(),
                sufficiency::DISTANCE_RATIO
            ));
        }
        if observations.fast_reaches < sufficiency::FAST_REACHES {
            missing.push(format!(
                "{} of {} fast reaches",
                observations.fast_reaches,
                sufficiency::FAST_REACHES
            ));
        }
        missing
    }

    /// The fit over everything folded in.
    #[must_use]
    pub fn estimate(&self) -> Option<Estimate> {
        Estimate::of(&self.observations)
    }
}

/// How well a seat's device is known, at the moment the match was filed.
///
/// **Recorded rather than recomputed**, which is the point of it being a field:
/// `docs/SCHEMA.md` §8 requires a distribution to say which stratum it was
/// computed over, and a stratum that has to be re-derived from the whole corpus
/// every time somebody asks is a stratum that quietly changes under a published
/// number. The observations stay in the record beside it, so the decision can be
/// audited; what is frozen is the decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalibrationState {
    /// Every clause of [`sufficiency`] is met, counting this session and the
    /// participant's earlier ones on the same device.
    Sufficient,
    /// Something was measured and it is not enough yet.
    ///
    /// The ordinary state of a first session, and it is named rather than
    /// treated as a failure: a corpus's first evening is a calibration evening
    /// and pretending otherwise would put an estimate nobody can support behind
    /// a word that says somebody can.
    Partial,
    /// Nothing was measured at all.
    ///
    /// A legitimate state — a client that never crossed a lobby, a session
    /// somebody joined late — and legitimate is why it is a value rather than a
    /// refusal.
    Absent,
    /// Something was measured and it does not match the profile this seat's
    /// device is on record as.
    ///
    /// The cheap half of this module: verifying that a device has not changed
    /// takes a handful of movements, and this is what those movements answer.
    /// It is **not** an accusation — a mouse replaced between two sessions
    /// produces exactly this — it is the corpus refusing to pool two devices
    /// under one profile.
    Mismatched,
}

impl CalibrationState {
    /// The tag this state is written as.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Sufficient => "sufficient",
            Self::Partial => "partial",
            Self::Absent => "absent",
            Self::Mismatched => "mismatched",
        }
    }

    /// The state this tag names, or `None`.
    ///
    /// No default. A seat record with no calibration line does not decode, for
    /// the reason `crate::session::Supervision::parse` has none: absent and
    /// stale must fail alike, or a corpus assembled before this field existed is
    /// readmitted by the silence of its own files.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "sufficient" => Some(Self::Sufficient),
            "partial" => Some(Self::Partial),
            "absent" => Some(Self::Absent),
            "mismatched" => Some(Self::Mismatched),
            _ => None,
        }
    }

    /// Whether a statistic that reads a distance or a speed may be computed on
    /// this seat.
    ///
    /// The one consumer this state has, and the treatment is M8's: a detector
    /// that depends on the scale answers `None` for a seat this returns `false`
    /// for, exactly as a detector with no calibrated threshold answers `None`
    /// for everybody. Nothing is blocked and nobody is refused.
    #[must_use]
    pub const fn scale_is_known(self) -> bool {
        matches!(self, Self::Sufficient)
    }

    /// What this seat's state is, given its own session and the profile its
    /// participant already has.
    ///
    /// The `profile` is the fold over the participant's **other** matches on this
    /// device; this session's observations are folded in here, so a first session
    /// is rated against itself and a fifth against all five.
    #[must_use]
    pub fn rate(session: &Observations, profile: &Profile) -> Self {
        if session.is_empty() {
            return Self::Absent;
        }
        // Verification first, and against the profile as it stood *before* this
        // session: folding this session in first would let a large enough
        // session drag the profile onto its own answer, which is a check
        // agreeing with itself.
        if let (Some(reading), Some(known)) = (Estimate::of(session), profile.estimate())
            && profile.sufficient()
            && !reading.matches(&known)
        {
            return Self::Mismatched;
        }
        let mut pooled = profile.clone();
        pooled.fold(*session);
        if pooled.sufficient() {
            Self::Sufficient
        } else {
            Self::Partial
        }
    }
}

/// Rates every occupied seat of a session record against the profile its
/// participant already has.
///
/// `profile_of` answers the fold over a participant's **earlier** matches on the
/// device that seat declares — `Corpus::profile_of` is the implementation, and it
/// is a parameter rather than a corpus so that this function stays testable
/// without a filesystem and so that `replay` keeps one place where a directory is
/// walked. It is given the seat index and the device label; the *pseudonym* comes
/// from the caller's manifest, because a session record deliberately does not
/// name one.
///
/// It **never refuses**: a seat whose device is unknown is filed as
/// [`CalibrationState::Partial`] or [`CalibrationState::Absent`] and the match is
/// stored. See this module's header.
pub fn rate_seats(
    session: &mut SessionRecord,
    profile_of: &impl Fn(usize, &DeviceProfileId) -> Profile,
) {
    for (index, seat) in session.seats.iter_mut().enumerate() {
        let SeatRecord::Human {
            declared,
            calibration,
            ..
        } = seat
        else {
            continue;
        };
        let profile = profile_of(index, &declared.device_profile_id);
        calibration.state = CalibrationState::rate(&calibration.observations, &profile);
    }
}

/// A seat's calibration, as the record holds it: what was measured, and how it
/// was rated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeatCalibration {
    /// What this session's crossing of the lobby measured.
    pub observations: Observations,
    /// How well the device was known when the match was filed.
    pub state: CalibrationState,
}

impl SeatCalibration {
    /// A seat that measured nothing and was rated as such.
    #[must_use]
    pub const fn absent() -> Self {
        Self {
            observations: Observations::new(),
            state: CalibrationState::Absent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CalibrationState, DeviceProfileId, Estimate, MAX_PROFILE_ID_BYTES, Observations, Profile,
        sufficiency,
    };

    /// Observations built from a set of `(distance, counts)` pairs at a known
    /// scale, so that the arithmetic below has something exact to recover.
    fn reaches(pairs: &[(f64, f64)], octants: u8, fast: u64) -> Observations {
        let mut out = Observations::new();
        for (distance, counts) in pairs {
            let e3 = |value: f64| (value * 1e3).round() as u64;
            out.reaches += 1;
            out.sum_distance_e3 += e3(*distance);
            out.sum_counts_e3 += e3(*counts);
            out.sum_distance_sq_e3 += e3(distance * distance);
            out.sum_distance_counts_e3 += e3(distance * counts);
            out.sum_counts_sq_e3 += e3(counts * counts);
            if out.min_distance_e3 == 0 || e3(*distance) < out.min_distance_e3 {
                out.min_distance_e3 = e3(*distance);
            }
            out.max_distance_e3 = out.max_distance_e3.max(e3(*distance));
        }
        out.octants = octants;
        out.fast_reaches = fast;
        out.fast_motions = fast * 100;
        out.fast_ns = fast * 800_000_000;
        out.quantum_e6 = 1_000_000;
        out
    }

    /// A profile label is a pseudonym's character set and nothing else, for the
    /// reason the audit reads bytes.
    #[test]
    fn a_device_profile_label_is_constrained_like_a_pseudonym() {
        assert!(DeviceProfileId::parse("mouse-a_1").is_some());
        for wrong in ["", "with space", "slash/es", "new\nline", "é"] {
            assert!(
                DeviceProfileId::parse(wrong).is_none(),
                "{wrong:?} parsed as a device profile label"
            );
        }
        assert!(DeviceProfileId::parse(&"a".repeat(MAX_PROFILE_ID_BYTES)).is_some());
        assert!(DeviceProfileId::parse(&"a".repeat(MAX_PROFILE_ID_BYTES + 1)).is_none());
    }

    /// **The slope is the scale and the intercept is the arrival cost.**
    ///
    /// The property the whole regression exists for, stated on data where the
    /// two are known: every reach costs `20` counts per world unit plus a fixed
    /// `9` counts of landing, and a ratio taken from any single one of them
    /// would report a scale between 20.3 and 21.8.
    #[test]
    fn a_regression_separates_the_scale_from_the_cost_of_arriving() {
        let observations = reaches(
            &[
                (40.0, 40.0 * 20.0 + 9.0),
                (80.0, 80.0 * 20.0 + 9.0),
                (160.0, 160.0 * 20.0 + 9.0),
                (240.0, 240.0 * 20.0 + 9.0),
            ],
            0xff,
            4,
        );
        let estimate = Estimate::of(&observations).expect("four distinct distances fit");
        assert!(
            (estimate.counts_per_unit - 20.0).abs() < 1e-6,
            "recovered {} counts per world unit against a true 20.0",
            estimate.counts_per_unit
        );
        assert!(
            (estimate.arrival_counts - 9.0).abs() < 1e-3,
            "recovered an arrival cost of {} counts against a true 9.0",
            estimate.arrival_counts
        );
        let naive: f64 = (40.0 * 20.0 + 9.0) / 40.0;
        assert!(
            (naive - 20.0).abs() > 0.2,
            "the shortest reach's own ratio is {naive}, which is what a direct \
             measurement would have reported"
        );
    }

    /// One distance repeated is a ratio, not a regression, and it answers
    /// nothing rather than answering the arrival cost as if it were the scale.
    #[test]
    fn one_distance_repeated_supports_no_estimate() {
        let observations = reaches(&[(100.0, 2009.0), (100.0, 2011.0)], 0xff, 4);
        assert!(Estimate::of(&observations).is_none());
    }

    /// Observations pool by addition, which is the whole of what makes an
    /// estimate accumulate across sessions.
    #[test]
    fn two_sessions_pool_into_one_estimate() {
        let first = reaches(&[(40.0, 809.0), (80.0, 1609.0)], 0b0000_0011, 1);
        let second = reaches(&[(160.0, 3209.0), (240.0, 4809.0)], 0b0000_1100, 3);
        let pooled = first.merge(second);
        assert_eq!(pooled.reaches, 4);
        assert_eq!(pooled.octants_covered(), 4);
        assert_eq!(pooled.fast_reaches, 4);
        assert!((pooled.distance_ratio() - 6.0).abs() < 1e-9);
        let estimate = Estimate::of(&pooled).expect("pooled reaches fit");
        assert!((estimate.counts_per_unit - 20.0).abs() < 1e-6);
    }

    /// Sufficiency is the four clauses and nothing else, and each of them can
    /// fail alone.
    #[test]
    fn each_clause_of_sufficiency_can_fail_on_its_own() {
        let device = DeviceProfileId::parse("mouse-a").expect("a label");
        let full = |octants: u8, fast: u64, ratio: bool| {
            let mut pairs = Vec::new();
            for index in 0..sufficiency::REACHES {
                let distance = if ratio && index > 0 { 200.0 } else { 40.0 };
                pairs.push((distance, distance * 20.0 + 9.0));
            }
            let mut profile = Profile::empty(device.clone());
            profile.fold(reaches(&pairs, octants, fast));
            profile
        };
        assert!(full(0xff, sufficiency::FAST_REACHES, true).sufficient());
        assert!(!full(0b0000_0111, sufficiency::FAST_REACHES, true).sufficient());
        assert!(!full(0xff, 0, true).sufficient());
        assert!(!full(0xff, sufficiency::FAST_REACHES, false).sufficient());

        let mut thin = Profile::empty(device);
        thin.fold(reaches(&[(40.0, 809.0), (240.0, 4809.0)], 0xff, 4));
        assert!(!thin.sufficient());
        assert!(
            thin.shortfall()
                .iter()
                .any(|clause| clause.contains("reaches"))
        );
    }

    /// **A device that changed is a profile that does not match**, and a device
    /// that did not is one that does.
    #[test]
    fn a_session_on_another_device_does_not_match_the_profile() {
        let device = DeviceProfileId::parse("mouse-a").expect("a label");
        let mut profile = Profile::empty(device);
        let mut pairs = Vec::new();
        for index in 0..sufficiency::REACHES {
            let distance = 40.0 + (index as f64) * 14.0;
            pairs.push((distance, distance * 20.0 + 9.0));
        }
        profile.fold(reaches(&pairs, 0xff, sufficiency::FAST_REACHES));
        assert!(profile.sufficient());

        let same = reaches(&[(40.0, 809.0), (200.0, 4009.0)], 0b0000_0011, 1);
        assert_eq!(
            CalibrationState::rate(&same, &profile),
            CalibrationState::Sufficient
        );

        // The same hand on a device reporting half as many counts per unit.
        let other = reaches(&[(40.0, 409.0), (200.0, 2009.0)], 0b0000_0011, 1);
        assert_eq!(
            CalibrationState::rate(&other, &profile),
            CalibrationState::Mismatched
        );
    }

    /// A seat that measured nothing is `Absent`, a first session is `Partial`,
    /// and neither is a refusal.
    #[test]
    fn a_first_session_is_partial_and_an_empty_one_is_absent() {
        let device = DeviceProfileId::parse("mouse-a").expect("a label");
        let profile = Profile::empty(device);
        assert_eq!(
            CalibrationState::rate(&Observations::new(), &profile),
            CalibrationState::Absent
        );
        let first = reaches(&[(40.0, 809.0), (200.0, 4009.0)], 0b0000_0011, 1);
        assert_eq!(
            CalibrationState::rate(&first, &profile),
            CalibrationState::Partial
        );
        assert!(!CalibrationState::Partial.scale_is_known());
        assert!(CalibrationState::Sufficient.scale_is_known());
    }
}
