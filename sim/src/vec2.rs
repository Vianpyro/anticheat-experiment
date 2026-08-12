//! Two-dimensional fixed-point vectors.
//!
//! The one thing worth reading here is that length and distance are computed
//! over `i64` and never materialise an intermediate [`Fx`]. A squared distance
//! leaves the Q15.16 range long before the distance itself does — two points at
//! opposite corners of the map are 256 units apart, and `256² = 65536` is
//! already outside the type — so an implementation that squares into `Fx` would
//! saturate on perfectly ordinary geometry and report every far-apart pair as
//! equally far apart. Widening first makes the operation exact over the whole
//! legal coordinate domain instead.

use crate::fx::Fx;

/// A point or a direction, in world units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct FxVec2 {
    /// Horizontal component. The lane runs along this axis.
    pub x: Fx,
    /// Vertical component.
    pub y: Fx,
}

impl FxVec2 {
    /// The origin, and the zero direction.
    pub const ZERO: Self = Self {
        x: Fx::ZERO,
        y: Fx::ZERO,
    };

    /// Builds a vector from two scalars.
    #[must_use]
    pub const fn new(x: Fx, y: Fx) -> Self {
        Self { x, y }
    }

    /// Component-wise saturating addition.
    #[must_use]
    pub const fn add(self, rhs: Self) -> Self {
        Self::new(self.x.add(rhs.x), self.y.add(rhs.y))
    }

    /// Component-wise saturating subtraction.
    #[must_use]
    pub const fn sub(self, rhs: Self) -> Self {
        Self::new(self.x.sub(rhs.x), self.y.sub(rhs.y))
    }

    /// Component-wise saturating negation.
    #[must_use]
    pub const fn neg(self) -> Self {
        Self::new(self.x.neg(), self.y.neg())
    }

    /// Scales both components by a scalar, saturating.
    #[must_use]
    pub const fn scale(self, factor: Fx) -> Self {
        Self::new(self.x.mul(factor), self.y.mul(factor))
    }

    /// Clamps both components into `[-limit, limit]`. This is how an
    /// attacker-supplied coordinate enters the simulation: the rules never
    /// reject a position for being out of range, they pull it back to the edge
    /// of the map, so there is no input that produces an error path.
    #[must_use]
    pub const fn clamp_components(self, limit: Fx) -> Self {
        Self::new(clamp_scalar(self.x, limit), clamp_scalar(self.y, limit))
    }

    /// The squared length, in Q31.32, as an exact widened integer.
    ///
    /// The unusual scale is deliberate: `x*x + y*y` where both operands are
    /// Q15.16 raw integers is naturally Q31.32, and keeping it there rather
    /// than shifting back loses nothing and lets [`FxVec2::length`] recover the
    /// Q15.16 result with a single integer square root.
    #[must_use]
    pub const fn length_squared_wide(self) -> i64 {
        let x = self.x.to_raw() as i64;
        let y = self.y.to_raw() as i64;
        x.saturating_mul(x).saturating_add(y.saturating_mul(y))
    }

    /// The length, truncating.
    ///
    /// `isqrt` of a Q31.32 value is directly the Q15.16 root, because
    /// `sqrt(v · 2^32) = sqrt(v) · 2^16`.
    #[must_use]
    pub const fn length(self) -> Fx {
        let squared = self.length_squared_wide();
        if squared <= 0 {
            return Fx::ZERO;
        }
        let root = (squared as u64).isqrt();
        if root > i32::MAX as u64 {
            Fx::MAX
        } else {
            Fx::from_raw(root as i32)
        }
    }

    /// The distance between two points, truncating.
    #[must_use]
    pub const fn distance(self, other: Self) -> Fx {
        self.sub(other).length()
    }

    /// Whether `other` lies within `radius` of `self`.
    ///
    /// Compares squared lengths over `i64`, so the answer is exact: it does not
    /// inherit the truncation of [`FxVec2::length`], and an entity exactly on
    /// the range boundary is inside it rather than one raw unit outside
    /// depending on which way the root rounded. Every range check in the rules
    /// goes through here for that reason.
    #[must_use]
    pub const fn within_range(self, other: Self, radius: Fx) -> bool {
        if radius.to_raw() < 0 {
            return false;
        }
        let radius_raw = radius.to_raw() as i64;
        self.sub(other).length_squared_wide() <= radius_raw.saturating_mul(radius_raw)
    }

    /// A unit vector in the same direction, or [`FxVec2::ZERO`] if the length
    /// is zero.
    ///
    /// # Accuracy
    ///
    /// The result is only guaranteed close to unit length when the input is at
    /// least [`crate::RULES`]'s `min_direction_length` long. Below that the integer
    /// square root has too few significant bits left: `(2^-16, 2^-16)` has a
    /// computed length of one raw unit and normalises to `(1.0, 1.0)`, which is
    /// 41% too long. The rules therefore reject a direction shorter than that
    /// bound rather than normalising it, and the property test asserts the
    /// accuracy only above it. This is a real restriction on the type, not a
    /// test convenience.
    #[must_use]
    pub const fn normalize_or_zero(self) -> Self {
        let length = self.length();
        if length.to_raw() == 0 {
            return Self::ZERO;
        }
        Self::new(self.x.div(length), self.y.div(length))
    }

    /// Moves `step` units from `self` toward `target`, stopping exactly on it
    /// if it is within reach.
    ///
    /// Landing exactly on the target matters for more than tidiness: without
    /// the snap, a champion that has arrived keeps taking sub-unit steps back
    /// and forth across its destination forever, and every one of those ticks
    /// writes a different position into the digest.
    #[must_use]
    pub const fn step_toward(self, target: Self, step: Fx) -> Self {
        let delta = target.sub(self);
        if delta.length().to_raw() <= step.to_raw() {
            return target;
        }
        self.add(delta.normalize_or_zero().scale(step))
    }
}

/// `Ord::clamp` is not a `const fn`, and these are used from `const` items.
const fn clamp_scalar(value: Fx, limit: Fx) -> Fx {
    if value.to_raw() > limit.to_raw() {
        limit
    } else if value.to_raw() < limit.neg().to_raw() {
        limit.neg()
    } else {
        value
    }
}
