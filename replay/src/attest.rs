//! The gate a match passes through to reach the corpus: **every seat that
//! played was a hand**.
//!
//! # The failure this exists to make unavailable
//!
//! `docs/SCOPE.md` fixes what the corpus can and cannot say about synthetic
//! play. The one mechanical thing a *file* can say is that a seat recorded zero
//! device events, which catches a client that drives the protocol and never
//! touches a device; a bot that moves a real mouse is not reachable from any
//! file, and what closes that is supervision — a fact about a person.
//!
//! Between those two there was a third case and nothing named it. This project
//! ships a **playtest bot** (`moba-bots`, `client::bot`) so that one or two
//! people can play a nine-seat match before nine of them are free. A bot
//! seat speaks the protocol, so the authority records its inputs exactly as it
//! records a person's; it writes no session part, because a session part is
//! written by a capture path and there is no device behind it. So the match it
//! played contains eight ninths synthetic play and, before this module, the only
//! thing standing between it and the corpus was
//! [`crate::corpus::Corpus::store`]'s comparison of the session record against
//! the **manifest's participant list** — which is a list the *operator* writes.
//! An operator who names only the seat a person sat in gets a session record and
//! a manifest that agree perfectly, no silent seat, and a stored match whose
//! circumstances were eight bots.
//!
//! # The shape, which is the one `docs/CONSENT.md` already uses
//!
//! `replay::Publishable` is the only value this workspace writes to a
//! publication directory and `Publishable::of` is its only constructor;
//! `replay::TrainingSet` is the only value that yields matches for training. The
//! rule those two implement — **the check is the only constructor of the value
//! the use needs** — is what is applied here, one level below a purpose:
//! [`Attested`] is the only value [`crate::corpus::Corpus::store`] accepts, and
//! [`Attested::of`] is its only constructor. Filing a match a program played is
//! not a mistake to avoid; it is a value that cannot be built.
//!
//! # What it reads, and why it reads that rather than the participant list
//!
//! **The seats that played come out of the replay's own input log.** A seat that
//! played sent one intention per tick and appears in the log many times; a seat
//! nobody occupied never appears. The log is covered by `input_log_digest`,
//! which is inside the signature, so this is a fact the *authority observed*
//! rather than a claim the operator typed — and that is the whole of why the
//! check is stated over it. The manifest's participant list and the session
//! record are both operator-side, and two operator-side files agreeing with each
//! other is exactly the agreement a bot-filled match already has.
//!
//! Against it stands the session record, seat by seat: a seat that played must
//! be [`SeatRecord::Human`], which is a record that exists only because a
//! client's capture path wrote a part for it (`client::health`), and which
//! `SeatRecord::decode_part` refuses to build from a part claiming any
//! provenance but a person's.
//!
//! # It is deliberately one-directional
//!
//! A seat that played and has no session record is refused. A seat that has a
//! session record and never played is **not** refused here, and the omission is
//! a decision: `Corpus::store` already refuses a session record and a manifest
//! that disagree about which seats are occupied, and a person whose client sent
//! nothing for a whole match is a broken client rather than synthetic play. The
//! failure this module is about has a direction — inputs with no hand behind
//! them — and a check that pointed both ways would refuse the fixtures whose
//! logs are shorter than their rosters for a reason that has nothing to do with
//! it.
//!
//! # What it does not establish
//!
//! The same sentence `docs/SCOPE.md` and `crate::session::Supervision` carry,
//! because a gate invites its reader to conclude more than it does: this refuses
//! a seat with **no** device behind it. A bot that moved a real mouse would
//! write a part like anybody's, and no file separates it from a person. What
//! guarantees a match is human is still supervision, and this is one more thing
//! that cannot happen by accident rather than a defence against somebody trying.

use std::io;

use sim::PLAYER_COUNT;

use crate::session::{SeatRecord, SessionRecord};
use crate::{Replay, Telemetry};

/// Seats the authority recorded inputs from that no person's session record
/// accounts for.
///
/// One variant's worth of information and therefore a struct: what an operator
/// needs is *which* seats, because the answer to this refusal is either "those
/// were the bots, do not file this match" or "a client failed to write its
/// part", and the seat numbers are what tells the two apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unattested {
    /// The seats, in seat order.
    pub seats: Vec<usize>,
}

impl core::fmt::Display for Unattested {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "seat(s) {:?} sent inputs to this match and no session record \
             accounts for them, so something that is not a hand was playing \
             there — a playtest bot, a headless client, or a script. A human \
             corpus contaminated with synthetic play is not a human corpus \
             (docs/SCOPE.md, docs/SCHEMA.md)",
            self.seats
        )
    }
}

impl core::error::Error for Unattested {}

impl From<Unattested> for io::Error {
    fn from(error: Unattested) -> Self {
        Self::new(io::ErrorKind::InvalidInput, error.to_string())
    }
}

/// A match every seat of which was played by a person.
///
/// **The only value [`crate::corpus::Corpus::store`] accepts**, and it has no
/// constructor but [`Attested::of`]. See this module's header for why that is
/// the shape rather than a check inside `store`.
///
/// It borrows rather than owns, which is the one place it differs from
/// [`crate::Publishable`]: that type is built *from* a corpus and has to carry
/// what it read off the disk, and this one is handed the three values its caller
/// already holds on the way in. Cloning a replay and a device stream to pass
/// them one function further would be a copy of the largest object in this
/// crate, made to satisfy a lifetime.
#[derive(Clone, Copy, Debug)]
pub struct Attested<'a> {
    replay: &'a Replay,
    session: &'a SessionRecord,
    telemetry: Option<&'a Telemetry>,
}

impl<'a> Attested<'a> {
    /// This match, if a person's session record accounts for every seat the
    /// replay's input log shows playing.
    ///
    /// # Errors
    ///
    /// [`Unattested`], naming the seats that played with nothing behind them.
    pub fn of(
        replay: &'a Replay,
        session: &'a SessionRecord,
        telemetry: Option<&'a Telemetry>,
    ) -> Result<Self, Unattested> {
        let played = seats_that_played(replay);
        let seats: Vec<usize> = (0..PLAYER_COUNT)
            .filter(|index| {
                played.get(*index).copied().unwrap_or(false)
                    && !matches!(session.seats.get(*index), Some(SeatRecord::Human { .. }))
            })
            .collect();
        if seats.is_empty() {
            Ok(Self {
                replay,
                session,
                telemetry,
            })
        } else {
            Err(Unattested { seats })
        }
    }

    /// The replay.
    #[must_use]
    pub const fn replay(&self) -> &'a Replay {
        self.replay
    }

    /// The session record beside it.
    #[must_use]
    pub const fn session(&self) -> &'a SessionRecord {
        self.session
    }

    /// The telemetry companion, where the match produced one.
    #[must_use]
    pub const fn telemetry(&self) -> Option<&'a Telemetry> {
        self.telemetry
    }
}

/// Which seats the authority recorded an input from.
///
/// Read out of the log rather than out of the manifest, which is the whole
/// point: the log is what the server observed and the manifest's participant
/// list is what an operator wrote down. A client sends one intention per tick,
/// so a seat that played appears many times over and a seat nobody sat in never
/// appears at all.
fn seats_that_played(replay: &Replay) -> [bool; PLAYER_COUNT] {
    let mut played = [false; PLAYER_COUNT];
    for timed in &replay.inputs {
        if let Some(slot) = played.get_mut(timed.input.player.index()) {
            *slot = true;
        }
    }
    played
}
