//! The lobby: the wait for the other players, laid out so that crossing it
//! measures the device.
//!
//! # What this is for, and the confound it exists to attack
//!
//! The corpus is nine people on nine mice, which `docs/SCOPE.md` fixes and
//! `docs/RISKS.md` R17 prices: **each hand appears with exactly one device, so
//! no analysis can separate a person's style from their hardware's response.**
//! That is not variance more data absorbs, it is a variable nothing in the
//! design identifies. The answer is not to standardise the hardware — a
//! production anti-cheat does not choose its players' mice — it is to *measure*
//! the hardware's contribution, so that a detector reading a distance or a speed
//! works in normalised units rather than in raw device counts.
//!
//! # There is no calibration screen, and that is a geometry decision
//!
//! Nothing here asks the player to do anything. The measurement hides in a dead
//! interval that already exists — waiting for eight other people — exactly as a
//! game hides a load behind a scene, and the lever is the **layout**:
//!
//! - the ready button is at the opposite corner from champion select;
//! - the pseudonym check and the consent confirmation sit at fixed positions
//!   far from both, and [`Lobby::ready`] is inert until all three are visited,
//!   so the traversal is forced by the interface rather than requested of the
//!   player;
//! - a training dummy stands at a **known distance** and moves to the next
//!   station in a fixed table each time it is hit, which is what fills the wait
//!   and what sweeps the directions and distances a single menu traversal
//!   cannot.
//!
//! Every element's position is a constant of this file, so a click on one is a
//! movement whose *endpoints* are known exactly and whose *cost in device
//! counts* is measured. That pair is the whole measurement.
//!
//! # The cursor is the game's cursor, and this is the constraint that binds
//!
//! The lobby is driven by [`crate::input::Aim`] — the same integrator over raw
//! device deltas that aims a skillshot, in the same world units, clamped to the
//! same rule constant. It is **never** driven by the operating system's pointer.
//!
//! That is not tidiness. A menu that reacted to the OS pointer would be
//! measuring the *accelerated* pointer, which is the quantity
//! `docs/SCHEMA.md` §4d refuses everywhere else in this client, and the scale
//! recovered from it would not be the scale the match is played at — so the
//! number would be worse than no number, because it would have the shape of a
//! calibration. `client/tests/lobby.rs` is where that dependency is forbidden:
//! the same device events under two windows six times apart in pixels per world
//! unit produce a byte-identical trace, an identical cursor and identical
//! reaches, which is `docs/ARCHITECTURE.md` invariant 12 restated over the menu.
//!
//! # What is extracted, and what is deliberately not
//!
//! A [`Reach`] is one click on an element of known position: a known distance, a
//! measured cost in device counts, a duration, a sample count and a direction.
//! From a session's reaches this type keeps the **sufficient statistics of a
//! linear regression** rather than its answer — `n`, `Σd`, `Σn`, `Σd²`, `Σdn` —
//! so that two sessions of one participant pool exactly by addition and the
//! estimate is computed once, by `replay::calibration`, on the trusted side.
//! That is the whole of what makes estimation accumulate.
//!
//! **What this does not measure is `device_cpi`, and saying so is part of the
//! deliverable.** A mouse reports counts; nothing in any stream this project
//! records says what physical distance produced them, and no geometry in a menu
//! changes that. `docs/SCHEMA.md` §4c keeps the true CPI in the unknown column
//! where it was. What the regression recovers is the map from **recorded device
//! counts to world units** — the conversion a distance-shaped statistic needs in
//! order to stop being a count — measured against geometry the build fixes
//! rather than taken from a number the client wrote about itself.

use sim::{FxVec2, RULES};

use crate::draw::{Mark, colour};

/// An element of the lobby, at a position this file fixes.
///
/// Five, and the absence of a sixth is the design: every one of them is
/// something a player has to touch before a match can start, so the traversal
/// that measures is the traversal that plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Element {
    /// The pseudonym the operator enrolled, shown for the participant to
    /// confirm. Top left.
    Name,
    /// The consent version this session is recorded under, confirmed here.
    /// Top right, the width of the lobby away from [`Element::Name`].
    Consent,
    /// Champion select. Bottom left, and deliberately the far corner from
    /// [`Element::Ready`].
    Champion,
    /// The training dummy, which moves to the next station in
    /// [`DUMMY_STATIONS`] each time it is hit.
    Dummy,
    /// Ready. Bottom right, inert until the other three have been visited.
    Ready,
}

/// Where the four fixed elements are, in world units.
///
/// The same coordinate frame the aim lives in, so a distance here and a distance
/// in the match are the same quantity — which is what makes the scale recovered
/// in the lobby the scale the match is played at. They are spread to the corners
/// of the drawn window rather than packed, because the measurement's leverage is
/// the *spread* of the distances and a compact menu produces one distance
/// repeated.
const fn station(element: Element) -> Option<FxVec2> {
    match element {
        Element::Name => Some(FxVec2::new(fx(-90.0), fx(95.0))),
        Element::Consent => Some(FxVec2::new(fx(90.0), fx(95.0))),
        Element::Champion => Some(FxVec2::new(fx(-90.0), fx(-60.0))),
        Element::Ready => Some(FxVec2::new(fx(90.0), fx(-60.0))),
        // The dummy moves; see `Lobby::dummy_at`.
        Element::Dummy => None,
    }
}

/// Where the training dummy stands, in order, one station per hit.
///
/// A **constant table** rather than a draw, for three reasons that are each a
/// property this measurement needs:
///
/// - the position is known geometry at every moment, which is what a reach's
///   distance is computed from;
/// - the sequence is a property of the build rather than of a seed, so the
///   direction coverage and the distance spread it reaches are assertable —
///   `docs/RISKS.md` R15's rule that a fixture states what it actually reaches,
///   applied to an interface;
/// - it sweeps **all eight octants** and a distance ratio above four, which is
///   what one traversal of a static menu cannot do and what makes a session
///   accumulate towards sufficiency instead of repeating itself.
///
/// The short hops are as deliberate as the long ones: `docs/SCHEMA.md` §4d
/// distinguishes distance-shaped statistics from shape-shaped ones, and the
/// regression separates a distance-proportional term from a fixed per-target
/// cost only if the distances actually differ.
pub const DUMMY_STATIONS: [(f64, f64); 12] = [
    (-40.0, 50.0),
    (5.0, -15.0),
    (15.0, 70.0),
    (0.0, 65.0),
    (-5.0, -75.0),
    (10.0, 40.0),
    (25.0, 35.0),
    (-100.0, 35.0),
    (-30.0, -40.0),
    (-40.0, -30.0),
    (90.0, 50.0),
    (-65.0, -80.0),
];

/// How near an element's centre a click has to be to land on it, in world units.
///
/// Two radii rather than one: the menu buttons are generous because nobody is
/// being asked to be accurate, and the dummy is small because a target that
/// needs aiming is a target a hand sweeps to at speed — which is the condition
/// [`FAST_UNITS_PER_SECOND`] selects on and the only condition under which the
/// device's report rate is readable at all.
pub const BUTTON_RADIUS: f64 = 11.0;
/// The dummy's radius. See [`BUTTON_RADIUS`].
pub const DUMMY_RADIUS: f64 = 6.0;

/// Below this, two elements are too close together for the click on the second
/// to be a movement rather than a twitch, and the reach is not recorded.
///
/// A reach shorter than this carries no leverage for the regression and its
/// distance is comparable with the radii, so its cost is mostly the landing slop
/// rather than the crossing.
pub const MIN_REACH_UNITS: f64 = 8.0;

/// A reach at or above this mean speed is **fast**, in world units per second.
///
/// The threshold exists for one measurement and one only: the device's report
/// rate. Samples per second over a whole session under-reports it, because a
/// hand at rest reports nothing and the client records one sample per device
/// event rather than one per interval (`crate::input`). Over a reach the hand
/// crossed at speed, the hand was moving for the whole interval, so samples over
/// duration *is* the report rate — measured, against `device_polling_hz`, which
/// `docs/SCHEMA.md` §4a can only ever record as a declaration.
pub const FAST_UNITS_PER_SECOND: f64 = 150.0;

/// One click on an element of known position.
///
/// The unit of measurement, and every field in it is either exact geometry or a
/// count the capture path took: there is no estimate here, and no quantity
/// derived from another one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reach {
    /// What was clicked.
    pub to: Element,
    /// The distance between the two known positions, in world units.
    ///
    /// **Centre to centre**, not click to click. The player lands somewhere
    /// inside a radius rather than on a point, and the difference between the
    /// two is a bounded per-target slop that the regression's intercept absorbs
    /// along with the rest of the fixed cost of arriving — which is the argument
    /// for a regression over a mix of distances rather than a ratio taken from
    /// one movement.
    pub distance: f64,
    /// The magnitude of the net device displacement over the leg, in the
    /// device's own counts.
    ///
    /// Net rather than path length, deliberately. A path length grows with how
    /// much the player overshot and corrected, which is style; the net
    /// displacement is what the *device* had to report for the cursor to arrive,
    /// and it is the quantity that stands in a fixed ratio to the distance.
    pub counts: f64,
    /// Motion events recorded during the leg.
    pub motions: u64,
    /// How long the leg took, from the first motion after the previous click to
    /// this one.
    pub took_ns: u64,
    /// Which of eight compass octants the geometry pointed in, `0` for east and
    /// counter-clockwise.
    ///
    /// Recorded because a measurement taken along one axis has already hidden an
    /// anisotropy once in this project (`docs/RISKS.md` R14: a character cell
    /// 1.158 world units across and 4.111 down), and a coverage criterion is the
    /// cheapest guard against doing it again.
    pub octant: u8,
}

impl Reach {
    /// Whether this reach was crossed fast enough to read a report rate off.
    #[must_use]
    pub fn fast(&self) -> bool {
        self.took_ns > 0 && self.distance * 1e9 / (self.took_ns as f64) >= FAST_UNITS_PER_SECOND
    }
}

/// What a session's reaches add up to, in the form that pools by addition.
///
/// **Sufficient statistics, not an estimate.** The five sums below are what a
/// least-squares fit of counts against distance needs, and they add across
/// sessions exactly — so a participant's device profile is the sum of their
/// sessions' observations and nothing has to be recomputed or stored to make it
/// so. `replay::calibration` is where the fit is done, on the trusted side,
/// which keeps this crate from computing a number a detector reads.
///
/// It crosses to `replay` as text in the session part, for the reason
/// [`crate::health::SessionPart`] gives: `client` may not link `replay`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Observations {
    /// Reaches recorded.
    pub reaches: u64,
    /// A bit per octant covered, `1 << octant`.
    pub octants: u8,
    /// The shortest reach, in world units.
    pub min_distance: f64,
    /// The longest.
    pub max_distance: f64,
    /// `Σ d`, world units.
    pub sum_distance: f64,
    /// `Σ n`, device counts.
    pub sum_counts: f64,
    /// `Σ d²`.
    pub sum_distance_sq: f64,
    /// `Σ d·n`.
    pub sum_distance_counts: f64,
    /// `Σ n²`.
    ///
    /// Carried only so that a fit can report how well it fits. A slope and an
    /// intercept need the four sums above; a spread needs this one, and a scale
    /// reported without one is the point estimate `docs/RISKS.md` R8 spends a
    /// page refusing everywhere else in this project.
    pub sum_counts_sq: f64,
    /// Reaches crossed at [`FAST_UNITS_PER_SECOND`] or above.
    pub fast_reaches: u64,
    /// Motion events recorded during those.
    pub fast_motions: u64,
    /// Nanoseconds they took, in total.
    pub fast_ns: u64,
    /// The finest non-zero delta component observed, in device counts.
    ///
    /// The hardware's own resolution, and the one number here that is neither
    /// style nor geometry: a mouse reporting whole counts gives `1`, a Wayland
    /// compositor's fixed-point relative motion gives `1/256`, and neither is
    /// anything a player can do differently. `0.0` means nothing moved.
    pub quantum: f64,
    /// Legs abandoned because the cursor reached the map clamp during them.
    ///
    /// Counted rather than silent, in the register of
    /// `crate::input::TraceStats::coincident`: a leg that saturated has a net
    /// displacement larger than the cursor actually travelled, so its ratio is
    /// not the device's, and discarding it without saying so would leave a
    /// denominator nobody can check.
    pub clamped: u64,
}

impl Observations {
    /// Nothing observed.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            reaches: 0,
            octants: 0,
            min_distance: 0.0,
            max_distance: 0.0,
            sum_distance: 0.0,
            sum_counts: 0.0,
            sum_distance_sq: 0.0,
            sum_distance_counts: 0.0,
            sum_counts_sq: 0.0,
            fast_reaches: 0,
            fast_motions: 0,
            fast_ns: 0,
            quantum: 0.0,
            clamped: 0,
        }
    }

    /// Folds one reach in.
    fn record(&mut self, reach: &Reach) {
        let Reach {
            to: _,
            distance,
            counts,
            motions,
            took_ns,
            octant,
        } = *reach;
        if self.reaches == 0 || distance < self.min_distance {
            self.min_distance = distance;
        }
        if distance > self.max_distance {
            self.max_distance = distance;
        }
        self.reaches = self.reaches.saturating_add(1);
        self.octants |= 1u8 << (octant & 7);
        self.sum_distance += distance;
        self.sum_counts += counts;
        self.sum_distance_sq += distance * distance;
        self.sum_distance_counts += distance * counts;
        self.sum_counts_sq += counts * counts;
        if reach.fast() {
            self.fast_reaches = self.fast_reaches.saturating_add(1);
            self.fast_motions = self.fast_motions.saturating_add(motions);
            self.fast_ns = self.fast_ns.saturating_add(took_ns);
        }
    }

    /// How many distinct octants are covered.
    #[must_use]
    pub const fn octants_covered(&self) -> u32 {
        self.octants.count_ones()
    }
}

/// The leg in progress: everything measured since the last click that landed.
#[derive(Clone, Copy, Debug)]
struct Leg {
    /// The known position the leg started from.
    from: FxVec2,
    /// Net device displacement so far.
    net: (f64, f64),
    /// Motion events so far.
    motions: u64,
    /// The first motion of the leg, or `None` while nothing has moved.
    began_ns: Option<u64>,
    /// Whether the cursor reached the map clamp during the leg.
    clamped: bool,
}

impl Leg {
    const fn from(at: FxVec2) -> Self {
        Self {
            from: at,
            net: (0.0, 0.0),
            motions: 0,
            began_ns: None,
            clamped: false,
        }
    }
}

/// The lobby, as a state machine with no window in it.
///
/// There is no viewport in this type and no way to reach one: the window is
/// [`crate::play::Play`]'s, which is what puts a rendering quantity and the
/// capture path in one struct so that `client/tests/lobby.rs` can drive two of
/// them differing only in window size and require the same answer. A property
/// that is true because a field is unreachable is worth something; a property
/// that stays red against a *changed* version of this file is worth more.
#[derive(Clone, Debug)]
pub struct Lobby {
    dummy: usize,
    leg: Leg,
    observations: Observations,
    visited_name: bool,
    visited_consent: bool,
    visited_champion: bool,
    ready: bool,
}

impl Default for Lobby {
    fn default() -> Self {
        Self::new()
    }
}

impl Lobby {
    /// A lobby nobody has touched.
    ///
    /// The first leg starts at the middle of the map, which is where
    /// [`crate::input::Aim::centred`] puts the cursor — a **known** position, so
    /// the very first click is already a reach rather than a movement with no
    /// origin.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            dummy: 0,
            leg: Leg::from(FxVec2::new(sim::Fx::ZERO, sim::Fx::ZERO)),
            observations: Observations::new(),
            visited_name: false,
            visited_consent: false,
            visited_champion: false,
            ready: false,
        }
    }

    /// Where the dummy is standing.
    #[must_use]
    pub fn dummy_at(&self) -> FxVec2 {
        let (x, y) = DUMMY_STATIONS[self.dummy % DUMMY_STATIONS.len()];
        FxVec2::new(fx(x), fx(y))
    }

    /// Where an element is, now.
    #[must_use]
    pub fn position_of(&self, element: Element) -> FxVec2 {
        station(element).unwrap_or_else(|| self.dummy_at())
    }

    /// Whether the player has asked to start.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        self.ready
    }

    /// Whether [`Element::Ready`] would do anything yet.
    ///
    /// This is the geometry's teeth. Ready is inert until the pseudonym, the
    /// consent version and champion select have each been visited, so the three
    /// long crossings of the lobby happen because the interface requires them
    /// and not because anybody was asked to make them.
    #[must_use]
    pub const fn can_ready(&self) -> bool {
        self.visited_name && self.visited_consent && self.visited_champion
    }

    /// What has been measured.
    #[must_use]
    pub const fn observations(&self) -> Observations {
        self.observations
    }

    /// One raw motion event, and where the cursor ended up after it.
    ///
    /// The cursor is **passed in** rather than held, and that is the shape of the
    /// constraint this module exists under: the position comes from
    /// [`crate::input::Aim`], which integrates the same deltas the trace records,
    /// and there is no other way for a position to reach this function. Nothing
    /// here may consult `self.viewport`.
    pub fn moved(&mut self, at_ns: u64, dx: f64, dy: f64, cursor: FxVec2) {
        self.leg.net.0 += dx;
        self.leg.net.1 += dy;
        self.leg.motions = self.leg.motions.saturating_add(1);
        if self.leg.began_ns.is_none() {
            self.leg.began_ns = Some(at_ns);
        }
        for component in [dx.abs(), dy.abs()] {
            if component > 0.0
                && (self.observations.quantum == 0.0 || component < self.observations.quantum)
            {
                self.observations.quantum = component;
            }
        }
        // The clamp is detected from the cursor rather than inferred from the
        // deltas, so that a leg is discarded for having hit the edge of the map
        // and never for the record and the cursor disagreeing about a scale —
        // which is the one thing this measurement exists to be able to see.
        let limit = RULES.map_half_extent;
        if cursor.x.abs() >= limit || cursor.y.abs() >= limit {
            self.leg.clamped = true;
        }
    }

    /// A click, at the cursor. Answers what it landed on, if anything.
    ///
    /// A click that lands on nothing closes no leg: the movement is still in
    /// progress, and a miss is a thing the player did rather than a thing to
    /// forget. `crate::play` records the press into the trace either way.
    pub fn clicked(&mut self, at_ns: u64, cursor: FxVec2) -> Option<Element> {
        let element = self.resolve(cursor)?;
        if element == Element::Ready && !self.can_ready() {
            return None;
        }
        let to = self.position_of(element);
        self.close_leg(at_ns, element, to);

        match element {
            Element::Name => self.visited_name = true,
            Element::Consent => self.visited_consent = true,
            Element::Champion => self.visited_champion = true,
            Element::Dummy => self.dummy = self.dummy.wrapping_add(1),
            Element::Ready => self.ready = true,
        }
        // The next leg starts from the position that was just *clicked*, which
        // is known geometry — and for the dummy that is where it stood, not
        // where it has moved to.
        self.leg = Leg::from(to);
        Some(element)
    }

    /// Which element the cursor is over.
    fn resolve(&self, cursor: FxVec2) -> Option<Element> {
        [
            Element::Name,
            Element::Consent,
            Element::Champion,
            Element::Ready,
            Element::Dummy,
        ]
        .into_iter()
        .find(|element| {
            let at = self.position_of(*element);
            let radius = if *element == Element::Dummy {
                DUMMY_RADIUS
            } else {
                BUTTON_RADIUS
            };
            distance(at, cursor) <= radius
        })
    }

    /// Turns the leg in progress into a reach, or accounts for why it is not
    /// one.
    fn close_leg(&mut self, at_ns: u64, to: Element, at: FxVec2) {
        let distance = distance(self.leg.from, at);
        if self.leg.clamped {
            self.observations.clamped = self.observations.clamped.saturating_add(1);
            return;
        }
        if distance < MIN_REACH_UNITS {
            return;
        }
        let Some(began) = self.leg.began_ns else {
            // A click with no motion behind it. There is no leg to measure and
            // no time to attribute it to.
            return;
        };
        let (net_x, net_y) = self.leg.net;
        let delta = at.sub(self.leg.from);
        let reach = Reach {
            to,
            distance,
            counts: net_x.hypot(net_y),
            motions: self.leg.motions,
            took_ns: at_ns.saturating_sub(began),
            octant: octant_of(
                f64::from(delta.x.to_raw()) / 65536.0,
                f64::from(delta.y.to_raw()) / 65536.0,
            ),
        };
        self.observations.record(&reach);
    }
}

/// Which of eight octants a vector points into, `0` east and counter-clockwise.
#[must_use]
pub fn octant_of(x: f64, y: f64) -> u8 {
    let turns = y.atan2(x) / core::f64::consts::TAU;
    let eighths = (turns * 8.0).round().rem_euclid(8.0);
    (eighths as u8) & 7
}

/// The distance between two world points, in world units.
fn distance(a: FxVec2, b: FxVec2) -> f64 {
    let dx = f64::from(a.x.to_raw() - b.x.to_raw()) / 65536.0;
    let dy = f64::from(a.y.to_raw() - b.y.to_raw()) / 65536.0;
    dx.hypot(dy)
}

/// A world coordinate written as an `f64` constant, in the fixed point `sim`
/// speaks.
///
/// `const` so that the layout is a compile-time constant rather than a value
/// somebody could make depend on a window.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a layout constant of this file, well inside Q15.16"
)]
const fn fx(units: f64) -> sim::Fx {
    sim::Fx::from_raw((units * 65536.0) as i32)
}

/// What to draw, from what the lobby knows.
///
/// A lobby of discs, in the register the rest of this client draws in: no text,
/// no font stack, no glyph atlas — `docs/MILESTONES.md` M4 asks for enough UI to
/// play and `client::draw` is explicit that a glyph atlas is not it. What a
/// player needs to know is which of the four things they have already touched,
/// and a filled disc says that.
#[must_use]
pub fn compose(lobby: &Lobby, cursor: FxVec2) -> Vec<Mark> {
    let mut marks = Vec::with_capacity(8);
    let button = |visited: bool| {
        if visited {
            colour::READY
        } else {
            colour::SPENT
        }
    };
    for (element, visited) in [
        (Element::Name, lobby.visited_name),
        (Element::Consent, lobby.visited_consent),
        (Element::Champion, lobby.visited_champion),
    ] {
        marks.push(Mark::Disc {
            at: lobby.position_of(element),
            radius: fx(BUTTON_RADIUS),
            colour: button(visited),
        });
    }
    marks.push(Mark::Disc {
        at: lobby.position_of(Element::Ready),
        radius: fx(BUTTON_RADIUS),
        colour: if lobby.can_ready() {
            colour::OWN
        } else {
            colour::SPENT
        },
    });
    marks.push(Mark::Disc {
        at: lobby.dummy_at(),
        radius: fx(DUMMY_RADIUS),
        colour: colour::DAMAGE,
    });
    // The cursor, which is the integrated one and the only one this client has.
    // It is passed in rather than held for the reason the whole module is about:
    // there is no path from here to a window, and drawing is the one direction
    // the position is allowed to travel.
    marks.push(Mark::Cross {
        at: cursor,
        colour: colour::AIM,
    });
    marks
}

#[cfg(test)]
mod tests {
    use super::{DUMMY_STATIONS, Element, Lobby, MIN_REACH_UNITS, octant_of};
    use sim::{Fx, FxVec2};

    fn at(x: f64, y: f64) -> FxVec2 {
        FxVec2::new(super::fx(x), super::fx(y))
    }

    /// The eight octants are the eight octants, and east is zero.
    #[test]
    fn an_octant_is_an_eighth_of_a_turn_from_east() {
        assert_eq!(octant_of(1.0, 0.0), 0);
        assert_eq!(octant_of(1.0, 1.0), 1);
        assert_eq!(octant_of(0.0, 1.0), 2);
        assert_eq!(octant_of(-1.0, 0.0), 4);
        assert_eq!(octant_of(0.0, -1.0), 6);
        assert_eq!(octant_of(1.0, -1.0), 7);
    }

    /// **The dummy's table reaches what the sufficiency criterion needs.**
    ///
    /// `docs/RISKS.md` R15: a fixture states what it actually reaches, and this
    /// table is a fixture in everything but name — a schedule of positions whose
    /// whole job is to sweep directions and distances. If somebody re-tunes it
    /// for how it looks on screen, this is where they find out that the
    /// measurement stopped being able to reach `Sufficient`.
    #[test]
    fn the_dummy_schedule_sweeps_every_octant_and_a_spread_of_distances() {
        let mut octants = 0u8;
        let mut shortest = f64::MAX;
        let mut longest: f64 = 0.0;
        for pair in DUMMY_STATIONS.windows(2) {
            let (from, to) = (pair[0], pair[1]);
            let (dx, dy) = (to.0 - from.0, to.1 - from.1);
            let length = dx.hypot(dy);
            assert!(
                length >= MIN_REACH_UNITS,
                "two stations {from:?} and {to:?} are {length} apart, which is \
                 below the shortest reach this lobby records"
            );
            octants |= 1u8 << octant_of(dx, dy);
            shortest = shortest.min(length);
            longest = longest.max(length);
        }
        assert_eq!(
            octants.count_ones(),
            8,
            "the dummy schedule covers {} of eight octants, and a measurement \
             aligned on too few directions has hidden an anisotropy in this \
             project before (docs/RISKS.md R14)",
            octants.count_ones()
        );
        assert!(
            longest / shortest >= 4.0,
            "the dummy schedule spans {shortest:.1} to {longest:.1} world units, \
             a ratio of {:.2}; a regression separates a distance-proportional \
             term from a fixed one only if the distances differ",
            longest / shortest
        );
        println!(
            "lobby: dummy schedule — {} station(s), 8 octants, {shortest:.1} to \
             {longest:.1} world units (ratio {:.2})",
            DUMMY_STATIONS.len(),
            longest / shortest
        );
    }

    /// Ready is inert until the three fixed elements have been visited, which is
    /// what forces the traversal.
    #[test]
    fn ready_is_inert_until_the_lobby_has_been_crossed() {
        let mut lobby = Lobby::new();
        assert!(!lobby.can_ready());
        // A click on Ready before the crossing lands on nothing at all.
        lobby.moved(1_000, 100.0, 100.0, at(90.0, -60.0));
        assert_eq!(lobby.clicked(2_000, at(90.0, -60.0)), None);
        assert!(!lobby.is_ready());

        for element in [Element::Name, Element::Consent, Element::Champion] {
            let to = lobby.position_of(element);
            lobby.moved(3_000, 10.0, 0.0, to);
            assert_eq!(lobby.clicked(4_000, to), Some(element));
        }
        assert!(lobby.can_ready());
        let ready = lobby.position_of(Element::Ready);
        lobby.moved(5_000, 10.0, 0.0, ready);
        assert_eq!(lobby.clicked(6_000, ready), Some(Element::Ready));
        assert!(lobby.is_ready());
    }

    /// A leg that hit the edge of the map is discarded and counted, never
    /// silently folded in.
    #[test]
    fn a_leg_that_reached_the_clamp_is_discarded_and_counted() {
        let mut lobby = Lobby::new();
        let name = lobby.position_of(Element::Name);
        lobby.moved(1_000, 0.0, 0.0, FxVec2::new(RULES_HALF, Fx::ZERO));
        lobby.moved(2_000, -100.0, -100.0, name);
        assert_eq!(lobby.clicked(3_000, name), Some(Element::Name));
        let observations = lobby.observations();
        assert_eq!(observations.reaches, 0);
        assert_eq!(observations.clamped, 1);
    }

    const RULES_HALF: Fx = sim::RULES.map_half_extent;
}
