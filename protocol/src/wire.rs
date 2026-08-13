//! Fixed-width big-endian primitives, and the encoding of the one `sim` type
//! that travels in both directions.
//!
//! The same shape as `sim`'s own canonical encoding and for the same reasons:
//! every value's width is a function of its type, one tag byte precedes every
//! enum, and nothing is self-describing because nothing needs to be. What is
//! different here is that these bytes arrive from an attacker, so every reader
//! is total — it returns `None` rather than panicking, on any input, including
//! a truncated one.

use sim::{Action, EntityId, Fx, FxVec2};

/// Appends fixed-width big-endian bytes.
pub(crate) struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(capacity),
        }
    }

    pub(crate) fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub(crate) fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    pub(crate) fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    pub(crate) fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    pub(crate) fn i32(&mut self, value: i32) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    pub(crate) fn bytes(&mut self, value: &[u8]) {
        self.bytes.extend_from_slice(value);
    }

    pub(crate) fn fx(&mut self, value: Fx) {
        self.i32(value.to_raw());
    }

    pub(crate) fn point(&mut self, value: FxVec2) {
        self.fx(value.x);
        self.fx(value.y);
    }

    pub(crate) fn entity(&mut self, value: EntityId) {
        self.u16(value.0);
    }

    /// One tag byte, then the variant's payload.
    ///
    /// Variants are not padded to a common width here, and they do not need to
    /// be: the *frame* is padded to a constant size, so the difference between a
    /// three-byte `Targeted` and a nine-byte `Move` is absorbed by the zeroes
    /// after it rather than being visible as a length. Padding each variant as
    /// well would be the same property claimed twice.
    pub(crate) fn action(&mut self, action: Action) {
        match action {
            Action::Idle => self.u8(0),
            Action::Move(destination) => {
                self.u8(1);
                self.point(destination);
            }
            Action::Skillshot(direction) => {
                self.u8(2);
                self.point(direction);
            }
            Action::Targeted(target) => {
                self.u8(3);
                self.entity(target);
            }
            Action::Attack(target) => {
                self.u8(4);
                self.entity(target);
            }
        }
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

/// Reads fixed-width big-endian bytes, and never reads past the end.
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub(crate) const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    /// How many bytes have been consumed. The caller checks that everything
    /// after this point is padding.
    pub(crate) const fn consumed(&self) -> usize {
        self.at
    }

    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(count)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }

    pub(crate) fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }

    pub(crate) fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }

    pub(crate) fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }

    pub(crate) fn u64(&mut self) -> Option<u64> {
        Some(u64::from_be_bytes(self.take(8)?.try_into().ok()?))
    }

    pub(crate) fn i32(&mut self) -> Option<i32> {
        Some(i32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    pub(crate) fn fx(&mut self) -> Option<Fx> {
        Some(Fx::from_raw(self.i32()?))
    }

    pub(crate) fn point(&mut self) -> Option<FxVec2> {
        Some(FxVec2::new(self.fx()?, self.fx()?))
    }

    pub(crate) fn entity(&mut self) -> Option<EntityId> {
        Some(EntityId(self.u16()?))
    }

    /// An action, or `None` for a tag that names no variant.
    ///
    /// Nothing else here is validated, deliberately. An `Action` can name a
    /// handle that does not exist, aim a skillshot at nothing, or ask to walk a
    /// thousand units off the map, and `sim`'s rules absorb every one of those:
    /// out-of-range coordinates clamp, unresolvable targets make the order a
    /// no-op. Rejecting them at the decoder would move the problem rather than
    /// solve it and would add error paths for conditions the rules already have
    /// a total answer for.
    pub(crate) fn action(&mut self) -> Option<Action> {
        match self.u8()? {
            0 => Some(Action::Idle),
            1 => Some(Action::Move(self.point()?)),
            2 => Some(Action::Skillshot(self.point()?)),
            3 => Some(Action::Targeted(self.entity()?)),
            4 => Some(Action::Attack(self.entity()?)),
            _ => None,
        }
    }
}
