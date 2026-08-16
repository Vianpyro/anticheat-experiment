//! A seat played by the program, so that the game can be played before nine
//! people are free.
//!
//! # What this is for, in the terms `docs/MILESTONES.md` uses
//!
//! It is a **playtest tool**. It fills the seats nobody is sitting in so that
//! one or two people can start a nine-seat match and find out whether the game
//! works: whether a fight reads on the screen, whether the prediction feels
//! right, whether a tower kills you at the range you thought it did. It does
//! **not** satisfy M4's exit criterion, which asks for three humans on two
//! operating systems and is a fact about a calendar; it produces **no corpus
//! data**, and `replay::Attested` is what makes that mechanical rather than a
//! promise; and it calibrates nothing, because there is no device behind it to
//! measure.
//!
//! # Why it is here and not in `cheat-client`
//!
//! `cheat_client::bot::Bot` exists, plays this game, and is a good read for the
//! decision logic — `follow`, in particular. It is deliberately not linked, and
//! this module reimplements what it needs instead.
//!
//! `docs/ARCHITECTURE.md` makes `cheat-client` the machine this project assumes
//! is compromised, `ci` asserts that no production binary links it, and
//! `docs/RISKS.md` R7's whole position is that the exploits stay expressed as
//! test assertions rather than as a usable tool. Handing a testers' build that
//! links the attacker would undo all three at once, for a saving of about eighty
//! lines. So the attacker keeps its bot and the client gets its own, and the two
//! are allowed to be different: this one plays to be *played against* — it
//! walks a lane, it fights, it dies and comes back — where the attacker's exists
//! to be scored by `anticheat`.
//!
//! # What it is allowed to do, and it is narrow
//!
//! It composes an [`Action`] and hands it to the caller, which sends it over the
//! protocol, one intention per tick, exactly as `client/tests/m4_exit.rs` drives
//! a seat and exactly as `client::play` composes one from a person's hands.
//!
//! **It synthesises no device input.** No `uinput`, no `SendInput`, no `XTest`,
//! no pointer, nothing that drives the operating system. `docs/RISKS.md` R7 is
//! explicit about why that line and not another: a layer that moves a real mouse
//! is game-independent by construction — it drives the OS rather than a wire —
//! so it is the one part of a bot that would be a *technique* rather than a
//! defect of this project, and it is also `docs/SCOPE.md`'s stated ceiling of
//! behavioural detection. This module's imports are the evidence, in the same
//! register R7 uses for `cheat-client`: `sim`'s rules vocabulary and this
//! crate's own view helper, and nothing else. There is no window here, no event
//! loop, and no `crate::input`.
//!
//! # And the standing order, which is the one rule it has to mirror
//!
//! [`follow`] below is a rule of `sim::step` and of nothing else: `Idle` stops a
//! champion, a `Move` replaces the standing order, an `Attack` replaces it too,
//! and a cast leaves it alone. It matters because the bot has to know what the
//! server is still holding for it when it sends a one-shot — the same reason
//! `client::play` keeps `standing` and `once` apart for a person.

use sim::view::{EntityView, PlayerView};
use sim::{Action, EntityId, Fx, FxVec2, Liveness, RULES, Seat, Team, base_position};

use crate::draw::nearest_enemy;

/// How near its destination a bot has to be to call it reached and turn round.
///
/// A champion covers `champion_speed` in a tick and a `MoveTo` order stops when
/// it arrives, so anything at or below one tick of movement would leave a bot
/// that oscillates on the spot. Two units is a couple of seconds of walking and
/// is far inside the vision radius, so turning round is not a thing that puts
/// anybody in or out of the fog.
const ARRIVED: Fx = Fx::from_int(2);

/// How far a skillshot travels before it expires.
///
/// Derived from the rules rather than written down, because it is the reach the
/// cast actually has and a balance change moves it. It is wider than the vision
/// radius, which is why the check exists at all: team vision includes what the
/// towers see, so a bot can be *shown* a champion it could not hit.
fn skillshot_reach() -> Fx {
    RULES
        .skillshot_speed
        .mul(Fx::from_int(i32::from(RULES.skillshot_lifetime_ticks)))
}

/// One seat, played by the program.
///
/// It holds what a client's session holds and nothing else: its own seat, the
/// order it believes the server is holding for it, and the two points it walks
/// between. Everything it knows about the world arrived in a [`PlayerView`] the
/// server chose to send it, so an enemy in the fog is an enemy it does not know
/// about — which is the whole reason a bot-filled match is worth playing rather
/// than only worth running.
#[derive(Clone, Debug)]
pub struct Bot {
    seat: Seat,
    /// The order the bot believes the server is holding for it.
    standing: Action,
    /// The two lane meeting points it patrols between.
    lanes: [FxVec2; 2],
    /// Which of the two it is walking to.
    lane: usize,
    /// Intentions composed, and how many of them were an attack or a cast. The
    /// second is what tells an operator the evening had a game in it rather than
    /// nine champions walking past each other — `docs/RISKS.md` R15's habit,
    /// applied to a tool.
    intentions: u32,
    fights: u32,
}

impl Bot {
    /// A bot seated here, holding position.
    ///
    /// The two points it patrols between are the **middles of its team's two
    /// lanes**, which is where the two teams contesting a lane meet: each lane
    /// joins two bases and carries one tower per contestant a quarter of the way
    /// down it from that contestant's own end, so the halfway point is the
    /// ground between the towers and is exactly where a fight happens. Both
    /// teams on a lane walk to the *same* point, so a match of these produces
    /// contact rather than nine champions on their own errands.
    #[must_use]
    pub fn new(seat: Seat) -> Self {
        let home = base_position(seat.team(), &RULES);
        let mut lanes = [home; 2];
        let mut found = 0usize;
        for team in Team::ALL {
            if team == seat.team() {
                continue;
            }
            let enemy = base_position(team, &RULES);
            // The midpoint as the average of the two ends, rather than as one
            // end plus half the way to the other. Both are the same number in
            // arithmetic and only the first is the same number in Q15.16:
            // `scale` truncates toward zero, so a half-offset added to a base
            // rounds *toward that base* and the two teams contesting a lane
            // would compute points a raw unit apart. `the_two_teams_of_a_lane_
            // walk_to_the_same_point` is what caught it.
            if let Some(slot) = lanes.get_mut(found) {
                *slot = home.add(enemy).scale(Fx::from_ratio(1, 2));
            }
            found = found.saturating_add(1);
        }
        Self {
            seat,
            standing: Action::Idle,
            lanes,
            // The three seats of a team start on opposite lanes, so a team
            // covers both of its own rather than arriving somewhere in a column.
            lane: seat.index() % 2,
            intentions: 0,
            fights: 0,
        }
    }

    /// The seat this bot is playing.
    #[must_use]
    pub const fn seat(&self) -> Seat {
        self.seat
    }

    /// Intentions composed, and how many of them were an attack or a cast.
    #[must_use]
    pub const fn counters(&self) -> (u32, u32) {
        (self.intentions, self.fights)
    }

    /// Folds in one view and answers with the intention for this tick.
    ///
    /// **Always answers.** `docs/ARCHITECTURE.md`'s one intention per tick is
    /// the traffic shape a person's client produces, and a seat that went quiet
    /// would be a seat with a different shape — which is a thing to avoid here
    /// for the dull reason rather than the interesting one: a bot that stopped
    /// talking would stop exercising the server's intake, which is half of what
    /// a playtest is for.
    pub fn observe(&mut self, view: &PlayerView) -> Action {
        let action = self.decide(view);
        self.standing = follow(self.standing, action);
        self.intentions = self.intentions.saturating_add(1);
        if matches!(
            action,
            Action::Attack(_) | Action::Skillshot(_) | Action::Targeted(_)
        ) {
            self.fights = self.fights.saturating_add(1);
        }
        action
    }

    /// What this bot wants, given what it was shown.
    fn decide(&mut self, view: &PlayerView) -> Action {
        let Liveness::Alive { .. } = view.own.liveness else {
            // Dead. `sim::step` discards every order from a dead champion and
            // clears the order on respawn, so what is sent here changes nothing
            // and the walk is re-issued on the tick it comes back.
            return self.standing;
        };
        let own = view.own.position;

        // Deliberately no retreat when hurt. Nothing in the rules restores hit
        // points short of dying — `grant_respawns` is the only thing that hands
        // any back — so a bot that walked home at a quarter health would walk
        // home once and stay there. Dying and coming back is both the honest
        // reading of the rules and the more useful one to play against: it puts
        // deaths, respawn timers and an empty lane into a playtest.
        if let Some((target, at)) = self.nearest_seen(view, own, RULES.champion_vision_radius) {
            // The one-shots first, because they leave the standing order alone
            // and the walk survives them — the same order `client::play` puts a
            // person's abilities in.
            if view.own.cooldowns.targeted == 0 && at.within_range(own, RULES.targeted_range) {
                return Action::Targeted(target);
            }
            if view.own.cooldowns.skillshot == 0 && at.within_range(own, skillshot_reach()) {
                let direction = at.sub(own);
                // Shorter than the rules will normalise is a cast `sim` discards
                // without spending the cooldown; sending it anyway would be a
                // bot that throws away its intention for the tick.
                if direction.length() >= RULES.min_direction_length {
                    return Action::Skillshot(direction);
                }
            }
            return Action::Attack(target);
        }

        // Nothing in sight: walk. Reaching the point turns the bot round, so a
        // bot with nobody to fight patrols its team's two lanes instead of
        // standing on a spot nobody comes to.
        if own.within_range(self.lanes[self.lane], ARRIVED) {
            self.lane = 1usize.saturating_sub(self.lane);
        }
        Action::Move(self.lanes[self.lane])
    }

    /// The nearest enemy champion within `reach` of a point, and where it was
    /// seen.
    ///
    /// `crate::draw::nearest_enemy` is the choice — the same function a person's
    /// right-click resolves through, so a bot cannot pick a target a player
    /// could not — and this looks the position up afterwards because a skillshot
    /// needs a direction and a handle is not one. It reads `view.visible`, which
    /// is already culled, so there is no way for this to name something the fog
    /// was hiding.
    fn nearest_seen(
        &self,
        view: &PlayerView,
        from: FxVec2,
        reach: Fx,
    ) -> Option<(EntityId, FxVec2)> {
        let id = nearest_enemy(view, self.seat, from, reach)?;
        view.visible.iter().find_map(|entity| match *entity {
            EntityView::Champion {
                id: seen, position, ..
            } if seen == id => Some((id, position)),
            _ => None,
        })
    }
}

/// The standing order after an action.
///
/// A rule of `sim::step` mirrored here, and it is mirrored rather than imported
/// because there is nothing to import: `apply_inputs` sets an `Order` inside a
/// `State` this client never holds. `Idle` stops the champion, a `Move` replaces
/// the order, an `Attack` replaces it too — `Order::Attack` is a standing order
/// that walks into range and keeps hitting — and the two casts are one-shots
/// that leave it alone.
///
/// **It differs from `cheat_client::bot::follow` in exactly one arm**, and the
/// difference is not a disagreement about the rules: that bot groups `Attack`
/// with the casts because it re-sends its attack every tick and never has to
/// know what the server is holding, and this one walks somewhere afterwards and
/// does.
const fn follow(standing: Action, action: Action) -> Action {
    match action {
        Action::Idle | Action::Move(_) | Action::Attack(_) => action,
        Action::Skillshot(_) | Action::Targeted(_) => standing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cast_leaves_the_standing_order_alone_and_a_move_replaces_it() {
        let walk = Action::Move(FxVec2::new(Fx::from_int(1), Fx::from_int(2)));
        assert_eq!(follow(Action::Idle, walk), walk);
        assert_eq!(
            follow(walk, Action::Skillshot(FxVec2::new(Fx::ONE, Fx::ZERO))),
            walk
        );
        assert_eq!(follow(walk, Action::Targeted(EntityId(4))), walk);
        assert_eq!(
            follow(walk, Action::Attack(EntityId(4))),
            Action::Attack(EntityId(4))
        );
        assert_eq!(follow(walk, Action::Idle), Action::Idle);
    }

    #[test]
    fn the_two_teams_of_a_lane_walk_to_the_same_point() {
        // The property the patrol rests on: a lane's meeting point is the
        // midpoint of two bases, so Blue's Red-lane point and Red's Blue-lane
        // point are the same square of the map. A bot that walked to a point
        // derived from its own base alone would produce a match in which nobody
        // meets anybody.
        let blue = Bot::new(Seat::Blue0);
        let red = Bot::new(Seat::Red0);
        let shared = blue
            .lanes
            .iter()
            .filter(|point| red.lanes.contains(point))
            .count();
        assert_eq!(
            shared, 1,
            "Blue's lane points are {:?} and Red's are {:?}",
            blue.lanes, red.lanes
        );
    }

    #[test]
    fn a_bot_with_nothing_in_sight_walks_and_turns_round_when_it_arrives() {
        let mut bot = Bot::new(Seat::Blue0);
        let destination = bot.lanes[bot.lane];
        let mut view = view_at(base_position(Team::Blue, &RULES));
        assert_eq!(bot.decide(&view), Action::Move(destination));

        // Standing on it: the next intention is for the other lane.
        view.own.position = destination;
        let turned = bot.decide(&view);
        assert_eq!(turned, Action::Move(bot.lanes[bot.lane]));
        assert_ne!(turned, Action::Move(destination));
    }

    /// A view of an empty world, from a champion standing at `position`.
    fn view_at(position: FxVec2) -> PlayerView {
        PlayerView {
            tick: sim::Tick(1),
            outcome: sim::Outcome::InProgress,
            own: sim::view::OwnView {
                id: EntityId(0),
                position,
                liveness: Liveness::Alive {
                    hp: RULES.champion_max_hp,
                },
                cooldowns: sim::Cooldowns::default(),
            },
            visible: Vec::new(),
            events: Vec::new(),
        }
    }
}
