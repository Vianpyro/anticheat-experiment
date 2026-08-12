//! The transition function: one state and a slice of inputs to the next state.
//!
//! # Contract
//!
//! `step` is pure. No clock, no I/O, no async, no floats, no allocation beyond
//! the returned state, and no source of variation other than its two arguments.
//! Everything it reads is either in the state, in the slice, or in
//! [`crate::RULES`].
//!
//! [`step_with_rules`] is the same function with the constants passed in. It is
//! not a configuration hook and the game has exactly one set of rules; it exists
//! so that a fixture which needs to reach a rule the default constants make slow
//! to reach — death and respawn inside thirty seconds, say — carries its own
//! [`Rules`] value instead of the alternative, which is editing the game's
//! balance until a test passes and leaving a number nobody can explain. The two
//! sets are told apart by [`Rules::hash`], so a digest recorded under one is
//! never silently compared against the other.
//!
//! Inputs arrive pre-sorted by `(player, seq)`; `step` neither sorts nor
//! deduplicates, because sorting is the caller's job and doing it here would
//! hide a caller that got it wrong. It does ignore inputs whose `tick` is not
//! the state's own, which makes a mis-bucketed or replayed input a no-op rather
//! than an effect at the wrong moment.
//!
//! # Order of operations
//!
//! The order below is part of the rules, not an implementation detail: change
//! it and every recorded replay resimulates differently. It is fixed here so
//! that it is written down in one place rather than inferred from the source.
//!
//! 1. **Cooldowns** tick down. Before inputs, so an ability whose cooldown
//!    expires this tick can be cast this tick.
//! 2. **Respawns** are granted. Before inputs, for the same reason.
//! 3. **Inputs** are applied in slice order. Casting the targeted spell deals
//!    its damage here, immediately.
//! 4. **Champions move**, each toward its standing order's destination.
//! 5. **Projectiles advance**, then check for a hit at their new position, then
//!    expire.
//! 6. **Towers fire**, in tower order.
//! 7. **Basic attacks resolve**, in seat order.
//! 8. **Deaths are resolved**, once, after every source of damage. A champion
//!    reduced to zero earlier in the tick therefore still acts during it. That
//!    is a rule, and the alternative — resolving after each damage source —
//!    would make the outcome depend on the order damage happened to arrive in.
//! 9. **The outcome** is evaluated.
//! 10. **The tick** advances by exactly one.
//!
//! Every loop iterates a fixed-size array by index. There is no iteration over
//! a hash map, no sort with a non-total comparator, and no parallelism —
//! `docs/RISKS.md` R9 is about exactly this, and the `clippy.toml` beside this
//! file is what keeps it true.

use crate::event::{Ability, Event, EventKind};
use crate::fx::Fx;
use crate::input::{Action, Input};
use crate::rules::{RULES, Rules};
use crate::state::{
    Cooldowns, EntityId, EntityRef, Liveness, MAX_PROJECTILES, Order, Outcome, PLAYER_COUNT,
    PlayerId, Projectile, State, TOWER_COUNT, Team, Tick, Tower, champion_entity_id, entity_id,
    entity_team, spawn_position, tower_position, tower_team,
};
use crate::vec2::FxVec2;

/// Advances the world by one tick, under [`crate::RULES`].
///
/// See the module documentation for the order of operations, which is part of
/// the rules. No input can make this function panic or produce a state outside
/// the legal domain; that is asserted by the property tests rather than assumed.
#[must_use]
pub fn step(state: &State, inputs: &[Input]) -> State {
    step_with_rules(state, inputs, &RULES)
}

/// Advances the world by one tick under the constants given.
///
/// Identical in every respect to [`step`] except where it reads a constant. See
/// the module documentation for why this exists and what it is not for.
#[must_use]
pub fn step_with_rules(state: &State, inputs: &[Input], rules: &Rules) -> State {
    let mut next = state.clone();

    // The events belong to one transition, not to a history, so the record of
    // the previous tick is dropped before this one starts. Before the
    // decided-match return as well: a frozen match announces nothing, and a
    // client that kept receiving the last tick's kills forever would be reading
    // an event stream that had stopped meaning anything.
    next.events.clear();

    // A decided match still advances its tick — the server keeps sending views
    // until everyone disconnects — but no rule runs, so the final state is
    // stable and the digest of tick 900 and tick 1000 of a match decided on
    // tick 800 differ only in the tick.
    if matches!(next.outcome, Outcome::Decided { .. }) {
        next.tick = Tick(next.tick.0.saturating_add(1));
        return next;
    }

    tick_cooldowns(&mut next);
    grant_respawns(&mut next, rules);
    apply_inputs(&mut next, inputs, rules);
    move_champions(&mut next, rules);
    advance_projectiles(&mut next, rules);
    fire_towers(&mut next, rules);
    resolve_basic_attacks(&mut next, rules);
    resolve_deaths(&mut next, rules);
    decide_outcome(&mut next);

    next.tick = Tick(next.tick.0.saturating_add(1));
    next
}

/// Records something that happened, at the place it happened.
///
/// The position is the culling key the visibility projection uses, so it is a
/// parameter rather than something derived later: by the end of the tick a
/// champion that died has been moved back to its spawn point, and the place it
/// died at exists nowhere but here.
fn emit(state: &mut State, at: FxVec2, kind: EventKind) {
    state.events.push(Event { kind, at });
}

fn tick_cooldowns(state: &mut State) {
    for champion in &mut state.champions {
        champion.cooldowns.basic_attack = champion.cooldowns.basic_attack.saturating_sub(1);
        champion.cooldowns.skillshot = champion.cooldowns.skillshot.saturating_sub(1);
        champion.cooldowns.targeted = champion.cooldowns.targeted.saturating_sub(1);
    }
    for tower in &mut state.towers {
        tower.cooldown = tower.cooldown.saturating_sub(1);
    }
}

fn grant_respawns(state: &mut State, rules: &Rules) {
    let now = state.tick;
    for (index, champion) in state.champions.iter_mut().enumerate() {
        let Liveness::Dead { respawn_at } = champion.liveness else {
            continue;
        };
        if now < respawn_at {
            continue;
        }
        champion.liveness = Liveness::Alive {
            hp: rules.champion_max_hp,
        };
        champion.position = spawn_position(index, rules);
        champion.order = Order::Idle;
        // Cooldowns are cleared on respawn. Fifteen seconds of death is longer
        // than every cooldown in the game except the skillshot's, so the rule
        // mostly matters for that one; it is stated rather than left to be
        // discovered.
        champion.cooldowns = Cooldowns::default();
    }
}

fn apply_inputs(state: &mut State, inputs: &[Input], rules: &Rules) {
    let now = state.tick;
    for input in inputs {
        if input.tick != now {
            continue;
        }
        let Some(seat) = seat_index(input.player) else {
            continue;
        };
        let Some(champion) = state.champions.get(seat).copied() else {
            continue;
        };
        if !matches!(champion.liveness, Liveness::Alive { .. }) {
            continue;
        }

        match input.action {
            Action::Idle => set_order(state, seat, Order::Idle),
            Action::Move(destination) => set_order(
                state,
                seat,
                Order::MoveTo(destination.clamp_components(rules.map_half_extent)),
            ),
            Action::Attack(target) => {
                // An order to attack an ally, a corpse, a rubble heap or an
                // entity that never existed is discarded, leaving the previous
                // order in place. Storing it and resolving it later would let
                // an attacker write arbitrary handles into the state.
                if let Some(entity) = state.resolve(target)
                    && entity_team(entity) != Team::of_player(input.player)
                {
                    set_order(state, seat, Order::Attack(target));
                }
            }
            Action::Skillshot(direction) => cast_skillshot(state, seat, direction, rules),
            Action::Targeted(target) => cast_targeted(state, seat, target, rules),
        }
    }
}

fn set_order(state: &mut State, seat: usize, order: Order) {
    if let Some(champion) = state.champions.get_mut(seat) {
        champion.order = order;
    }
}

fn cast_skillshot(state: &mut State, seat: usize, direction: FxVec2, rules: &Rules) {
    let Some(champion) = state.champions.get(seat).copied() else {
        return;
    };
    if champion.cooldowns.skillshot != 0 {
        return;
    }
    // Too short to normalise accurately; the cast is discarded and the cooldown
    // is not consumed. See `FxVec2::normalize_or_zero`.
    if direction.length() < rules.min_direction_length {
        return;
    }
    // Reserve the slot before the handle, so a cast that finds a full arena
    // does not burn an `EntityId` and change the state for nothing.
    if !state.projectiles.has_free_slot() {
        return;
    }

    let id = state.allocate_projectile_id();
    let placed = state.projectiles.insert(Projectile {
        id,
        owner: PlayerId(seat as u8),
        position: champion.position,
        velocity: direction.normalize_or_zero().scale(rules.skillshot_speed),
        remaining: rules.skillshot_lifetime_ticks,
    });
    if placed {
        if let Some(caster) = state.champions.get_mut(seat) {
            caster.cooldowns.skillshot = rules.skillshot_cooldown_ticks;
        }
        // Announced from where it was cast, not from where the projectile is:
        // the cue a nearby client gets is the champion's gesture, and the
        // projectile is a visible entity in its own right from the next tick on.
        emit(
            state,
            champion.position,
            EventKind::Cast {
                caster: champion_entity_id(seat),
                ability: Ability::Skillshot,
            },
        );
    }
}

fn cast_targeted(state: &mut State, seat: usize, target: EntityId, rules: &Rules) {
    let Some(champion) = state.champions.get(seat).copied() else {
        return;
    };
    if champion.cooldowns.targeted != 0 {
        return;
    }
    let Some(entity) = state.resolve(target) else {
        return;
    };
    if entity_team(entity) == Team::of_player(PlayerId(seat as u8)) {
        return;
    }
    if !champion
        .position
        .within_range(state.entity_position(entity, rules), rules.targeted_range)
    {
        return;
    }

    emit(
        state,
        champion.position,
        EventKind::Cast {
            caster: champion_entity_id(seat),
            ability: Ability::Targeted,
        },
    );
    damage(state, entity, rules.targeted_damage, rules);
    if let Some(caster) = state.champions.get_mut(seat) {
        caster.cooldowns.targeted = rules.targeted_cooldown_ticks;
    }
}

fn move_champions(state: &mut State, rules: &Rules) {
    for seat in 0..PLAYER_COUNT {
        let Some(champion) = state.champions.get(seat).copied() else {
            continue;
        };
        if !matches!(champion.liveness, Liveness::Alive { .. }) {
            continue;
        }

        let destination = match champion.order {
            Order::Idle => None,
            Order::MoveTo(point) => Some(point),
            // An attack order is also a move order until the target is in
            // range, which is what makes right-clicking an enemy work.
            Order::Attack(target) => match state.resolve(target) {
                Some(entity) => {
                    let target_position = state.entity_position(entity, rules);
                    if champion
                        .position
                        .within_range(target_position, rules.attack_range)
                    {
                        None
                    } else {
                        Some(target_position)
                    }
                }
                None => None,
            },
        };

        let Some(destination) = destination else {
            continue;
        };
        let moved = champion
            .position
            .step_toward(destination, rules.champion_speed)
            .clamp_components(rules.map_half_extent);

        let Some(champion) = state.champions.get_mut(seat) else {
            continue;
        };
        champion.position = moved;
        if matches!(champion.order, Order::MoveTo(point) if point == moved) {
            champion.order = Order::Idle;
        }
    }
}

fn advance_projectiles(state: &mut State, rules: &Rules) {
    for slot in 0..MAX_PROJECTILES {
        let Some(mut projectile) = state.projectiles.slots.get(slot).copied().flatten() else {
            continue;
        };
        projectile.position = projectile.position.add(projectile.velocity);
        projectile.remaining = projectile.remaining.saturating_sub(1);

        if let Some(hit) = first_hit(state, &projectile, rules) {
            damage(state, hit, rules.skillshot_damage, rules);
            clear_slot(state, slot);
            continue;
        }
        if projectile.remaining == 0 || outside_map(projectile.position, rules) {
            clear_slot(state, slot);
            continue;
        }
        if let Some(occupant) = state.projectiles.slots.get_mut(slot) {
            *occupant = Some(projectile);
        }
    }
}

/// The first enemy the projectile overlaps: champions in seat order, then
/// towers in tower order.
///
/// "First" rather than "nearest" on purpose. Nearest needs a tie-break for two
/// entities at equal distance, and a tie-break is one more rule to state and
/// get identical on three platforms; index order already is a total order and
/// costs nothing to agree on.
fn first_hit(state: &State, projectile: &Projectile, rules: &Rules) -> Option<EntityRef> {
    let owner_team = Team::of_player(projectile.owner);

    for seat in 0..PLAYER_COUNT {
        if Team::of_player(PlayerId(seat as u8)) == owner_team {
            continue;
        }
        let Some(champion) = state.champions.get(seat) else {
            continue;
        };
        if !matches!(champion.liveness, Liveness::Alive { .. }) {
            continue;
        }
        let reach = rules.skillshot_radius.add(rules.champion_radius);
        if projectile.position.within_range(champion.position, reach) {
            return Some(EntityRef::Champion(seat));
        }
    }

    for index in 0..TOWER_COUNT {
        if tower_team(index) == owner_team {
            continue;
        }
        let Some(tower) = state.towers.get(index) else {
            continue;
        };
        if !tower.is_standing() {
            continue;
        }
        let reach = rules.skillshot_radius.add(rules.tower_radius);
        if projectile
            .position
            .within_range(tower_position(index, rules), reach)
        {
            return Some(EntityRef::Tower(index));
        }
    }

    None
}

fn clear_slot(state: &mut State, slot: usize) {
    if let Some(occupant) = state.projectiles.slots.get_mut(slot) {
        *occupant = None;
    }
}

fn outside_map(position: FxVec2, rules: &Rules) -> bool {
    position.x.abs() > rules.map_half_extent || position.y.abs() > rules.map_half_extent
}

fn fire_towers(state: &mut State, rules: &Rules) {
    for index in 0..TOWER_COUNT {
        let Some(tower) = state.towers.get(index).copied() else {
            continue;
        };
        if !tower.is_standing() || tower.cooldown != 0 {
            continue;
        }

        let Some(seat) = lowest_enemy_in_range(
            state,
            tower_team(index).opponent(),
            tower_position(index, rules),
            rules.tower_range,
        ) else {
            continue;
        };

        damage(state, EntityRef::Champion(seat), rules.tower_damage, rules);
        if let Some(tower) = state.towers.get_mut(index) {
            tower.cooldown = rules.tower_cooldown_ticks;
        }
    }
}

/// The lowest-numbered seat of `team` that is alive and within `radius`.
///
/// Lowest seat rather than nearest, lowest health, or most recent aggressor.
/// All four are defensible game design and this one is a fixture, so the
/// tie-break that needs no second rule wins.
fn lowest_enemy_in_range(state: &State, team: Team, origin: FxVec2, radius: Fx) -> Option<usize> {
    for seat in 0..PLAYER_COUNT {
        if Team::of_player(PlayerId(seat as u8)) != team {
            continue;
        }
        let Some(champion) = state.champions.get(seat) else {
            continue;
        };
        if !matches!(champion.liveness, Liveness::Alive { .. }) {
            continue;
        }
        if origin.within_range(champion.position, radius) {
            return Some(seat);
        }
    }
    None
}

fn resolve_basic_attacks(state: &mut State, rules: &Rules) {
    for seat in 0..PLAYER_COUNT {
        let Some(champion) = state.champions.get(seat).copied() else {
            continue;
        };
        if !matches!(champion.liveness, Liveness::Alive { .. })
            || champion.cooldowns.basic_attack != 0
        {
            continue;
        }
        let Order::Attack(target) = champion.order else {
            continue;
        };
        let Some(entity) = state.resolve(target) else {
            continue;
        };
        if entity_team(entity) == Team::of_player(PlayerId(seat as u8)) {
            continue;
        }
        if !champion
            .position
            .within_range(state.entity_position(entity, rules), rules.attack_range)
        {
            continue;
        }

        damage(state, entity, rules.attack_damage, rules);
        if let Some(attacker) = state.champions.get_mut(seat) {
            attacker.cooldowns.basic_attack = rules.attack_cooldown_ticks;
        }
    }
}

/// Applies damage, clamping at zero, and records it.
///
/// The clamp is not cosmetic: without it a champion killed by a tower shot and
/// the same champion killed by a skillshot would sit at different negative hit
/// points, and two matches that played out identically in every visible respect
/// would produce different digests.
///
/// Every source of damage in the rules goes through here, which is why the
/// event is emitted here too: a damage path that forgot to announce itself
/// would be a hit a client never learns about, and the way to make that
/// impossible is to leave exactly one place where damage can be applied.
fn damage(state: &mut State, target: EntityRef, amount: Fx, rules: &Rules) {
    let at = state.entity_position(target, rules);
    emit(
        state,
        at,
        EventKind::Damage {
            target: entity_id(target),
            amount,
        },
    );
    match target {
        EntityRef::Champion(seat) => {
            if let Some(champion) = state.champions.get_mut(seat)
                && let Liveness::Alive { hp } = champion.liveness
            {
                champion.liveness = Liveness::Alive {
                    hp: hp.sub(amount).max(Fx::ZERO),
                };
            }
        }
        EntityRef::Tower(index) => {
            if let Some(tower) = state.towers.get_mut(index) {
                tower.hp = tower.hp.sub(amount).max(Fx::ZERO);
            }
        }
    }
}

fn resolve_deaths(state: &mut State, rules: &Rules) {
    let now = state.tick;
    let respawn_at = Tick(now.0.saturating_add(u32::from(rules.respawn_ticks)));
    let mut died = [None; PLAYER_COUNT];
    for (seat, champion) in state.champions.iter_mut().enumerate() {
        let Liveness::Alive { hp } = champion.liveness else {
            continue;
        };
        if hp.to_raw() > 0 {
            continue;
        }
        // Where it fell, kept before the body is moved. This is the only place
        // that position exists, and the visibility projection needs it: a death
        // is shown to whoever could see the spot it happened at, which is not
        // the same set as whoever can see the spawn point.
        died[seat] = Some(champion.position);
        champion.liveness = Liveness::Dead { respawn_at };
        champion.order = Order::Idle;
        // The body returns to the spawn point immediately rather than lying
        // where it fell. A corpse at the place of death is state that no rule
        // reads but the digest still covers, and it would be a position the fog
        // projection has to remember to hide.
        champion.position = spawn_position(seat, rules);
    }
    for (seat, at) in died.iter().enumerate() {
        let Some(at) = *at else {
            continue;
        };
        emit(
            state,
            at,
            EventKind::Death {
                entity: champion_entity_id(seat),
            },
        );
    }
}

fn decide_outcome(state: &mut State) {
    if matches!(state.outcome, Outcome::Decided { .. }) {
        return;
    }

    let mut blue_stands = false;
    let mut red_stands = false;
    for (index, tower) in state.towers.iter().enumerate() {
        if !Tower::is_standing(*tower) {
            continue;
        }
        match tower_team(index) {
            Team::Blue => blue_stands = true,
            Team::Red => red_stands = true,
        }
    }

    let winner = match (blue_stands, red_stands) {
        (true, true) => return,
        (true, false) => Team::Blue,
        (false, true) => Team::Red,
        // Both sides losing their last tower on the same tick is reachable —
        // two projectiles land in the same tick — and needs an answer that is
        // a rule rather than whichever loop happened to run last. Blue is
        // awarded it. Arbitrary, stated, and identical on every platform.
        (false, false) => Team::Blue,
    };
    state.outcome = Outcome::Decided {
        winner,
        at: state.tick,
    };
}

const fn seat_index(player: PlayerId) -> Option<usize> {
    let seat = player.0 as usize;
    if seat < PLAYER_COUNT {
        Some(seat)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::step;
    use crate::fx::Fx;
    use crate::input::{Action, Input};
    use crate::rules::RULES;
    use crate::state::{
        EntityId, Liveness, Order, Outcome, PlayerId, State, Team, Tick, new_state, tower_position,
    };
    use crate::vec2::FxVec2;

    fn input(tick: u32, player: u8, action: Action) -> Input {
        Input {
            tick: Tick(tick),
            seq: 0,
            player: PlayerId(player),
            action,
        }
    }

    fn run(mut state: State, ticks: u32, inputs: &[Input]) -> State {
        for _ in 0..ticks {
            state = step(&state, inputs);
        }
        state
    }

    fn hp_of(state: &State, seat: usize) -> Fx {
        match state.champions[seat].liveness {
            Liveness::Alive { hp } => hp,
            Liveness::Dead { .. } => Fx::ZERO,
        }
    }

    #[test]
    fn tick_advances_by_exactly_one() {
        let state = new_state(7);
        assert_eq!(step(&state, &[]).tick(), Tick(1));
    }

    #[test]
    fn step_does_not_mutate_its_argument() {
        let state = new_state(7);
        let before = state.digest();
        let _ = step(&state, &[input(0, 0, Action::Move(FxVec2::ZERO))]);
        assert_eq!(state.digest(), before);
    }

    #[test]
    fn an_input_for_another_tick_is_ignored() {
        let state = new_state(7);
        let applied = step(&state, &[input(0, 0, Action::Move(FxVec2::ZERO))]);
        let ignored = step(&state, &[input(9, 0, Action::Move(FxVec2::ZERO))]);
        assert_ne!(applied.digest(), ignored.digest());
        assert_eq!(ignored.champions[0].order, Order::Idle);
    }

    #[test]
    fn a_move_order_walks_and_then_stops() {
        let state = new_state(7);
        let start = state.champions[0].position;
        let destination = start.add(FxVec2::new(Fx::from_int(1), Fx::ZERO));
        let state = step(&state, &[input(0, 0, Action::Move(destination))]);
        assert!(state.champions[0].position.x > start.x);

        // One unit at 6 units per second is 5 ticks, plus slack for truncation.
        let state = run(state, 10, &[]);
        assert_eq!(state.champions[0].position, destination);
        assert_eq!(state.champions[0].order, Order::Idle);
    }

    #[test]
    fn a_destination_outside_the_map_is_clamped_not_rejected() {
        let state = new_state(7);
        let far = FxVec2::new(Fx::from_int(30_000), Fx::from_int(-30_000));
        let state = step(&state, &[input(0, 0, Action::Move(far))]);
        let Order::MoveTo(point) = state.champions[0].order else {
            panic!("expected a move order");
        };
        assert_eq!(point.x, RULES.map_half_extent);
        assert_eq!(point.y, RULES.map_half_extent.neg());
    }

    #[test]
    fn attacking_an_ally_or_a_ghost_leaves_the_order_alone() {
        let state = new_state(7);
        let state = step(&state, &[input(0, 0, Action::Attack(EntityId(1)))]);
        assert_eq!(state.champions[0].order, Order::Idle, "ally");
        let state = step(&state, &[input(1, 0, Action::Attack(EntityId(9999)))]);
        assert_eq!(state.champions[0].order, Order::Idle, "unknown handle");
    }

    #[test]
    fn a_basic_attack_needs_range_and_respects_its_cooldown() {
        let mut state = new_state(7);
        state.place_champion(0, FxVec2::ZERO, RULES.champion_max_hp);
        state.place_champion(3, FxVec2::new(Fx::ONE, Fx::ZERO), RULES.champion_max_hp);

        let orders = [input(0, 0, Action::Attack(EntityId(3)))];
        let state = step(&state, &orders);
        assert_eq!(
            hp_of(&state, 3),
            RULES.champion_max_hp.sub(RULES.attack_damage)
        );

        // The next tick is inside the cooldown, so nothing further lands.
        let state = step(&state, &[]);
        assert_eq!(
            hp_of(&state, 3),
            RULES.champion_max_hp.sub(RULES.attack_damage)
        );

        // And after it expires, exactly one more attack lands.
        let state = run(state, u32::from(RULES.attack_cooldown_ticks), &[]);
        assert_eq!(
            hp_of(&state, 3),
            RULES
                .champion_max_hp
                .sub(RULES.attack_damage)
                .sub(RULES.attack_damage)
        );
    }

    #[test]
    fn a_skillshot_flies_and_hits_the_first_enemy_in_its_path() {
        let mut state = new_state(7);
        state.place_champion(0, FxVec2::ZERO, RULES.champion_max_hp);
        state.place_champion(
            3,
            FxVec2::new(Fx::from_int(5), Fx::ZERO),
            RULES.champion_max_hp,
        );

        let cast = [input(
            0,
            0,
            Action::Skillshot(FxVec2::new(Fx::ONE, Fx::ZERO)),
        )];
        let state = step(&state, &cast);
        assert_eq!(state.projectiles().count(), 1);
        assert_eq!(
            state.champions[0].cooldowns.skillshot,
            RULES.skillshot_cooldown_ticks
        );

        // Five units at 18 units per second is under ten ticks.
        let state = run(state, 12, &[]);
        assert_eq!(state.projectiles().count(), 0, "consumed on hit");
        assert_eq!(
            hp_of(&state, 3),
            RULES.champion_max_hp.sub(RULES.skillshot_damage)
        );
    }

    #[test]
    fn a_skillshot_too_short_to_aim_is_discarded_without_cost() {
        let state = new_state(7);
        let nudge = FxVec2::new(Fx::EPSILON, Fx::ZERO);
        let state = step(&state, &[input(0, 0, Action::Skillshot(nudge))]);
        assert_eq!(state.projectiles().count(), 0);
        assert_eq!(state.champions[0].cooldowns.skillshot, 0, "cooldown intact");
    }

    #[test]
    fn a_skillshot_does_not_hit_the_caster_or_their_team() {
        let mut state = new_state(7);
        state.place_champion(0, FxVec2::ZERO, RULES.champion_max_hp);
        state.place_champion(
            1,
            FxVec2::new(Fx::from_int(3), Fx::ZERO),
            RULES.champion_max_hp,
        );
        let cast = [input(
            0,
            0,
            Action::Skillshot(FxVec2::new(Fx::ONE, Fx::ZERO)),
        )];
        let state = run(step(&state, &cast), 12, &[]);
        assert_eq!(hp_of(&state, 0), RULES.champion_max_hp);
        assert_eq!(hp_of(&state, 1), RULES.champion_max_hp);
    }

    #[test]
    fn the_targeted_spell_needs_range() {
        let mut state = new_state(7);
        state.place_champion(0, FxVec2::ZERO, RULES.champion_max_hp);
        state.place_champion(
            3,
            FxVec2::new(RULES.targeted_range.add(Fx::ONE), Fx::ZERO),
            RULES.champion_max_hp,
        );

        let cast = [input(0, 0, Action::Targeted(EntityId(3)))];
        let out_of_range = step(&state, &cast);
        assert_eq!(hp_of(&out_of_range, 3), RULES.champion_max_hp);
        assert_eq!(out_of_range.champions[0].cooldowns.targeted, 0);

        state.place_champion(3, FxVec2::new(Fx::ONE, Fx::ZERO), RULES.champion_max_hp);
        let in_range = step(&state, &cast);
        assert_eq!(
            hp_of(&in_range, 3),
            RULES.champion_max_hp.sub(RULES.targeted_damage)
        );
        assert_eq!(
            in_range.champions[0].cooldowns.targeted,
            RULES.targeted_cooldown_ticks
        );
    }

    #[test]
    fn a_tower_shoots_the_lowest_seat_in_range_and_a_champion_dies_and_respawns() {
        let mut state = new_state(7);
        // Red seats 3 and 4 both stand on Blue's outer tower; seat 3 is shot.
        let under_tower = tower_position(0, &RULES);
        state.place_champion(3, under_tower, RULES.tower_damage);
        state.place_champion(4, under_tower, RULES.champion_max_hp);

        let state = step(&state, &[]);
        assert!(
            matches!(state.champions[3].liveness, Liveness::Dead { .. }),
            "seat 3 took the shot"
        );
        assert_eq!(hp_of(&state, 4), RULES.champion_max_hp, "seat 4 untouched");
        assert_eq!(
            state.champions[3].position,
            crate::state::spawn_position(3, &RULES),
            "the body returns to spawn"
        );

        let state = run(state, u32::from(RULES.respawn_ticks), &[]);
        assert_eq!(hp_of(&state, 3), RULES.champion_max_hp, "respawned");
    }

    #[test]
    fn destroying_both_enemy_towers_decides_the_match_and_freezes_it() {
        let mut state = new_state(7);
        state.set_tower_hp(2, Fx::ZERO);
        state.set_tower_hp(3, Fx::ZERO);
        let state = step(&state, &[]);
        assert_eq!(
            state.outcome(),
            Outcome::Decided {
                winner: Team::Blue,
                at: Tick(0)
            }
        );

        // Once decided, only the tick moves.
        let frozen = step(&state, &[input(1, 0, Action::Move(FxVec2::ZERO))]);
        assert_eq!(frozen.tick(), Tick(2));
        assert_eq!(frozen.champions, state.champions);
    }

    #[test]
    fn a_dead_champion_takes_no_orders() {
        let mut state = new_state(7);
        state.place_champion(0, FxVec2::ZERO, Fx::EPSILON);
        state.place_champion(3, FxVec2::new(Fx::ONE, Fx::ZERO), RULES.champion_max_hp);
        let state = step(&state, &[input(0, 3, Action::Attack(EntityId(0)))]);
        let state = run(state, 2, &[]);
        assert!(matches!(state.champions[0].liveness, Liveness::Dead { .. }));

        let ignored = step(
            &state,
            &[input(state.tick().0, 0, Action::Move(FxVec2::ZERO))],
        );
        assert_eq!(ignored.champions[0].order, Order::Idle);
    }
}
