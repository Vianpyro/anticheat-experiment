//! What a recording session cost the machine that recorded it.
//!
//! # The risk this exists for
//!
//! `docs/RISKS.md` R16. The client's tick budget was set once, on a fixture, and
//! then raised once, on a fixture: `client/tests/m4_exit.rs` compresses the
//! server's period, found four milliseconds enough for a match in which nothing
//! happened, and found it *not* enough the moment the same three clients walked
//! under a tower and started receiving damage events. The lesson recorded there
//! is the one this module answers — **the period is a budget for the client, and
//! a match that reaches more of the game spends more of it** — and the case
//! nobody has run is the one M6 records: nine occupied seats, full views, and
//! events at the frame's cap.
//!
//! A client that falls behind does not lose data. It writes a *delay* into the
//! record: an intention decided one pass late is an intention the corpus reports
//! as a hand that hesitated. That is exactly the class of contamination R14 spent
//! a milestone removing from the timestamp, arriving through a different door,
//! and the answer here is the same answer — **measure it, record it, and make a
//! session that overran identifiable in the corpus rather than pooled into it.**
//!
//! # The budget is a rule constant and not a taste
//!
//! [`BUDGET_NS`] is one tick of the game, derived from `sim::TICKS_PER_SECOND`.
//! Nothing here picks a number: a pass of the capture loop that takes longer than
//! the interval between two server frames is a pass that answers the second frame
//! late, whatever the machine. A harness that compresses the period compresses
//! the budget with it — that is what [`Cadence::with_budget`] is for, and it is
//! the shape `client/tests/cadence.rs` measures the fixture at two budgets with.
//!
//! # What this is *not*
//!
//! It is not evidence and no detector may read it. Every number here is produced
//! by the client, which `docs/SCOPE.md` assumes is compromised and lying; an
//! attacker who wants their session to look healthy writes that it was. What it
//! is, is a **data-quality covariate for a corpus collected from consenting
//! participants** — the same register as the mouse's counts per inch — and its
//! only consumer is the operator deciding whether a session belongs in a
//! distribution.

use sim::TICKS_PER_SECOND;

/// One pass of the capture loop's budget, in nanoseconds: one tick of the game.
///
/// Derived rather than written down, so that a change to the tick rate — which
/// `docs/RISKS.md` R2 freezes and `rules_hash()` covers — moves this with it
/// instead of leaving a stale constant behind.
pub const BUDGET_NS: u64 = 1_000_000_000 / (TICKS_PER_SECOND as u64);

/// The capture loop, measured against the tick it has to keep up with.
///
/// A *pass* is one turn of the event loop: from the moment the platform hands
/// control back with work to do, to the moment the loop is ready to wait again.
/// The wait itself is not in it — an idle client is not a late client — and
/// `client::gfx` brackets exactly that interval, between `new_events` and
/// `about_to_wait`.
#[derive(Clone, Copy, Debug)]
pub struct Cadence {
    budget_ns: u64,
    passes: u64,
    over_budget: u64,
    worst_overrun_ns: u64,
    worst_pass_ns: u64,
}

impl Default for Cadence {
    fn default() -> Self {
        Self::new()
    }
}

impl Cadence {
    /// A cadence measured against one tick of the game.
    #[must_use]
    pub const fn new() -> Self {
        Self::with_budget(BUDGET_NS)
    }

    /// A cadence measured against a budget somebody else chose.
    ///
    /// For a harness that compresses the server's period: the budget is the
    /// interval between two frames, so a compressed match has a compressed
    /// budget and the counters below still mean what they say.
    #[must_use]
    pub const fn with_budget(budget_ns: u64) -> Self {
        Self {
            budget_ns,
            passes: 0,
            over_budget: 0,
            worst_overrun_ns: 0,
            worst_pass_ns: 0,
        }
    }

    /// Folds in one pass of the loop.
    ///
    /// Unconditional, in the same spirit as [`crate::input::InputTrace::moved`]:
    /// there is no predicate here and there must never be one. A cadence that
    /// only counted the passes it thought were interesting would be a cadence
    /// whose denominator is a function of its numerator.
    pub const fn pass(&mut self, took_ns: u64) {
        self.passes = self.passes.saturating_add(1);
        if took_ns > self.worst_pass_ns {
            self.worst_pass_ns = took_ns;
        }
        let overrun = took_ns.saturating_sub(self.budget_ns);
        if overrun > 0 {
            self.over_budget = self.over_budget.saturating_add(1);
            if overrun > self.worst_overrun_ns {
                self.worst_overrun_ns = overrun;
            }
        }
    }

    /// What was measured.
    #[must_use]
    pub const fn report(&self) -> CadenceReport {
        CadenceReport {
            budget_ns: self.budget_ns,
            passes: self.passes,
            passes_over_budget: self.over_budget,
            worst_overrun_ns: self.worst_overrun_ns,
            worst_pass_ns: self.worst_pass_ns,
        }
    }
}

/// The two numbers `docs/RISKS.md` R16 asks every recording session to report,
/// and the three it takes to read them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CadenceReport {
    /// The budget one pass was measured against.
    pub budget_ns: u64,
    /// Passes of the loop.
    pub passes: u64,
    /// **How many passes exceeded the budget.**
    pub passes_over_budget: u64,
    /// **The worst overrun observed**, in nanoseconds beyond the budget.
    pub worst_overrun_ns: u64,
    /// The longest single pass, which is what the overrun is measured from and
    /// is reported so that "the budget was 10 ms and the worst pass was 10.1"
    /// and "the budget was 10 ms and the worst pass was 40" are one glance apart.
    pub worst_pass_ns: u64,
}

impl CadenceReport {
    /// Whether this session fell behind at all.
    ///
    /// One pass over budget is enough. The threshold is deliberately not a rate:
    /// a single 40 ms stall in a match is a single input recorded a tick late,
    /// and whether that matters is a question for whoever is building a
    /// distribution — which is precisely why the corpus records the fact rather
    /// than deciding it (`docs/SCHEMA.md`).
    #[must_use]
    pub const fn degraded(&self) -> bool {
        self.passes_over_budget > 0
    }
}

/// The magic line every session part carries, so that a file of some other
/// shape is refused rather than half-read.
pub const PART_FORMAT: &str = "moba/session-part/v1";

/// What the participant was asked and answered.
///
/// Three numbers, none of them measurable from inside this process, all of them
/// covariates a behavioural detector reads whether it means to or not: a mouse at
/// 400 counts per inch and one at 1600 describe the same hand differently, and
/// without the number a difference of hardware reads as a difference of style.
/// `docs/SCHEMA.md` is the field-by-field account, including what refusing
/// pointer acceleration costs and what it does not buy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Declared {
    /// Counts per inch, as the participant reports their mouse configured.
    pub device_cpi: u32,
    /// The device's report rate in hertz, as the participant reports it. The
    /// client measures its own arrival rate beside this, so the two can be read
    /// against each other — which is the only check available on a declaration.
    pub device_polling_hz: u32,
    /// Whether the operating system's pointer acceleration was left on.
    ///
    /// It is required to be **off**, and a session that says otherwise is
    /// refused entry to the corpus rather than flagged: acceleration makes the
    /// map from device counts to world units a function of speed, so a
    /// trajectory recorded through it is the operating system's curve as much as
    /// it is the hand's, and no covariate recorded here recovers the curve.
    pub pointer_acceleration: bool,
}

/// The platform the session was recorded on.
///
/// Recorded because `docs/ARCHITECTURE.md`'s device-timestamp table is per
/// platform: what a timestamp *is* differs between them, and a corpus that
/// pooled two platforms without saying which is which would have a covariate
/// nobody can remove afterwards.
#[must_use]
pub const fn platform() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "other"
    }
}

/// Where a session part is written, and what the participant declared.
///
/// Present exactly when the client was told to record — `--record <directory>`
/// — because a session that nobody is collecting has nothing to declare and no
/// business writing a file. The directory is the operator's staging area beside
/// the corpus, never the corpus itself: `replay store` is what files a match.
#[derive(Clone, Debug)]
pub struct Recorded {
    /// Where the part is written.
    pub directory: std::path::PathBuf,
    /// What the participant answered.
    pub declared: Declared,
}

/// One seat's account of its own session, as the client writes it out.
///
/// # Why this crosses as text rather than as a type
///
/// `docs/ARCHITECTURE.md` forbids `client` a normal dependency on `replay`,
/// because `replay` owns the signing key and a client that can link it is a
/// client that can seal a replay. So the two crates cannot share this type, and
/// the record crosses the boundary as bytes — one `key: value` per line, in the
/// same shape `replay::corpus::ConsentRecord` already uses, hand-written on both
/// sides.
///
/// The coupling that creates is closed rather than hoped: `client` has `replay`
/// as a **dev**-dependency, so `client/tests/session_part.rs` writes a part with
/// this function and parses it with `replay::session::SeatRecord::decode`, and a
/// field added on one side and not the other fails there.
#[derive(Clone, Copy, Debug)]
pub struct SessionPart {
    /// The seat this client sat in.
    pub seat: sim::Seat,
    /// What the participant declared.
    pub declared: Declared,
    /// What the capture path recorded.
    pub trace: crate::input::TraceStats,
    /// What the loop cost.
    pub cadence: CadenceReport,
}

impl SessionPart {
    /// The part as it is stored.
    ///
    /// Written by exhaustive destructuring, in this workspace's usual style and
    /// for its usual reason: a field added to the schema and not written out is
    /// a field the corpus does not have, and `docs/SCHEMA.md` is a document
    /// about what the corpus has.
    #[must_use]
    pub fn encode(&self) -> String {
        let Self {
            seat,
            declared,
            trace,
            cadence,
        } = self;
        let Declared {
            device_cpi,
            device_polling_hz,
            pointer_acceleration,
        } = declared;
        let CadenceReport {
            budget_ns,
            passes,
            passes_over_budget,
            worst_overrun_ns,
            worst_pass_ns,
        } = cadence;

        // The sensitivity in millionths of a world unit per device count, so
        // that the record holds an exact integer rather than a rendered float.
        // It is a build constant rather than a setting, and it is written down
        // because a later build that changes it changes what a recorded aim
        // means without changing a single byte of recorded telemetry.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a build constant of 0.05, written to a text record"
        )]
        let sensitivity_e6 = (crate::input::WORLD_UNITS_PER_COUNT * 1e6) as u64;

        let mut out = String::new();
        out.push_str(&format!("format: {PART_FORMAT}\n"));
        out.push_str(&format!("seat: {}\n", seat.index()));
        out.push_str("provenance: human\n");
        out.push_str(&format!("device_cpi: {device_cpi}\n"));
        out.push_str(&format!("device_polling_hz: {device_polling_hz}\n"));
        out.push_str(&format!(
            "pointer_acceleration: {}\n",
            if *pointer_acceleration { "on" } else { "off" }
        ));
        out.push_str(&format!("platform: {}\n", platform()));
        out.push_str(&format!(
            "clock: {}\n",
            match crate::input::CLOCK {
                crate::input::Clock::Device => "device",
                crate::input::Clock::Dequeue => "dequeue",
            }
        ));
        out.push_str(&format!("world_units_per_count_e6: {sensitivity_e6}\n"));
        out.push_str(&format!("samples: {}\n", trace.samples));
        out.push_str(&format!("motions: {}\n", trace.moves));
        out.push_str(&format!("coincident: {}\n", trace.coincident));
        out.push_str(&format!("median_gap_ns: {}\n", trace.gaps_ns.p50));
        out.push_str(&format!("budget_ns: {budget_ns}\n"));
        out.push_str(&format!("passes: {passes}\n"));
        out.push_str(&format!("passes_over_budget: {passes_over_budget}\n"));
        out.push_str(&format!("worst_overrun_ns: {worst_overrun_ns}\n"));
        out.push_str(&format!("worst_pass_ns: {worst_pass_ns}\n"));
        out
    }

    /// The file name a part is written under, which names the seat and nothing
    /// else.
    ///
    /// **No pseudonym.** The manifest is the one statement of who sat where and
    /// it is inside the signature; a second one here would be the derived index
    /// M5 removed, in a new place (`docs/CONSENT.md`, "There is no derived
    /// index").
    #[must_use]
    pub fn file_name(&self) -> String {
        format!("seat-{}.session-part", self.seat.index())
    }
}

#[cfg(test)]
mod tests {
    use super::{BUDGET_NS, Cadence};

    /// The budget is one tick, and the arithmetic is stated so that a change to
    /// the tick rate arrives here as a failure rather than as a stale comment.
    #[test]
    fn the_budget_is_one_tick_of_the_game() {
        assert_eq!(BUDGET_NS, 33_333_333);
    }

    /// A pass inside the budget is counted and does not accuse anybody.
    #[test]
    fn a_pass_inside_the_budget_is_not_an_overrun() {
        let mut cadence = Cadence::with_budget(10_000_000);
        cadence.pass(9_999_999);
        cadence.pass(10_000_000);
        let report = cadence.report();
        assert_eq!(report.passes, 2);
        assert_eq!(report.passes_over_budget, 0);
        assert_eq!(report.worst_overrun_ns, 0);
        assert_eq!(report.worst_pass_ns, 10_000_000);
        assert!(!report.degraded());
    }

    /// And one outside it is counted, measured, and makes the session
    /// identifiable.
    #[test]
    fn a_pass_outside_the_budget_is_counted_and_measured() {
        let mut cadence = Cadence::with_budget(10_000_000);
        cadence.pass(1_000_000);
        cadence.pass(12_000_000);
        cadence.pass(40_000_000);
        cadence.pass(11_000_000);
        let report = cadence.report();
        assert_eq!(report.passes, 4);
        assert_eq!(report.passes_over_budget, 3);
        assert_eq!(report.worst_overrun_ns, 30_000_000);
        assert_eq!(report.worst_pass_ns, 40_000_000);
        assert!(report.degraded());
    }
}
