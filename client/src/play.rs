//! The input stage: device events in, one intention per tick out.
//!
//! This is where the two paths of [`crate::input`] meet the game. A device event
//! is recorded verbatim and integrated into the aim; a frame from the server
//! asks for the intention that has accumulated since the last one. Nothing here
//! reads a clock, opens anything or talks to a window — [`crate::gfx`] is the
//! half that does, and it is deliberately dull for the same reason the terminal
//! client's `term` module was.
//!
//! # Why the viewport lives in this type at all
//!
//! [`Play`] holds a [`Viewport`], which is a rendering quantity, next to an
//! [`Aim`] and an [`InputTrace`], which must never depend on one. That is not an
//! accident of layout: it is what makes the property testable. If the viewport
//! were unreachable from the capture path by construction, the assertion would
//! be "this code does not compile", which is worth something but is not a test
//! anybody can run against a *changed* version of the code. With both fields in
//! one struct, `client/tests/capture.rs` can drive the same device events
//! through two `Play`s that differ only in window size and require the recorded
//! trace and the resulting aim to be identical — a property that stays red for
//! any future edit that reaches for `self.viewport` inside [`Play::moved`].
//!
//! # One intention per tick, and `Idle` is not silence
//!
//! The loop sends exactly one intention for every frame it receives, which is
//! the protocol's shape and the traffic under which [`crate::predict`] is exact.
//! What it sends when the player is doing nothing is the *standing* intention
//! repeated, not `Action::Idle`: idling is a rule that stops the champion, so a
//! client that sent it every tick would cancel its own walk on the tick after it
//! started. One-shot abilities are sent once and leave the standing order alone,
//! exactly as `sim::step` treats them.

use sim::view::PlayerView;
use sim::{Action, Fx, FxVec2, RULES, Seat};

use crate::draw::{Viewport, nearest_enemy};
use crate::input::{Aim, Control, InputTrace};
use crate::lobby::{Element, Lobby};

/// What a control press needs to know about the world to become an order.
///
/// Passed in at the press rather than held, because it is a fact about the last
/// frame and [`Play`] holds no world of its own.
#[derive(Clone, Copy, Debug)]
pub struct Aiming<'a> {
    /// The last view, which is the only thing a click may consult.
    pub view: &'a PlayerView,
    /// This client's seat.
    pub seat: Seat,
    /// The predicted position of this player's champion.
    pub own: FxVec2,
}

/// The input stage of the playable client.
#[derive(Clone, Debug)]
pub struct Play {
    /// The aim, integrated from raw device deltas.
    aim: Aim,
    /// Every device event, verbatim.
    trace: InputTrace,
    /// The window. **Rendering only** — nothing that touches `aim` or `trace`
    /// may read this field, and `client/tests/capture.rs` is the assertion.
    viewport: Viewport,
    /// The order the client believes the server is holding for it.
    standing: Action,
    /// A one-shot ability asked for since the last frame. Takes precedence for
    /// the tick it was asked on and leaves the standing order alone.
    once: Option<Action>,
    /// The wait for the other players, and the measurement hidden in it.
    ///
    /// Beside the aim rather than in front of it, because the lobby is driven by
    /// **the same cursor the match is played with** — see [`crate::lobby`]. It
    /// stops being fed the moment the player asks to start, so a click on a
    /// champion during the match cannot be resolved against a menu that is no
    /// longer on the screen.
    lobby: Lobby,
}

impl Default for Play {
    fn default() -> Self {
        Self::new()
    }
}

impl Play {
    /// A client that has captured nothing.
    ///
    /// The aim starts at the middle of the *map*, which is a rule constant. It
    /// deliberately does not start at the middle of the window, because at this
    /// point there is no window and because "where the aim starts" is not a
    /// question a display gets to answer.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            aim: Aim::centred(),
            trace: InputTrace::new(),
            viewport: Viewport::new(1280, 800),
            standing: Action::Idle,
            once: None,
            lobby: Lobby::new(),
        }
    }

    /// The lobby, and what crossing it has measured.
    #[must_use]
    pub const fn lobby(&self) -> &Lobby {
        &self.lobby
    }

    /// Whether the lobby is still up.
    ///
    /// One question and not a phase field: the lobby is up until the player asks
    /// to start, and asking to start is a thing the lobby itself records.
    #[must_use]
    pub const fn in_lobby(&self) -> bool {
        !self.lobby.is_ready()
    }

    /// The window changed size.
    pub const fn resized(&mut self, width: u32, height: u32) {
        self.viewport = Viewport::new(width, height);
    }

    /// The window, for the renderer.
    #[must_use]
    pub const fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// Where the player is aiming, in the world.
    #[must_use]
    pub fn aim(&self) -> FxVec2 {
        self.aim.world()
    }

    /// Everything the devices have said.
    #[must_use]
    pub const fn trace(&self) -> &InputTrace {
        &self.trace
    }

    /// One raw motion event from a pointing device.
    ///
    /// Two statements, in this order and with nothing between them: record what
    /// arrived, then integrate it. The recording is unconditional — see
    /// [`InputTrace::moved`] — and neither line may consult `self.viewport`.
    pub fn moved(&mut self, at_ns: u64, dx: f64, dy: f64) {
        self.trace.moved(at_ns, dx, dy);
        self.aim.apply(dx, dy);
        if self.in_lobby() {
            // Third, and after the aim, because the lobby measures the cursor
            // *this* event produced. It is handed the position rather than
            // asked for one: there is no path from `crate::lobby` to a window,
            // and this is the line that would have to change for one to exist.
            self.lobby.moved(at_ns, dx, dy, self.aim.world());
        }
    }

    /// One control transition while the lobby is up.
    ///
    /// A separate entry point from [`Play::pressed`] because a lobby click has
    /// no world to consult: [`Aiming`] is a fact about the last view, and in the
    /// lobby there is no view and no match yet. What the two share is the line
    /// that matters — the press is recorded into the same [`InputTrace`], first
    /// and unconditionally, including a click that lands on nothing.
    ///
    /// Answers what was clicked, so that the caller can send `Ready`.
    pub fn pressed_in_lobby(
        &mut self,
        at_ns: u64,
        control: Control,
        down: bool,
    ) -> Option<Element> {
        self.trace.pressed(at_ns, control, down);
        if !down || control != Control::Move {
            return None;
        }
        self.lobby.clicked(at_ns, self.aim.world())
    }

    /// A view arrived and this client answered it with intention `seq`.
    ///
    /// Recorded into the same trace as the device events, and it is the only
    /// entry in it that is not the hand: see [`InputTrace::viewed`]. It takes the
    /// timestamp rather than reading one, in the same way [`Play::moved`] does
    /// and for the same reason — nothing in this module reads a clock.
    pub fn viewed(&mut self, at_ns: u64, tick: u32, seq: u32) {
        self.trace.viewed(at_ns, tick, seq);
    }

    /// One control transition.
    ///
    /// Recorded first and always, including releases and including presses that
    /// produce no order at all — a `Targeted` with nobody in range is a thing a
    /// player did, and a record that only kept the presses that worked would be
    /// a record of the game's rules rather than of the hand.
    pub fn pressed(&mut self, at_ns: u64, control: Control, down: bool, at: &Aiming<'_>) {
        self.trace.pressed(at_ns, control, down);
        if !down {
            return;
        }
        let aim = self.aim.world();
        match control {
            Control::Move => self.standing = Action::Move(aim),
            Control::Attack => {
                // Within a couple of units of the aim, so that a player does not
                // have to be exact. An enemy the fog has taken away is not in
                // the view and therefore cannot be attacked, which is the same
                // restriction the rules put on the order.
                self.standing = nearest_enemy(at.view, at.seat, aim, ATTACK_CLICK_REACH)
                    .map_or(Action::Move(aim), Action::Attack);
            }
            Control::Skillshot => {
                // Aimed from the champion through the aim, which is what a
                // skillshot is. A direction shorter than the rules will
                // normalise is discarded by `sim` without spending the cooldown,
                // so aiming at your own feet costs nothing.
                self.once = Some(Action::Skillshot(aim.sub(at.own)));
            }
            Control::Targeted => {
                if let Some(id) = nearest_enemy(at.view, at.seat, at.own, RULES.targeted_range) {
                    self.once = Some(Action::Targeted(id));
                }
            }
            Control::Stop => self.standing = Action::Idle,
        }
    }

    /// The intention for this tick, and forgets the one-shot.
    pub fn intention(&mut self) -> Action {
        self.once.take().unwrap_or(self.standing)
    }
}

/// How near the aim an enemy has to be for a right-click to mean "attack it"
/// rather than "walk there".
const ATTACK_CLICK_REACH: Fx = Fx::from_int(6);
