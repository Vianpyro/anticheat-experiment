//! The exit criterion of `docs/MILESTONES.md` M2, over the M1 fixtures.
//!
//! # The claim
//!
//! For every tick of both fixtures and for every one of the six players, no
//! `EntityId` outside that player's vision appears anywhere in
//! `view_for`'s output — in the entity list or in the derived events.
//!
//! # Why this file re-derives the vision rule instead of calling `sim`'s
//!
//! Asserting that `view_for` agrees with `sim::view::can_see` would be
//! asserting that a function agrees with itself. So the visibility predicate is
//! written out again here, from the rules as `docs/MILESTONES.md` states them —
//! living champions and standing towers of the player's own team, each covering
//! a disc — and the two implementations are compared over a hundred thousand
//! (tick, player, entity) triples. A change to one that is not a change to the
//! other fails here.
//!
//! # Why it also asserts what *is* present
//!
//! A culling test that only checks for absences passes trivially against a
//! projection that returns nothing, and "true by vacuity" is the failure mode
//! this project has already found once, in a `Serialize` grep that had never
//! rejected anything. So every assertion below has a completeness half — the
//! view contains everything in vision — and the run ends by asserting that the
//! fixture actually exercised culling: that entities and events were withheld
//! on a large number of ticks, and that a view built with unlimited vision
//! would have been strictly larger.

#![deny(clippy::float_arithmetic, unsafe_code)]

mod fixture;

use std::collections::BTreeSet;

use fixture::{DUEL_RULES, DUEL_SEED, SEED, duel_script, script};
use sim::view::{EntityView, PlayerView, VisibleEvent, view_for, view_for_with_rules};
use sim::{
    Event, EventKind, Fx, FxVec2, Input, Liveness, PLAYER_COUNT, PlayerId, RULES, Rules, State,
    TOWER_COUNT, Team, Tick, champion_entity_id, new_state, new_state_with_rules, step_with_rules,
    tower_entity_id, tower_position,
};

/// Vision, re-derived from `docs/MILESTONES.md` M2 rather than from `sim`.
///
/// Sources are the player's own team: champions that are alive, towers that
/// still stand. Deliberately written as the naive double loop it is — this is
/// the specification, and it is supposed to be boring enough to read as one.
fn team_sees(state: &State, team: Team, point: FxVec2, rules: &Rules) -> bool {
    for seat in 0..PLAYER_COUNT {
        if Team::of_player(PlayerId(seat as u8)) != team {
            continue;
        }
        let champion = state.champions()[seat];
        if !matches!(champion.liveness, Liveness::Alive { .. }) {
            continue;
        }
        if champion
            .position
            .distance(point)
            .to_raw()
            .saturating_sub(rules.champion_vision_radius.to_raw())
            <= 0
        {
            return true;
        }
    }
    for index in 0..TOWER_COUNT {
        if sim::tower_team(index) != team {
            continue;
        }
        if !state.towers()[index].is_standing() {
            continue;
        }
        if tower_position(index, rules)
            .distance(point)
            .to_raw()
            .saturating_sub(rules.tower_vision_radius.to_raw())
            <= 0
        {
            return true;
        }
    }
    false
}

/// Every handle in a view, paired with the position it was reported at.
///
/// This pairing is the whole argument: the criterion is that no entity outside
/// vision is named, and an entity is outside vision exactly when the point it
/// was seen at is. A handle with no position attached could not be checked, so
/// the view type does not have one.
fn handles_with_positions(view: &PlayerView) -> Vec<(u16, FxVec2)> {
    let mut out = Vec::new();
    for entity in &view.visible {
        let (id, position) = match entity {
            EntityView::Champion { id, position, .. }
            | EntityView::Tower { id, position, .. }
            | EntityView::Projectile { id, position, .. } => (*id, *position),
        };
        out.push((id.0, position));
    }
    for event in &view.events {
        let (id, at) = match event {
            VisibleEvent::Cast { caster: id, at, .. }
            | VisibleEvent::Damage { target: id, at, .. }
            | VisibleEvent::Death { entity: id, at } => (*id, *at),
        };
        out.push((id.0, at));
    }
    out
}

/// The ids of the entities on the map that this player is entitled to see,
/// computed from the state and the re-derived predicate above. The player's own
/// champion is excluded: it lives in `own`, never in `visible`.
fn expected_visible(state: &State, player: PlayerId, rules: &Rules) -> BTreeSet<u16> {
    let team = Team::of_player(player);
    let mut expected = BTreeSet::new();
    for seat in 0..PLAYER_COUNT {
        if seat == player.0 as usize {
            continue;
        }
        let champion = state.champions()[seat];
        if !matches!(champion.liveness, Liveness::Alive { .. }) {
            continue;
        }
        if team_sees(state, team, champion.position, rules) {
            expected.insert(champion_entity_id(seat).0);
        }
    }
    for index in 0..TOWER_COUNT {
        if team_sees(state, team, tower_position(index, rules), rules) {
            expected.insert(tower_entity_id(index).0);
        }
    }
    for projectile in state.projectiles().iter() {
        if team_sees(state, team, projectile.position, rules) {
            expected.insert(projectile.id.0);
        }
    }
    expected
}

/// What the events of this tick reduce to for this player.
fn expected_events(state: &State, player: PlayerId, rules: &Rules) -> Vec<Event> {
    let team = Team::of_player(player);
    state
        .events()
        .iter()
        .filter(|event| team_sees(state, team, event.at, rules))
        .copied()
        .collect()
}

fn same_event(state: &Event, view: &VisibleEvent) -> bool {
    match (state.kind, view) {
        (
            EventKind::Cast { caster, ability },
            VisibleEvent::Cast {
                caster: seen_caster,
                ability: seen_ability,
                at,
            },
        ) => caster == *seen_caster && ability == *seen_ability && state.at == *at,
        (
            EventKind::Damage { target, amount },
            VisibleEvent::Damage {
                target: seen_target,
                amount: seen_amount,
                at,
            },
        ) => target == *seen_target && amount == *seen_amount && state.at == *at,
        (
            EventKind::Death { entity },
            VisibleEvent::Death {
                entity: seen_entity,
                at,
            },
        ) => entity == *seen_entity && state.at == *at,
        _ => false,
    }
}

/// What one run of a fixture observed, so that the end of the test can assert
/// the fixture was worth running.
#[derive(Default)]
struct Coverage {
    views: u64,
    entities_withheld: u64,
    events_withheld: u64,
    events_delivered: u64,
    ticks_smaller_than_omniscient: u64,
    largest_encoding: usize,
}

/// Runs a fixture, projecting every tick for every player and checking the
/// criterion on each of them.
fn audit(seed: u64, log: &[Vec<Input>], rules: &Rules) -> Coverage {
    // Vision wide enough to cover the map's diagonal from anywhere. This is the
    // "accidental full-state leak" the milestone asks the size assertion to
    // catch, built deliberately so that the real projection can be compared
    // against it rather than against a number somebody once measured.
    let omniscient = Rules {
        champion_vision_radius: Fx::from_int(512),
        tower_vision_radius: Fx::from_int(512),
        ..*rules
    };

    let mut coverage = Coverage::default();
    let mut state = new_state_with_rules(seed, rules);

    for inputs in log {
        state = step_with_rules(&state, inputs, rules);

        for seat in 0..PLAYER_COUNT {
            let player = PlayerId(seat as u8);
            let team = Team::of_player(player);
            let view = view_for_with_rules(&state, player, rules);
            coverage.views += 1;

            // 1. The criterion itself. Every handle named anywhere in the view
            //    was named at a point this player can see.
            for (id, position) in handles_with_positions(&view) {
                assert!(
                    team_sees(&state, team, position, rules),
                    "tick {:?}, player {seat}: entity {id} reported at {position:?}, \
                     which is outside this team's vision",
                    state.tick()
                );
            }

            // 2. The player's own handle is its own, and it is never in the
            //    entity list: `own` is where it lives, and duplicating it there
            //    would be a second place for the culling rule to have to apply.
            //    It *can* appear in an event, and that is correct rather than a
            //    leak — a death is shown to whoever could see the ground it
            //    happened on, and the victim's own team is often among them.
            assert_eq!(view.own.id, champion_entity_id(seat));
            assert!(
                !view.visible.iter().any(|entity| matches!(
                    entity,
                    EntityView::Champion { id, .. } if *id == view.own.id
                )),
                "tick {:?}, player {seat}: own champion duplicated into the entity list",
                state.tick()
            );

            // 3. Completeness, which is what stops "return nothing" from
            //    passing. The visible set is exactly the entitled set.
            let reported: BTreeSet<u16> = view
                .visible
                .iter()
                .map(|entity| match entity {
                    EntityView::Champion { id, .. }
                    | EntityView::Tower { id, .. }
                    | EntityView::Projectile { id, .. } => id.0,
                })
                .collect();
            let expected = expected_visible(&state, player, rules);
            assert_eq!(
                reported,
                expected,
                "tick {:?}, player {seat}: the visible set is not the entitled set",
                state.tick()
            );

            // 4. Reported positions and hit points are the real ones. Culling
            //    is the only transformation applied; a view that quietly
            //    rounded a position would be a different bug wearing this
            //    test's uniform.
            for entity in &view.visible {
                match entity {
                    EntityView::Champion { id, position, hp } => {
                        let champion = state.champions()[id.0 as usize];
                        assert_eq!(*position, champion.position);
                        assert_eq!(Liveness::Alive { hp: *hp }, champion.liveness);
                    }
                    EntityView::Tower { id, position, hp } => {
                        let index = (id.0 - 10) as usize;
                        assert_eq!(*position, tower_position(index, rules));
                        assert_eq!(*hp, state.towers()[index].hp);
                    }
                    EntityView::Projectile {
                        id,
                        position,
                        velocity,
                    } => {
                        let found = state
                            .projectiles()
                            .iter()
                            .find(|projectile| projectile.id == *id)
                            .expect("a projectile that is not in the arena");
                        assert_eq!(*position, found.position);
                        assert_eq!(*velocity, found.velocity);
                    }
                }
            }

            // 5. The events are exactly the state's events that happened
            //    somewhere this player can see, in the order the rules produced
            //    them.
            let entitled = expected_events(&state, player, rules);
            assert_eq!(
                view.events.len(),
                entitled.len(),
                "tick {:?}, player {seat}: event count",
                state.tick()
            );
            for (expected, seen) in entitled.iter().zip(&view.events) {
                assert!(
                    same_event(expected, seen),
                    "tick {:?}, player {seat}: {seen:?} is not {expected:?}",
                    state.tick()
                );
            }

            // 6. The size bound, and the leak it exists to catch.
            let encoded = view.encode();
            assert!(
                encoded.len() <= PlayerView::MAX_ENCODED_BYTES,
                "tick {:?}, player {seat}: {} bytes exceeds the bound of {}",
                state.tick(),
                encoded.len(),
                PlayerView::MAX_ENCODED_BYTES
            );
            let leaked = view_for_with_rules(&state, player, &omniscient).encode();
            assert!(
                encoded.len() <= leaked.len(),
                "a culled view cannot be larger than an unculled one"
            );

            coverage.largest_encoding = coverage.largest_encoding.max(encoded.len());
            if encoded.len() < leaked.len() {
                coverage.ticks_smaller_than_omniscient += 1;
            }
            let all_entities = expected_visible(&state, player, &omniscient);
            coverage.entities_withheld += (all_entities.len() - expected.len()) as u64;
            coverage.events_withheld += (state.events().count() - entitled.len()) as u64;
            coverage.events_delivered += entitled.len() as u64;
        }
    }
    coverage
}

/// The exit criterion, over the 1000-tick scripted match.
#[test]
fn no_entity_outside_vision_appears_anywhere_in_a_view() {
    let coverage = audit(SEED, &script(), &RULES);

    assert_eq!(coverage.views, 6000, "six players, one thousand ticks");

    println!(
        "visibility: views={} entities_withheld={} events_withheld={} \
         events_delivered={} largest_encoding={}B bound={}B",
        coverage.views,
        coverage.entities_withheld,
        coverage.events_withheld,
        coverage.events_delivered,
        coverage.largest_encoding,
        PlayerView::MAX_ENCODED_BYTES
    );

    // The assertions above are all conditional on there being something to
    // cull. These are what make the run mean something: over the fixture the
    // projection withheld entities tens of thousands of times, withheld events,
    // delivered events, and produced a strictly smaller message than an
    // unculled projection would have on the overwhelming majority of ticks.
    assert!(
        coverage.entities_withheld > 10_000,
        "only {} entity sightings were withheld; the fixture is not exercising \
         culling and the criterion is close to vacuous",
        coverage.entities_withheld
    );
    // Events are scarcer than sightings and they happen where the fighting is,
    // which is where both teams tend to have vision — so the margin here is
    // thin by nature. The run above withholds 30 of 258; the floor is set below
    // that rather than at it, and a change that drops it to zero is a culling
    // rule that stopped culling derived signals.
    assert!(
        coverage.events_withheld >= 20,
        "only {} events were withheld",
        coverage.events_withheld
    );
    assert!(
        coverage.events_delivered > 100,
        "only {} events were delivered; culling everything is not culling",
        coverage.events_delivered
    );
    assert!(
        coverage.ticks_smaller_than_omniscient > 5_000,
        "only {} of 6000 views were smaller than their unculled counterpart",
        coverage.ticks_smaller_than_omniscient
    );
}

/// The same criterion over the duel, which is where death and respawn happen.
///
/// Deaths are the case the culling rule is most likely to get wrong: a champion
/// that died this tick is off the map, so a projection that culled by looking up
/// the entity's current position would have to invent an exception for it. This
/// fixture is what makes that an assertion rather than an argument.
#[test]
fn the_duel_is_culled_correctly_through_its_deaths_and_respawns() {
    let coverage = audit(DUEL_SEED, &duel_script(), &DUEL_RULES);
    assert!(coverage.events_delivered > 100);
    assert!(coverage.entities_withheld > 1_000);

    // And a death was actually delivered to somebody, which is the case above
    // reaching the assertion rather than being skipped.
    let mut state = new_state_with_rules(DUEL_SEED, &DUEL_RULES);
    let mut deaths_seen = 0u32;
    for inputs in &duel_script() {
        state = step_with_rules(&state, inputs, &DUEL_RULES);
        for seat in 0..PLAYER_COUNT {
            deaths_seen += view_for_with_rules(&state, PlayerId(seat as u8), &DUEL_RULES)
                .events
                .iter()
                .filter(|event| matches!(event, VisibleEvent::Death { .. }))
                .count() as u32;
        }
    }
    assert!(
        deaths_seen > 0,
        "no death was ever shown to anybody, so the hardest case is untested"
    );
}

/// `step` never reads visibility, asserted rather than reviewed.
///
/// `docs/MILESTONES.md` M2 requires it and a grep for `view` in `step.rs` is
/// not a test. This is: the whole fixture is run twice under constants that
/// differ *only* in the two vision radii, and the state digests have to be
/// identical at every checkpoint. If any rule ever consulted a vision radius —
/// a tower that only fires at what it can see, a projectile that despawns in
/// fog — the two runs would part company and this would say where.
#[test]
fn changing_the_vision_radius_cannot_move_a_single_state_digest() {
    let blind = Rules {
        champion_vision_radius: Fx::EPSILON,
        tower_vision_radius: Fx::EPSILON,
        ..RULES
    };
    assert_ne!(
        RULES.hash(),
        blind.hash(),
        "the two rule sets must differ, or this test compares a run with itself"
    );

    let log = script();
    let mut normal = new_state_with_rules(SEED, &RULES);
    let mut altered = new_state_with_rules(SEED, &blind);
    for (index, inputs) in log.iter().enumerate() {
        normal = step_with_rules(&normal, inputs, &RULES);
        altered = step_with_rules(&altered, inputs, &blind);
        assert_eq!(
            normal.digest(),
            altered.digest(),
            "the simulation diverged at tick {}, so some rule is reading vision",
            index + 1
        );
    }
}

/// Two players on opposite teams do not receive the same message, and neither
/// receives the whole world.
#[test]
fn the_two_teams_are_told_different_things() {
    let mut state = new_state(9);
    for _ in 0..30 {
        state = sim::step(&state, &[]);
    }
    let blue = view_for(&state, PlayerId(0));
    let red = view_for(&state, PlayerId(3));
    assert_ne!(blue.encode(), red.encode());

    let everything = PLAYER_COUNT + TOWER_COUNT - 1;
    assert!(blue.visible.len() < everything, "blue sees the whole board");
    assert!(red.visible.len() < everything, "red sees the whole board");
}

/// The projection does not depend on which order the caller asks in, and asking
/// twice gives the same bytes. Cheap, and it is the assumption the transport
/// will make when it starts hashing views at M3.
#[test]
fn the_projection_is_a_function_of_its_arguments() {
    let mut state = new_state(4);
    for inputs in script().iter().take(120) {
        state = sim::step(&state, inputs);
    }
    for seat in 0..PLAYER_COUNT {
        let player = PlayerId(seat as u8);
        assert_eq!(
            view_for(&state, player).encode(),
            view_for(&state, player).encode()
        );
    }
    assert_eq!(view_for(&state, PlayerId(0)).tick, state.tick());
    assert_eq!(state.tick(), Tick(120));
}
