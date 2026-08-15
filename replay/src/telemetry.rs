//! The telemetry companion: the device-event stream, at its own cadence, sealed.
//!
//! # What this is, and why it is not in the replay
//!
//! `sim` consumes **one intention per tick at 30 Hz**. A hand produces device
//! events at 125 Hz to 1 kHz. `docs/RISKS.md` R14 rebuilt the client's capture
//! path so that every one of those events is recorded verbatim with its own
//! timestamp, and then M5 and M6 kept the whole stream out of the corpus: a
//! replay carries the seed and the log, `docs/SCHEMA.md` §4b carries four summary
//! numbers per seat, and the stream itself lived in a client's memory until the
//! process exited.
//!
//! `docs/detectors/README.md` is where that bill came due. Two of
//! `docs/MILESTONES.md` M8's five candidate signals — the inter-arrival
//! distribution and aim-correction curvature — are not detectors whose thresholds
//! nobody has calibrated. They are detectors **whose quantity is not in the
//! corpus**, at any resolution, and no amount of recording under that policy
//! produces it. This file is that decision reversed, at the last moment at which
//! reversing it destroys nothing: the corpus is empty.
//!
//! # Three properties, and each of them is a refusal somewhere
//!
//! **1. It is a separate file and the replay does not contain it.** M5's
//! invariant does not move: a resimulation is a function of the seed and the
//! input log alone, and nothing no rule reads can influence it. Folding a
//! kilohertz stream into the artefact resimulation is a function of would have
//! made the resimulation a function of something `step` never sees, which is the
//! reason `crate::manifest` gave for excluding it and the reason is still good.
//!
//! **2. The replay's manifest carries this file's digest, not its contents.**
//! [`crate::manifest::Commitment`]. That is what makes a companion
//! unsubstitutable — an attacker holding a key the registry accepts can seal a
//! second, smoother companion for the same match, and the replay's commitment is
//! what refuses it ([`TelemetryError::Substituted`]) — and it is what keeps a
//! replay verifiable *without* one. A replay that commits to
//! [`crate::manifest::Commitment::Absent`] is a complete replay of a match that
//! recorded no device telemetry, and `verify` says so in those words rather than
//! failing.
//!
//! **3. There is one file format and it is sealed.** The M5 lesson applied to
//! this artefact: a reader that accepts a sealed and an unsealed companion
//! accepts the weaker, and a corpus holding both holds files nobody can tell
//! apart at a glance. [`TelemetryLog`] is the in-memory product with no encoding
//! at all, and [`seal`] is the only path to a disk.
//!
//! The one thing that is not sealed is a [`TelemetryPart`], and it cannot be:
//! `docs/ARCHITECTURE.md` forbids `client` a normal dependency on `replay`
//! because `replay` owns the signing key, so a client structurally cannot sign
//! anything. A part is a **transport between two processes**, in the same
//! register as `*.session-part`, it names one seat rather than a match, and it is
//! not a corpus artefact: `Corpus::audit` reports a match directory holding a
//! file this schema does not name, so a part left in one is a finding rather than
//! a second format.
//!
//! # What a sample is, and the quantisation that is refused
//!
//! A [`Sample`] is a timestamp and one of three things. The motion carries the
//! platform's `f64` pair **verbatim, by its bits** — unscaled, unrounded and
//! unquantised.
//!
//! Rounding those to integers is the obvious saving and it is **refused**, by
//! name: device counts look like integers, a two-byte pair would take the record
//! from 25 bytes to 11, and what it would put in the corpus is a grid. That is
//! `docs/RISKS.md` R14 exactly — a transformation applied before the sample
//! exists, which no precision downstream undoes — and the detector it would
//! destroy is the curvature detector this file exists to make possible, whose
//! whole subject is the *shape* of a trajectory at the finest resolution the
//! device produced. `docs/SCHEMA.md` §11 carries the size budget that decision
//! costs, in bytes, per match and per corpus.
//!
//! # The clock, and what is deliberately absent
//!
//! `at_ns` is the client's own monotonic clock, from the source
//! `client::input::CLOCK` names, and [`SeatTrace::clock`] records which source
//! that was per seat rather than assuming it. Its epoch is the moment that client
//! started and is **not** comparable to any other seat's.
//!
//! That is a decision rather than an oversight. Two seats' streams have no common
//! time reference here, and the only one they have is the **tick**, through
//! [`Event::Viewed`] — which is the server's. A wall-clock anchor would be the
//! client's own clock, which `docs/SCOPE.md`'s adversary model puts in the
//! attacker's hands by definition, so a corpus that aligned two hands on it would
//! be aligning them on a number a client wrote.

use sim::{Digest, PLAYER_COUNT, Seat, Tick, digest_bytes};

use crate::ByteReader;
use crate::keys::{KeyRegistry, KeyStatus, Signature, SigningKey, VerifyingKey};
use crate::manifest::{Commitment, MatchId, SessionFacts};
use crate::session::{Clock, Platform};

/// What every sealed companion starts with.
const MAGIC: [u8; 8] = *b"MOBATLMY";

/// The magic a client's per-seat part starts with. A different eight bytes from
/// [`MAGIC`], so that handing a part to a reader of companions is refused by the
/// first eight bytes rather than by an arithmetic error further in.
const PART_MAGIC: [u8; 8] = *b"MOBATPRT";

/// The companion container format this build writes.
///
/// Its own number, not the replay's and not the protocol's: one is how a match
/// is laid out on a disk, one is how its telemetry is, and they change for
/// different reasons.
pub const FORMAT: u16 = 1;

/// The part format this build reads. Carried by the file a client writes.
pub const PART_FORMAT: u16 = 1;

/// One record's width on disk, in bytes.
///
/// Fixed across all three [`Event`] variants, which is what lets a reader bound
/// the record count against the buffer before allocating and what makes the
/// file's length a function of how many events it holds rather than of which
/// ones. The widest variant is the motion, and it is the one that must not
/// shrink: see this module's header on the quantisation that is refused.
pub const SAMPLE_BYTES: usize = 1 + 8 + 16;

/// A discrete control, as the corpus records it.
///
/// The mirror of `client::input::Control`, here for the reason
/// [`crate::session::Clock`] mirrors `client::input::Clock`: `client` cannot link
/// `replay`, so the two crates cannot share a type and the record crosses as
/// bytes. `client/tests/telemetry_part.rs` is what keeps the two in step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// Left mouse button: walk to the aim.
    Move,
    /// Right mouse button: attack the enemy nearest the aim, or walk there.
    Attack,
    /// `Q`: the skillshot.
    Skillshot,
    /// `W`: the targeted spell.
    Targeted,
    /// `S`: stop.
    Stop,
}

impl Control {
    /// The byte this control is recorded as, written out rather than derived
    /// from a discriminant so that reordering the enum cannot reinterpret a
    /// recorded corpus.
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

    /// The control this byte names, or `None`.
    #[must_use]
    pub const fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(Self::Move),
            1 => Some(Self::Attack),
            2 => Some(Self::Skillshot),
            3 => Some(Self::Targeted),
            4 => Some(Self::Stop),
            _ => None,
        }
    }
}

/// What one record in the stream is.
///
/// Three variants, and the third is the one that is not a device event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// A relative device motion, in the device's own units, exactly as the
    /// platform reported it.
    ///
    /// `f64` because that is what the platform reports and what
    /// `client::input::Motion::Moved` holds; rounding here would be this format's
    /// version of the mistake `docs/RISKS.md` R14 is about. Downward-positive is
    /// the platforms' convention and is kept rather than corrected, because a
    /// record that silently flips a sign is a record somebody has to know a
    /// secret about.
    Moved {
        /// Rightward device motion, in the device's units.
        dx: f64,
        /// Downward device motion, in the device's units.
        dy: f64,
    },
    /// A control changed state.
    Pressed {
        /// Which control.
        control: Control,
        /// Down, or up.
        down: bool,
    },
    /// **The anchor, and the only thing in this file that is not the hand.**
    ///
    /// A server view for `tick` reached this client, and the client answered it
    /// with the intention numbered `seq`. One record per received frame, thirty a
    /// second, which is where the two namespaces meet: `tick` is the replay's
    /// clock and `seq` is the replay log's per-player counter, so every sample in
    /// this stream can be placed against the match without either side carrying a
    /// wall clock.
    ///
    /// Without it, a device stream is a hand in a vacuum: an inter-arrival
    /// distribution and a curvature statistic can be computed from motions alone,
    /// but a **reaction** cannot, because a reaction is measured from the moment
    /// the player was shown something — and the only clock on which that moment
    /// and the click that answered it are both readable is this one.
    ///
    /// The moment the intention actually left is deliberately **not** a fourth
    /// record. It is this moment plus one pass of the capture loop, which
    /// `client::health::Cadence` bounds per session and `docs/RISKS.md` R14
    /// measured at 16 µs of standard deviation in `release`; a record per
    /// intention would have cost another thirty records a second to restate a
    /// number two other fields already bound.
    Viewed {
        /// The tick of the view that arrived.
        tick: Tick,
        /// The sequence number of the intention this client sent in answer.
        seq: u32,
    },
}

impl Event {
    /// Whether this is something a device produced.
    ///
    /// The distinction is load bearing rather than tidy: `docs/SCHEMA.md` §6
    /// refuses a seat that recorded **zero device events**, which is the corpus's
    /// one mechanical defence against a headless client, and a headless client
    /// receives views. Counting an anchor as a device event would hand that
    /// defence to the exact attacker it exists to catch.
    #[must_use]
    pub const fn is_device(&self) -> bool {
        matches!(self, Self::Moved { .. } | Self::Pressed { .. })
    }
}

/// One record, as it arrived.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// Nanoseconds on this seat's own monotonic clock, from the source
    /// [`SeatTrace::clock`] names. Comparable within a seat and with nothing
    /// else; see this module's header.
    pub at_ns: u64,
    /// What happened.
    pub event: Event,
}

impl Sample {
    fn write(&self, out: &mut Vec<u8>) {
        let Self { at_ns, event } = self;
        let before = out.len();
        match *event {
            Event::Moved { dx, dy } => {
                out.push(0);
                out.extend_from_slice(&at_ns.to_be_bytes());
                out.extend_from_slice(&dx.to_bits().to_be_bytes());
                out.extend_from_slice(&dy.to_bits().to_be_bytes());
            }
            Event::Pressed { control, down } => {
                out.push(1);
                out.extend_from_slice(&at_ns.to_be_bytes());
                out.push(control.tag());
                out.push(u8::from(down));
            }
            Event::Viewed { tick, seq } => {
                out.push(2);
                out.extend_from_slice(&at_ns.to_be_bytes());
                out.extend_from_slice(&tick.0.to_be_bytes());
                out.extend_from_slice(&seq.to_be_bytes());
            }
        }
        // Fixed width, zero padded. The reader refuses a non-zero pad, so one
        // sample has exactly one encoding — otherwise the stream's digest would
        // stop being a function of the stream.
        out.resize(before.saturating_add(SAMPLE_BYTES), 0);
    }

    fn read(reader: &mut ByteReader<'_>) -> Option<Self> {
        let bytes = reader.array::<SAMPLE_BYTES>()?;
        let mut record = ByteReader::new(&bytes);
        let tag = record.u8()?;
        let at_ns = record.u64()?;
        let event = match tag {
            0 => {
                let dx = f64::from_bits(record.u64()?);
                let dy = f64::from_bits(record.u64()?);
                // A device does not report a NaN or an infinity, so a file that
                // holds one is not a file this build reads. That is a
                // well-formedness rule in the register of the padding check
                // above, and deliberately **not** a filter on the contents of
                // the record: nothing here drops a sample it dislikes, it
                // refuses a file that is not one.
                if !dx.is_finite() || !dy.is_finite() {
                    return None;
                }
                Event::Moved { dx, dy }
            }
            1 => Event::Pressed {
                control: Control::from_tag(record.u8()?)?,
                down: match record.u8()? {
                    0 => false,
                    1 => true,
                    _ => return None,
                },
            },
            2 => Event::Viewed {
                tick: Tick(record.u32()?),
                seq: record.u32()?,
            },
            _ => return None,
        };
        if record.rest().iter().any(|byte| *byte != 0) {
            return None;
        }
        Some(Self { at_ns, event })
    }
}

/// What one seat's stream is, as the signed manifest states it.
///
/// The counts are in the manifest rather than only derivable from the body
/// because the manifest is what is signed: a stream whose record count nobody
/// committed to is a stream an attacker can shorten. They are also what
/// `Corpus::store` cross-checks against the session record, so the two files
/// cannot drift.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeatTrace {
    /// Which clock `at_ns` came from (`client::input::CLOCK`).
    pub clock: Clock,
    /// The platform this seat was recorded on. What a device count and a
    /// timestamp *are* differs between them (`docs/ARCHITECTURE.md`).
    pub platform: Platform,
    /// The build's sensitivity, in millionths of a world unit per device count.
    ///
    /// It scales the *aim* and not the record, so it is here to make the aim
    /// reconstructible from the stream rather than to be divided out of it.
    pub world_units_per_count_e6: u64,
    /// Device events recorded: motions and control transitions.
    pub samples: u64,
    /// Motions among them.
    pub motions: u64,
    /// [`Event::Viewed`] anchors, which are not device events.
    pub views: u64,
    /// Device events that arrived after the client's buffer was full.
    ///
    /// `client::input::InputTrace::dropped`, which nothing else in the corpus
    /// carries. A stream that lost its tail without saying so would be a stream
    /// whose inter-arrival distribution has a hole nobody can see.
    pub dropped: u64,
}

/// One seat's fixed width in the manifest: a presence tag and the facts.
const SEAT_TRACE_BYTES: usize = 1 + 1 + 1 + 8 * 5;

impl SeatTrace {
    /// How many records this seat's stream holds.
    #[must_use]
    pub const fn records(&self) -> u64 {
        self.samples.saturating_add(self.views)
    }
}

/// The signed half of a companion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelemetryManifest {
    /// The match this is the telemetry of. Checked against the replay's.
    pub match_id: MatchId,
    /// The key that sealed it, which must be the key that sealed the replay.
    pub server_identity: VerifyingKey,
    /// When the match started, in milliseconds since the Unix epoch. The
    /// retention clock in `docs/CONSENT.md` runs from it, and a companion that
    /// outlived its replay would otherwise carry no date to destroy it on.
    pub started_at_unix_ms: u64,
    /// One entry per seat, in seat order. `None` is a seat nobody sat in.
    pub seats: [Option<SeatTrace>; PLAYER_COUNT],
    /// The digest of the body: every seat's records, in seat order.
    pub stream_digest: Digest,
}

/// The manifest's encoded width, which is constant.
pub const TELEMETRY_MANIFEST_BYTES: usize = 16      // match_id
    + 32                                            // server_identity
    + 8                                             // started_at_unix_ms
    + PLAYER_COUNT * SEAT_TRACE_BYTES               // seats
    + 32; // stream_digest

impl TelemetryManifest {
    /// The manifest's bytes.
    ///
    /// Hand-written by exhaustive destructuring, in this workspace's usual style
    /// and for its usual reason: a field added and not encoded is a field outside
    /// the signature, which is a field an attacker changes for free.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let Self {
            match_id,
            server_identity,
            started_at_unix_ms,
            seats,
            stream_digest,
        } = self;
        let mut out = Vec::with_capacity(TELEMETRY_MANIFEST_BYTES);
        out.extend_from_slice(&match_id.0);
        out.extend_from_slice(server_identity.as_bytes());
        out.extend_from_slice(&started_at_unix_ms.to_be_bytes());
        for seat in seats {
            let before = out.len();
            match seat {
                None => out.push(0),
                Some(trace) => {
                    let SeatTrace {
                        clock,
                        platform,
                        world_units_per_count_e6,
                        samples,
                        motions,
                        views,
                        dropped,
                    } = trace;
                    out.push(1);
                    out.push(clock_tag(*clock));
                    out.push(platform_tag(*platform));
                    out.extend_from_slice(&world_units_per_count_e6.to_be_bytes());
                    out.extend_from_slice(&samples.to_be_bytes());
                    out.extend_from_slice(&motions.to_be_bytes());
                    out.extend_from_slice(&views.to_be_bytes());
                    out.extend_from_slice(&dropped.to_be_bytes());
                }
            }
            // One fixed-width slot per seat, occupied or not, so that the
            // manifest's length does not report how many people played.
            out.resize(before.saturating_add(SEAT_TRACE_BYTES), 0);
        }
        out.extend_from_slice(stream_digest.as_bytes());
        out
    }

    /// Reads a manifest, or `None` if these are not the bytes of one.
    ///
    /// Total on every byte string, for the reason `crate::manifest::Manifest` is:
    /// a companion is something a third party hands you.
    #[must_use]
    pub fn decode(reader: &mut ByteReader<'_>) -> Option<Self> {
        let match_id = MatchId(reader.array::<16>()?);
        let server_identity = VerifyingKey::from_bytes(reader.array::<32>()?);
        let started_at_unix_ms = reader.u64()?;

        let mut seats: [Option<SeatTrace>; PLAYER_COUNT] = [const { None }; PLAYER_COUNT];
        for slot in &mut seats {
            let bytes = reader.array::<SEAT_TRACE_BYTES>()?;
            let mut seat = ByteReader::new(&bytes);
            match seat.u8()? {
                0 => {}
                1 => {
                    *slot = Some(SeatTrace {
                        clock: clock_of(seat.u8()?)?,
                        platform: platform_of(seat.u8()?)?,
                        world_units_per_count_e6: seat.u64()?,
                        samples: seat.u64()?,
                        motions: seat.u64()?,
                        views: seat.u64()?,
                        dropped: seat.u64()?,
                    });
                }
                _ => return None,
            }
            if seat.rest().iter().any(|byte| *byte != 0) {
                return None;
            }
        }
        let stream_digest = Digest::from_bytes(reader.array::<32>()?);
        Some(Self {
            match_id,
            server_identity,
            started_at_unix_ms,
            seats,
            stream_digest,
        })
    }

    /// The seats this companion covers, in seat order.
    #[must_use]
    pub fn occupied(&self) -> Vec<usize> {
        self.seats
            .iter()
            .enumerate()
            .filter(|(_, seat)| seat.is_some())
            .map(|(index, _)| index)
            .collect()
    }

    /// Every record this manifest commits to, across every seat.
    #[must_use]
    pub fn records(&self) -> u64 {
        self.seats
            .iter()
            .flatten()
            .fold(0u64, |total, seat| total.saturating_add(seat.records()))
    }
}

const fn clock_tag(clock: Clock) -> u8 {
    match clock {
        Clock::Device => 0,
        Clock::Dequeue => 1,
    }
}

const fn clock_of(tag: u8) -> Option<Clock> {
    match tag {
        0 => Some(Clock::Device),
        1 => Some(Clock::Dequeue),
        _ => None,
    }
}

const fn platform_tag(platform: Platform) -> u8 {
    match platform {
        Platform::Linux => 0,
        Platform::Windows => 1,
        Platform::MacOs => 2,
        Platform::Other => 3,
    }
}

const fn platform_of(tag: u8) -> Option<Platform> {
    match tag {
        0 => Some(Platform::Linux),
        1 => Some(Platform::Windows),
        2 => Some(Platform::MacOs),
        3 => Some(Platform::Other),
        _ => None,
    }
}

/// A match's device telemetry, as the operator assembled it and before it is
/// sealed.
///
/// **This type has no encoding**, and that is the same decision
/// [`crate::Recording`] records: the only thing that reaches a disk is a
/// [`Telemetry`], which is this bound to an identity and a match by a signature.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TelemetryLog {
    /// One stream per seat, in seat order. `None` is a seat nobody sat in.
    pub seats: [Option<SeatStream>; PLAYER_COUNT],
}

/// One seat's stream, before sealing.
#[derive(Clone, Debug, PartialEq)]
pub struct SeatStream {
    /// Which clock the timestamps came from.
    pub clock: Clock,
    /// The platform.
    pub platform: Platform,
    /// The build's sensitivity, in millionths of a world unit per device count.
    pub world_units_per_count_e6: u64,
    /// Device events the client's buffer refused.
    pub dropped: u64,
    /// Every record, in arrival order.
    pub samples: Vec<Sample>,
}

impl SeatStream {
    /// The facts a manifest states about this stream.
    #[must_use]
    pub fn facts(&self) -> SeatTrace {
        let mut samples = 0u64;
        let mut motions = 0u64;
        let mut views = 0u64;
        for sample in &self.samples {
            match sample.event {
                Event::Moved { .. } => {
                    samples = samples.saturating_add(1);
                    motions = motions.saturating_add(1);
                }
                Event::Pressed { .. } => samples = samples.saturating_add(1),
                Event::Viewed { .. } => views = views.saturating_add(1),
            }
        }
        SeatTrace {
            clock: self.clock,
            platform: self.platform,
            world_units_per_count_e6: self.world_units_per_count_e6,
            samples,
            motions,
            views,
            dropped: self.dropped,
        }
    }
}

impl TelemetryLog {
    /// A log with nothing in it.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Assembles a log from the parts the clients wrote.
    ///
    /// The mirror of `crate::session::SessionRecord::assemble`, and it refuses
    /// the same three mistakes an operator collecting files from nine machines
    /// actually makes.
    ///
    /// # Errors
    ///
    /// A message naming what was wrong: a file that is not a part, a part whose
    /// seat does not exist, or two parts claiming one seat.
    pub fn assemble(parts: &[(String, Vec<u8>)]) -> Result<Self, String> {
        let mut log = Self::new();
        for (name, bytes) in parts {
            let part = TelemetryPart::decode(bytes)
                .ok_or_else(|| format!("{name} is not a telemetry part"))?;
            let slot = log
                .seats
                .get_mut(part.seat.index())
                .ok_or_else(|| format!("{name} claims a seat that does not exist"))?;
            if slot.is_some() {
                return Err(format!(
                    "two parts claim seat {}; one of them is {name}",
                    part.seat.index()
                ));
            }
            *slot = Some(part.stream);
        }
        Ok(log)
    }

    /// The seats this log covers.
    #[must_use]
    pub fn occupied(&self) -> Vec<usize> {
        self.seats
            .iter()
            .enumerate()
            .filter(|(_, seat)| seat.is_some())
            .map(|(index, _)| index)
            .collect()
    }

    /// The body: every seat's records, in seat order.
    #[must_use]
    pub fn body(&self) -> Vec<u8> {
        let width = usize::try_from(
            self.seats
                .iter()
                .flatten()
                .fold(0u64, |total, seat| {
                    total.saturating_add(seat.samples.len() as u64)
                })
                .saturating_mul(SAMPLE_BYTES as u64),
        )
        .unwrap_or(0);
        let mut out = Vec::with_capacity(width);
        for seat in self.seats.iter().flatten() {
            for sample in &seat.samples {
                sample.write(&mut out);
            }
        }
        out
    }
}

/// One client's account of its own seat, as it crosses from `client` to here.
///
/// Not a corpus artefact: see this module's header. It is read by whoever seals,
/// once, and the corpus holds the sealed companion instead.
#[derive(Clone, Debug, PartialEq)]
pub struct TelemetryPart {
    /// The seat this client sat in.
    pub seat: Seat,
    /// Everything it recorded.
    pub stream: SeatStream,
}

impl TelemetryPart {
    /// Reads a part, or `None` if these are not the bytes of one.
    ///
    /// Total on every byte string. A part arrives from another process, and the
    /// answer to a file that is not one is `None` rather than a partial record.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut reader = ByteReader::new(bytes);
        if reader.array::<8>()? != PART_MAGIC {
            return None;
        }
        if reader.u16()? != PART_FORMAT {
            return None;
        }
        let seat = Seat::from_index(reader.u8()?)?;
        let clock = clock_of(reader.u8()?)?;
        let platform = platform_of(reader.u8()?)?;
        let world_units_per_count_e6 = reader.u64()?;
        let dropped = reader.u64()?;
        let count = usize::try_from(reader.u64()?).ok()?;
        // Bounded against what is left before anything is allocated: a header
        // claiming four billion records is malformed, not large, and reserving
        // for it first would make that a memory exhaustion rather than a refusal.
        if count.checked_mul(SAMPLE_BYTES)? != reader.remaining() {
            return None;
        }
        let mut samples = Vec::with_capacity(count);
        for _ in 0..count {
            samples.push(Sample::read(&mut reader)?);
        }
        Some(Self {
            seat,
            stream: SeatStream {
                clock,
                platform,
                world_units_per_count_e6,
                dropped,
                samples,
            },
        })
    }

    /// The bytes a client writes.
    ///
    /// Here rather than in `client` so that there is one statement of the layout
    /// and the writer cannot drift from the reader; `client::health` calls it
    /// through no dependency at all, because `client` links `replay` only as a
    /// dev-dependency — `client/tests/telemetry_part.rs` is where the two meet.
    /// The client's own writer is `client::health::telemetry_part`, which
    /// produces these bytes from `client::input::InputTrace` without linking this
    /// crate, and that test requires the two to agree byte for byte.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let Self { seat, stream } = self;
        let SeatStream {
            clock,
            platform,
            world_units_per_count_e6,
            dropped,
            samples,
        } = stream;
        let mut out = Vec::with_capacity(28usize.saturating_add(samples.len() * SAMPLE_BYTES));
        out.extend_from_slice(&PART_MAGIC);
        out.extend_from_slice(&PART_FORMAT.to_be_bytes());
        out.push(seat.index() as u8);
        out.push(clock_tag(*clock));
        out.push(platform_tag(*platform));
        out.extend_from_slice(&world_units_per_count_e6.to_be_bytes());
        out.extend_from_slice(&dropped.to_be_bytes());
        out.extend_from_slice(&(samples.len() as u64).to_be_bytes());
        for sample in samples {
            sample.write(&mut out);
        }
        out
    }
}

/// A match's device telemetry, sealed.
#[derive(Clone, Debug, PartialEq)]
pub struct Telemetry {
    /// The signed half.
    pub manifest: TelemetryManifest,
    /// The signature over [`signed_bytes`].
    pub signature: Signature,
    /// The streams, in seat order.
    pub log: TelemetryLog,
}

/// Seals a telemetry log into a companion.
///
/// The key is the same key that seals the replay, and [`verify`] refuses a
/// companion sealed by another one. Sealing happens here rather than in the
/// client for the reason `crate::seal` gives about `server::Match`: the component
/// that produces the data is the one that must hold no secret.
#[must_use]
pub fn seal(log: &TelemetryLog, session: &SessionFacts, key: &SigningKey) -> Telemetry {
    let mut seats: [Option<SeatTrace>; PLAYER_COUNT] = [const { None }; PLAYER_COUNT];
    for (slot, stream) in seats.iter_mut().zip(log.seats.iter()) {
        *slot = stream.as_ref().map(SeatStream::facts);
    }
    let manifest = TelemetryManifest {
        match_id: session.match_id,
        server_identity: key.verifying(),
        started_at_unix_ms: session.started_at_unix_ms,
        seats,
        stream_digest: digest_bytes(&log.body()),
    };
    let signature = key.sign(&signed_bytes(&manifest));
    Telemetry {
        manifest,
        signature,
        log: log.clone(),
    }
}

/// The bytes a companion's signature covers: the magic, the format, and the
/// manifest.
///
/// The magic and the format are inside the signature for the reason
/// `crate::signed_bytes` puts them there: a file cannot be re-labelled as another
/// format's and re-parsed under different rules while keeping a signature that
/// verifies.
#[must_use]
pub fn signed_bytes(manifest: &TelemetryManifest) -> Vec<u8> {
    let mut out = Vec::with_capacity(TELEMETRY_MANIFEST_BYTES.saturating_add(10));
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&FORMAT.to_be_bytes());
    out.extend_from_slice(&manifest.encode());
    out
}

/// Why a byte string is not a companion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The first eight bytes are not this format's.
    NotTelemetry,
    /// A container format this build does not read.
    UnsupportedFormat(u16),
    /// The bytes ran out, or a field held a value that names nothing.
    Malformed,
    /// Bytes after the last record.
    TrailingBytes,
}

impl core::fmt::Display for ReadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotTelemetry => write!(f, "not a telemetry companion"),
            Self::UnsupportedFormat(found) => {
                write!(f, "companion format {found}, this build reads {FORMAT}")
            }
            Self::Malformed => write!(f, "malformed telemetry companion"),
            Self::TrailingBytes => write!(f, "trailing bytes after the last record"),
        }
    }
}

impl core::error::Error for ReadError {}

impl Telemetry {
    /// The companion's bytes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = signed_bytes(&self.manifest);
        out.extend_from_slice(self.signature.as_bytes());
        out.extend_from_slice(&self.log.body());
        out
    }

    /// The digest a replay's manifest commits to: this whole file.
    ///
    /// The **file** rather than the stream, deliberately. A digest over the
    /// stream alone would let a companion keep its records and change its own
    /// manifest — its match, its identity, its counts — while the replay's
    /// commitment still held.
    #[must_use]
    pub fn digest(&self) -> Digest {
        digest_bytes(&self.encode())
    }

    /// Reads a companion.
    ///
    /// Total on every byte string. Note what this does **not** do: it does not
    /// check the signature, the identity, the stream digest, or the replay that
    /// commits to it. A companion that decodes is a companion whose bytes are
    /// laid out correctly and nothing more; every claim about it comes from
    /// [`verify`].
    ///
    /// # Errors
    ///
    /// [`ReadError`] for anything that is not exactly one well-formed companion.
    pub fn decode(bytes: &[u8]) -> Result<Self, ReadError> {
        let mut reader = ByteReader::new(bytes);
        if reader.array::<8>().ok_or(ReadError::Malformed)? != MAGIC {
            return Err(ReadError::NotTelemetry);
        }
        let format = reader.u16().ok_or(ReadError::Malformed)?;
        if format != FORMAT {
            return Err(ReadError::UnsupportedFormat(format));
        }
        let manifest = TelemetryManifest::decode(&mut reader).ok_or(ReadError::Malformed)?;
        let signature = Signature::from_bytes(reader.array::<64>().ok_or(ReadError::Malformed)?);

        // The record count comes from the signed manifest rather than from a
        // length beside the body, so a shortened body is short against something
        // an attacker cannot change for free.
        let claimed = usize::try_from(manifest.records()).map_err(|_| ReadError::Malformed)?;
        if claimed.saturating_mul(SAMPLE_BYTES) > reader.remaining() {
            return Err(ReadError::Malformed);
        }
        let mut log = TelemetryLog::new();
        for (slot, facts) in log.seats.iter_mut().zip(manifest.seats.iter()) {
            let Some(facts) = facts else { continue };
            let count = usize::try_from(facts.records()).map_err(|_| ReadError::Malformed)?;
            let mut samples = Vec::with_capacity(count);
            for _ in 0..count {
                samples.push(Sample::read(&mut reader).ok_or(ReadError::Malformed)?);
            }
            *slot = Some(SeatStream {
                clock: facts.clock,
                platform: facts.platform,
                world_units_per_count_e6: facts.world_units_per_count_e6,
                dropped: facts.dropped,
                samples,
            });
        }
        if reader.remaining() != 0 {
            return Err(ReadError::TrailingBytes);
        }

        Ok(Self {
            manifest,
            signature,
            log,
        })
    }
}

/// What a verified companion establishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelemetryVerified {
    /// The match it belongs to, which is the replay's.
    pub match_id: MatchId,
    /// The key that sealed it, which is the key that sealed the replay.
    pub signer: VerifyingKey,
    /// Whether that key has been retired. Reported and not acted on, exactly as
    /// `crate::Verified::retired` is (`docs/RISKS.md` R4).
    pub retired: bool,
    /// Device events across every seat.
    pub samples: u64,
    /// Motions among them.
    pub motions: u64,
}

/// Why a companion is not the companion a replay commits to.
///
/// One variant per check, in the order [`verify`] runs them, and the ordering is
/// what makes a table of tamper cases a table of *answers* rather than one answer
/// repeated. Each catches the attacker who stopped one step short of the next,
/// exactly as `crate::VerifyError` does for the replay — and, exactly as there,
/// the interesting rows need an attacker who **can re-sign**, because every edit
/// is a signature failure otherwise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TelemetryError {
    /// The replay commits to no companion at all, so there is nothing for this
    /// one to be.
    ///
    /// **The first check and the one that matters most**, because absence is a
    /// legitimate state: a replay that records `Absent` is a complete replay of a
    /// match that collected no device telemetry, and attaching a companion to it
    /// afterwards is the purest form of substitution there is.
    NotCommitted,
    /// The companion names an identity this registry does not hold.
    UnknownKey(VerifyingKey),
    /// The signature is not that key's over these bytes.
    Signature,
    /// The body is not the body the companion's own manifest names.
    Stream {
        /// What the manifest claims.
        claimed: Digest,
        /// What the body hashes to.
        computed: Digest,
    },
    /// A seat's records are not the records its own manifest entry counts.
    Counts {
        /// The seat.
        seat: usize,
        /// Device events claimed, and motions among them.
        claimed: (u64, u64),
        /// Device events found, and motions among them.
        found: (u64, u64),
    },
    /// The companion was sealed by another key than the replay.
    Identity {
        /// The key that sealed the replay.
        replay: VerifyingKey,
        /// The key that sealed this companion.
        telemetry: VerifyingKey,
    },
    /// The companion is the telemetry of another match.
    Match {
        /// The replay's match.
        claimed: MatchId,
        /// The companion's.
        found: MatchId,
    },
    /// The companion covers other seats than the replay names participants for.
    Seats {
        /// The seats the replay's manifest names a participant in.
        replay: Vec<usize>,
        /// The seats this companion carries a stream for.
        telemetry: Vec<usize>,
    },
    /// Everything above is consistent and these are still not the bytes the
    /// replay committed to.
    ///
    /// **This is the check the whole commitment exists for.** An attacker holding
    /// a key the registry accepts can seal a second companion for the same match,
    /// covering the same seats, internally consistent in every way — a smoothed
    /// trajectory, a regularised inter-arrival distribution — and nothing in
    /// *that file* is false. What refuses it is that the replay named a different
    /// thirty-two bytes.
    Substituted {
        /// What the replay's manifest commits to.
        claimed: Digest,
        /// What this companion hashes to.
        computed: Digest,
    },
}

impl core::fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotCommitted => write!(
                f,
                "this replay commits to no telemetry companion, so no companion is its"
            ),
            Self::UnknownKey(key) => {
                write!(f, "sealed by {key}, which is not a key I accept")
            }
            Self::Signature => write!(f, "the signature is not that key's over these bytes"),
            Self::Stream { .. } => write!(f, "the stream is not the stream the manifest names"),
            Self::Counts {
                seat,
                claimed,
                found,
            } => write!(
                f,
                "seat {seat} claims {} device event(s) of which {} are motions, and holds \
                 {} of which {} are",
                claimed.0, claimed.1, found.0, found.1
            ),
            Self::Identity { .. } => write!(
                f,
                "the companion and the replay were sealed by different keys"
            ),
            Self::Match { claimed, found } => {
                write!(
                    f,
                    "this is the telemetry of match {found}, not of {claimed}"
                )
            }
            Self::Seats { replay, telemetry } => write!(
                f,
                "the replay names participants in seats {replay:?} and the companion \
                 carries streams for {telemetry:?}"
            ),
            Self::Substituted { .. } => write!(
                f,
                "this is not the companion the replay committed to: a different \
                 companion for the same match is still a different companion"
            ),
        }
    }
}

impl core::error::Error for TelemetryError {}

/// Verifies a companion against the replay that commits to it.
///
/// The order of the checks is the substance; see [`TelemetryError`].
///
/// # Errors
///
/// [`TelemetryError`], one variant per check, in the order they run.
pub fn verify(
    replay: &crate::Replay,
    telemetry: &Telemetry,
    keys: &KeyRegistry,
) -> Result<TelemetryVerified, TelemetryError> {
    // 1. Is there anything for this to be? Absence is a state rather than an
    //    error, which makes attaching a companion to a replay that names none
    //    the substitution attack in its simplest form.
    let Commitment::Sealed(committed) = replay.manifest.telemetry else {
        return Err(TelemetryError::NotCommitted);
    };

    // 2 and 3. Provenance, then the seal over the manifest.
    let identity = telemetry.manifest.server_identity;
    let entry = keys
        .find(identity)
        .ok_or(TelemetryError::UnknownKey(identity))?;
    if !identity.verifies(&signed_bytes(&telemetry.manifest), &telemetry.signature) {
        return Err(TelemetryError::Signature);
    }

    // 4. The body is the body the manifest names.
    let computed = digest_bytes(&telemetry.log.body());
    if computed != telemetry.manifest.stream_digest {
        return Err(TelemetryError::Stream {
            claimed: telemetry.manifest.stream_digest,
            computed,
        });
    }

    // 5. …and each seat holds what its entry counts. The digest above covers the
    //    bytes; this covers what the manifest says about them, which is what a
    //    reader of the corpus actually consults and what `Corpus::store`
    //    cross-checks the session record against.
    for (index, (facts, stream)) in telemetry
        .manifest
        .seats
        .iter()
        .zip(telemetry.log.seats.iter())
        .enumerate()
    {
        let (Some(facts), Some(stream)) = (facts, stream) else {
            continue;
        };
        let found = stream.facts();
        if (found.samples, found.motions) != (facts.samples, facts.motions) {
            return Err(TelemetryError::Counts {
                seat: index,
                claimed: (facts.samples, facts.motions),
                found: (found.samples, found.motions),
            });
        }
    }

    // 6, 7 and 8. The three ways an honestly sealed companion can belong to
    //             something else: another signer, another match, another set of
    //             seats.
    if identity != replay.manifest.server_identity {
        return Err(TelemetryError::Identity {
            replay: replay.manifest.server_identity,
            telemetry: identity,
        });
    }
    if telemetry.manifest.match_id != replay.manifest.match_id {
        return Err(TelemetryError::Match {
            claimed: replay.manifest.match_id,
            found: telemetry.manifest.match_id,
        });
    }
    let named: Vec<usize> = replay
        .manifest
        .participants
        .iter()
        .enumerate()
        .filter(|(_, slot)| slot.is_some())
        .map(|(index, _)| index)
        .collect();
    let covered = telemetry.manifest.occupied();
    if named != covered {
        return Err(TelemetryError::Seats {
            replay: named,
            telemetry: covered,
        });
    }

    // 9. And the bytes are the bytes. Everything above can be made true by an
    //    attacker with a key; this cannot, because the replay said which
    //    thirty-two bytes before the attacker arrived.
    let digest = telemetry.digest();
    if digest != committed {
        return Err(TelemetryError::Substituted {
            claimed: committed,
            computed: digest,
        });
    }

    let (samples, motions) =
        telemetry
            .manifest
            .seats
            .iter()
            .flatten()
            .fold((0u64, 0u64), |(samples, motions), seat| {
                (
                    samples.saturating_add(seat.samples),
                    motions.saturating_add(seat.motions),
                )
            });
    Ok(TelemetryVerified {
        match_id: telemetry.manifest.match_id,
        signer: identity,
        retired: entry.status == KeyStatus::Retired,
        samples,
        motions,
    })
}
