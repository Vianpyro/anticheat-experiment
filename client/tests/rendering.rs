//! What the renderer may draw, and what a click means.
//!
//! Two things about a game client can be wrong in ways nobody notices by
//! looking at it, and both are here. The first is the projection: a click has to
//! mean the ground the player was pointing at, or every input in the corpus is
//! aimed somewhere the player did not aim. The second is the one that matters
//! for this project — **the renderer must not draw anything the server did not
//! send it.** A last-known-position marker is an ordinary MOBA feature and a
//! small maphack, and it is the sort of thing that arrives in a commit about
//! something else.
//!
//! `client::term` is not tested here and cannot be: raw mode and a person are
//! not a test fixture. Everything it does that could be wrong has been moved out
//! of it and into `client::ui`, which is a pure function, and that migration is
//! the reason this file can exist at all.

#![deny(unsafe_code)]

use client::ui::{Camera, Scene, Status, compose, nearest_enemy};
use sim::view::{EntityView, OwnView, PlayerView, VisibleEvent};
use sim::{
    Cooldowns, EntityId, Fx, FxVec2, Liveness, Outcome, RULES, Seat, Tick, base_position,
    champion_entity_id, new_state, step, view::view_for,
};

fn camera() -> Camera {
    Camera::fit(120, 44).expect("a terminal this size fits a map")
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
        cursor: (0, 0),
        status: Status::default(),
    }
}

/// The map area only.
///
/// The status lines are prose and prose contains letters: `right-click` alone
/// carries an `r` and a `g`, and a test that searched the whole screen for an
/// enemy glyph would be searching its own help text. What is under test is the
/// map, so the map is what is looked at.
fn glyphs(lines: &[String], camera: Camera) -> String {
    lines
        .iter()
        .take(usize::from(camera.rows))
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
}

/// A click is the ground under it, to within a cell.
///
/// The round trip is the property that matters: the renderer puts an entity at a
/// cell, the player clicks a cell, and the point that comes back has to be the
/// same piece of ground. Exact equality is not available — a cell is 2.75 world
/// units across and that is the whole of R14 — so the assertion is that the trip
/// never moves a point out of the cell it started in.
#[test]
fn a_click_lands_in_the_cell_it_was_aimed_at() {
    let camera = camera();
    let mut checked = 0u32;
    for col in (0..camera.cols).step_by(7) {
        for row in (0..camera.rows).step_by(5) {
            let point = camera.world(col, row);
            let (back_col, back_row) = camera
                .cell(point)
                .expect("a point the camera produced is off its own map");
            assert_eq!(
                (back_col, back_row),
                (col, row),
                "cell ({col}, {row}) came back as ({back_col}, {back_row})"
            );
            checked = checked.saturating_add(1);
        }
    }
    assert!(checked > 100, "only {checked} cells were checked");
}

/// The whole triangle is on the map, which is what a fixed window is for.
#[test]
fn every_base_and_every_tower_is_somewhere_on_the_screen() {
    let camera = camera();
    for team in [sim::Team::Blue, sim::Team::Red, sim::Team::Green] {
        assert!(
            camera.cell(base_position(team, &RULES)).is_some(),
            "{team:?}'s base is off the map"
        );
    }
    for index in 0..sim::TOWER_COUNT {
        assert!(
            camera.cell(sim::tower_position(index, &RULES)).is_some(),
            "tower {index} is off the map"
        );
    }
}

/// Nothing outside the view reaches the screen.
///
/// The maphack test, stated the way the project states them: not "the renderer
/// looks right", but that a champion the projection withheld leaves *no* mark.
/// The two views compared are the same tick from two seats on different teams,
/// which is exactly the pair a maphack would collapse.
#[test]
fn a_champion_the_view_withheld_leaves_no_mark_on_the_screen() {
    let camera = camera();
    let state = step(&new_state(0xB005), &[]);

    let blue = view_for(&state, Seat::Blue0);
    let drawn = glyphs(&compose(&scene(&blue, Seat::Blue0), camera), camera);

    // At tick 1 the teams are at their own bases, a lane apart, so Blue sees no
    // enemy at all. The screen must therefore carry no enemy glyph — and the
    // ally glyph must be there, or "no enemies drawn" is a statement about a
    // renderer that draws nothing.
    assert!(
        blue.visible.iter().all(|entity| !matches!(
            entity,
            EntityView::Champion { id, .. }
                if Seat::from_index(u8::try_from(id.0).unwrap_or(u8::MAX))
                    .is_some_and(|seat| seat.team() != sim::Team::Blue)
        )),
        "the fixture is wrong: Blue can see an enemy at tick 1"
    );
    assert!(!drawn.contains('r'), "a Red champion reached the screen");
    assert!(!drawn.contains('g'), "a Green champion reached the screen");
    assert!(drawn.contains('A'), "no ally was drawn, so nothing was");
    assert!(
        drawn.contains('@'),
        "the player's own champion was not drawn"
    );
}

/// The renderer holds no memory: the same call twice gives the same screen, and
/// a champion that leaves the view leaves the screen with it.
#[test]
fn an_entity_that_leaves_the_view_leaves_the_screen() {
    let camera = camera();
    let seat = Seat::Blue0;
    let mut view = empty_view(seat);
    let enemy = FxVec2::new(Fx::from_int(0), Fx::from_int(95));
    view.visible.push(EntityView::Champion {
        id: champion_entity_id(Seat::Red0),
        position: enemy,
        hp: RULES.champion_max_hp,
    });

    let with = glyphs(&compose(&scene(&view, seat), camera), camera);
    assert!(with.contains('r'), "the enemy in the view was not drawn");

    view.visible.clear();
    let without = glyphs(&compose(&scene(&view, seat), camera), camera);
    assert!(
        !without.contains('r'),
        "an enemy the view stopped carrying stayed on the screen"
    );

    // …and the renderer is a function: same input, same screen.
    assert_eq!(
        without,
        glyphs(&compose(&scene(&view, seat), camera), camera),
        "two calls with the same view drew different screens"
    );
}

/// Derived signals are drawn where they happened and only where the view put
/// them.
#[test]
fn an_event_is_drawn_at_the_place_the_view_says_it_happened() {
    let camera = camera();
    let seat = Seat::Blue0;
    let mut view = empty_view(seat);
    let where_it_happened = FxVec2::new(Fx::from_int(20), Fx::from_int(20));
    view.events.push(VisibleEvent::Death {
        entity: champion_entity_id(Seat::Red1),
        at: where_it_happened,
    });

    let lines = compose(&scene(&view, seat), camera);
    let (col, row) = camera.cell(where_it_happened).expect("on the map");
    let drawn = lines
        .get(row as usize)
        .and_then(|line| line.chars().nth(col as usize));
    assert_eq!(
        drawn,
        Some('X'),
        "the death was not drawn where it happened"
    );
}

/// The cursor never hides an entity.
///
/// A pointer drawn over a champion is a champion a player cannot see, which in a
/// game about withholding information is the wrong direction for a bug to go.
#[test]
fn the_cursor_is_drawn_only_where_nothing_else_is() {
    let camera = camera();
    let seat = Seat::Blue0;
    let mut view = empty_view(seat);
    let enemy = FxVec2::new(Fx::from_int(30), Fx::from_int(10));
    view.visible.push(EntityView::Champion {
        id: champion_entity_id(Seat::Red0),
        position: enemy,
        hp: RULES.champion_max_hp,
    });
    let (col, row) = camera.cell(enemy).expect("on the map");

    let mut over = scene(&view, seat);
    over.cursor = (col, row);
    let lines = compose(&over, camera);
    assert_eq!(
        lines
            .get(row as usize)
            .and_then(|line| line.chars().nth(col as usize)),
        Some('r'),
        "the cursor drew over a champion"
    );
}

/// Right-click finds the enemy nearest the click, and never an ally or a
/// champion the fog is hiding.
#[test]
fn the_target_finder_reads_the_view_and_not_the_world() {
    let seat = Seat::Blue0;
    let mut view = empty_view(seat);
    let near = FxVec2::new(Fx::from_int(10), Fx::ZERO);
    let far = FxVec2::new(Fx::from_int(14), Fx::ZERO);
    view.visible.push(EntityView::Champion {
        id: champion_entity_id(Seat::Red0),
        position: far,
        hp: RULES.champion_max_hp,
    });
    view.visible.push(EntityView::Champion {
        id: champion_entity_id(Seat::Green0),
        position: near,
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

    // The ally standing closest to the click is never the answer.
    assert_ne!(
        nearest_enemy(&view, seat, click, Fx::from_int(6)),
        Some(champion_entity_id(Seat::Blue1))
    );

    // And an enemy that is not in the view cannot be clicked, whatever the world
    // says: the finder is given a view and has no other source.
    view.visible.clear();
    assert_eq!(nearest_enemy(&view, seat, click, Fx::from_int(60)), None);
}

/// A terminal too small to draw a map in is refused rather than drawn wrong.
#[test]
fn a_terminal_that_cannot_hold_a_map_is_refused() {
    assert!(Camera::fit(19, 40).is_none(), "19 columns");
    assert!(Camera::fit(120, 13).is_none(), "13 rows");
    assert!(
        Camera::fit(120, 3).is_none(),
        "fewer rows than the status area"
    );
    assert!(Camera::fit(20, 14).is_some(), "the floor itself");
}

/// The status area is there, it is the size it says it is, and no line runs off
/// the screen.
#[test]
fn the_screen_is_the_size_the_camera_says() {
    let camera = camera();
    let seat = Seat::Blue0;
    let view = empty_view(seat);
    let lines = compose(&scene(&view, seat), camera);

    assert_eq!(
        lines.len(),
        usize::from(camera.rows) + usize::from(client::ui::STATUS_ROWS)
    );
    for (index, line) in lines.iter().enumerate() {
        assert!(
            line.chars().count() <= usize::from(camera.cols),
            "line {index} is {} cells wide against {} columns",
            line.chars().count(),
            camera.cols
        );
    }
}

/// The identity of an entity is not invented: a handle that names no seat is
/// drawn as a question mark rather than as somebody.
#[test]
fn a_handle_that_names_no_seat_is_not_given_a_team() {
    let camera = camera();
    let seat = Seat::Blue0;
    let mut view = empty_view(seat);
    view.visible.push(EntityView::Champion {
        id: EntityId(200),
        position: FxVec2::ZERO,
        hp: RULES.champion_max_hp,
    });
    let drawn = glyphs(&compose(&scene(&view, seat), camera), camera);
    assert!(drawn.contains('?'), "an impossible handle was given a team");
}
