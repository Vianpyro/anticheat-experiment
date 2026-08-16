//! Turning a view into marks, with no window anywhere in it.
//!
//! [`compose`] takes what the client knows and returns what to put on the
//! screen, in **world** coordinates. [`rasterize`] turns marks into pixels and
//! is the only thing here that knows a window exists. That split is the same one
//! the terminal client had and it survives for the same reason: everything about
//! a renderer that can be wrong in a way nobody notices by looking at it is in
//! the first function, and the first function is pure and has tests.
//!
//! # The projection runs one way, and that is the point
//!
//! [`Viewport::pixel`] maps a world point to a pixel. **There is no inverse.**
//! Nothing in this client turns a screen coordinate back into a world
//! coordinate, and the absence is structural rather than an omission: the
//! terminal client's `Camera::world` was exactly that inverse, and it was the
//! function `docs/RISKS.md` R14 was about. A pointing device whose reading has
//! to be run back through the display's grid inherits the display's grid.
//!
//! Where the aim comes from instead is [`crate::input::Aim`], which integrates
//! raw device deltas and has never heard of a pixel. The renderer is *told* the
//! aim so it can draw a cursor; it is not asked where the cursor is.
//!
//! # It draws what it was given and never fills in the rest
//!
//! One rule about content and it is the project's: **nothing is drawn that did
//! not arrive in a `PlayerView`.** No remembered enemy positions, no
//! last-known-location markers, no fading ghosts. Those are ordinary features of
//! a MOBA client and every one of them is a small maphack — information the
//! server withheld, reconstructed on the machine `docs/SCOPE.md` assumes is
//! compromised. That the reconstruction would be for the player's convenience is
//! not a distinction an attacker respects.
//!
//! The one exception is the player's own champion, drawn at the predicted
//! position rather than the authoritative one. That is not reconstruction: it is
//! this client's own outstanding input applied to information it was given. See
//! [`crate::predict`].
//!
//! # The window does not move, and there is no text
//!
//! No camera follow, no zoom: the whole triangle is on screen at once, so the
//! projection is a constant and a screenshot is comparable between two players.
//! The letterboxing keeps the map square whatever shape the window is, because a
//! stretched map would make a distance mean different things along the two axes
//! — which is the anisotropy the terminal had, in a renderer that had no excuse
//! for it.
//!
//! There is no text and therefore no font stack. What a player needs to know
//! that a shape cannot say — hit points, three cooldowns — is four bars along
//! the bottom edge, and the match outcome is a border in the winner's colour.
//! `docs/SCOPE.md` calls the game a fixture and `docs/MILESTONES.md` M4 asks for
//! "enough UI to play"; a glyph atlas is neither.

use sim::view::{EntityView, PlayerView, VisibleEvent};
use sim::{
    EntityId, Fx, FxVec2, Liveness, Outcome, PLAYER_COUNT, RULES, Seat, TOWER_ID_BASE, Team,
};

/// The world window the map is drawn to, in world units.
///
/// **It is the map**, `RULES.map_half_extent`, derived rather than written down
/// so the two cannot drift. Square, so the two axes have the same scale, and
/// fixed rather than fitted so a screenshot means the same thing in every
/// session.
///
/// It was `115.0` with the centre pushed up to `y = 20`, which framed the
/// triangle — bases at `(0, 100)`, `(86.6, -50)` and `(-86.6, -50)` — with a
/// margin and wasted no strip at the bottom. That was a better *photograph* and
/// a worse *window*, and the first playtest is what said so: `client::input::Aim`
/// clamps to `RULES.map_half_extent`, so the reachable area is the map and the
/// drawn area was smaller than it at the bottom and larger at the sides. The
/// cursor stopped against a wall nothing was drawn at, and disappeared off the
/// bottom edge before reaching one.
///
/// So the drawn area *is* the reachable area, and [`aim_limit`] paints its
/// boundary. `docs/ARCHITECTURE.md` already says the map is square and the game
/// is a triangle inscribed in it; this is the renderer agreeing with that
/// sentence. The cost is that everything is drawn about 11% smaller.
const HALF_SPAN: f64 = RULES.map_half_extent.to_raw() as f64 / 65536.0;
/// The middle of that window, which is the middle of the map.
const CENTRE_Y: f64 = 0.0;

/// Colours, `0x00RRGGBB`, in one table because a collision in it is a lie on the
/// screen rather than a compile error.
pub mod colour {
    /// Behind everything.
    pub const BACKGROUND: u32 = 0x000d_0f12;
    /// The three lanes, which are public information.
    pub const LANE: u32 = 0x0028_2d36;
    /// This player's own champion, at the predicted position.
    pub const OWN: u32 = 0x00ff_ffff;
    /// A teammate.
    pub const ALLY: u32 = 0x0086_b8ff;
    /// Blue, as an enemy or as a base.
    pub const BLUE: u32 = 0x0033_6fd4;
    /// Red, as an enemy or as a base.
    pub const RED: u32 = 0x00d4_4b3b;
    /// Green, as an enemy or as a base.
    pub const GREEN: u32 = 0x0044_bf5a;
    /// A standing tower of this player's own team.
    pub const OWN_TOWER: u32 = 0x009f_d0ff;
    /// A tower at zero hit points.
    pub const RUBBLE: u32 = 0x0055_5a60;
    /// A projectile in flight.
    pub const PROJECTILE: u32 = 0x00ff_d479;
    /// A cast, where the view says it happened.
    pub const CAST: u32 = 0x00ff_e08a;
    /// Damage.
    pub const DAMAGE: u32 = 0x00ff_6b5a;
    /// A death.
    pub const DEATH: u32 = 0x00ff_2d2d;
    /// The aim cursor.
    pub const AIM: u32 = 0x00ff_ffff;
    /// A gauge that is ready, or full.
    pub const READY: u32 = 0x0079_e08a;
    /// A gauge that is spent, or empty.
    pub const SPENT: u32 = 0x003a_4048;
    /// Hit points.
    pub const HEALTH: u32 = 0x00e0_5a5a;
}

/// Gauge slots along the bottom edge, left to right.
pub const GAUGES: u8 = 4;

/// One thing to put on the screen, in world coordinates.
///
/// A flat enum rather than a trait, because there is one renderer and
/// `docs/ARCHITECTURE.md` sets the bar for an abstraction at more than one
/// implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// A filled circle at a world point.
    Disc {
        /// Where.
        at: FxVec2,
        /// Radius in world units.
        radius: Fx,
        /// Colour.
        colour: u32,
    },
    /// A straight line between two world points.
    Segment {
        /// One end.
        from: FxVec2,
        /// The other.
        to: FxVec2,
        /// Colour.
        colour: u32,
    },
    /// A small cross at a world point: the aim cursor, and only that.
    Cross {
        /// Where.
        at: FxVec2,
        /// Colour.
        colour: u32,
    },
    /// A bar along the bottom edge, in screen space rather than world space.
    Gauge {
        /// Which of [`GAUGES`] slots, from the left.
        slot: u8,
        /// How full, clamped to `0..=1`.
        fill: Fx,
        /// Colour of the filled part.
        colour: u32,
    },
    /// A border around the whole window: the match is over.
    Border {
        /// The winner's colour.
        colour: u32,
    },
}

/// The four sides of the box the aim cannot leave.
///
/// `client::input::Aim` clamps to `RULES.map_half_extent`, which is a rule
/// constant rather than a window — clamping to the window would make a recorded
/// aim a function of a monitor, which is the whole of what
/// `client/tests/capture.rs` asserts. The consequence is a wall, and until the
/// first playtest nothing drew it: the cursor stopped, at a place where the
/// screen showed empty ground, and the report that came back was "the cursor is
/// confined to an invisible box inside the window".
///
/// A boundary a player can see is a rule. The same boundary unpainted is a bug
/// report, and it was one. Drawn first, so everything else covers it.
///
/// It sits **inside** a window whose shape is not square: the projection
/// letterboxes on the shorter axis, so a 16:10 window shows more world across
/// than down and the side walls stand off the left and right edges. That is not
/// a defect to fix by cropping — a distance has to mean the same number of
/// pixels whichever way it points.
pub fn aim_limit(marks: &mut Vec<Mark>) {
    let extent = RULES.map_half_extent;
    let corners = [
        FxVec2::new(extent.neg(), extent.neg()),
        FxVec2::new(extent, extent.neg()),
        FxVec2::new(extent, extent),
        FxVec2::new(extent.neg(), extent),
    ];
    for (index, from) in corners.iter().enumerate() {
        marks.push(Mark::Segment {
            from: *from,
            to: corners[index.saturating_add(1) % corners.len()],
            colour: colour::SPENT,
        });
    }
}

/// Everything the renderer is allowed to know.
///
/// A borrowed view and three values the client owns. There is deliberately no
/// history in this type: a renderer that could remember an enemy is a renderer
/// that can draw one the fog has taken away.
#[derive(Clone, Copy, Debug)]
pub struct Scene<'a> {
    /// The last view the server sent, culled.
    pub view: &'a PlayerView,
    /// This client's seat, which is what makes an ally an ally.
    pub seat: Seat,
    /// Where to draw this player's own champion: the predicted position.
    pub own: FxVec2,
    /// Where the player is aiming, from [`crate::input::Aim`].
    pub aim: FxVec2,
}

/// What to draw, from what the client knows.
#[must_use]
pub fn compose(scene: &Scene<'_>) -> Vec<Mark> {
    let mut marks = Vec::with_capacity(64);
    aim_limit(&mut marks);

    // The three lanes and the three bases, derived from the rules rather than
    // remembered. A lane's position is a constant of the game and therefore
    // public; drawing it tells nobody anything they could not compute.
    let bases = [
        sim::base_position(Team::Blue, &RULES),
        sim::base_position(Team::Red, &RULES),
        sim::base_position(Team::Green, &RULES),
    ];
    for (index, from) in bases.iter().enumerate() {
        let to = bases[index.saturating_add(1) % bases.len()];
        marks.push(Mark::Segment {
            from: *from,
            to,
            colour: colour::LANE,
        });
    }
    for (index, base) in bases.iter().enumerate() {
        marks.push(Mark::Disc {
            at: *base,
            radius: Fx::from_int(4),
            colour: match index {
                0 => colour::BLUE,
                1 => colour::RED,
                _ => colour::GREEN,
            },
        });
    }

    // Everything the server said is there, and nothing else.
    for entity in &scene.view.visible {
        match *entity {
            EntityView::Champion { id, position, .. } => marks.push(Mark::Disc {
                at: position,
                radius: RULES.champion_radius,
                colour: champion_colour(id, scene.seat),
            }),
            EntityView::Tower { id, position, hp } => marks.push(Mark::Disc {
                at: position,
                radius: Fx::from_int(3),
                colour: tower_colour(id, scene.seat, hp),
            }),
            EntityView::Projectile { position, .. } => marks.push(Mark::Disc {
                at: position,
                radius: RULES.skillshot_radius,
                colour: colour::PROJECTILE,
            }),
        }
    }

    // This tick's derived signals, over the entities, because a hit is worth
    // more on the screen than the thing it hit.
    for event in &scene.view.events {
        let (at, colour) = match *event {
            VisibleEvent::Cast { at, .. } => (at, colour::CAST),
            VisibleEvent::Damage { at, .. } => (at, colour::DAMAGE),
            VisibleEvent::Death { at, .. } => (at, colour::DEATH),
        };
        marks.push(Mark::Disc {
            at,
            radius: Fx::from_ratio(3, 2),
            colour,
        });
    }

    // The player's own champion, at the predicted position, over everything.
    if matches!(scene.view.own.liveness, Liveness::Alive { .. }) {
        marks.push(Mark::Disc {
            at: scene.own,
            radius: RULES.champion_radius,
            colour: colour::OWN,
        });
    }

    // The aim, last, so it is never hidden — and it is the only thing on the
    // screen that did not come from the server.
    marks.push(Mark::Cross {
        at: scene.aim,
        colour: colour::AIM,
    });

    // Hit points and the three cooldowns, as bars. A cooldown gauge fills as it
    // recovers, so "full" means "ready" for all four and a player reads one
    // shape rather than four.
    let cooldowns = scene.view.own.cooldowns;
    marks.push(Mark::Gauge {
        slot: 0,
        fill: match scene.view.own.liveness {
            Liveness::Alive { hp } => ratio(hp, RULES.champion_max_hp),
            Liveness::Dead { .. } => Fx::ZERO,
        },
        colour: colour::HEALTH,
    });
    for (slot, (remaining, total)) in [
        (cooldowns.skillshot, RULES.skillshot_cooldown_ticks),
        (cooldowns.targeted, RULES.targeted_cooldown_ticks),
        (cooldowns.basic_attack, RULES.attack_cooldown_ticks),
    ]
    .into_iter()
    .enumerate()
    {
        marks.push(Mark::Gauge {
            slot: u8::try_from(slot.saturating_add(1)).unwrap_or(GAUGES),
            fill: recovered(remaining, total),
            colour: if remaining == 0 {
                colour::READY
            } else {
                colour::SPENT
            },
        });
    }

    if let Outcome::Decided { winner, .. } = scene.view.outcome {
        marks.push(Mark::Border {
            colour: team_colour(winner),
        });
    }

    marks
}

/// `part / whole`, clamped to `0..=1`.
fn ratio(part: Fx, whole: Fx) -> Fx {
    if whole.to_raw() <= 0 || part.to_raw() <= 0 {
        return Fx::ZERO;
    }
    if part.to_raw() >= whole.to_raw() {
        return Fx::ONE;
    }
    part.div(whole)
}

/// How much of a cooldown has come back: full when nothing is remaining.
fn recovered(remaining: u16, total: u16) -> Fx {
    if remaining == 0 || total == 0 {
        return Fx::ONE;
    }
    let done = total.saturating_sub(remaining.min(total));
    ratio(
        Fx::from_int(i32::from(done)),
        Fx::from_int(i32::from(total)),
    )
}

/// The colour of a champion handle: its own, an ally's, or one of the two enemy
/// teams'.
///
/// A champion's handle *is* its seat and its team follows from it, which
/// `docs/ARCHITECTURE.md` records as the one thing in a view that may
/// distinguish one enemy team from the other. So drawing Red and Green
/// differently is not a leak; it is the only distinction the view carries,
/// rendered.
fn champion_colour(id: EntityId, own: Seat) -> u32 {
    let Some(seat) = Seat::from_index(u8::try_from(id.0).unwrap_or(u8::MAX)) else {
        // A handle that names no seat is not given a team. Inventing one would
        // be the renderer asserting something the view did not say.
        return colour::RUBBLE;
    };
    if seat == own {
        return colour::OWN;
    }
    if seat.team() == own.team() {
        return colour::ALLY;
    }
    team_colour(seat.team())
}

fn team_colour(team: Team) -> u32 {
    match team {
        Team::Blue => colour::BLUE,
        Team::Red => colour::RED,
        Team::Green => colour::GREEN,
    }
}

fn tower_colour(id: EntityId, own: Seat, hp: Fx) -> u32 {
    if hp.to_raw() <= 0 {
        return colour::RUBBLE;
    }
    match tower_team(id) {
        Some(team) if team == own.team() => colour::OWN_TOWER,
        Some(team) => team_colour(team),
        None => colour::RUBBLE,
    }
}

/// Which team a tower handle belongs to, from the public layout.
fn tower_team(id: EntityId) -> Option<Team> {
    let index = usize::from(id.0.checked_sub(TOWER_ID_BASE)?);
    if index >= sim::TOWER_COUNT {
        return None;
    }
    Some(sim::tower_team(index))
}

/// A window, in pixels.
///
/// The only type in the client that knows how big the screen is, and nothing in
/// `crate::input` may be given one — `client/tests/capture.rs` is what says so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl Viewport {
    /// A viewport for a window of this size.
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Pixels per world unit: the smaller of the two axes, so the map is
    /// letterboxed rather than stretched and a world distance means the same
    /// thing whichever way it points.
    #[must_use]
    pub fn scale(self) -> f64 {
        let across = f64::from(self.width.max(1)) / (HALF_SPAN * 2.0);
        let down = f64::from(self.height.max(1)) / (HALF_SPAN * 2.0);
        across.min(down)
    }

    /// The pixel a world point falls on.
    ///
    /// One way only. See this module's header for why there is no inverse.
    #[must_use]
    pub fn pixel(self, point: FxVec2) -> (f64, f64) {
        let scale = self.scale();
        let x = f64::from(point.x.to_raw()) / 65536.0;
        let y = f64::from(point.y.to_raw()) / 65536.0;
        (
            f64::from(self.width) / 2.0 + x * scale,
            f64::from(self.height) / 2.0 - (y - CENTRE_Y) * scale,
        )
    }
}

/// Paints marks into a pixel buffer, `0x00RRGGBB` per pixel.
///
/// The dull half. It clears to [`colour::BACKGROUND`] first, then paints in the
/// order the marks arrive, so `compose`'s ordering is what decides what covers
/// what.
pub fn rasterize(marks: &[Mark], viewport: Viewport, pixels: &mut [u32]) {
    let width = viewport.width as usize;
    let height = viewport.height as usize;
    if width == 0 || height == 0 {
        return;
    }
    for pixel in pixels.iter_mut() {
        *pixel = colour::BACKGROUND;
    }

    let scale = viewport.scale();
    for mark in marks {
        match *mark {
            Mark::Disc { at, radius, colour } => {
                let (cx, cy) = viewport.pixel(at);
                let r = (f64::from(radius.to_raw()) / 65536.0 * scale).max(1.5);
                disc(pixels, viewport, cx, cy, r, colour);
            }
            Mark::Segment { from, to, colour } => {
                let (x0, y0) = viewport.pixel(from);
                let (x1, y1) = viewport.pixel(to);
                segment(pixels, viewport, x0, y0, x1, y1, colour);
            }
            Mark::Cross { at, colour } => {
                let (cx, cy) = viewport.pixel(at);
                let arm = 7.0;
                segment(pixels, viewport, cx - arm, cy, cx - 2.0, cy, colour);
                segment(pixels, viewport, cx + 2.0, cy, cx + arm, cy, colour);
                segment(pixels, viewport, cx, cy - arm, cx, cy - 2.0, colour);
                segment(pixels, viewport, cx, cy + 2.0, cx, cy + arm, colour);
            }
            Mark::Gauge { slot, fill, colour } => {
                gauge(pixels, viewport, slot, fill, colour);
            }
            Mark::Border { colour } => {
                border(pixels, viewport, colour);
            }
        }
    }
}

fn put(pixels: &mut [u32], viewport: Viewport, x: i64, y: i64, colour: u32) {
    if x < 0 || y < 0 || x >= i64::from(viewport.width) || y >= i64::from(viewport.height) {
        return;
    }
    let index = (y as usize).saturating_mul(viewport.width as usize) + (x as usize);
    if let Some(pixel) = pixels.get_mut(index) {
        *pixel = colour;
    }
}

fn disc(pixels: &mut [u32], viewport: Viewport, cx: f64, cy: f64, r: f64, colour: u32) {
    let radius = r.max(0.5);
    let top = (cy - radius).floor() as i64;
    let bottom = (cy + radius).ceil() as i64;
    let left = (cx - radius).floor() as i64;
    let right = (cx + radius).ceil() as i64;
    for y in top..=bottom {
        for x in left..=right {
            let dx = x as f64 + 0.5 - cx;
            let dy = y as f64 + 0.5 - cy;
            if dx * dx + dy * dy <= radius * radius {
                put(pixels, viewport, x, y, colour);
            }
        }
    }
}

fn segment(
    pixels: &mut [u32],
    viewport: Viewport,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    colour: u32,
) {
    let steps = ((x1 - x0).abs().max((y1 - y0).abs()).ceil() as i64).clamp(1, 8192);
    for step in 0..=steps {
        let t = step as f64 / steps as f64;
        let x = (x0 + (x1 - x0) * t).round() as i64;
        let y = (y0 + (y1 - y0) * t).round() as i64;
        put(pixels, viewport, x, y, colour);
    }
}

fn gauge(pixels: &mut [u32], viewport: Viewport, slot: u8, fill: Fx, colour: u32) {
    let margin = 8i64;
    let height = 10i64;
    let width = i64::from(viewport.width);
    let usable = (width - margin * 2).max(GAUGES as i64);
    let each = usable / i64::from(GAUGES);
    let left = margin + i64::from(slot) * each;
    let top = i64::from(viewport.height) - margin - height;
    let filled =
        ((f64::from(fill.to_raw()) / 65536.0).clamp(0.0, 1.0) * (each - 4) as f64).round() as i64;
    for y in top..top + height {
        for x in left..left + each - 4 {
            let paint = if x - left < filled {
                colour
            } else {
                colour::SPENT
            };
            put(pixels, viewport, x, y, paint);
        }
    }
}

fn border(pixels: &mut [u32], viewport: Viewport, colour: u32) {
    let thickness = 4i64;
    let width = i64::from(viewport.width);
    let height = i64::from(viewport.height);
    for offset in 0..thickness {
        for x in 0..width {
            put(pixels, viewport, x, offset, colour);
            put(pixels, viewport, x, height - 1 - offset, colour);
        }
        for y in 0..height {
            put(pixels, viewport, offset, y, colour);
            put(pixels, viewport, width - 1 - offset, y, colour);
        }
    }
}

/// The visible enemy champion nearest a world point, if any is within `reach`.
///
/// Used by the click handlers, and it reads the *view* rather than any memory of
/// one: an enemy the fog has taken away cannot be clicked, which is the same
/// restriction the rules already put on the order.
#[must_use]
pub fn nearest_enemy(view: &PlayerView, seat: Seat, point: FxVec2, reach: Fx) -> Option<EntityId> {
    let mut best: Option<(i64, EntityId)> = None;
    for entity in &view.visible {
        let EntityView::Champion { id, position, .. } = *entity else {
            continue;
        };
        let Some(other) = Seat::from_index(u8::try_from(id.0).unwrap_or(u8::MAX)) else {
            continue;
        };
        if other.team() == seat.team() {
            continue;
        }
        if !position.within_range(point, reach) {
            continue;
        }
        let distance = position.sub(point).length_squared_wide();
        if best.is_none_or(|(closest, _)| distance < closest) {
            best = Some((distance, id));
        }
    }
    best.map(|(_, id)| id)
}

/// How many champion handles there are, so that a colour table cannot silently
/// stop covering the roster.
const _: () = assert!(PLAYER_COUNT == 9);
