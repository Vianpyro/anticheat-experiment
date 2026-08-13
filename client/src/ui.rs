//! Turning a view into characters, with no terminal anywhere in it.
//!
//! # Why the drawing is a pure function
//!
//! [`compose`] takes what the client knows and returns the lines to print. It
//! opens nothing, reads no clock and touches no global state, which is what
//! makes the part of the renderer that can be wrong the part that has tests:
//! whether an entity lands on the cell it should, whether the projection is
//! reversible enough for a click to mean the point a player aimed at, and
//! whether anything the fog withheld can reach the screen. `crate::term` is the
//! other half and is deliberately dull — raw mode, an event loop and a `print!`.
//!
//! # It draws what it was given and never fills in the rest
//!
//! There is exactly one rule about content here and it is the project's:
//! **nothing is drawn that did not arrive in a `PlayerView`.** No remembered
//! enemy positions, no last-known-location markers, no fading ghosts of a
//! champion that walked into the fog. Those are ordinary features of a MOBA
//! client's UI and every one of them is a small maphack: the information they
//! render is information the server withheld, reconstructed on the machine
//! `docs/SCOPE.md` assumes is compromised. A client that keeps a decaying memory
//! of where enemies were has implemented half of the thing this project exists
//! to stop, and the fact that it would be for the player's convenience is not a
//! distinction an attacker respects.
//!
//! The one exception is the player's own champion, which is drawn at the
//! predicted position rather than the authoritative one. That is not
//! reconstruction: it is this client's own outstanding input, applied to
//! information it was given. See [`crate::predict`].
//!
//! # The map is drawn to a window that does not move
//!
//! No camera follow, no zoom. The whole triangle is on screen at once, which a
//! two-hundred-unit map and a terminal can just about manage, and it means the
//! projection is a constant: a cell is always the same piece of ground, so a
//! click is unambiguous and a screenshot is comparable between two players.
//!
//! # Aim resolution, and what it costs the corpus
//!
//! A terminal cell is a coarse pointing device: at eighty columns across two
//! hundred and twenty world units, one cell is about 2.75 units, which is wider
//! than a champion. Aim is therefore quantised, and that is a real cost to state
//! rather than hide — `docs/MILESTONES.md` M8 lists aim-correction trajectory
//! curvature among the behavioural signals, and a corpus recorded through this
//! renderer cannot support a curvature detector at any threshold, because the
//! trajectories in it are the renderer's grid rather than the player's hand.
//! What it does support is everything timing-shaped: inter-arrival
//! distributions, reaction latency floors, and claimed-against-observed
//! timestamp drift, none of which the quantisation touches. `docs/RISKS.md` R14
//! carries the decision.

use sim::view::{EntityView, PlayerView, VisibleEvent};
use sim::{EntityId, Fx, FxVec2, Liveness, Outcome, PLAYER_COUNT, Seat, TOWER_ID_BASE, Team, Tick};

/// The world window the map is drawn to, in world units.
///
/// Wide enough for the whole triangle — bases at `(0, 100)`, `(86.6, -50)` and
/// `(-86.6, -50)` — with a margin, and fixed rather than fitted so that a cell
/// means the same ground in every session.
const LEFT: f64 = -110.0;
const RIGHT: f64 = 110.0;
const BOTTOM: f64 = -75.0;
const TOP: f64 = 110.0;

/// Rows the status area takes underneath the map.
pub const STATUS_ROWS: u16 = 4;

/// Where the map is drawn and how big it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Camera {
    /// Columns of map.
    pub cols: u16,
    /// Rows of map.
    pub rows: u16,
}

impl Camera {
    /// A camera for a terminal of this size, or `None` if there is not enough
    /// of it to draw a map on.
    #[must_use]
    pub fn fit(terminal_cols: u16, terminal_rows: u16) -> Option<Self> {
        let rows = terminal_rows.checked_sub(STATUS_ROWS)?;
        if terminal_cols < 20 || rows < 10 {
            return None;
        }
        Some(Self {
            cols: terminal_cols,
            rows,
        })
    }

    /// The cell a world point falls in, or `None` if it is outside the window.
    #[must_use]
    pub fn cell(self, point: FxVec2) -> Option<(u16, u16)> {
        let x = raw_to_f64(point.x);
        let y = raw_to_f64(point.y);
        if !(LEFT..=RIGHT).contains(&x) || !(BOTTOM..=TOP).contains(&y) {
            return None;
        }
        let col = ((x - LEFT) / (RIGHT - LEFT)) * f64::from(self.cols);
        let row = ((TOP - y) / (TOP - BOTTOM)) * f64::from(self.rows);
        Some((
            (col as u16).min(self.cols.saturating_sub(1)),
            (row as u16).min(self.rows.saturating_sub(1)),
        ))
    }

    /// The world point at the middle of a cell.
    ///
    /// The inverse of [`Camera::cell`] up to the cell's own width, which is the
    /// resolution a player is pointing with. A round trip through both moves a
    /// point by less than one cell and never by more.
    #[must_use]
    pub fn world(self, col: u16, row: u16) -> FxVec2 {
        let across = (f64::from(col) + 0.5) / f64::from(self.cols);
        let down = (f64::from(row) + 0.5) / f64::from(self.rows);
        FxVec2::new(
            f64_to_fx(LEFT + across * (RIGHT - LEFT)),
            f64_to_fx(TOP - down * (TOP - BOTTOM)),
        )
    }
}

fn raw_to_f64(value: Fx) -> f64 {
    f64::from(value.to_raw()) / 65536.0
}

fn f64_to_fx(value: f64) -> Fx {
    // Clamped into the fixed-point type's own range before the cast, so that a
    // terminal reporting a nonsensical size cannot produce an out-of-range raw
    // value. The rules clamp coordinates into the map anyway; this is about
    // keeping the conversion total.
    let clamped = value.clamp(-32768.0, 32767.0);
    Fx::from_raw((clamped * 65536.0) as i32)
}

/// Everything the renderer is allowed to know.
///
/// A borrowed view and two numbers the client owns. There is deliberately no
/// history in this type: a renderer that could not remember an enemy is a
/// renderer that cannot draw one the fog has taken away.
#[derive(Clone, Copy, Debug)]
pub struct Scene<'a> {
    /// The last view the server sent, culled.
    pub view: &'a PlayerView,
    /// This client's seat, which is what makes an ally an ally.
    pub seat: Seat,
    /// Where to draw this player's own champion: the predicted position.
    pub own: FxVec2,
    /// Where the pointer is, in cells.
    pub cursor: (u16, u16),
    /// What to say in the status area.
    pub status: Status,
}

/// The numbers under the map.
#[derive(Clone, Copy, Debug, Default)]
pub struct Status {
    /// Intentions sent and not yet acknowledged.
    pub outstanding: usize,
    /// The last and worst prediction corrections, in raw fixed-point units.
    pub corrections: (i32, i32),
    /// Frames abandoned for a missing shard, and shards that arrived too late.
    pub losses: (u32, u32),
    /// Views discarded for not being newer.
    pub stale: u32,
    /// What the last click asked for, for a player who wants to know the click
    /// registered.
    pub last_order: Option<&'static str>,
}

/// The glyph table, in one place because a collision in it is a lie on the
/// screen rather than a compile error.
///
/// | Glyph | What |
/// | --- | --- |
/// | `@` | this player's champion, at the predicted position |
/// | `A` | an ally |
/// | `b` `r` `g` | an enemy champion, by team — the one distinction a view carries |
/// | `B` `R` `G` | a base, whose position is a constant of the rules |
/// | `T` `t` | a standing tower, this team's and another's |
/// | `u` | rubble: a tower at zero hit points |
/// | `*` | a projectile |
/// | `!` `x` `X` | a cast, a hit, a death, at the place the view says |
/// | `.` | a lane |
/// | `+` | the pointer, drawn only where nothing else is |
///
/// The lines to print: `camera.rows` of map, then [`STATUS_ROWS`] of status.
#[must_use]
pub fn compose(scene: &Scene<'_>, camera: Camera) -> Vec<String> {
    let mut grid = vec![vec![' '; camera.cols as usize]; camera.rows as usize];

    // The three lanes, so that a player can tell which way is which. They are
    // derived from the rules rather than remembered, and they are public
    // information — a lane's position is a constant of the game.
    let bases = [
        sim::base_position(Team::Blue, &sim::RULES),
        sim::base_position(Team::Red, &sim::RULES),
        sim::base_position(Team::Green, &sim::RULES),
    ];
    for (index, from) in bases.iter().enumerate() {
        let to = bases[index.saturating_add(1) % bases.len()];
        draw_line(&mut grid, camera, *from, to, '.');
    }
    for (index, base) in bases.iter().enumerate() {
        let mark = ['B', 'R', 'G'][index];
        put(&mut grid, camera, *base, mark);
    }

    // Everything the server said is there, and nothing else.
    for entity in &scene.view.visible {
        match *entity {
            EntityView::Champion { id, position, .. } => {
                put(&mut grid, camera, position, champion_glyph(id, scene.seat));
            }
            EntityView::Tower { id, position, hp } => {
                let standing = hp.to_raw() > 0;
                let own = tower_team(id) == Some(scene.seat.team());
                put(
                    &mut grid,
                    camera,
                    position,
                    match (own, standing) {
                        (true, true) => 'T',
                        (false, true) => 't',
                        (_, false) => 'u',
                    },
                );
            }
            EntityView::Projectile { position, .. } => {
                put(&mut grid, camera, position, '*');
            }
        }
    }

    // This tick's derived signals, over the entities, because a hit is worth
    // more on the screen than the thing it hit.
    for event in &scene.view.events {
        let (at, glyph) = match *event {
            VisibleEvent::Cast { at, .. } => (at, '!'),
            VisibleEvent::Damage { at, .. } => (at, 'x'),
            VisibleEvent::Death { at, .. } => (at, 'X'),
        };
        put(&mut grid, camera, at, glyph);
    }

    // The player's own champion, at the predicted position, over everything.
    if matches!(scene.view.own.liveness, Liveness::Alive { .. }) {
        put(&mut grid, camera, scene.own, '@');
    }

    // The cursor, which is drawn last and only where nothing else is, so that
    // it can never hide an entity from the player.
    let (cursor_col, cursor_row) = scene.cursor;
    if let Some(row) = grid.get_mut(cursor_row as usize)
        && let Some(cell) = row.get_mut(cursor_col as usize)
        && *cell == ' '
    {
        *cell = '+';
    }

    let mut lines: Vec<String> = grid
        .into_iter()
        .map(|row| row.into_iter().collect())
        .collect();
    lines.extend(status_lines(scene, camera));
    lines
}

/// The four status lines.
fn status_lines(scene: &Scene<'_>, camera: Camera) -> Vec<String> {
    let view = scene.view;
    let Tick(tick) = view.tick;
    let liveness = match view.own.liveness {
        Liveness::Alive { hp } => format!("hp {:.0}", raw_to_f64(hp)),
        Liveness::Dead { respawn_at } => format!("dead, back at tick {}", respawn_at.0),
    };
    let outcome = match view.outcome {
        Outcome::InProgress => "playing".to_owned(),
        Outcome::Decided { winner, at } => format!("{winner:?} won at tick {at:?}"),
    };
    let cooldowns = view.own.cooldowns;
    let (last, worst) = scene.status.corrections;
    let (incomplete, late) = scene.status.losses;

    let mut lines = vec![
        format!(
            "seat {:?}  tick {tick}  {liveness}  {outcome}  seen {}",
            scene.seat,
            view.visible.len()
        ),
        format!(
            "q skillshot {}  w targeted {}  attack {}  |  {}",
            cooldown(cooldowns.skillshot),
            cooldown(cooldowns.targeted),
            cooldown(cooldowns.basic_attack),
            scene.status.last_order.unwrap_or("-")
        ),
        format!(
            "outstanding {}  correction {last}/{worst}  frames lost {incomplete}  \
             shards late {late}  stale {}",
            scene.status.outstanding, scene.status.stale
        ),
        "click move · right-click attack · q cast · w spell · s stop · esc leave".to_owned(),
    ];
    for line in &mut lines {
        line.truncate(camera.cols as usize);
    }
    lines
}

fn cooldown(ticks: u16) -> String {
    if ticks == 0 {
        "ready".to_owned()
    } else {
        format!("{ticks}")
    }
}

/// The glyph for a champion handle: its own, an ally's, or one of the two enemy
/// teams'.
///
/// A champion's handle *is* its seat and its team follows from it, which
/// `docs/ARCHITECTURE.md` records as the one thing in a view that may
/// distinguish one enemy team from the other. So drawing `R` and `G` differently
/// is not a leak; it is the only distinction the view carries, rendered.
fn champion_glyph(id: EntityId, own: Seat) -> char {
    let Some(seat) = Seat::from_index(u8::try_from(id.0).unwrap_or(u8::MAX)) else {
        return '?';
    };
    if seat == own {
        return '@';
    }
    if seat.team() == own.team() {
        return 'A';
    }
    match seat.team() {
        Team::Blue => 'b',
        Team::Red => 'r',
        Team::Green => 'g',
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

fn put(grid: &mut [Vec<char>], camera: Camera, point: FxVec2, glyph: char) {
    let Some((col, row)) = camera.cell(point) else {
        return;
    };
    if let Some(line) = grid.get_mut(row as usize)
        && let Some(cell) = line.get_mut(col as usize)
    {
        *cell = glyph;
    }
}

/// A straight line between two world points, in cells, drawn only where the
/// cell is still empty.
fn draw_line(grid: &mut [Vec<char>], camera: Camera, from: FxVec2, to: FxVec2, glyph: char) {
    let steps = 400;
    for step in 0..=steps {
        let t = f64::from(step) / f64::from(steps);
        let x = raw_to_f64(from.x) + (raw_to_f64(to.x) - raw_to_f64(from.x)) * t;
        let y = raw_to_f64(from.y) + (raw_to_f64(to.y) - raw_to_f64(from.y)) * t;
        let point = FxVec2::new(f64_to_fx(x), f64_to_fx(y));
        let Some((col, row)) = camera.cell(point) else {
            continue;
        };
        if let Some(line) = grid.get_mut(row as usize)
            && let Some(cell) = line.get_mut(col as usize)
            && *cell == ' '
        {
            *cell = glyph;
        }
    }
}

/// The visible enemy champion nearest a world point, if any is within `reach`.
///
/// Used by the click handlers, and it reads the *view* rather than any memory of
/// one: an enemy the fog has taken away cannot be right-clicked, which is the
/// same restriction the rules already put on the order.
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

/// How many champion handles there are, so that a glyph table cannot silently
/// stop covering the roster.
const _: () = assert!(PLAYER_COUNT == 9);
