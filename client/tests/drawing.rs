//! What the renderer may draw.
//!
//! One thing about a game client can be wrong in a way nobody notices by looking
//! at it, and it is the one that matters for this project: **the renderer must
//! not draw anything the server did not send it.** A last-known-position marker
//! is an ordinary MOBA feature and a small maphack, and it is the sort of thing
//! that arrives in a commit about something else.
//!
//! The other thing a renderer can get wrong — what a click means — is not here
//! any more, and its absence is the change. The terminal client had to invert
//! its own projection to turn a cell into a world point, so a round-trip test
//! was the only guard on the aim; `client::draw` has no inverse at all, the aim
//! comes from `client::input`, and `client/tests/capture.rs` is where it is
//! asserted.
//!
//! `client::gfx` is not tested here and cannot be: a window and a person are not
//! a test fixture. Everything it does that could be wrong has been moved out of
//! it, which is why this file and `capture.rs` can exist at all — and
//! `rasterize` is included because a CPU framebuffer is a pure function of a
//! slice and therefore runs in a CI job with no display.

#![deny(unsafe_code)]

use client::draw::{Mark, Scene, Viewport, colour, compose, nearest_enemy, rasterize};
use sim::view::{EntityView, OwnView, PlayerView, VisibleEvent};
use sim::{
    Cooldowns, EntityId, Fx, FxVec2, Liveness, Outcome, RULES, Seat, Team, Tick, base_position,
    champion_entity_id, new_state, step, view::view_for,
};

fn viewport() -> Viewport {
    Viewport::new(1280, 800)
}

fn empty_view(seat: Seat) -> PlayerView {
    PlayerView {
        tick: Tick(1),
        outcome: Outcome::InProgress,
        own: OwnView {
            id: champion_entity_id(seat),
            position: base_position(seat.team(), &RULES),
            liveness: Liveness::Alive {
                hp: RULES.champion_max_hp,
            },
            cooldowns: Cooldowns::default(),
        },
        visible: Vec::new(),
        events: Vec::new(),
    }
}

fn scene<'a>(view: &'a PlayerView, seat: Seat) -> Scene<'a> {
    Scene {
        view,
        seat,
        own: view.own.position,
        aim: FxVec2::ZERO,
    }
}

/// The colours of every disc drawn, which is what "who is on the screen" means
/// here.
fn discs(marks: &[Mark]) -> Vec<u32> {
    marks
        .iter()
        .filter_map(|mark| match *mark {
            Mark::Disc { colour, .. } => Some(colour),
            _ => None,
        })
        .collect()
}

/// Nothing outside the view reaches the screen.
///
/// The maphack test, stated the way the project states them: not "the renderer
/// looks right", but that a champion the projection withheld leaves *no* mark.
/// The tick chosen is one where Blue can see no enemy at all, and the ally
/// assertion is the completeness half — "no enemies drawn" is a statement about
/// a renderer that draws nothing.
#[test]
fn a_champion_the_view_withheld_leaves_no_mark_on_the_screen() {
    let state = step(&new_state(0xB005), &[]);
    let blue = view_for(&state, Seat::Blue0);
    let marks = compose(&scene(&blue, Seat::Blue0));
    let drawn = discs(&marks);

    assert!(
        blue.visible.iter().all(|entity| !matches!(
            entity,
            EntityView::Champion { id, .. }
                if Seat::from_index(u8::try_from(id.0).unwrap_or(u8::MAX))
                    .is_some_and(|seat| seat.team() != Team::Blue)
        )),
        "the fixture is wrong: Blue can see an enemy at tick 1"
    );

    // The bases are drawn in the team colours and are public information, so
    // the enemy-champion test has to be about champions rather than about a
    // colour appearing anywhere. There are exactly three base discs.
    let enemy_marks = marks
        .iter()
        .filter(|mark| {
            matches!(mark, Mark::Disc { radius, colour, .. }
                if *radius == RULES.champion_radius
                    && (*colour == colour::RED || *colour == colour::GREEN))
        })
        .count();
    assert_eq!(enemy_marks, 0, "an enemy champion reached the screen");
    assert!(
        drawn.contains(&colour::ALLY),
        "no ally was drawn, so nothing was"
    );
    assert!(
        drawn.contains(&colour::OWN),
        "the player's own champion was not drawn"
    );
}

/// The renderer holds no memory: the same call twice gives the same screen, and
/// a champion that leaves the view leaves the screen with it.
#[test]
fn an_entity_that_leaves_the_view_leaves_the_screen() {
    let seat = Seat::Blue0;
    let mut view = empty_view(seat);
    view.visible.push(EntityView::Champion {
        id: champion_entity_id(Seat::Red0),
        position: FxVec2::new(Fx::ZERO, Fx::from_int(95)),
        hp: RULES.champion_max_hp,
    });

    let with = compose(&scene(&view, seat));
    assert!(
        discs(&with).contains(&colour::RED),
        "the enemy in the view was not drawn"
    );

    view.visible.clear();
    let without = compose(&scene(&view, seat));
    let champions = without
        .iter()
        .filter(|mark| {
            matches!(mark, Mark::Disc { radius, colour, .. }
                if *radius == RULES.champion_radius && *colour == colour::RED)
        })
        .count();
    assert_eq!(
        champions, 0,
        "an enemy the view stopped carrying stayed on the screen"
    );

    // …and the renderer is a function: same input, same screen.
    assert_eq!(
        without,
        compose(&scene(&view, seat)),
        "two calls with the same view drew different screens"
    );
}

/// Derived signals are drawn where they happened and only where the view put
/// them.
#[test]
fn an_event_is_drawn_at_the_place_the_view_says_it_happened() {
    let seat = Seat::Blue0;
    let mut view = empty_view(seat);
    let where_it_happened = FxVec2::new(Fx::from_int(20), Fx::from_int(20));
    view.events.push(VisibleEvent::Death {
        entity: champion_entity_id(Seat::Red1),
        at: where_it_happened,
    });

    let marks = compose(&scene(&view, seat));
    assert!(
        marks.iter().any(|mark| matches!(*mark,
            Mark::Disc { at, colour, .. } if at == where_it_happened && colour == colour::DEATH)),
        "the death was not drawn where it happened"
    );
}

/// The aim cursor is the last thing composed, so nothing can cover it, and it is
/// the only mark on the screen that did not come from the server.
#[test]
fn the_aim_is_drawn_last_and_is_the_only_thing_the_server_did_not_send() {
    let seat = Seat::Blue0;
    let view = empty_view(seat);
    let mut drawn = scene(&view, seat);
    let aim = FxVec2::new(Fx::from_int(30), Fx::from_int(-10));
    drawn.aim = aim;
    let marks = compose(&drawn);

    let crosses: Vec<&Mark> = marks
        .iter()
        .filter(|mark| matches!(mark, Mark::Cross { .. }))
        .collect();
    assert_eq!(crosses.len(), 1, "there is not exactly one aim cursor");
    assert_eq!(
        *crosses[0],
        Mark::Cross {
            at: aim,
            colour: colour::AIM
        }
    );

    let cursor_at = marks
        .iter()
        .position(|mark| matches!(mark, Mark::Cross { .. }))
        .expect("the cursor is composed");
    let last_world_mark = marks
        .iter()
        .rposition(|mark| matches!(mark, Mark::Disc { .. } | Mark::Segment { .. }))
        .expect("something else is composed");
    assert!(
        cursor_at > last_world_mark,
        "the aim cursor is composed before something that can cover it"
    );
}

/// The whole triangle is on screen, which is what a fixed window is for.
#[test]
fn every_base_and_every_tower_lands_inside_the_window() {
    let viewport = viewport();
    let inside = |point| {
        let (x, y) = viewport.pixel(point);
        x >= 0.0 && y >= 0.0 && x < f64::from(viewport.width) && y < f64::from(viewport.height)
    };
    for team in [Team::Blue, Team::Red, Team::Green] {
        assert!(
            inside(base_position(team, &RULES)),
            "{team:?}'s base is off the window"
        );
    }
    for index in 0..sim::TOWER_COUNT {
        assert!(
            inside(sim::tower_position(index, &RULES)),
            "tower {index} is off the window"
        );
    }
}

/// The projection is isotropic: a world distance is the same number of pixels
/// whichever way it points, at any window shape.
///
/// This is the terminal's anisotropy, refused. A character cell is about twice
/// as tall as it is wide and a terminal is far wider than it is tall, so the
/// vertical resolution was about four times the coarser — a *directional* bias
/// in every aimed input, on top of the quantisation `docs/RISKS.md` R14 named.
/// Letterboxing is what makes that impossible here.
#[test]
fn a_world_distance_is_the_same_size_whichever_way_it_points() {
    for (width, height) in [(1280u32, 800u32), (400, 1600), (3840, 600), (1000, 1000)] {
        let viewport = Viewport::new(width, height);
        let origin = viewport.pixel(FxVec2::ZERO);
        let across = viewport.pixel(FxVec2::new(Fx::from_int(10), Fx::ZERO));
        let down = viewport.pixel(FxVec2::new(Fx::ZERO, Fx::from_int(10)));
        let horizontal = (across.0 - origin.0).abs();
        let vertical = (down.1 - origin.1).abs();
        assert!(
            (horizontal - vertical).abs() < 1e-9,
            "{width}×{height}: ten units is {horizontal} pixels across and \
             {vertical} down"
        );
    }
}

/// Right-click finds the enemy nearest the aim, and never an ally or a champion
/// the fog is hiding.
#[test]
fn the_target_finder_reads_the_view_and_not_the_world() {
    let seat = Seat::Blue0;
    let mut view = empty_view(seat);
    view.visible.push(EntityView::Champion {
        id: champion_entity_id(Seat::Red0),
        position: FxVec2::new(Fx::from_int(14), Fx::ZERO),
        hp: RULES.champion_max_hp,
    });
    view.visible.push(EntityView::Champion {
        id: champion_entity_id(Seat::Green0),
        position: FxVec2::new(Fx::from_int(10), Fx::ZERO),
        hp: RULES.champion_max_hp,
    });
    view.visible.push(EntityView::Champion {
        id: champion_entity_id(Seat::Blue1),
        position: FxVec2::new(Fx::from_int(9), Fx::ZERO),
        hp: RULES.champion_max_hp,
    });

    let click = FxVec2::new(Fx::from_int(9), Fx::ZERO);
    assert_eq!(
        nearest_enemy(&view, seat, click, Fx::from_int(6)),
        Some(champion_entity_id(Seat::Green0)),
        "the nearest enemy to the click was not chosen"
    );
    assert_ne!(
        nearest_enemy(&view, seat, click, Fx::from_int(6)),
        Some(champion_entity_id(Seat::Blue1)),
        "an ally was targeted"
    );

    // An enemy that is not in the view cannot be clicked, whatever the world
    // says: the finder is given a view and has no other source.
    view.visible.clear();
    assert_eq!(nearest_enemy(&view, seat, click, Fx::from_int(60)), None);
}

/// The identity of an entity is not invented: a handle that names no seat is not
/// given a team colour.
#[test]
fn a_handle_that_names_no_seat_is_not_given_a_team() {
    let seat = Seat::Blue0;
    let mut view = empty_view(seat);
    view.visible.push(EntityView::Champion {
        id: EntityId(200),
        position: FxVec2::ZERO,
        hp: RULES.champion_max_hp,
    });
    let marks = compose(&scene(&view, seat));
    assert!(
        marks.iter().any(|mark| matches!(*mark,
            Mark::Disc { at, colour, .. } if at == FxVec2::ZERO && colour == colour::RUBBLE)),
        "an impossible handle was given a team"
    );
}

/// The rasteriser stays inside its buffer, at every window shape, including ones
/// no window manager would produce.
///
/// It is the one piece of the renderer that indexes memory by arithmetic on
/// floats, and `docs/SCOPE.md` assumes the client is compromised — so the case
/// that matters is not a plausible window but the smallest and largest ones the
/// type allows.
#[test]
fn the_rasteriser_stays_inside_the_buffer() {
    let seat = Seat::Blue0;
    let mut view = empty_view(seat);
    view.visible.push(EntityView::Champion {
        id: champion_entity_id(Seat::Red0),
        position: FxVec2::new(Fx::from_int(-90), Fx::from_int(105)),
        hp: RULES.champion_max_hp,
    });
    view.outcome = Outcome::Decided {
        winner: Team::Green,
        at: Tick(9),
    };
    let mut drawn = scene(&view, seat);
    drawn.aim = FxVec2::new(Fx::from_int(-128), Fx::from_int(128));
    let marks = compose(&drawn);

    for (width, height) in [(1u32, 1u32), (3, 200), (200, 3), (640, 480), (2560, 1440)] {
        let viewport = Viewport::new(width, height);
        let mut pixels = vec![0u32; (width as usize) * (height as usize)];
        rasterize(&marks, viewport, &mut pixels);
        assert!(
            pixels.iter().any(|pixel| *pixel != 0),
            "{width}×{height} drew nothing at all"
        );
    }
}

/// Something is actually painted where the view said an entity was.
///
/// The completeness half of the maphack test, at the pixel level: a rasteriser
/// that drew nothing would satisfy every "this is not on the screen" assertion
/// above.
#[test]
fn an_entity_in_the_view_reaches_a_pixel() {
    let seat = Seat::Blue0;
    let mut view = empty_view(seat);
    let enemy = FxVec2::new(Fx::from_int(40), Fx::from_int(-20));
    view.visible.push(EntityView::Champion {
        id: champion_entity_id(Seat::Red0),
        position: enemy,
        hp: RULES.champion_max_hp,
    });

    let viewport = viewport();
    let mut pixels = vec![0u32; 1280 * 800];
    rasterize(&compose(&scene(&view, seat)), viewport, &mut pixels);

    let (x, y) = viewport.pixel(enemy);
    let index = (y as usize) * 1280 + (x as usize);
    assert_eq!(
        pixels[index],
        colour::RED,
        "nothing red was painted where the enemy was"
    );
}

/// The gauges say what they are for: full when a cooldown is ready, empty when it
/// has just been spent.
#[test]
fn a_spent_cooldown_reads_differently_from_a_ready_one() {
    let seat = Seat::Blue0;
    let mut view = empty_view(seat);
    view.own.cooldowns = Cooldowns {
        skillshot: RULES.skillshot_cooldown_ticks,
        ..Cooldowns::default()
    };
    let marks = compose(&scene(&view, seat));
    let gauges: Vec<(u8, Fx)> = marks
        .iter()
        .filter_map(|mark| match *mark {
            Mark::Gauge { slot, fill, .. } => Some((slot, fill)),
            _ => None,
        })
        .collect();

    assert_eq!(gauges.len(), usize::from(client::draw::GAUGES));
    assert_eq!(
        gauges[1].1,
        Fx::ZERO,
        "a just-spent cooldown reads as ready"
    );
    assert_eq!(gauges[2].1, Fx::ONE, "an untouched cooldown reads as spent");
}

/// **The wall the cursor stops against is drawn, and it is drawn where the aim
/// actually stops.**
///
/// `client::input::Aim` clamps to `RULES.map_half_extent` and nothing painted
/// that boundary until the first playtest reported it as an invisible box inside
/// the window. The assertion is over both screens because the lobby is the one a
/// player meets first, and it compares against the *rule constant* rather than
/// against a number written here — a boundary drawn somewhere other than where
/// the clamp is would be worse than no boundary at all, because it would be a
/// wrong explanation of a real wall.
#[test]
fn the_boundary_the_aim_stops_at_is_drawn_on_both_screens() {
    let extent = RULES.map_half_extent;
    let corners = [
        FxVec2::new(extent.neg(), extent.neg()),
        FxVec2::new(extent, extent.neg()),
        FxVec2::new(extent, extent),
        FxVec2::new(extent.neg(), extent),
    ];

    let seat = Seat::Blue0;
    let view = empty_view(seat);
    for (what, marks) in [
        ("the match", compose(&scene(&view, seat))),
        (
            "the lobby",
            client::lobby::compose(&client::lobby::Lobby::new(), FxVec2::ZERO),
        ),
    ] {
        for (index, from) in corners.iter().enumerate() {
            let to = corners[(index + 1) % corners.len()];
            assert!(
                marks.iter().any(|mark| matches!(
                    *mark,
                    Mark::Segment { from: a, to: b, .. } if a == *from && b == to
                )),
                "{what} does not draw the side of the aim's boundary from {from:?} \
                 to {to:?}, so the cursor stops there against nothing"
            );
        }
    }

    // …and the whole of it is inside the window at the shape the client opens,
    // which is what makes "drawn" mean "visible". The projection letterboxes on
    // the shorter axis, so this is a claim about the *vertical* extent: the
    // reachable area used to run 33 world units past the bottom edge, and a
    // cursor that leaves the screen before it stops is the same complaint in the
    // other direction.
    let viewport = viewport();
    for corner in corners {
        let (x, y) = viewport.pixel(corner);
        assert!(
            y >= 0.0 && y <= f64::from(viewport.height),
            "the aim can reach {corner:?}, which is at pixel ({x}, {y}) in a \
             {}×{} window: off the screen",
            viewport.width,
            viewport.height
        );
    }
}
