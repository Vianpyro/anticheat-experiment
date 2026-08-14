//! Exploit class 1, the half that does not read a single byte of payload.
//!
//! `docs/MILESTONES.md` M7: *recover the number of nearby entities from message
//! sizes and arrival times*. This is the attacker that does it, and it is the one
//! that decides whether the padding budget in `docs/ARCHITECTURE.md` bought
//! anything — culling is worth nothing if the length and the count of the
//! messages report what was culled.
//!
//! # What the attacker is
//!
//! Somebody watching the packets. Not a player, not a session: an observer with
//! a capture of the datagrams going to one seat, who cannot decrypt any of them.
//! QUIC has already denied them the contents; what is left is the shape, and the
//! shape is three numbers per tick — **how many datagrams, how large, and how
//! long since the last one**.
//!
//! # The model, which is the published format and not a fit
//!
//! A view encodes as a fixed part, then a fixed width per entity, per projectile
//! and per event. Those widths are in `docs/ARCHITECTURE.md`'s padding table, so
//! the attacker does not have to learn them from traffic: it inverts the
//! arithmetic. [`Wiretap::estimate_entities`] is that inversion, and on a tick
//! with nothing in flight and nothing happening it is not an estimate at all —
//! it is exact. `tests/traffic.rs` asserts exactly that against an unpadded
//! transport, which is what establishes that the attacker works before it is
//! pointed at the one this project ships.
//!
//! # Timing, in ticks rather than in milliseconds
//!
//! The cadence half of the traffic-shape invariant is about *when* messages
//! arrive, and a test that measured wall-clock arrival on a shared CI runner
//! would be measuring the runner (`docs/RISKS.md` R16 has the general form of
//! that mistake). So the attacker measures the gap in **server ticks between one
//! arrival and the next**, which is the same channel with the scheduler taken out
//! of it: a server that sends a frame only when something changed produces gaps
//! that a stopwatch would have read as silences, and a server that sends one
//! frame per tick whatever happened produces a gap of one, forever.
//!
//! What that does not cover is jitter — a real network's inter-arrival times
//! carry noise this cannot see. It is the honest half of the channel to assert
//! on, and the half a padded, constant-cadence sender is answerable for.

use std::collections::BTreeSet;

/// The parts of a view encoding that are always present: the tick, the outcome,
/// the recipient's own champion, and the two list lengths.
///
/// From `docs/ARCHITECTURE.md`'s padding table. The attacker reads the format
/// rather than learning it, because the format is published — that is what makes
/// this exploit cheap and what makes a defence that relies on the numbers being
/// secret no defence at all.
pub const VIEW_FIXED_BYTES: usize = 35;

/// Bytes per champion or tower in a view.
pub const ENTITY_BYTES: usize = 15;

/// Bytes per projectile.
pub const PROJECTILE_BYTES: usize = 19;

/// Bytes per event.
pub const EVENT_BYTES: usize = 15;

/// The frame header and the input acknowledgement in front of every view.
pub const FRAME_OVERHEAD_BYTES: usize = protocol::HEADER_BYTES + protocol::APPLIED_BYTES;

/// What one tick looked like from outside.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Footprint {
    /// Datagrams that arrived for this recipient.
    pub datagrams: usize,
    /// Their total size, headers included.
    pub bytes: usize,
    /// The largest of them.
    pub largest: usize,
    /// Server ticks since the previous arrival. One, for a sender that speaks
    /// every tick whatever happened.
    pub gap: u32,
}

/// An observer with a packet capture and no key.
#[derive(Clone, Debug, Default)]
pub struct Wiretap {
    footprints: Vec<Footprint>,
    /// Bytes of transport header the attacker subtracts before inverting the
    /// view encoding. Known, because the shard header is part of the published
    /// format.
    shard_overhead: usize,
    last: Option<u32>,
}

impl Wiretap {
    /// An observer that has seen nothing, watching a sender whose datagrams
    /// carry `shard_overhead` bytes of transport header each.
    #[must_use]
    pub const fn new(shard_overhead: usize) -> Self {
        Self {
            footprints: Vec::new(),
            shard_overhead,
            last: None,
        }
    }

    /// Records what arrived on server tick `at`. An empty slice is a tick on
    /// which nothing arrived, which is itself an observation and is why this is
    /// called for every tick rather than only for the ones with traffic.
    pub fn saw(&mut self, at: u32, sizes: &[usize]) {
        if sizes.is_empty() {
            return;
        }
        let gap = self.last.map_or(1, |previous| at.saturating_sub(previous));
        self.last = Some(at);
        self.footprints.push(Footprint {
            datagrams: sizes.len(),
            bytes: sizes.iter().sum(),
            largest: sizes.iter().copied().max().unwrap_or(0),
            gap,
        });
    }

    /// Everything the attacker saw, in order.
    #[must_use]
    pub fn footprints(&self) -> &[Footprint] {
        &self.footprints
    }

    /// How many distinct shapes the capture contains.
    ///
    /// **This is the leak, as one number.** One means every tick looked the same
    /// from outside, so nothing an observer measured can separate any two of
    /// them; more than one means the capture partitions the match's ticks into
    /// classes an observer was not given.
    #[must_use]
    pub fn distinct_footprints(&self) -> usize {
        self.footprints
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len()
    }

    /// The attacker's reading of how many entities were visible on each arrival.
    ///
    /// Exact on a tick with no projectile in flight and no event fired, because
    /// then the encoding is a fixed part plus a constant per entity and the
    /// arithmetic inverts. On a busier tick it is an over-estimate by the events,
    /// which is a bias the attacker knows the sign of and can still act on: an
    /// over-estimate that rises when a fight starts is a fight starting.
    ///
    /// `None` for an arrival too small to be a view at all, which is what a
    /// padded sender's frames are not and an unpadded sender's never are either.
    #[must_use]
    pub fn estimate_entities(&self) -> Vec<Option<usize>> {
        self.footprints
            .iter()
            .map(|seen| {
                let overhead = self
                    .shard_overhead
                    .saturating_mul(seen.datagrams)
                    .saturating_add(FRAME_OVERHEAD_BYTES)
                    .saturating_add(VIEW_FIXED_BYTES);
                let payload = seen.bytes.checked_sub(overhead)?;
                Some(payload / ENTITY_BYTES)
            })
            .collect()
    }
}
