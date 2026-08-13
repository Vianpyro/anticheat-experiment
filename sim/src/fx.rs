//! The fixed-point scalar, and the only arithmetic the rules are allowed to do.
//!
//! # Representation
//!
//! [`Fx`] is a signed 32-bit integer read as a multiple of `2^-16` — Q15.16. It
//! represents values in `[-32768, 32767.99998]` with a uniform resolution of
//! `1 / 65536 ≈ 0.0000153` world units. The choice is frozen by `docs/RISKS.md`
//! R2: the fractional width is baked into every replay and every recorded human
//! match, so changing it invalidates the corpus rather than the code. It is one
//! of the fields covered by [`crate::rules_hash`], which is what turns a change
//! here into a loud verification failure instead of a silent resimulation into
//! garbage.
//!
//! # Overflow semantics
//!
//! **Every operation on `Fx` saturates.** Not wrapping, not panicking:
//!
//! - *Not wrapping*, because wrapping is what release builds do to `i32` by
//!   default, and it is the failure mode this type exists to remove. A position
//!   that wraps teleports across the map; a position that saturates stops at
//!   the edge. Only one of those is debuggable.
//! - *Not panicking*, because `step` runs inside an authoritative server for
//!   nine players and a panic is a match everyone loses. A total function has no
//!   failure path to get wrong.
//! - Saturation is nevertheless still a silent change of value, so it is not
//!   trusted as a design: the *legal domain* below is stated, and the property
//!   tests assert that no operation saturates anywhere inside it. Saturation is
//!   a floor under the failure, not a licence to reach it.
//!
//! [`Fx::checked_add`] and its siblings expose the same operations with the
//! saturation made visible, and are what the property tests actually assert on:
//! "in-domain" is defined as "`checked_*` returns `Some`".
//!
//! Division by zero is defined rather than trapped: it yields [`Fx::MAX`] or
//! [`Fx::MIN`] following the sign of the numerator, and [`Fx::ZERO`] for
//! `0 / 0`. [`Fx::checked_div`] returns `None` for it.
//!
//! # Rounding
//!
//! Multiplication and division truncate **toward zero**, not to nearest. It is
//! worth saying out loud because "round to nearest" is what a reader assumes
//! and then spends an afternoon disproving.
//!
//! Toward zero rather than toward negative infinity — which an arithmetic shift
//! would have given for free — so that the type is symmetric about the origin:
//! `(-a) * b == -(a * b)` holds, and the property tests assert it. On a map
//! whose origin is the middle of the map, a rounding rule that drifts one
//! direction is a rounding rule that treats one team differently from the
//! others.

use core::fmt;
use core::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

/// Number of fractional bits in [`Fx`]. Frozen by `docs/RISKS.md` R2.
pub const FRAC_BITS: u32 = 16;

/// Raw representation of `1.0`.
const ONE_RAW: i32 = 1 << FRAC_BITS;
const ONE_RAW_I64: i64 = ONE_RAW as i64;
const FRAC_MASK: u32 = ONE_RAW as u32 - 1;

/// A fixed-point scalar in Q15.16. See the module documentation for the
/// overflow and rounding semantics, which are part of the type's contract and
/// not an implementation detail.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Fx(i32);

/// `Fx` holds exactly one `i32`, and nothing else.
///
/// This is the one hole in the digest's compile-time guarantee, closed. Every
/// other type reaching [`crate::State::digest`] is destructured exhaustively in
/// `crate::canonical`, so a field added and not hashed stops the build. `Fx`
/// cannot be: its field is private to this module, so `canonical` encodes it
/// through [`Fx::to_raw`] instead, and `to_raw` is a canonical encoding only as
/// long as there is nothing else in the type to encode. A second field would
/// have been hashed by nobody and noticed by nothing — the determinism suite
/// would have stayed green over a digest that no longer covered the whole
/// value, which is the exact failure mode `crate::canonical` exists to make
/// impossible.
///
/// The assertion is on the size rather than on the field count, because the
/// field count is not something a `const` expression can ask about. That leaves
/// one residual gap, stated rather than papered over: **a zero-sized second
/// field would pass this check.** A `PhantomData`, a `()`, or a unit struct
/// changes no size and would slip through. It is tolerable for the reason that
/// makes it possible: a zero-sized field holds no value, so there is nothing in
/// it that the digest could fail to cover. Anything carrying an actual bit —
/// a flag, a second scalar, a wider representation — changes the size and stops
/// the build here, with this comment as the explanation.
const _: () = assert!(
    size_of::<Fx>() == size_of::<i32>(),
    "Fx must hold exactly one i32: `to_raw` is its canonical encoding for the \
     digest, and a second field would be silently left out of it"
);

impl Fx {
    /// `0.0`.
    pub const ZERO: Self = Self(0);
    /// `1.0`.
    pub const ONE: Self = Self(ONE_RAW);
    /// `-1.0`.
    pub const NEG_ONE: Self = Self(-ONE_RAW);
    /// The smallest representable value, `-32768.0`.
    pub const MIN: Self = Self(i32::MIN);
    /// The largest representable value, just under `32768.0`.
    pub const MAX: Self = Self(i32::MAX);
    /// The distance between two adjacent representable values, `2^-16`.
    pub const EPSILON: Self = Self(1);

    /// Reads a raw Q15.16 integer as a scalar. The inverse of [`Fx::to_raw`].
    #[must_use]
    pub const fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    /// The underlying Q15.16 integer. This is the canonical encoding of an
    /// `Fx`, and what the digest hashes.
    #[must_use]
    pub const fn to_raw(self) -> i32 {
        self.0
    }

    /// Converts a whole number, saturating outside the representable range.
    #[must_use]
    pub const fn from_int(value: i32) -> Self {
        Self(value.saturating_mul(ONE_RAW))
    }

    /// Converts `num / den`, saturating. Usable in a `const` item, which is how
    /// the balance constants in [`crate::Rules`] are written: a speed stays
    /// readable as "6 units per second divided by the tick rate" instead of
    /// arriving as an opaque raw integer.
    ///
    /// `den == 0` follows [`Fx::div`].
    #[must_use]
    pub const fn from_ratio(num: i32, den: i32) -> Self {
        Self::from_int(num).div(Self::from_int(den))
    }

    /// The greatest whole number less than or equal to `self`.
    #[must_use]
    pub const fn floor_to_int(self) -> i32 {
        self.0 >> FRAC_BITS
    }

    /// Saturating addition.
    #[must_use]
    pub const fn add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }

    /// Saturating subtraction.
    #[must_use]
    pub const fn sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }

    /// Saturating multiplication, truncating toward zero.
    #[must_use]
    pub const fn mul(self, rhs: Self) -> Self {
        // Exact: two i32 factors need at most 62 bits.
        let product = (self.0 as i64).saturating_mul(rhs.0 as i64);
        Self(clamp_to_i32(scale_down(product)))
    }

    /// Saturating division, truncating toward zero. Division by zero yields
    /// [`Fx::MAX`], [`Fx::MIN`] or [`Fx::ZERO`] following the sign of the
    /// numerator, rather than panicking.
    #[must_use]
    pub const fn div(self, rhs: Self) -> Self {
        match self.checked_div(rhs) {
            Some(value) => value,
            // Either the divisor is zero or the quotient left the range; in
            // both cases the sign of the numerator picks the saturation end.
            // `0 / 0` is zero, which is the only value that is not a lie.
            None if rhs.0 == 0 && self.0 == 0 => Self::ZERO,
            None if (self.0 < 0) != (rhs.0 < 0) => Self::MIN,
            None => Self::MAX,
        }
    }

    /// Saturating negation. `-Fx::MIN` is [`Fx::MAX`], not [`Fx::MIN`].
    #[must_use]
    pub const fn neg(self) -> Self {
        Self(self.0.saturating_neg())
    }

    /// Saturating absolute value.
    #[must_use]
    pub const fn abs(self) -> Self {
        Self(self.0.saturating_abs())
    }

    /// `-1`, `0` or `1` as an `Fx`, following the sign of `self`.
    #[must_use]
    pub const fn signum(self) -> Self {
        if self.0 > 0 {
            Self::ONE
        } else if self.0 < 0 {
            Self::NEG_ONE
        } else {
            Self::ZERO
        }
    }

    /// Square root, truncating. Negative inputs yield [`Fx::ZERO`]: the rules
    /// never take the root of a negative quantity, and returning zero keeps the
    /// function total rather than adding an error path nothing handles.
    #[must_use]
    pub const fn sqrt(self) -> Self {
        if self.0 <= 0 {
            return Self::ZERO;
        }
        // sqrt(v) in Q15.16 is isqrt(raw << 16): raw is at most 2^31, so the
        // widened value is at most 2^47 and the root at most 2^23.5.
        let widened = (self.0 as u64).saturating_mul(ONE_RAW as u64);
        Self(clamp_to_i32(widened.isqrt() as i64))
    }

    /// Addition, or `None` on overflow.
    #[must_use]
    pub const fn checked_add(self, rhs: Self) -> Option<Self> {
        match self.0.checked_add(rhs.0) {
            Some(raw) => Some(Self(raw)),
            None => None,
        }
    }

    /// Subtraction, or `None` on overflow.
    #[must_use]
    pub const fn checked_sub(self, rhs: Self) -> Option<Self> {
        match self.0.checked_sub(rhs.0) {
            Some(raw) => Some(Self(raw)),
            None => None,
        }
    }

    /// Multiplication, or `None` if the product leaves the representable range.
    #[must_use]
    pub const fn checked_mul(self, rhs: Self) -> Option<Self> {
        let product = (self.0 as i64).saturating_mul(rhs.0 as i64);
        fits_in_i32(scale_down(product))
    }

    /// Division, or `None` if the divisor is zero or the quotient leaves the
    /// representable range.
    #[must_use]
    pub const fn checked_div(self, rhs: Self) -> Option<Self> {
        // Exact: a 32-bit numerator scaled by 2^16 needs at most 48 bits.
        let numerator = (self.0 as i64).saturating_mul(ONE_RAW_I64);
        match numerator.checked_div(rhs.0 as i64) {
            Some(quotient) => fits_in_i32(quotient),
            None => None,
        }
    }
}

/// Reduces a Q31.32 product back to Q15.16, truncating toward zero.
///
/// An arithmetic shift would be one instruction and would truncate toward
/// negative infinity instead; see the module documentation for why the
/// symmetric rounding is worth the division, which the compiler turns back into
/// a shift and a correction anyway.
const fn scale_down(product: i64) -> i64 {
    match product.checked_div(ONE_RAW_I64) {
        Some(value) => value,
        // The divisor is a non-zero constant and the dividend is at most 62
        // bits, so neither failure mode of `checked_div` is reachable.
        None => 0,
    }
}

/// `Some` if the value fits a Q15.16 scalar without saturating.
const fn fits_in_i32(value: i64) -> Option<Fx> {
    if value < i32::MIN as i64 || value > i32::MAX as i64 {
        None
    } else {
        Some(Fx(value as i32))
    }
}

/// Clamps a widened intermediate back into the representable range.
const fn clamp_to_i32(value: i64) -> i32 {
    if value < i32::MIN as i64 {
        i32::MIN
    } else if value > i32::MAX as i64 {
        i32::MAX
    } else {
        value as i32
    }
}

impl Add for Fx {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Fx::add(self, rhs)
    }
}

impl Sub for Fx {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Fx::sub(self, rhs)
    }
}

impl Mul for Fx {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Fx::mul(self, rhs)
    }
}

impl Div for Fx {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        Fx::div(self, rhs)
    }
}

impl Neg for Fx {
    type Output = Self;
    fn neg(self) -> Self {
        Fx::neg(self)
    }
}

impl AddAssign for Fx {
    fn add_assign(&mut self, rhs: Self) {
        *self = Fx::add(*self, rhs);
    }
}

impl SubAssign for Fx {
    fn sub_assign(&mut self, rhs: Self) {
        *self = Fx::sub(*self, rhs);
    }
}

/// Prints the value in decimal rather than as a raw integer, because a test
/// failure reading `Fx(13107)` costs the reader a division. Five fractional
/// digits is the resolution of the type.
impl fmt::Debug for Fx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let magnitude = self.0.unsigned_abs();
        let whole = magnitude >> FRAC_BITS;
        let fraction = u64::from(magnitude & FRAC_MASK).saturating_mul(100_000) >> FRAC_BITS;
        let sign = if self.0 < 0 { "-" } else { "" };
        write!(f, "{sign}{whole}.{fraction:05}")
    }
}

impl fmt::Display for Fx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}
