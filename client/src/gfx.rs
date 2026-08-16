//! The window, the event loop, and the thread that talks to the server.
//!
//! Deliberately dull, in the same way the terminal client's `term` module was.
//! Everything that can be wrong about *what is drawn* lives in [`crate::draw`],
//! which is a pure function with tests; everything that can be wrong about *what
//! is captured* lives in [`crate::input`] and [`crate::play`], which are pure
//! state machines with tests; everything that can be wrong about *what is
//! predicted* lives in [`crate::predict`]. What is left here is a window, a
//! framebuffer, an event loop and a socket.
//!
//! # Why a window, and why this window
//!
//! `docs/RISKS.md` R14 chose a terminal at M4 and priced the choice at "a
//! curvature detector cannot be written against this corpus". A trace of a real
//! session said the price was higher than that: a terminal reports the pointer
//! only when it crosses into a new character cell, so the sampling *rate* is a
//! function of pointer speed and the timing statistics R14 called untouched are
//! contaminated too. That is not a resolution problem with a resolution fix, so
//! the renderer changed. `docs/ARCHITECTURE.md` carries the library decision and
//! its reopening criterion.
//!
//! `winit` for the window because the input path is what motivated the change
//! and `winit` is the layer that exposes raw device motion on every platform in
//! `docs/ENGINEERING.md`'s matrix. `softbuffer` for the pixels because the scene
//! is nine discs, six towers and some projectiles: a CPU framebuffer draws that,
//! needs no shader, no adapter and no driver, and — the part that decided it —
//! leaves `rasterize` a pure function of a slice, so the renderer keeps its
//! tests in a CI job that has no display at all.
//!
//! # The timestamp is taken here, and it is taken per event
//!
//! [`ApplicationHandler::device_event`] reads the clock on its first line. That
//! callback runs once per event as the platform's queue drains, so the stamp
//! does not inherit the frame cadence — which matters, because
//! `docs/MILESTONES.md` M8's timing detectors have to tell a bot's regularity
//! from a renderer's jitter, and a timestamp read once per frame would have put
//! the renderer's jitter into every sample. It is still not a *device*
//! timestamp; [`crate::input::CLOCK`] says so, per platform, in a type.
//!
//! # The loop measures itself against the tick it has to keep
//!
//! [`ApplicationHandler::new_events`] starts a stopwatch and
//! [`ApplicationHandler::about_to_wait`] stops it, so a *pass* is one turn of
//! the loop with the wait excluded. [`crate::health::Cadence`] counts the passes
//! that took longer than one tick and the worst of them, and the client prints
//! both on the way out — `docs/RISKS.md` R16, which is the observation that the
//! only tick budget this project ever measured was measured on a fixture, and
//! that a client which falls behind writes the delay into the corpus as though
//! it were the hand.
//!
//! **This wiring is the one part of that mechanism no test covers, and it says so
//! rather than passing for evidence.** `winit` cannot open a window without a
//! display server and CI has none, so `Cadence` itself is tested directly
//! (`crate::health`), the loop shape is measured against a real match and a real
//! server in `client/tests/cadence.rs`, and the two callbacks below — which is
//! where the measurement is *attached* to the playable client — are checked by
//! nobody. What limits the damage is that the failure is loud rather than silent:
//! an unpaired bracket records no pass at all, so the number the client prints on
//! the way out is zero, and a session part with `passes: 0` is a session part an
//! operator reads as broken.
//!
//! # The view anchor is attached here, and that wiring is uncovered too
//!
//! [`Session::advance`] records a `client::input::Event::Viewed` for every frame
//! it folds in, which is what ties the device stream to the match
//! (`docs/SCHEMA.md` §11c). It is one line in the same callback as everything
//! else here, and it is checked by nobody for the same reason the `Cadence`
//! bracket is: this loop needs a display server and CI has none.
//!
//! **So the failure was made loud rather than left silent.** A stream with no
//! anchors in it is a client whose wiring is broken, not a session — a seat that
//! played a match received frames — so `replay::Corpus::store` refuses a traced
//! seat whose companion carries zero view anchors, by name and at the door. An
//! operator finds out when they file the match rather than when a detector reads
//! a corpus that cannot answer the question it was recorded for.
//!
//! # The pointer is hidden, and grabbed only when nothing is being recorded
//!
//! Cursor visibility is state on a device the process does not own; it is
//! released when the window is dropped, which winit does on exit, including on
//! the way out of a panic.
//!
//! The pointer used to be left ungrabbed in every session, and the reason was
//! measured rather than argued: an X11 pointer grab makes the server deliver
//! every raw motion event twice. [`Screen::hold`] carries that measurement, what
//! the first real playtest found the alternative to cost — a lobby whose corners
//! cannot be reached, so `Ready` never leaves and the match never starts — and
//! the line the two are split along. **A recording session is never grabbed; a
//! session that records nothing is**, because a session that records nothing
//! cannot reach a corpus (`replay::Attested`) and therefore cannot contaminate
//! one.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use protocol::{ClientFrame, ClientMessage, SERVER_FRAME_BYTES};
use sim::Seat;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, DeviceEvents, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::draw::{Mark, Viewport, compose, rasterize};
use crate::health::{Cadence, CadenceReport, Recorded, SessionPart};
use crate::input::{Control, TraceStats};
use crate::lobby::Element;
use crate::play::{Aiming, Play};
use crate::predict::Prediction;
use crate::{ClientError, Headless};

/// What the network thread wakes the event loop for.
#[derive(Clone, Copy, Debug)]
pub struct Wake;

/// The default window size, in pixels. A window, not a claim: the projection
/// letterboxes, so any size draws the same map.
const WINDOW: (u32, u32) = (1280, 800);

/// This client's own clock, sent with every input and trusted by nobody.
///
/// `docs/SCOPE.md`'s adversary model puts this field in the attacker's hands by
/// definition, and `docs/ARCHITECTURE.md` records that its divergence from the
/// server's arrival timestamp is exploit class 4's signal. An honest client
/// reads its own clock and says so; that is the whole of what makes the
/// divergence mean anything for the dishonest one.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// The physical keys this client reads, and nothing else.
///
/// A key the client does not use produces no sample, because a record of keys
/// nobody can interpret is not telemetry.
const fn control_for(key: KeyCode) -> Option<Control> {
    match key {
        KeyCode::KeyQ => Some(Control::Skillshot),
        KeyCode::KeyW => Some(Control::Targeted),
        KeyCode::KeyS => Some(Control::Stop),
        _ => None,
    }
}

/// What the OS pointer is doing while this window has focus.
///
/// Three states rather than a `bool`, because which grab a platform *gave* is
/// not the same question as which one was asked for, and the difference is the
/// one that matters: `Locked` and `Confined` cost different things, and only one
/// of them is the mode the measurement below is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pointer {
    /// Not grabbed. The invisible OS pointer is free to leave the window and a
    /// click after it has gone is delivered to whatever is under it.
    ///
    /// **What a recording session runs under, always** — see [`Screen::hold`].
    Free,
    /// Pinned to one point inside the window (`CursorGrabMode::Locked`).
    ///
    /// The best of the three and the one asked for first: the pointer cannot
    /// leave, and it is not the X11 pointer grab the measurement is about.
    /// Wayland, Windows and macOS.
    Locked,
    /// Kept inside the window's area (`CursorGrabMode::Confined`).
    ///
    /// The fallback, and on X11 it is the mode that delivers every raw motion
    /// **twice**. Taken only because the alternative on that platform is a
    /// pointer that walks off the window.
    Confined,
}

impl Pointer {
    /// How this reads in a log line.
    const fn tag(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Locked => "locked",
            Self::Confined => "confined",
        }
    }
}

/// The window and its framebuffer.
struct Screen {
    window: Arc<Window>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    /// What the platform actually gave when a grab was asked for.
    pointer: Pointer,
}

impl Screen {
    fn open(event_loop: &ActiveEventLoop, title: &str, confine: bool) -> Result<Self, String> {
        let attributes = Window::default_attributes()
            .with_title(title)
            .with_inner_size(winit::dpi::LogicalSize::new(WINDOW.0, WINDOW.1));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| error.to_string())?,
        );
        let context =
            softbuffer::Context::new(Arc::clone(&window)).map_err(|error| error.to_string())?;
        let surface = softbuffer::Surface::new(&context, Arc::clone(&window))
            .map_err(|error| error.to_string())?;

        // The OS pointer is hidden, because the aim a player sees is drawn by
        // this client from raw deltas and has nothing to do with where the OS
        // thinks the pointer is.
        window.set_cursor_visible(false);
        let pointer = Self::hold(&window, confine);
        eprintln!(
            "capture: pointer {}{}",
            pointer.tag(),
            match pointer {
                Pointer::Free => "",
                Pointer::Locked => " — the OS pointer cannot leave this window",
                Pointer::Confined =>
                    " — on X11 this mode delivers every raw motion twice; the \
                     coincident count printed at exit is what says whether it did",
            }
        );
        Ok(Self {
            window,
            surface,
            pointer,
        })
    }

    /// Asks the platform for a grab, and answers what it gave.
    ///
    /// # Why a grab is asked for at all now, and why never while recording
    ///
    /// This client used to take none, and the reason was measured rather than
    /// argued: `CursorGrabMode::Confined` on X11 makes the server deliver every
    /// raw motion event **twice**, five microseconds apart — 50 synthesised
    /// device motions produced 50 `DeviceEvent::MouseMotion` without the grab
    /// and 100 with it, measured against `winit` alone with none of this crate
    /// involved. A duplicate is invisible on screen and is a second mode near
    /// zero in every inter-arrival distribution a detector at M8 would read,
    /// which is exactly the platform artefact a corpus must not be quietly
    /// calibrated on. The cost accepted instead was that the invisible pointer
    /// drifts and a click after it has left the window goes elsewhere.
    ///
    /// **The first playtest priced that cost and it is not small.** The aim
    /// moves `client::input::WORLD_UNITS_PER_COUNT` per device count and the
    /// renderer draws about 3.5 pixels per world unit at the default window, so
    /// one count moves the drawn aim about a sixth of a pixel: reaching a lobby
    /// element in the corner is a couple of thousand counts, and the invisible
    /// OS pointer is off the window long before the aim arrives. The clicks then
    /// land on somebody else's window, `Ready` never leaves, and the server —
    /// which ticks only when every occupied seat is ready — never starts the
    /// match. A menu nobody can finish is not a usability wart.
    ///
    /// So the grab is asked for **when this client is not recording**, and never
    /// when it is. That is a line the corpus can hold rather than a compromise:
    /// a session that records nothing cannot contaminate a distribution, a match
    /// played without `--record` cannot enter the corpus at all
    /// (`replay::Attested`), and `crate::gfx::play`'s caller refuses `--record`
    /// together with an explicit grab rather than silently preferring one.
    ///
    /// `Locked` is asked for first and is not the mode the measurement is about:
    /// it pins the pointer to a point, X11 does not implement it, and the
    /// platforms that do — Wayland, Windows, macOS — reach it without the X11
    /// pointer grab. `Confined` is the fallback, which is to say X11 is the
    /// fallback, and the coincident count reported at exit is what tells an
    /// operator what it cost on their machine.
    fn hold(window: &Window, confine: bool) -> Pointer {
        if !confine {
            let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
            return Pointer::Free;
        }
        for (mode, held) in [
            (winit::window::CursorGrabMode::Locked, Pointer::Locked),
            (winit::window::CursorGrabMode::Confined, Pointer::Confined),
        ] {
            if window.set_cursor_grab(mode).is_ok() {
                return held;
            }
        }
        Pointer::Free
    }

    /// Paints one frame.
    fn present(&mut self, marks: &[Mark], viewport: Viewport) -> Result<(), String> {
        let (Some(width), Some(height)) = (
            std::num::NonZeroU32::new(viewport.width),
            std::num::NonZeroU32::new(viewport.height),
        ) else {
            return Ok(());
        };
        self.surface
            .resize(width, height)
            .map_err(|error| error.to_string())?;
        let mut buffer = self
            .surface
            .buffer_mut()
            .map_err(|error| error.to_string())?;
        rasterize(marks, viewport, &mut buffer);
        buffer.present().map_err(|error| error.to_string())
    }
}

/// The playable client.
struct Session {
    screen: Option<Screen>,
    epoch: Instant,
    headless: Headless,
    prediction: Option<Prediction>,
    play: Play,
    seat: Seat,
    inbox: Receiver<[u8; SERVER_FRAME_BYTES]>,
    outbox: tokio::sync::mpsc::Sender<ClientFrame>,
    /// The `Ready` frame, held until the player asks to start.
    ///
    /// It used to be sent the moment the server assigned a seat, which made the
    /// lobby a thing that did not exist: the match began as soon as the last
    /// client connected. Holding it is what turns the wait for the other players
    /// into an interval the client is in charge of — and `crate::lobby` is what
    /// that interval is for.
    ready: Option<ClientFrame>,
    /// Whether to ask the platform to keep the OS pointer inside the window.
    /// **False for a recording session, always** — see [`Screen::hold`].
    confine: bool,
    /// The loop, measured against one tick. See this module's header.
    cadence: Cadence,
    /// When the current pass began, or `None` between passes.
    pass_began: Option<Instant>,
    /// Why the loop stopped. `Over` is not an error: the server closing the
    /// connection is what the end of a match looks like from here, and it has
    /// its own variant rather than a string somebody has to compare, because a
    /// normal ending reported as a failure is a client that exits non-zero on
    /// every match it finishes.
    outcome: Option<Ending>,
}

/// Why the playable client's event loop stopped.
enum Ending {
    /// The match ended, or the player left.
    Over,
    /// Something to print once and exit on.
    Failed(String),
}

impl Session {
    fn at_ns(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }

    /// Folds in every frame that has arrived, and answers each with one
    /// intention.
    fn advance(&mut self) {
        loop {
            let bytes = match self.inbox.try_recv() {
                Ok(bytes) => bytes,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.outcome.get_or_insert(Ending::Over);
                    return;
                }
            };
            if let Err(error) = self.headless.receive(&bytes) {
                self.outcome = Some(Ending::Failed(describe(error)));
                return;
            }
            let Some(view) = self.headless.view().cloned() else {
                continue;
            };

            let prediction = self
                .prediction
                .get_or_insert_with(|| Prediction::anchored(&view));
            prediction.observe(&view, self.headless.applied_through());

            // The intention is decided and recorded *before* the frame is
            // drawn, and the order is the whole of what prediction buys.
            // Drawing first would put the authoritative position on the screen
            // — the position the server reported for a tick it has already
            // left — and the click the player just made would not appear until
            // the round trip came back, which is the latency this client exists
            // to hide.
            let action = self.play.intention();
            let seq = self.headless.next_seq();
            prediction.sent(seq, action);
            let frame = self.headless.intend(action, now_ms());
            // The anchor, on the same monotonic clock as every device sample and
            // taken here rather than at the top of the loop: what a reaction is
            // measured from is the moment this client *had* the view and could
            // act on it, and that moment is this one. `client::input::Event::Viewed`
            // is why the record exists at all.
            self.play.viewed(self.at_ns(), view.tick.0, seq);
            if self.outbox.try_send(frame).is_err() {
                self.outcome.get_or_insert(Ending::Over);
                return;
            }
        }
    }

    fn redraw(&mut self) {
        if self.play.in_lobby() {
            let marks = crate::lobby::compose(self.play.lobby(), self.play.aim());
            let viewport = self.play.viewport();
            if let Some(screen) = self.screen.as_mut()
                && let Err(error) = screen.present(&marks, viewport)
            {
                self.outcome = Some(Ending::Failed(error));
            }
            return;
        }
        let (Some(view), Some(prediction)) = (self.headless.view(), self.prediction.as_ref())
        else {
            // Ready has left and no frame has come back yet. Drawing nothing
            // here leaves the *lobby's* last frame on the screen, which is a
            // client that looks frozen at exactly the moment a player is
            // wondering whether their click did anything — the playtest
            // report was "nothing happens once I click the white button". So
            // the map is drawn empty with the cursor on it: the player sees
            // where they are aiming and that the client is alive, and sees it
            // without being told anything the server has not sent, because
            // there is nothing here to tell.
            let mut marks = Vec::with_capacity(8);
            crate::draw::aim_limit(&mut marks);
            marks.push(crate::draw::Mark::Cross {
                at: self.play.aim(),
                colour: crate::draw::colour::AIM,
            });
            let viewport = self.play.viewport();
            if let Some(screen) = self.screen.as_mut()
                && let Err(error) = screen.present(&marks, viewport)
            {
                self.outcome = Some(Ending::Failed(error));
            }
            return;
        };
        let scene = crate::draw::Scene {
            view,
            seat: self.seat,
            own: prediction.position(),
            aim: self.play.aim(),
        };
        let marks = compose(&scene);
        let viewport = self.play.viewport();
        if let Some(screen) = self.screen.as_mut()
            && let Err(error) = screen.present(&marks, viewport)
        {
            self.outcome = Some(Ending::Failed(error));
        }
    }

    /// What a control press needs to know, or `None` before the first frame.
    fn aiming(&self) -> Option<(sim::view::PlayerView, sim::FxVec2)> {
        let view = self.headless.view()?.clone();
        let own = self
            .prediction
            .as_ref()
            .map_or(view.own.position, Prediction::position);
        Some((view, own))
    }
}

impl ApplicationHandler<Wake> for Session {
    /// The platform is handing control back with work to do: a pass begins.
    ///
    /// Reading the clock here rather than in each callback is deliberate. What
    /// the budget is about is the whole turn of the loop — draining the device
    /// queue, folding in every view that arrived, answering each with an
    /// intention, and drawing — because a client that spends 40 ms doing all of
    /// that answers the next frame late no matter which of the four was slow.
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, _cause: winit::event::StartCause) {
        self.pass_began = Some(Instant::now());
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.screen.is_some() {
            return;
        }
        match Screen::open(event_loop, "moba", self.confine) {
            Ok(screen) => {
                let size = screen.window.inner_size();
                self.play.resized(size.width, size.height);
                self.screen = Some(screen);
                event_loop.listen_device_events(DeviceEvents::WhenFocused);
            }
            Err(error) => {
                self.outcome = Some(Ending::Failed(error));
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _wake: Wake) {
        self.advance();
        if self.outcome.is_some() {
            // Both endings exit here, and `Over` is the one that matters: it is
            // what the transport being gone looks like from this side, and a
            // loop that kept running would leave a window nobody can explain.
            event_loop.exit();
            return;
        }
        if let Some(screen) = self.screen.as_ref() {
            screen.window.request_redraw();
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device: DeviceId,
        event: DeviceEvent,
    ) {
        // The clock is read first, before anything decides what this event
        // means. See this module's header.
        let at_ns = self.at_ns();
        if let DeviceEvent::MouseMotion { delta } = event {
            self.play.moved(at_ns, delta.0, delta.1);
            // In the lobby nothing else asks for a frame: the server is not
            // ticking yet, because it waits for every occupied seat to be ready,
            // so there are no views arriving to wake the loop. A cursor that
            // only moves when the player clicks is a cursor nobody can aim.
            // …and between `Ready` and the first frame, for the same reason:
            // nothing else asks for a redraw in that gap either, and a cursor
            // that stops moving is how a player reads a client as hung.
            if (self.play.in_lobby() || self.headless.view().is_none())
                && let Some(screen) = self.screen.as_ref()
            {
                screen.window.request_redraw();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window: WindowId,
        event: WindowEvent,
    ) {
        let at_ns = self.at_ns();
        match event {
            WindowEvent::CloseRequested => self.surrender(event_loop),
            WindowEvent::Resized(size) => {
                self.play.resized(size.width, size.height);
                if let Some(screen) = self.screen.as_ref() {
                    screen.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            // A grab does not survive losing focus on every platform, and it
            // must not: a window that kept the pointer while the player was in
            // another one would be a window nobody can leave. So it is released
            // on the way out and asked for again on the way back in, which is
            // what makes alt-tab an ordinary thing to do rather than a thing to
            // avoid.
            WindowEvent::Focused(focused) => {
                if let Some(screen) = self.screen.as_mut() {
                    screen.pointer = Screen::hold(&screen.window, self.confine && focused);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let control = match button {
                    MouseButton::Left => Control::Move,
                    MouseButton::Right => Control::Attack,
                    _ => return,
                };
                self.press(at_ns, control, state.is_pressed());
            }
            WindowEvent::KeyboardInput { event, .. } => {
                // A key the OS repeated is not a key a hand pressed, so it is
                // filtered rather than recorded. Auto-repeat in a corpus would
                // be the operating system's key-repeat setting showing up in a
                // behavioural distribution as if it were a person.
                if event.repeat {
                    return;
                }
                if matches!(event.physical_key, PhysicalKey::Code(KeyCode::Escape))
                    && event.state == ElementState::Pressed
                {
                    self.surrender(event_loop);
                    return;
                }
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                let Some(control) = control_for(code) else {
                    return;
                };
                self.press(at_ns, control, event.state.is_pressed());
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // The pass ends here, before the loop blocks. Taking the stopwatch
        // rather than reading it means a turn is measured once even if the
        // platform calls this twice, and that an unpaired `about_to_wait`
        // records nothing rather than recording the time since the last one —
        // which would be the wait, and the wait is not the client being late.
        if let Some(began) = self.pass_began.take() {
            self.cadence
                .pass(u64::try_from(began.elapsed().as_nanos()).unwrap_or(u64::MAX));
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

impl Session {
    /// The player asked to leave. `Surrender` frees the seat; it does not decide
    /// the match, because whether a team that concedes loses is a rule and rules
    /// live in `sim` where a replay resimulates them.
    fn surrender(&mut self, event_loop: &ActiveEventLoop) {
        self.outcome.get_or_insert(Ending::Over);
        let _ = self
            .outbox
            .try_send(ClientFrame::encode(&ClientMessage::Surrender));
        event_loop.exit();
    }

    /// The player asked to start: the held `Ready` leaves, and the server begins
    /// ticking once every occupied seat has done the same.
    fn start(&mut self) {
        let Some(ready) = self.ready.take() else {
            return;
        };
        if self.outbox.try_send(ready).is_err() {
            self.outcome.get_or_insert(Ending::Over);
            return;
        }
        // Printed, because from here the screen is a map with nothing on it
        // until the first frame arrives and the player has no other way to tell
        // "waiting for the other seats" from "this client is broken". The wait
        // is a real one: the server runs no tick until every occupied seat has
        // sent this.
        eprintln!(
            "ready: waiting for the other seats — the match starts when every \
             one of them has clicked Ready"
        );
    }

    fn press(&mut self, at_ns: u64, control: Control, down: bool) {
        if self.play.in_lobby() {
            // The lobby's own click path. It records the press into the same
            // trace either way — including a click that lands on nothing, which
            // is a thing the player did — and answers what was hit so that
            // `Ready` can leave.
            if self.play.pressed_in_lobby(at_ns, control, down) == Some(Element::Ready) {
                self.start();
            }
            if let Some(screen) = self.screen.as_ref() {
                screen.window.request_redraw();
            }
            return;
        }
        let Some((view, own)) = self.aiming() else {
            return;
        };
        self.play.pressed(
            at_ns,
            control,
            down,
            &Aiming {
                view: &view,
                seat: self.seat,
                own,
            },
        );
    }
}

fn describe(error: ClientError) -> String {
    error.to_string()
}

/// Connects, opens a window, and plays until the match ends or the player
/// leaves.
///
/// Synchronous, because `winit` owns the thread it runs on. The transport keeps
/// its `tokio` runtime on a thread of its own and the two meet over two
/// channels and an event-loop proxy, which is also what keeps every decision —
/// the session state machine, the prediction, the capture — on one thread with
/// no locking to reason about.
///
/// `recorded` is `Some` when this session is part of a recording session, and it
/// is what makes the session part reach a disk. A client playing for fun writes
/// nothing at all.
///
/// # Errors
///
/// A string, because everything this can fail at is something to print once and
/// exit on: a refused connection, a server on another build, a display that will
/// not give out a window, or a declared pointer acceleration this project does
/// not record against.
pub fn play(
    address: std::net::SocketAddr,
    certificate: &[u8],
    recorded: Option<&Recorded>,
    confine: bool,
) -> Result<(), String> {
    // Before the connection rather than after the match, for the reason
    // `moba-server` loads its signing key before playing: a session that plays a
    // match and then discovers it may not be recorded has thrown the match away.
    if let Some(recorded) = recorded
        && recorded.declared.pointer_acceleration
    {
        return Err("this session declares the operating system's pointer \
                    acceleration left on, and a corpus recorded through it \
                    measures the operating system's curve as much as the hand \
                    (docs/SCHEMA.md). Turn it off, or record nothing."
            .to_owned());
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;

    let event_loop = EventLoop::<Wake>::with_user_event()
        .build()
        .map_err(|error| error.to_string())?;
    let proxy = event_loop.create_proxy();

    let mut headless = Headless::new();
    let certificate = certificate.to_vec();
    let join = headless.join();
    let (mut wire, accepted) = runtime.block_on(async move {
        let mut wire = crate::net::Wire::connect(address, &certificate)
            .await
            .map_err(|error| error.to_string())?;
        wire.send(&join).await.map_err(|error| error.to_string())?;
        let accepted = wire
            .recv_session()
            .await
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((wire, accepted))
    })?;
    headless.receive(&accepted).map_err(describe)?;
    let seat = headless.seat().ok_or("the server assigned no seat")?;
    let ready = headless.ready();

    let (in_tx, in_rx) = std::sync::mpsc::channel();
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<ClientFrame>(64);

    runtime.spawn(async move {
        loop {
            tokio::select! {
                // `read_datagram` is cancel-safe and the assembler's state is
                // only touched after a datagram has been read, so losing this
                // branch to the other loses nothing.
                state = wire.recv_state() => {
                    let Ok(bytes) = state else { break };
                    if in_tx.send(bytes).is_err() {
                        break;
                    }
                    let _ = proxy.send_event(Wake);
                }
                outbound = out_rx.recv() => {
                    let Some(frame) = outbound else { break };
                    if wire.send(&frame).await.is_err() {
                        break;
                    }
                }
            }
        }
        // **The last thing this task does is wake the loop**, and it is the
        // difference between a client that says the connection ended and one
        // that hangs. `Session::advance` learns the transport is gone by finding
        // the channel disconnected, and it is only ever called from
        // `user_event` — which is woken by this task. Returning without this
        // line leaves a window drawing its last frame forever, which is what a
        // playtest reported as "nothing happens": the connection had timed out
        // during the lobby and there was nothing left to say so.
        drop(in_tx);
        let _ = proxy.send_event(Wake);
    });

    let mut session = Session {
        screen: None,
        epoch: Instant::now(),
        headless,
        prediction: None,
        play: Play::new(),
        seat,
        inbox: in_rx,
        outbox: out_tx,
        ready: Some(ready),
        confine,
        cadence: Cadence::new(),
        pass_began: None,
        outcome: None,
    };
    event_loop
        .run_app(&mut session)
        .map_err(|error| error.to_string())?;

    let stats = session.play.trace().stats();
    let cadence = session.cadence.report();
    report(stats);
    report_cadence(cadence);
    if let Some(recorded) = recorded {
        let part = SessionPart {
            seat,
            declared: recorded.declared.clone(),
            trace: stats,
            cadence,
            calibration: session.play.lobby().observations(),
        };
        std::fs::create_dir_all(&recorded.directory)
            .map_err(|error| format!("{}: {error}", recorded.directory.display()))?;
        let path = recorded.directory.join(part.file_name());
        std::fs::write(&path, part.encode())
            .map_err(|error| format!("{}: {error}", path.display()))?;
        eprintln!("capture: session part {}", path.display());

        // And the device stream itself, which is the artefact the session part
        // only summarises. It goes to whoever *seals*, before the replay exists,
        // because the replay's manifest commits to the companion's digest and a
        // digest has to exist before something can commit to it
        // (`replay::manifest::Commitment`).
        let telemetry = crate::health::telemetry_part(seat, session.play.trace());
        let path = recorded
            .directory
            .join(crate::health::telemetry_part_name(seat));
        std::fs::write(&path, &telemetry)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        eprintln!(
            "capture: telemetry part {} — {} bytes, {} device event(s), {} view \
             anchor(s)",
            path.display(),
            telemetry.len(),
            stats.samples,
            stats.views
        );
    }
    match session.outcome {
        Some(Ending::Failed(reason)) => Err(reason),
        Some(Ending::Over) | None => Ok(()),
    }
}

/// The measurement instrument: a window that captures and reports, with no
/// server and no game.
///
/// `docs/MILESTONES.md` asks for milestones verifiable by running a command, and
/// this is the command behind the claim that the capture path's sampling rate no
/// longer follows the pointer's speed. It opens a window, records every device
/// event for `seconds`, and prints the distribution of inter-arrival times and
/// the finest motion observed.
///
/// # Errors
///
/// A string, if there is no display to open a window on.
pub fn probe(seconds: u64, confine: bool) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|error| error.to_string())?;
    let mut probe = Probe {
        screen: None,
        epoch: Instant::now(),
        play: Play::new(),
        until: seconds,
        confine,
        failure: None,
    };
    event_loop
        .run_app(&mut probe)
        .map_err(|error| error.to_string())?;
    if let Some(failure) = probe.failure {
        return Err(failure);
    }
    report(probe.play.trace().stats());
    for sample in probe.play.trace().samples() {
        match sample.event {
            crate::input::Event::Moved { dx, dy } => {
                println!("{}\tmove\t{dx}\t{dy}", sample.at_ns);
            }
            crate::input::Event::Pressed { control, down } => {
                println!("{}\tpress\t{control:?}\t{down}", sample.at_ns);
            }
            crate::input::Event::Viewed { tick, seq } => {
                println!("{}\tview\t{tick}\t{seq}", sample.at_ns);
            }
        }
    }
    Ok(())
}

struct Probe {
    screen: Option<Screen>,
    epoch: Instant,
    play: Play,
    until: u64,
    failure: Option<String>,
    /// Whether to measure the capture path **under a grab**.
    ///
    /// The reopening criterion `Screen::hold` names, made runnable: a platform
    /// whose grab is shown to deliver one event per motion could take one while
    /// recording, and this is the instrument that would show it. `moba-client
    /// --probe-input 10 --confine on` against the same run with `off` is the
    /// comparison, and the coincident count is the answer.
    confine: bool,
}

impl ApplicationHandler for Probe {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.screen.is_some() {
            return;
        }
        match Screen::open(event_loop, "moba — input probe", self.confine) {
            Ok(screen) => {
                let size = screen.window.inner_size();
                self.play.resized(size.width, size.height);
                self.screen = Some(screen);
                event_loop.listen_device_events(DeviceEvents::Always);
            }
            Err(error) => {
                self.failure = Some(error);
                event_loop.exit();
            }
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device: DeviceId,
        event: DeviceEvent,
    ) {
        let at_ns = u64::try_from(self.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
        if let DeviceEvent::MouseMotion { delta } = event {
            self.play.moved(at_ns, delta.0, delta.1);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => self.play.resized(size.width, size.height),
            WindowEvent::RedrawRequested => {
                if let Some(screen) = self.screen.as_mut() {
                    let marks = [Mark::Cross {
                        at: self.play.aim(),
                        colour: crate::draw::colour::AIM,
                    }];
                    let _ = screen.present(&marks, self.play.viewport());
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.epoch.elapsed().as_secs() >= self.until {
            event_loop.exit();
            return;
        }
        if let Some(screen) = self.screen.as_ref() {
            screen.window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + std::time::Duration::from_millis(16),
        ));
    }
}

/// Prints what a captured trace measures, to standard error so that the probe's
/// sample dump on standard output stays machine-readable.
fn report(stats: TraceStats) {
    let TraceStats {
        samples,
        moves,
        views,
        span_ns,
        gaps_ns,
        gap_mean_ns,
        gap_sd_ns,
        finest_count,
        finest_world_units,
        travelled_counts,
        coincident,
    } = stats;
    let ms = |ns: u64| (ns as f64) / 1e6;
    eprintln!(
        "capture: {samples} device events ({moves} motion) and {views} view \
         anchor(s) over {:.3} s",
        ms(span_ns) / 1e3
    );
    eprintln!(
        "capture: inter-arrival ms  min {:.3}  p05 {:.3}  p50 {:.3}  p95 {:.3}  \
         p99 {:.3}  max {:.3}",
        ms(gaps_ns.min),
        ms(gaps_ns.p05),
        ms(gaps_ns.p50),
        ms(gaps_ns.p95),
        ms(gaps_ns.p99),
        ms(gaps_ns.max)
    );
    eprintln!(
        "capture: inter-arrival mean {:.3} ms, standard deviation {:.3} ms",
        gap_mean_ns / 1e6,
        gap_sd_ns / 1e6
    );
    eprintln!(
        "capture: clock {:?}; finest motion {} counts = {} world units; travelled {travelled_counts:.1} counts",
        crate::input::CLOCK,
        finest_count.map_or("n/a".to_owned(), |value| format!("{value}")),
        finest_world_units.map_or("n/a".to_owned(), |value| format!("{value}")),
    );
    eprintln!(
        "capture: coincident samples {coincident} — a non-zero count means the \
         platform delivered one device event more than once, and the record is \
         not one sample per motion (see client::input::TraceStats)"
    );
}

/// Prints what the loop cost, which every recording session owes
/// `docs/RISKS.md` R16.
///
/// It is printed on every run and not only on a bad one, for the reason R15
/// gives about counters: a reader who sees `28714 passes, 0 over a budget of
/// 33.333 ms` has been told something, and a reader who sees nothing has been
/// told that nothing was measured.
fn report_cadence(report: CadenceReport) {
    let CadenceReport {
        budget_ns,
        passes,
        passes_over_budget,
        worst_overrun_ns,
        worst_pass_ns,
    } = report;
    let ms = |ns: u64| (ns as f64) / 1e6;
    eprintln!(
        "cadence: {passes} passes, {passes_over_budget} over a budget of {:.3} ms; \
         worst overrun {:.3} ms, longest pass {:.3} ms",
        ms(budget_ns),
        ms(worst_overrun_ns),
        ms(worst_pass_ns)
    );
    if report.degraded() {
        eprintln!(
            "cadence: this session fell behind the tick on {passes_over_budget} \
             pass(es). It is recorded as degraded and must not be pooled with \
             sessions that did not (docs/SCHEMA.md, docs/RISKS.md R16)."
        );
    }
}
