//! Input capture: two paths from one device event, and neither of them is the
//! screen.
//!
//! This module is the reason the client stopped being a terminal. Rendering was
//! never the problem; `docs/RISKS.md` R14 named the problem as aim resolution,
//! and a trace of a real session said the problem was wider than R14 stated. So
//! this file exists to hold three properties that the terminal could not have,
//! and every one of them is stated as a test in `client/tests/capture.rs` rather
//! than as a paragraph here.
//!
//! # 1. The rendering path rounds; the capture path never does
//!
//! A device event arrives as a **relative delta in the device's own units**. It
//! goes two places and they are not the same place:
//!
//! - into [`InputTrace`], **verbatim** — the `f64` pair the platform reported,
//!   unscaled, unrounded, unclamped, with its own timestamp. This is the
//!   telemetry, and it is what a behavioural detector at M8 reads.
//! - into [`Aim`], which integrates it into a fixed-point world point for the
//!   simulation to consume, because `sim::Action::Move` takes an `FxVec2` and
//!   the rules are integers.
//!
//! The second path rounds and the first does not, which is the whole design.
//! What is forbidden is the reverse dependency: **nothing in the capture path
//! may read a quantity that came from the renderer.** Not the window size, not
//! the pixel the cursor is over, not a scale factor, not the drawn position of
//! anything. A capture derived from where the aim was *drawn* is R14 rebuilt in
//! pixels instead of character cells — finer, and wrong in exactly the same way,
//! because the resolution of the record would again be a property of the display
//! rather than of the hand.
//!
//! The clamp in [`Aim::apply`] is the place that decision is most visible: the
//! aim is confined to `RULES.map_half_extent`, which is a rule constant, and
//! **not** to the window, which is a rendering quantity. Confining it to the
//! window would have been the natural thing to write and would have made the
//! recorded aim a function of somebody's monitor.
//!
//! # 2. The timestamp comes from as close to the device as the platform allows,
//!    and this build says exactly how close that is
//!
//! See [`CLOCK`]. The short version, because it is the finding that mattered
//! most and it contradicts what this work set out assuming: **no platform in
//! `docs/ENGINEERING.md`'s matrix hands this client a device timestamp through
//! `winit`.** The stamp is taken when the event is *dequeued from the platform*,
//! which is one callback per event as the queue drains — not once per rendered
//! frame, which would stamp every event in a frame with the frame's time and
//! bake the renderer's jitter into the record M8 has to read jitter out of.
//!
//! [`Clock`] is recorded beside the samples so that a later analysis reads what
//! the number is rather than assuming what it ought to be.
//!
//! # 3. One sample per device event, never one sample per change
//!
//! [`InputTrace::moved`] appends unconditionally. It does not ask whether the
//! aim moved, whether the drawn position changed, or whether a frame was due.
//!
//! This is the property the terminal could not have and the one R14 did not
//! notice it was missing. A terminal emits a mouse report only when the pointer
//! crosses into a new character cell, so the *rate* of events is a function of
//! how fast the pointer is moving: sweep quickly and every cell crossed
//! produces an event; creep slowly and almost nothing is emitted. Inter-arrival
//! times recorded that way measure the pointer's speed, not the hand's rhythm —
//! which contaminates the timing statistics R14 explicitly claimed were
//! untouched. Recording each device event as it arrives makes the sampling rate
//! the device's report rate, which is a property of the mouse and is constant
//! whatever the hand is doing.
//!
//! The alternative permitted design — sample the aim at a fixed interval — was
//! rejected, and the reason is that it would be a *resampling* of a stream this
//! client already has in full. It would need a second clock, it would alias
//! anything faster than its interval, and it could only lose information
//! relative to recording the events themselves. There is no third option:
//! "record when the position changes" is the terminal's failure with a window
//! in front of it.

use sim::{Digest, Fx, FxVec2, RULES, digest_bytes};

/// World units the aim travels per unit of device motion.
///
/// A sensitivity, and the one number in the capture path that is a taste rather
/// than a fact. At this value a 1600-count-per-inch mouse crosses the 256-unit
/// map in about an inch, and one device count moves the aim by `0.15` world
/// units — which is the *effective aim resolution* of this client, and is set by
/// the mouse rather than by the display.
///
/// **It was `0.05` until the first playtest, and what moved it is that the taste
/// was tasted.** The complaint that came back was that the cursor in the window
/// moves more slowly than the pointer in every other application, and the
/// arithmetic is the answer rather than the defence: the drawn aim advances
/// `WORLD_UNITS_PER_COUNT` times the projection's pixels-per-world-unit, which
/// at the default window is 3.125, so `0.05` was **a sixth of a pixel per
/// count** against a desktop pointer's one. Crossing the map cost 3.2 inches of
/// desk at 1600 CPI. At `0.15` it costs about one, which is roughly what the
/// same hand spends crossing a screen.
///
/// What it costs is stated rather than absorbed: the aim's quantum is three
/// times coarser, `0.15` world units against a champion radius of `0.5`. That is
/// affordable because nothing in this client asks a player to be accurate to a
/// world unit — an attack click resolves to the nearest enemy within six of the
/// aim, the targeted spell within `RULES.targeted_range`, and a skillshot is a
/// direction, where `0.15` at a twenty-unit throw is under half a degree.
///
/// It scales the [`Aim`] path only. [`InputTrace`] records the device's own
/// units, so changing this number does not change a single byte of recorded
/// telemetry — which is the point of the two paths being separate, and is
/// asserted in `client/tests/capture.rs`. What it *does* change is what a
/// recorded **aim** means, which is why `docs/SCHEMA.md` §4b records it per
/// session as `world_units_per_count_e6`: a corpus spanning a change can be
/// split rather than pooled. Moving it now is free because the corpus is empty;
/// moving it after the first recording session is not.
pub const WORLD_UNITS_PER_COUNT: f64 = 0.15;

/// How many samples a trace holds before it starts counting instead of keeping.
///
/// A thousand-hertz mouse for twenty minutes is 1.2 million motion events, so
/// the bound is generous rather than tight; what it exists for is that an
/// unbounded buffer in a client is a way to run a machine out of memory, and a
/// silent one is worse than a counted one.
pub const MAX_SAMPLES: usize = 4_000_000;

/// Where a sample's timestamp came from. Never guessed, always recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clock {
    /// The timestamp the input device itself reported, carried through the
    /// platform unchanged. Nothing in this build produces one; see [`CLOCK`].
    Device,
    /// This client's monotonic clock, read in the callback that received the
    /// event as the platform's queue drained.
    ///
    /// It includes the device's transport latency, the kernel's, the display
    /// server's and this process's scheduling delay. It excludes the rendering
    /// cadence, because it is not read once per frame.
    Dequeue,
}

/// What [`Sample::at_ns`] actually is, on this platform, in this build.
///
/// `Dequeue` everywhere, and the per-platform findings behind that one word are
/// worth having in the file rather than only in `docs/ARCHITECTURE.md`, because
/// this constant is what a reader will find first:
///
/// | Platform | Does a device timestamp exist? | Does it reach this client? |
/// | --- | --- | --- |
/// | Linux / Wayland | **Yes.** `zwp_relative_pointer_v1::relative_motion` carries `utime_hi`/`utime_lo`, a microsecond stamp the compositor takes from `libinput`, which takes it from the kernel's `evdev` event | **No.** `winit` destructures that event as `{ dx_unaccel, dy_unaccel, .. }` and the `..` is the timestamp |
/// | Linux / X11 | **Partly.** `XIRawEvent.time` is an X server millisecond stamp, taken when the server processed the event — nearer the device than this process is, but not the device's | **No.** `winit` reads it only to remember the connection's last-seen server time |
/// | Windows | **No.** `WM_INPUT`'s `RAWMOUSE` has no timestamp field at all. `GetMessageTime()` exists and is a millisecond queue-post time, not a device time | **No.** `winit` does not surface it either |
/// | macOS | **Yes.** `NSEvent::timestamp` is a process-uptime stamp taken when the event entered the window server | **No.** `winit` does not surface it |
///
/// So the honest statement is per platform and it is the same on all four:
/// **the device timestamp is either absent from the platform or discarded by the
/// window library, and this client substitutes the dequeue time and says so.**
/// The substitution is the thing R14's successor has to be honest about, and
/// naming it in a type is the difference between a limitation and a lie.
///
/// The reopening criterion is in `docs/ARCHITECTURE.md`: `winit` carrying a
/// timestamp on `DeviceEvent` moves this constant to `Device` on the platforms
/// that have one, and the same move would follow from reading `evdev`
/// directly — which was rejected for being a second input stack, a permission
/// a participant has to be granted, and available on one of the two platforms
/// the corpus is recorded on.
pub const CLOCK: Clock = Clock::Dequeue;

/// A discrete control, whichever device it is on.
///
/// Mouse buttons and keys in one type because the telemetry does not care which
/// device a press came from — what it records is that a control went down at a
/// time. The variants are the client's five orders and nothing else, so a key
/// this client does not use produces no sample rather than a sample nobody can
/// interpret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// Left mouse button: walk to the aim.
    Move,
    /// Right mouse button: attack the enemy nearest the aim, or walk there.
    Attack,
    /// `Q`: the skillshot, aimed from the champion through the aim.
    Skillshot,
    /// `W`: the targeted spell.
    Targeted,
    /// `S`: stop.
    Stop,
}

impl Control {
    /// The byte this control is recorded as. Written out rather than derived
    /// from the discriminant, so that reordering the enum cannot silently
    /// reinterpret a recorded trace.
    ///
    /// Public because it is part of the corpus's format now:
    /// `crate::health::telemetry_part` writes it and
    /// `replay::telemetry::Control::from_tag` reads it.
    #[must_use]
    pub const fn tag(self) -> u8 {
        match self {
            Self::Move => 0,
            Self::Attack => 1,
            Self::Skillshot => 2,
            Self::Targeted => 3,
            Self::Stop => 4,
        }
    }
}

/// What one device event was.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// A relative movement, in the device's own units, exactly as reported.
    ///
    /// Not pixels: on every backend this client uses, these are the
    /// unaccelerated counts the pointing device produced. They are `f64`
    /// because the platform reports them that way and rounding them here would
    /// be this module's own version of the mistake it exists to avoid.
    Moved {
        /// Rightward device motion, in the device's units.
        dx: f64,
        /// Downward device motion, in the device's units. Downward-positive is
        /// the platforms' convention and is kept here rather than corrected,
        /// because a record that silently flips a sign is a record somebody has
        /// to know a secret about.
        dy: f64,
    },
    /// A control changed state.
    Pressed {
        /// Which control.
        control: Control,
        /// Down, or up.
        down: bool,
    },
    /// **The one thing in this trace that is not the hand.**
    ///
    /// A server view for `tick` reached this client and the client answered it
    /// with the intention numbered `seq`. Thirty a second, against the device's
    /// hundreds; `docs/SCHEMA.md` §11 carries what that costs.
    ///
    /// It is here because a device stream without it is a hand in a vacuum. An
    /// inter-arrival distribution and a curvature statistic can be computed from
    /// motions alone; a **reaction** cannot, because a reaction is measured from
    /// the moment the player was shown something, and the only clock on which
    /// that moment and the click that answered it are both readable is this one.
    /// `tick` is the replay's clock and `seq` is the replay log's per-player
    /// counter, so this record is where the two namespaces meet.
    ///
    /// **It is not a device event**, and [`InputTrace::stats`] counts it apart
    /// from them. That distinction is load bearing rather than tidy:
    /// `docs/SCHEMA.md` §6 refuses a seat that recorded zero device events, which
    /// is the corpus's one mechanical defence against a headless client — and a
    /// headless client receives views. Counting these among `samples` would hand
    /// that defence to the exact attacker it exists to catch.
    Viewed {
        /// The tick of the view that arrived.
        tick: u32,
        /// The sequence number of the intention sent in answer.
        seq: u32,
    },
}

impl Event {
    /// Whether a device produced this.
    #[must_use]
    pub const fn is_device(&self) -> bool {
        matches!(self, Self::Moved { .. } | Self::Pressed { .. })
    }
}

/// One recorded event, as it arrived.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// Nanoseconds on this client's monotonic clock, from the source [`CLOCK`]
    /// names.
    pub at_ns: u64,
    /// What happened.
    pub event: Event,
}

/// Every device event of a session, in arrival order.
///
/// Deliberately not a replay, not a recording and not a corpus file: `M5` is
/// where a record format is decided, and this work runs before it precisely so
/// that M5 knows what telemetry exists to put in one. What this type is, is the
/// answer to that question — raw deltas, dequeue-stamped, one entry per device
/// event — held in memory and reported on by
/// [`InputTrace::stats`].
#[derive(Clone, Debug, Default)]
pub struct InputTrace {
    samples: Vec<Sample>,
    dropped: u64,
}

impl InputTrace {
    /// An empty trace.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            samples: Vec::new(),
            dropped: 0,
        }
    }

    /// Records one motion event, **unconditionally**.
    ///
    /// There is no predicate here and there must never be one. A condition on
    /// this call — the aim moved, the frame is due, the value differs from the
    /// last — is the speed-dependent sampling this module exists to remove, and
    /// `client/tests/capture.rs` fails on a motion whose effect on the aim is
    /// nil precisely so that adding one turns a test red rather than a
    /// distribution subtly wrong.
    pub fn moved(&mut self, at_ns: u64, dx: f64, dy: f64) {
        self.push(Sample {
            at_ns,
            event: Event::Moved { dx, dy },
        });
    }

    /// Records one control transition.
    pub fn pressed(&mut self, at_ns: u64, control: Control, down: bool) {
        self.push(Sample {
            at_ns,
            event: Event::Pressed { control, down },
        });
    }

    /// Records the arrival of a view and the intention that answered it.
    ///
    /// Appended into the same sequence as the device events rather than into a
    /// list beside it, so that the **order** of a click against the frame that
    /// prompted it is a property of the record rather than of whoever merges two
    /// lists afterwards. See [`Event::Viewed`] for why it exists and why it is
    /// not counted as a device event.
    pub fn viewed(&mut self, at_ns: u64, tick: u32, seq: u32) {
        self.push(Sample {
            at_ns,
            event: Event::Viewed { tick, seq },
        });
    }

    fn push(&mut self, sample: Sample) {
        if self.samples.len() >= MAX_SAMPLES {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        self.samples.push(sample);
    }

    /// Everything recorded, in arrival order.
    #[must_use]
    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    /// Events that arrived after the buffer was full.
    #[must_use]
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }

    /// How many samples there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether nothing has been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// A fingerprint of the whole trace.
    ///
    /// Hand-written and exhaustively destructured, in the workspace's usual
    /// style and for the workspace's usual reason: a field added to a sample and
    /// not hashed would leave two traces comparing equal after they had stopped
    /// being fully compared. The float bits are hashed rather than any rounded
    /// form, because the claim this digest is used to make — that two runs
    /// captured *the same thing* — is a claim about the bytes the device
    /// produced.
    #[must_use]
    pub fn digest(&self) -> Digest {
        let Self { samples, dropped } = self;
        let mut out = Vec::with_capacity(samples.len().saturating_mul(20).saturating_add(32));
        out.extend_from_slice(b"moba/client/trace/v1");
        out.extend_from_slice(&dropped.to_be_bytes());
        out.extend_from_slice(&(samples.len() as u64).to_be_bytes());
        for sample in samples {
            let Sample { at_ns, event } = sample;
            out.extend_from_slice(&at_ns.to_be_bytes());
            match *event {
                Event::Moved { dx, dy } => {
                    out.push(0);
                    out.extend_from_slice(&dx.to_bits().to_be_bytes());
                    out.extend_from_slice(&dy.to_bits().to_be_bytes());
                }
                Event::Pressed { control, down } => {
                    out.push(1);
                    out.push(control.tag());
                    out.push(u8::from(down));
                }
                Event::Viewed { tick, seq } => {
                    out.push(2);
                    out.extend_from_slice(&tick.to_be_bytes());
                    out.extend_from_slice(&seq.to_be_bytes());
                }
            }
        }
        digest_bytes(&out)
    }

    /// The measurement `docs/RISKS.md` R14 is closed against.
    #[must_use]
    pub fn stats(&self) -> TraceStats {
        let mut gaps: Vec<u64> = Vec::with_capacity(self.samples.len());
        let mut previous: Option<u64> = None;
        let mut samples = 0usize;
        let mut moves = 0usize;
        let mut views = 0usize;
        let mut smallest: Option<f64> = None;
        let mut travelled = 0.0f64;
        let mut coincident = 0usize;
        let mut last_move: Option<(u64, f64, f64)> = None;

        for sample in &self.samples {
            // Over **device events only**. `median_gap_ns` is the number
            // `docs/SCHEMA.md` §4b requires to be read against the declared
            // `device_polling_hz`, and a view anchor thirty times a second in the
            // middle of that distribution would be the client's tick rate showing
            // up as if it were the mouse's report rate.
            if !sample.event.is_device() {
                views = views.saturating_add(1);
                continue;
            }
            samples = samples.saturating_add(1);
            if let Some(last) = previous {
                gaps.push(sample.at_ns.saturating_sub(last));
            }
            previous = Some(sample.at_ns);
            if let Event::Moved { dx, dy } = sample.event {
                if let Some((was, x, y)) = last_move
                    && sample.at_ns.saturating_sub(was) < COINCIDENT_NS
                    && x.to_bits() == dx.to_bits()
                    && y.to_bits() == dy.to_bits()
                {
                    coincident = coincident.saturating_add(1);
                }
                last_move = Some((sample.at_ns, dx, dy));
                moves = moves.saturating_add(1);
                let length = dx.hypot(dy);
                travelled += length;
                if length > 0.0 && smallest.is_none_or(|least| length < least) {
                    smallest = Some(length);
                }
            }
        }
        gaps.sort_unstable();
        // The one place this module does floating-point arithmetic on a
        // timestamp, and it is a summary rather than a record: the samples keep
        // their integer nanoseconds and nothing downstream of `stats` reaches
        // the trace. A mean over a million `u64` nanosecond gaps overflows
        // nothing here — the sum is bounded by the span — but it is computed in
        // `f64` because the standard deviation beside it has to be.
        #[expect(
            clippy::cast_precision_loss,
            reason = "a summary of a distribution, never a recorded value"
        )]
        let (gap_mean_ns, gap_sd_ns) = {
            if gaps.is_empty() {
                (0.0, 0.0)
            } else {
                let count = gaps.len() as f64;
                let mean = gaps.iter().map(|gap| *gap as f64).sum::<f64>() / count;
                let variance = gaps
                    .iter()
                    .map(|gap| {
                        let deviation = *gap as f64 - mean;
                        deviation * deviation
                    })
                    .sum::<f64>()
                    / count;
                (mean, variance.sqrt())
            }
        };

        TraceStats {
            samples,
            moves,
            views,
            span_ns: match (self.samples.first(), self.samples.last()) {
                (Some(first), Some(last)) => last.at_ns.saturating_sub(first.at_ns),
                _ => 0,
            },
            gaps_ns: Percentiles::of(&gaps),
            gap_mean_ns,
            gap_sd_ns,
            finest_count: smallest,
            finest_world_units: smallest.map(|counts| counts * WORLD_UNITS_PER_COUNT),
            travelled_counts: travelled,
            coincident,
        }
    }
}

/// A distribution, reported as order statistics rather than as a mean.
///
/// A mean inter-arrival time is exactly the number that hides the failure this
/// work is about: a stream that alternates between a burst and a stall has the
/// same mean as one that ticks steadily, and only the spread tells them apart.
///
/// # Why there is a 99th percentile and not only a 95th
///
/// The two failures this record can have are different distributions and only
/// one of them is visible at the 95th. Gaussian jitter of a fraction of a
/// millisecond is harmless — it is a small fraction of the human variability
/// `docs/MILESTONES.md` M8's detectors are looking for, and it widens a
/// distribution without moving its shape. An occasional stall of ten or fifteen
/// milliseconds is not harmless at all: at 125 Hz that is one sample in a
/// hundred or fewer, so it does not reach the 95th percentile, and what it looks
/// like in the record is a hand that hesitated. A detector calibrated on a
/// corpus with those in it has been calibrated on the client's scheduler.
///
/// So the tail is reported at a resolution where a one-in-a-hundred event is
/// visible, and [`TraceStats::gap_sd_ns`] is reported beside it, because "the
/// spread is small" and "the tail is bounded" are two claims and the residual
/// R14 leaves open needs both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Percentiles {
    /// Smallest observation.
    pub min: u64,
    /// 5th percentile.
    pub p05: u64,
    /// Median.
    pub p50: u64,
    /// 95th percentile.
    pub p95: u64,
    /// 99th percentile. The tail a long frame writes into, and the one a mean
    /// and a 95th percentile both hide.
    pub p99: u64,
    /// Largest observation.
    pub max: u64,
}

impl Percentiles {
    /// Order statistics of an already-sorted slice.
    #[must_use]
    pub fn of(sorted: &[u64]) -> Self {
        let at = |fraction: usize| -> u64 {
            if sorted.is_empty() {
                return 0;
            }
            let last = sorted.len().saturating_sub(1);
            let index = last.saturating_mul(fraction).saturating_div(100);
            sorted.get(index).copied().unwrap_or(0)
        };
        Self {
            min: sorted.first().copied().unwrap_or(0),
            p05: at(5),
            p50: at(50),
            p95: at(95),
            p99: at(99),
            max: sorted.last().copied().unwrap_or(0),
        }
    }
}

/// What a captured trace measures, reported rather than asserted.
#[derive(Clone, Copy, Debug)]
pub struct TraceStats {
    /// Device events recorded, of every kind. **Anchors are not among them**;
    /// see [`Event::Viewed`] for why the distinction has teeth.
    pub samples: usize,
    /// Motion events among them.
    pub moves: usize,
    /// [`Event::Viewed`] anchors, counted apart from the device events.
    pub views: usize,
    /// Nanoseconds between the first sample and the last.
    pub span_ns: u64,
    /// Inter-arrival times, in nanoseconds.
    pub gaps_ns: Percentiles,
    /// The mean inter-arrival time, in nanoseconds.
    ///
    /// Reported *beside* the order statistics and never instead of them, for
    /// the reason [`Percentiles`] gives. It is here because the pair
    /// (mean, standard deviation) is what a claim of the form "the noise this
    /// client adds is a few per cent of the signal M8 is looking for" is
    /// arithmetic about, and a claim with no number in it is what
    /// `docs/RISKS.md` R14 was made of the first time.
    pub gap_mean_ns: f64,
    /// The population standard deviation of the inter-arrival times.
    ///
    /// The number `docs/RISKS.md` R14's timestamp half is closed against, or
    /// not. Against a signal whose human spread is of the order of ten
    /// milliseconds, a client-side spread under one is noise worth a few per
    /// cent of it; anything approaching the signal is a covariate a detector
    /// would have to model.
    pub gap_sd_ns: f64,
    /// The smallest non-zero motion observed, in device counts. This is the
    /// spatial resolution of the record.
    pub finest_count: Option<f64>,
    /// The same number in world units, which is what it is worth against
    /// `docs/RISKS.md` R14's character cell.
    pub finest_world_units: Option<f64>,
    /// Total path length in device counts, so that a resolution figure can be
    /// read against how far the pointer actually went.
    pub travelled_counts: f64,
    /// Consecutive motion samples less than [`COINCIDENT_NS`] apart carrying
    /// bit-identical deltas.
    ///
    /// **Reported, never filtered**, and the distinction is the whole of why
    /// this field exists. No hand produces two identical deltas five
    /// microseconds apart, so a non-zero count here means the platform
    /// delivered one device event more than once and the record is no longer
    /// one sample per motion. That is not hypothetical: it is what an X11
    /// pointer grab does, and it is why `crate::gfx` does not take one.
    ///
    /// Dropping the duplicates instead would be a predicate on the contents of
    /// the record, which is the speed-dependent sampling this whole module
    /// exists to refuse — with a better excuse. So the trace keeps everything
    /// it was given and the statistic says whether what it was given was sound.
    pub coincident: usize,
}

/// How close together two identical motion samples have to be before they are
/// evidence that the platform delivered one event twice rather than that a hand
/// did the same thing twice.
///
/// A tenth of a millisecond. The fastest pointing device in ordinary use reports
/// at 8 kHz, which is 125 microseconds apart, so this is inside the shortest
/// legitimate gap and does not accuse a real device of duplicating.
pub const COINCIDENT_NS: u64 = 100_000;

/// Where the player is pointing, in the world.
///
/// An integrator over raw device deltas, and that is the whole of it. It holds
/// no pixel, is told no window size, and has no way to ask what was drawn — the
/// absence of those parameters is the property, and `client/tests/capture.rs`
/// asserts it by running the same events under two different windows and
/// requiring the same answer.
///
/// The accumulator is `f64` rather than `Fx` so that motion finer than the
/// fixed-point resolution is not lost between events: a stream of deltas each
/// below `2^-16` world units would round to nothing individually and does not
/// here. Only [`Aim::world`] rounds, and it rounds once, at the boundary where
/// the simulation's integers begin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aim {
    x: f64,
    y: f64,
}

impl Default for Aim {
    fn default() -> Self {
        Self::centred()
    }
}

impl Aim {
    /// An aim at the middle of the map.
    #[must_use]
    pub const fn centred() -> Self {
        Self { x: 0.0, y: 0.0 }
    }

    /// Integrates one raw device delta.
    ///
    /// Note what this function is not given: no viewport, no scale factor, no
    /// cursor position, nothing the renderer produced. It could not consult the
    /// screen if it wanted to, and that is deliberate rather than incidental.
    ///
    /// The clamp is to `RULES.map_half_extent` — a constant of the game that
    /// `rules_hash()` covers — and not to the drawn window, because clamping to
    /// the window would make the aim a function of a monitor.
    ///
    /// `dy` is negated because device motion is reported downward-positive and
    /// the world's `y` grows upward. That is a change of sign, not a change of
    /// resolution, and it is the only transformation between the device and the
    /// accumulator besides the sensitivity.
    pub fn apply(&mut self, dx: f64, dy: f64) {
        let limit = f64::from(RULES.map_half_extent.to_raw()) / 65536.0;
        self.x = (self.x + dx * WORLD_UNITS_PER_COUNT).clamp(-limit, limit);
        self.y = (self.y - dy * WORLD_UNITS_PER_COUNT).clamp(-limit, limit);
    }

    /// The world point the player is aiming at, in the fixed point the
    /// simulation speaks.
    ///
    /// The one rounding in the capture path, and it happens here rather than on
    /// the way in, so that everything upstream of the simulation keeps the
    /// device's own precision.
    #[must_use]
    pub fn world(self) -> FxVec2 {
        FxVec2::new(to_fx(self.x), to_fx(self.y)).clamp_components(RULES.map_half_extent)
    }
}

/// A world coordinate in `f64`, rounded into `Fx`.
///
/// Clamped into the fixed-point type's own range before the cast so that the
/// conversion is total; the aim is already inside the map, so the clamp is a
/// guard rather than a rule.
fn to_fx(value: f64) -> Fx {
    let clamped = value.clamp(-32768.0, 32767.0);
    Fx::from_raw((clamped * 65536.0) as i32)
}
